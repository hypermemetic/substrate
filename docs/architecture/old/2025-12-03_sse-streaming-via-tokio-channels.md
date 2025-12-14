# SSE Streaming via Tokio Channels

**Date**: 2025-12-03
**Status**: Planning

## Problem

Tool executions (especially LLM calls) can be long-running. We need to:
1. Return immediately from RPC calls with a `stream_id`
2. Stream execution results incrementally to the client
3. Support multiple concurrent streams
4. Store all events in SQLite for persistence
5. Expose streams over HTTP via Server-Sent Events (SSE)

## Solution: Tokio Channel Bus + SSE

### Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    RPC Handler                          │
│  tools/call → Create stream_id → Return immediately     │
└────────────────────┬────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────┐
│              Tool Execution (async)                      │
│  • Wraps tool output in Stream                          │
│  • Forwards StreamEvents to Tokio Channel               │
│  • Associates events with stream_id                     │
└────────────────────┬────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────┐
│            Central StreamBus (Tokio Channel)            │
│  mpsc::unbounded_channel::<StreamEvent>                 │
│  • Receives events from all tool executions             │
│  • Routes events by stream_id                           │
└────────────────────┬────────────────────────────────────┘
                     │
                     ├──────────────────┬─────────────────┐
                     ▼                  ▼                 ▼
           ┌─────────────────┐  ┌─────────────┐  ┌──────────────┐
           │  SQLite Writer  │  │ SSE Handler │  │ Future: Logs │
           │  • Persist all  │  │ • Subscribe │  │ • Metrics    │
           │    events       │  │   per ID    │  │ • Webhooks   │
           │  • Queryable    │  │ • Push to   │  └──────────────┘
           │    history      │  │   client    │
           └─────────────────┘  └─────────────┘
```

### Flow: RPC Call → Stream → Channel → SSE

```
1. Client calls tools/call with tool name + args
   ├─→ RPC Handler receives request
   ├─→ Generate stream_id (UUID)
   ├─→ Return {"stream_id": "...", "sse_url": "/streams/{stream_id}"}
   └─→ Spawn async task for tool execution

2. Tool execution begins (async)
   ├─→ Tool yields Stream<Item = StreamChunk>
   ├─→ Wrapper intercepts each chunk
   ├─→ Create StreamEvent { stream_id, event_type, data, timestamp }
   └─→ Send to central Tokio channel

3. Central StreamBus receives events
   ├─→ Broadcast to all subscribers for this stream_id
   ├─→ SQLite Writer persists event
   └─→ SSE Handler pushes to connected client

4. Client connects to GET /streams/{stream_id}
   ├─→ SSE endpoint subscribes to StreamBus for this stream_id
   ├─→ Receives events from channel
   ├─→ Formats as SSE: "data: {...}\n\n"
   └─→ Client processes incremental updates
```

## Data Model

### StreamEvent
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    pub stream_id: String,        // UUID
    pub event_type: StreamEventType,
    pub data: serde_json::Value,  // Flexible payload
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEventType {
    Start,      // Execution started
    Chunk,      // Incremental data (LLM token, stdout line, etc.)
    Progress,   // Progress update (e.g., "Step 2/5")
    Done,       // Execution completed successfully
    Error,      // Execution failed
}
```

### StreamMetadata (SQLite)
```sql
CREATE TABLE streams (
    id TEXT PRIMARY KEY,              -- stream_id (UUID)
    tool_name TEXT NOT NULL,          -- Which tool is executing
    arguments TEXT NOT NULL,          -- JSON arguments
    status TEXT NOT NULL,             -- 'running' | 'completed' | 'failed'
    created_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE TABLE stream_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    stream_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    data TEXT NOT NULL,               -- JSON payload
    timestamp TEXT NOT NULL,
    FOREIGN KEY (stream_id) REFERENCES streams(id)
);

CREATE INDEX idx_stream_events_stream_id ON stream_events(stream_id);
```

### RPC Response Format
```json
{
  "jsonrpc": "2.0",
  "result": {
    "stream_id": "550e8400-e29b-41d4-a716-446655440000",
    "sse_url": "/streams/550e8400-e29b-41d4-a716-446655440000",
    "status": "running"
  },
  "id": 1
}
```

### SSE Event Format
```
event: chunk
data: {"stream_id": "550e...", "event_type": "Chunk", "data": {"text": "Hello"}, "timestamp": "2025-12-03T..."}

event: chunk
data: {"stream_id": "550e...", "event_type": "Chunk", "data": {"text": " world"}, "timestamp": "2025-12-03T..."}

event: done
data: {"stream_id": "550e...", "event_type": "Done", "data": {"total_tokens": 42}, "timestamp": "2025-12-03T..."}
```

## Implementation Plan

### Phase 1: Core Streaming Infrastructure
1. **Create `src/streaming/` module**
   - `types.rs` - StreamEvent, StreamEventType, StreamMetadata
   - `bus.rs` - Central Tokio channel + routing logic
   - `registry.rs` - Track active streams
   - `mod.rs` - Public interface

2. **SQLite schema migration**
   - Add `streams` and `stream_events` tables
   - Migration helper in `src/agent/database.rs`

3. **StreamBus singleton**
   - Global `Arc<StreamBus>` accessible to all tools
   - `mpsc::unbounded_channel` for event distribution
   - Per-stream subscription via `tokio::sync::broadcast`

### Phase 2: Tool Integration (Start with agent_chat)
1. **Modify Plugin trait**
   ```rust
   #[async_trait]
   pub trait Plugin: Send + Sync {
       // Existing sync method (keep for backward compat)
       async fn call_tool(&self, name: &str, args: Value)
           -> anyhow::Result<CallToolResult>;

       // NEW: Streaming method
       async fn call_tool_stream(&self, name: &str, args: Value)
           -> anyhow::Result<impl Stream<Item = StreamChunk>>;
   }
   ```

2. **Wrap agent_chat to emit StreamEvents**
   - LLM yields tokens → StreamEvent::Chunk
   - Function calls → StreamEvent::Progress
   - Final response → StreamEvent::Done

3. **Generic StreamWrapper**
   ```rust
   async fn execute_streaming_tool<S>(
       stream_id: String,
       tool_stream: S,
       bus: Arc<StreamBus>,
   ) -> anyhow::Result<()>
   where
       S: Stream<Item = StreamChunk>,
   {
       bus.send(StreamEvent::start(stream_id.clone())).await;

       tokio::pin!(tool_stream);
       while let Some(chunk) = tool_stream.next().await {
           bus.send(StreamEvent::chunk(stream_id.clone(), chunk)).await;
       }

       bus.send(StreamEvent::done(stream_id.clone())).await;
       Ok(())
   }
   ```

### Phase 3: SSE HTTP Endpoint
1. **Add SSE route to Axum**
   ```rust
   async fn stream_handler(
       Path(stream_id): Path<String>,
       State(bus): State<Arc<StreamBus>>,
   ) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
       let rx = bus.subscribe(stream_id).await;

       let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
           .map(|event| {
               Ok(Event::default()
                   .event(event.event_type.as_str())
                   .data(serde_json::to_string(&event).unwrap()))
           });

       Sse::new(stream).keep_alive(KeepAlive::default())
   }

   // Route: GET /streams/:stream_id
   app.route("/streams/:stream_id", get(stream_handler))
   ```

2. **Client connection handling**
   - Keep-alive every 30s
   - Auto-cleanup when client disconnects
   - Replay from SQLite if client reconnects

### Phase 4: RPC Handler Updates
1. **Modify `tools/call` handler**
   ```rust
   handler.add_method("tools/call", move |params: Params| {
       async move {
           let stream_id = Uuid::new_v4().to_string();
           let (tool_name, args) = parse_params(params)?;

           // Store stream metadata in SQLite
           db.create_stream(&stream_id, &tool_name, &args).await?;

           // Spawn async execution
           let bus_clone = bus.clone();
           let registry_clone = registry.clone();
           tokio::spawn(async move {
               let stream = registry_clone
                   .call_tool_stream(&tool_name, args)
                   .await?;
               execute_streaming_tool(stream_id.clone(), stream, bus_clone).await
           });

           // Return immediately
           Ok(json!({
               "stream_id": stream_id,
               "sse_url": format!("/streams/{}", stream_id),
               "status": "running"
           }))
       }
   });
   ```

2. **Add `tools/call_sync` for backward compatibility**
   - Calls `call_tool_stream` but waits for completion
   - Returns final result directly (no streaming)

## Key Design Decisions

### 1. Why Tokio Channels?
- **Decoupling**: Tool execution doesn't know about SSE
- **Fan-out**: One channel → multiple consumers (SQLite, SSE, logs)
- **Backpressure**: Bounded channels prevent memory issues
- **Async-native**: Works seamlessly with tokio runtime

### 2. Why SSE over WebSockets?
- **Simpler**: One-way streaming, no handshake complexity
- **Auto-reconnect**: Browsers handle reconnection automatically
- **HTTP/1.1**: No protocol upgrade needed
- **Firewall-friendly**: Standard HTTP GET request

### 3. Why Store in SQLite?
- **Persistence**: Query stream history later
- **Replay**: Client can reconnect and resume
- **Debugging**: Inspect what happened in past executions
- **Analytics**: Aggregate metrics from stored events

### 4. Why stream_id in every event?
- **Routing**: Channel consumers can filter by ID
- **Multiplexing**: Same channel handles all streams
- **Flexibility**: Easy to add new event consumers

## Testing Strategy

### Unit Tests
```rust
#[tokio::test]
async fn test_stream_bus_routing() {
    let bus = StreamBus::new();
    let stream_id = "test-123".to_string();

    let mut rx = bus.subscribe(stream_id.clone()).await;

    bus.send(StreamEvent::start(stream_id.clone())).await;
    bus.send(StreamEvent::chunk(stream_id.clone(), json!({"text": "hi"}))).await;
    bus.send(StreamEvent::done(stream_id.clone())).await;

    assert_eq!(rx.recv().await.unwrap().event_type, StreamEventType::Start);
    assert_eq!(rx.recv().await.unwrap().event_type, StreamEventType::Chunk);
    assert_eq!(rx.recv().await.unwrap().event_type, StreamEventType::Done);
}
```

### Integration Tests
```bash
# Terminal 1: Start server
cargo run -- rpc --http --port 8081

# Terminal 2: Call streaming tool
curl -X POST http://localhost:8081/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"agent_chat","arguments":{"message":"Hello"}},"id":1}'

# Response: {"result": {"stream_id": "550e...", "sse_url": "/streams/550e..."}}

# Terminal 3: Subscribe to stream
curl -N http://localhost:8081/streams/550e8400-e29b-41d4-a716-446655440000
```

## Migration Path

### Backward Compatibility
1. Keep existing `call_tool` method (non-streaming)
2. Add new `call_tool_stream` method
3. Tools can implement one or both
4. RPC handler checks which method is available

### Opt-in Streaming
```rust
// Agent DB plugin opts into streaming
impl Plugin for AgentDbPlugin {
    async fn call_tool_stream(&self, name: &str, args: Value)
        -> anyhow::Result<impl Stream<Item = StreamChunk>>
    {
        match name {
            "agent_chat" => self.agent_chat_stream(args).await,
            _ => self.call_tool(name, args).await.into_stream() // Fallback
        }
    }
}

// System plugin stays non-streaming
impl Plugin for SystemPlugin {
    async fn call_tool(&self, name: &str, args: Value)
        -> anyhow::Result<CallToolResult>
    {
        // Works as-is
    }
}
```

## Files to Create/Modify

### New Files
- `src/streaming/mod.rs`
- `src/streaming/types.rs`
- `src/streaming/bus.rs`
- `src/streaming/registry.rs`
- `src/streaming/wrapper.rs`

### Modified Files
- `src/lib.rs` - Add `pub mod streaming;`
- `src/main.rs` - Initialize StreamBus, add SSE route, update tools/call handler
- `src/plugin/traits.rs` - Add optional `call_tool_stream` method
- `src/plugin/builtins/agent_db.rs` - Implement streaming for agent_chat
- `src/agent/database.rs` - Add streams and stream_events tables
- `Cargo.toml` - Add `tokio-stream`, `tower-http` (for SSE)

## Performance Considerations

### Memory
- Use **unbounded channel** for main bus (backpressure from consumers)
- Use **broadcast channel** for per-stream subscribers (fixed buffer size)
- Limit max concurrent streams (e.g., 100)

### SQLite Write Performance
- Batch writes every 100ms or 50 events
- Use WAL mode for concurrent reads/writes
- Index on stream_id for fast queries

### SSE Keep-Alive
- Send comment every 30s to keep connection alive
- Client auto-reconnects if dropped
- Server timeout after 5 minutes of inactivity

## Next Steps

1. Implement Phase 1: Core streaming infrastructure
2. Add SQLite schema migration
3. Modify agent_chat to stream LLM tokens
4. Add SSE endpoint to Axum router
5. Test end-to-end with rpc-client
6. Add streaming to other tools (run_bash, agent_query)

## Future Enhancements

- **Stream replay**: `GET /streams/{id}?from=timestamp`
- **Stream cancellation**: `DELETE /streams/{id}`
- **Stream aggregation**: `GET /streams?status=running`
- **WebSocket alternative**: For bidirectional use cases
- **Stream compression**: Gzip SSE events
- **Multi-tenancy**: Namespace streams by user/session

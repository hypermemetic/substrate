# Streaming Implementation Status

**Date**: 2025-12-03
**Branch**: `feat/stream`

## Completed ✅

### 1. POC Tests (Commit b95d07e)
- `tests/streaming_poc.rs` - 10/10 tests ✅
  - Basic Stream → Channel → SSE transformations
  - Support for String and StreamItem<T> types
  - Concurrent stream multiplexing

- `tests/streaming_type_erasure_poc.rs` - 7/7 tests ✅
  - Content-type metadata for arbitrary types
  - Type erasure via `serde_json::Value`
  - Self-describing messages

- `tests/streaming_schema_negotiation_poc.rs` - 9/9 tests ✅
  - Opt-in schema transmission
  - Client-side caching strategy
  - Zero overhead by default (first occurrence only)

### 2. Core Infrastructure (Commit 5adfcbf)
- `src/streaming/types.rs` - 3/3 tests ✅
  - `ContentTypeId` - Type identifiers with versioning
  - `ContentType` - Self-describing type enum
  - `ChannelMessage` - Type-erased messages
  - `SseEvent` - SSE wire format

- `src/streaming/bus.rs` - 3/3 tests ✅
  - `StreamBus` - Central tokio channel bus
  - `StreamBusHandle` - Background message router
  - Per-stream broadcast channels
  - Automatic routing by stream_id

## Next Steps

### 3. RPC Handler Integration (✅ Completed - Commit a271448)

**Implemented**:

**`tools/call_stream`** - Initiate streaming tool execution
```json
Request:
{
  "jsonrpc": "2.0",
  "method": "tools/call_stream",
  "params": {
    "name": "agent_chat",
    "arguments": {"message": "Hello"}
  },
  "id": 1
}

Response (immediate):
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

**SSE Endpoint** - Subscribe to stream
```http
GET /streams/{stream_id}
GET /streams/{stream_id}?schemas=all (opt-in schema transmission)

Response (SSE):
event: chunk
data: {"stream_id":"550e...","content_type":"StreamItem<AgentResponseTypeT>","data":{...}}

event: chunk
data: {"stream_id":"550e...","content_type":"StreamItem<AgentResponseTypeT>","data":{...}}

event: done
data: {"stream_id":"550e...","status":"completed"}
```

### 4. Agent Chat Streaming

Current implementation (`src/agent/core.rs:33-127`):
```rust
pub async fn chat_with_agent_logic(...) -> Result<ChatResponse, ApiError> {
    // ...
    let parsed_response: ParsedResponse<AgentResponseTypeT<TextResponse>> =
        resolver.query(prompt).await?;
    let agent_response = parsed_response.first_required()?;
    // ...
}
```

New streaming implementation needed:
```rust
pub async fn chat_with_agent_stream(
    db: &AgentDatabase,
    agent_id: AgentId,
    request: ChatRequest,
    stream_bus: Arc<StreamBus>,
    stream_id: String,
) -> anyhow::Result<()> {
    // ... (same setup as chat_with_agent_logic)

    // Use stream_query instead of query
    let stream = resolver.stream_query::<AgentResponseTypeT<TextResponse>>(prompt).await?;

    // Forward StreamItems to bus
    stream_bus.submit_stream(stream_id, stream).await?;

    Ok(())
}
```

### 5. Main.rs Wiring

```rust
// In main.rs

// Create global StreamBus
let (stream_bus, bus_handle) = StreamBus::new();
tokio::spawn(async move {
    bus_handle.run().await;
});

// Add to Axum router
let app = app.route("/streams/:stream_id", get(sse_stream_handler));

async fn sse_stream_handler(
    Path(stream_id): Path<String>,
    State(bus): State<Arc<StreamBus>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = bus.subscribe(stream_id.clone()).await;

    let stream = async_stream::stream! {
        while let Ok(event) = rx.recv().await {
            yield Ok(Event::default()
                .event(&event.event_type)
                .data(serde_json::to_string(&event).unwrap()));
        }
    };

    Sse::new(stream)
}
```

## Design Decisions Made

### Type Erasure Strategy
- Use `serde_json::Value` for content (runtime flexibility)
- `ContentTypeId` carries type information
- Clients deserialize based on `content_type` metadata

### Schema Transmission
- **Default**: No schema overhead (only first occurrence)
- **Opt-in**: `?schemas=all` or `?schemas=ToolCall,Analysis`
- **Alternative**: Separate `/schemas/{type_id}` endpoint

### Channel Architecture
- Main `mpsc::unbounded_channel` for all streams
- Per-stream `broadcast::channel` for SSE subscribers
- Background task routes messages by `stream_id`

### Backward Compatibility
- Keep `tools/call` for synchronous execution
- Add `tools/call_stream` for streaming execution
- Tools can support both or just one

## Files Modified So Far

- ✅ `src/streaming/mod.rs` - Module definition
- ✅ `src/streaming/types.rs` - Core types
- ✅ `src/streaming/bus.rs` - Channel bus implementation
- ✅ `src/lib.rs` - Added `pub mod streaming;`
- ⏳ `src/main.rs` - Need to add SSE endpoint + RPC method
- ⏳ `src/agent/core.rs` - Need streaming version of chat_with_agent
- ⏳ `src/plugin/builtins/agent_db.rs` - Need to call streaming version

## Testing Strategy

1. ✅ Unit tests for types and bus (all passing)
2. ⏳ Integration test: RPC call → Stream → SSE
3. ⏳ End-to-end test: agent_chat streaming
4. ⏳ Manual test with `curl -N`

## Dependencies to Add

```toml
[dependencies]
async-stream = "0.3"  # For SSE stream generation
axum-sse = "0.1"      # Or use axum's built-in Sse
tower-http = "0.5"    # For CORS if needed
```

## Open Questions

1. **Should `tools/call` automatically detect streaming tools?**
   - Option A: New method `tools/call_stream` (explicit)
   - Option B: `tools/call` checks if tool supports streaming
   - **Decision**: Option A for clarity

2. **How to handle tool errors in streams?**
   - Send `event: error` with error details
   - Close stream after error
   - Client displays error message

3. **Stream cleanup strategy?**
   - Auto-cleanup after `event: done`
   - Timeout after 5 minutes of inactivity
   - Manual cleanup endpoint `DELETE /streams/{id}`

## Next Immediate Task

Wire StreamBus into `main.rs` and add `/streams/{id}` SSE endpoint.

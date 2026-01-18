# Arbor-Buffered Streaming for ClaudeCode Loopback

> **SUPERSEDED**: This document describes the original design. The API has been simplified to use `session_id` as the single identifier (removing `stream_id`). See [ClaudeCode Loopback Integration](16677965632570341631_claudecode-loopback-integration.md) for the current implementation.

## Problem Statement

When `claudecode_chat` is called via MCP, the parent Claude blocks waiting for the stream to complete. When the child Claude needs tool approval, it calls `loopback_permit` which also blocks. This creates a deadlock:

```
Parent Claude                    Child Claude
     │                                │
     ├─── claudecode_chat ───────────►│
     │    (blocks waiting)            │
     │                                ├─── wants to run Bash tool
     │                                │
     │                                ├─── calls loopback_permit
     │                                │    (blocks waiting for approval)
     │                                │
     ▼ DEADLOCK ◄─────────────────────▼
```

## Solution: Non-Blocking Chat with Event Buffer

We implemented a non-blocking variant of `claudecode_chat` that:

1. **Returns immediately** with a `stream_id`
2. **Spawns a background task** that runs the chat
3. **Buffers events** in memory for polling
4. **Tracks status** including `AwaitingPermission`

### New MCP Methods

#### `claudecode_chat_async`
Starts a chat in the background, returning immediately.

```json
{
  "method": "claudecode.chat_async",
  "params": {
    "name": "my-session",
    "prompt": "curl httpbin.org/get"
  }
}

// Returns immediately:
{
  "type": "started",
  "stream_id": "550e8400-e29b-41d4-a716-446655440000",
  "session_id": "123e4567-e89b-12d3-a456-426614174000"
}
```

#### `claudecode_poll`
Reads events from the stream buffer.

```json
{
  "method": "claudecode.poll",
  "params": {
    "stream_id": "550e8400-e29b-41d4-a716-446655440000",
    "from_seq": 0,    // optional, auto-tracks read position
    "limit": 100      // optional, default 100
  }
}

// Returns:
{
  "type": "ok",
  "status": "awaiting_permission",
  "events": [
    {"seq": 0, "event": {"type": "start", ...}},
    {"seq": 1, "event": {"type": "content", "text": "I'll run..."}},
    {"seq": 2, "event": {"type": "tool_use", "tool_name": "Bash", ...}}
  ],
  "read_position": 3,
  "total_events": 3,
  "has_more": false
}
```

#### `claudecode_streams`
Lists active streams, optionally filtered by session.

```json
{
  "method": "claudecode.streams",
  "params": {
    "session_id": null  // optional filter
  }
}
```

### Stream Status

```rust
enum StreamStatus {
    Running,            // Actively receiving events
    AwaitingPermission, // Waiting for tool approval
    Complete,           // Successfully finished
    Failed,             // Errored
}
```

The status transitions to `AwaitingPermission` when we detect the child Claude calling `mcp__plexus__loopback_permit`.

## Usage Flow

```
1. Parent: claudecode_chat_async("curl httpbin")
   → Returns: { stream_id: "abc", status: "running" }

2. Parent: claudecode_poll("abc")
   → Returns: [Start, Content("I'll run...")]

3. Parent: claudecode_poll("abc")
   → Returns: [ToolUse(Bash)], status: "awaiting_permission"

4. Parent: loopback_pending()
   → Returns: [{ id: "xyz", tool_name: "Bash", ... }]

5. Parent: loopback_respond("xyz", approve=true)
   → Returns: ok

6. Parent: claudecode_poll("abc")
   → Returns: [ToolResult(...), Content(...), Complete]
```

## Implementation Details

### In-Memory Buffer

Events are stored in an in-memory buffer per stream:

```rust
struct ActiveStreamBuffer {
    info: StreamInfo,           // metadata, status, positions
    events: Vec<BufferedEvent>, // ordered events with seq numbers
}
```

Storage lives in `ClaudeCodeStorage.streams: RwLock<HashMap<StreamId, ActiveStreamBuffer>>`.

### Background Task

The chat logic runs in a spawned tokio task with tracing instrumentation:

```rust
tokio::spawn(async move {
    Self::run_chat_background(storage, executor, config, prompt, is_ephemeral, stream_id).await;
}.instrument(tracing::info_span!("chat_async_bg", stream_id = %stream_id)));
```

### Read Position Tracking

Each poll updates the stream's `read_position` to avoid re-fetching events:

```rust
// First poll: returns events [0, 1, 2], read_position = 3
// Second poll: returns events starting at seq 3
```

Clients can also specify `from_seq` to replay from a specific position.

## Related Files

- `src/activations/claudecode/activation.rs` - New methods: `chat_async`, `poll`, `streams`
- `src/activations/claudecode/storage.rs` - Stream buffer management
- `src/activations/claudecode/types.rs` - New types: `StreamId`, `StreamStatus`, `StreamInfo`, etc.
- `src/activations/claudecode_loopback/` - Loopback approval system (unchanged)

## Future Considerations

1. **Event Cleanup**: Currently streams persist until manually cleaned up. Consider auto-cleanup after completion + timeout.

2. **Persistence**: Events are in-memory only. For durability, could write events to Arbor nodes.

3. **Multiple Consumers**: Currently one read_position per stream. Could support multiple cursors for fan-out patterns.

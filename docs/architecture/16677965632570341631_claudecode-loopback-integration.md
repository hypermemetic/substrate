# ClaudeCode Loopback Integration

> Complete guide to the async chat + loopback approval flow

## Overview

The ClaudeCode plugin supports a "loopback" mode where tool permissions are routed to a parent for approval. This enables hierarchical agent control - a parent Claude can spawn child Claudes and approve/deny their tool usage.

## Architecture

```
Parent Claude (via MCP)
    │
    ├── claudecode.create(loopback_enabled=true)
    │   └── Creates session with loopback mode
    │
    ├── claudecode.chat_async(name, prompt)
    │   └── Returns session_id, spawns background task
    │
    ├── claudecode.poll(session_id)  ◄─── polling loop
    │   └── Returns events + pending_approvals
    │
    ├── loopback.respond(approval_id, approve)
    │   └── Approves/denies tool, unblocks child
    │
    └── claudecode.poll(session_id)
        └── Returns more events → complete
```

## Key Concepts

### Session ID as Universal Identifier

The `session_id` (UUID) is the single identifier used throughout:
- Returned by `create` and `chat_async`
- Used by `poll` to fetch events
- Used to filter `pending_approvals`
- Correlates tool_use events with approval requests

### Non-Blocking Chat

The `chat_async` method is **required** for loopback-enabled sessions. The blocking `chat` method will deadlock because the parent can't approve while waiting for the stream.

```
❌ DEADLOCK with blocking chat:
Parent: chat() ──blocks──► Child: needs approval ──blocks──► Parent can't approve

✅ WORKS with async chat:
Parent: chat_async() ──returns immediately──► poll() ──sees pending──► respond()
```

### Event Buffer

Each session has an in-memory event buffer:
- Created/reset when `chat_async` is called
- Events are pushed by the background task
- `poll` reads events and advances read position
- Buffer persists until next `chat_async` or Plexus restart

## API Reference

### `claudecode.create`

Create a session with loopback enabled:

```json
{
  "method": "claudecode.create",
  "params": {
    "name": "my-agent",
    "working_dir": "/tmp",
    "model": "haiku",
    "loopback_enabled": true
  }
}

// Response:
{
  "type": "created",
  "id": "17ee2aba-3aa2-4a87-8bae-3ad00b404783",
  "head": { "tree_id": "...", "node_id": "..." }
}
```

### `claudecode.chat_async`

Start a non-blocking chat:

```json
{
  "method": "claudecode.chat_async",
  "params": {
    "name": "my-agent",
    "prompt": "Run curl https://httpbin.org/ip"
  }
}

// Returns immediately:
{
  "type": "started",
  "session_id": "17ee2aba-3aa2-4a87-8bae-3ad00b404783"
}
```

### `claudecode.poll`

Read events and pending approvals:

```json
{
  "method": "claudecode.poll",
  "params": {
    "session_id": "17ee2aba-3aa2-4a87-8bae-3ad00b404783",
    "from_seq": null,  // optional: replay from position
    "limit": null      // optional: max events (default 100)
  }
}

// Response:
{
  "type": "ok",
  "status": "running",  // running | awaiting_permission | complete | failed
  "events": [
    {"seq": 0, "event": {"type": "start", "id": "...", "user_position": {...}}},
    {"seq": 1, "event": {"type": "content", "text": "I'll run..."}},
    {"seq": 2, "event": {"type": "tool_use", "tool_name": "Bash", "tool_use_id": "toolu_xxx", "input": {...}}}
  ],
  "pending_approvals": [
    {
      "id": "6f0c927d-256a-4dbf-bfa7-372ecfa72b25",
      "tool_name": "Bash",
      "tool_use_id": "toolu_xxx",
      "input": {"command": "curl https://httpbin.org/ip"},
      "created_at": 1768725727
    }
  ],
  "read_position": 3,
  "total_events": 3,
  "has_more": false
}
```

### `loopback.respond`

Approve or deny a pending tool request:

```json
{
  "method": "loopback.respond",
  "params": {
    "approval_id": "6f0c927d-256a-4dbf-bfa7-372ecfa72b25",
    "approve": true,
    "message": null  // optional: reason for denial
  }
}

// Response:
{
  "type": "ok",
  "approval_id": "6f0c927d-256a-4dbf-bfa7-372ecfa72b25"
}
```

## Complete Flow Example

```
1. CREATE SESSION
   claudecode.create(name="agent", loopback_enabled=true)
   → session_id: "17ee2aba-..."

2. START ASYNC CHAT
   claudecode.chat_async(name="agent", prompt="curl httpbin.org/ip")
   → session_id: "17ee2aba-..."

3. POLL FOR EVENTS
   claudecode.poll(session_id="17ee2aba-...")
   → events: [Start, Content("I'll run..."), ToolUse(Bash)]
   → pending_approvals: [{id: "6f0c...", tool_name: "Bash", input: {command: "curl..."}}]
   → status: "running"

4. APPROVE THE TOOL
   loopback.respond(approval_id="6f0c...", approve=true)
   → ok

5. POLL FOR COMPLETION
   claudecode.poll(session_id="17ee2aba-...")
   → events: [Content("Your IP is..."), Complete]
   → pending_approvals: []
   → status: "complete"
```

## Implementation Details

### Tool-to-Session Correlation

When the child Claude wants to run a tool, it calls `loopback_permit` via MCP. The challenge is correlating this with the correct session.

**Solution: Pre-registration**

1. Background task sees `ToolUse` event with `tool_use_id`
2. Calls `loopback_storage.register_tool_session(tool_use_id, session_id)`
3. When `loopback_permit` is called, it looks up session via `lookup_session_by_tool(tool_use_id)`
4. Creates approval with correct `session_id`
5. `poll` filters `pending_approvals` by `session_id`

```rust
// In run_chat_background, when ToolUse is detected:
if let Some(ref lb) = loopback_storage {
    lb.register_tool_session(&tool_use_id, &session_id.to_string());
}
```

### Buffer Lifecycle

```
chat_async called
    │
    ├── buffer_create(session_id)  // Creates or resets buffer
    │
    ├── spawn background task
    │   │
    │   ├── buffer_push_event(Start)
    │   ├── buffer_push_event(Content)
    │   ├── buffer_push_event(ToolUse)
    │   ├── buffer_set_status(AwaitingPermission)  // when loopback_permit called
    │   ├── buffer_push_event(ToolResult)
    │   ├── buffer_push_event(Complete)
    │   └── buffer_set_status(Complete)
    │
    └── poll reads from buffer
```

### Status Transitions

```
Running ──────► AwaitingPermission ──────► Running ──────► Complete
   │                                           │              │
   └──────────────────► Failed ◄───────────────┴──────────────┘
```

## Multiple Chats to Same Session

You can submit multiple chats to the same session:

```
chat_async(name="agent", prompt="first task")
→ poll until complete

chat_async(name="agent", prompt="second task")  // resets buffer
→ poll until complete
```

Each `chat_async` resets the buffer (seq starts at 0) but preserves:
- Session context (`claude_session_id` for resume)
- Arbor tree history
- loopback_enabled setting

## Related Files

- `src/activations/claudecode/activation.rs` - Main activation with `chat_async`, `poll`
- `src/activations/claudecode/storage.rs` - Session + buffer storage
- `src/activations/claudecode/types.rs` - Types: `ChatEvent`, `StreamStatus`, `PollResult`
- `src/activations/claudecode_loopback/activation.rs` - Loopback approval system
- `src/activations/claudecode_loopback/storage.rs` - Approval storage with tool-session mapping

## Limitations

1. **In-memory buffers**: Event buffers are lost on Plexus restart
2. **Single consumer**: One read position per buffer (no fan-out)
3. **No persistence**: Events are not written to Arbor (future enhancement)

## See Also

- [Loopback MCP Conformance Analysis](16678130375925173503_loopback-mcp-conformance-analysis.md) - How loopback integrates with Claude Code's `--permission-prompt-tool`

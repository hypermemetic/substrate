# HANDLE-4: Define ClaudeCodeHandle Enum

**blocked_by**: [HANDLE-2]
**unlocks**: [HANDLE-5]

## Scope

Define handle types for ClaudeCode activation (Claude Code CLI sessions).

## Acceptance Criteria

- [ ] Define `ClaudeCodeHandle` enum for sessions and events
- [ ] Add `chat_events` table for event storage (for stream map)
- [ ] Integrate with existing session storage
- [ ] Enable handle resolution for Arbor Stream Map feature

## Handle Definition

```rust
#[derive(HandleEnum)]
#[handle(plugin_id = "CLAUDECODE_PLUGIN_ID", version = "1.0.0")]
pub enum ClaudeCodeHandle {
    #[handle(method = "session", table = "claudecode_sessions", key = "id")]
    Session { session_id: String },

    #[handle(method = "chat_event", table = "chat_events", key = "id")]
    ChatEvent { event_id: String },

    #[handle(method = "approval", table = "pending_approvals", key = "id")]
    Approval { approval_id: String },
}
```

## Storage Tables

Existing:
- `claudecode_sessions` - session configuration

New (for Arbor Stream Map):
- `chat_events` - persisted chat events for handle resolution

```sql
CREATE TABLE chat_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    event_data TEXT NOT NULL,  -- JSON
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES claudecode_sessions(id)
);
```

## Integration with Arbor Stream Map

This ticket enables the Arbor Stream Map design:
1. Chat events stored in `chat_events` table
2. Arbor nodes reference events via `ClaudeCodeHandle::ChatEvent`
3. Polling resolves handles to actual event data

## Migration

1. Add `ClaudeCodeHandle` enum to `claudecode/types.rs`
2. Add `chat_events` table to storage
3. Update event pushing to store in table AND create handles
4. Implement `resolve_handle` for event retrieval

## Estimated Complexity

Medium - requires new table and integration with streaming

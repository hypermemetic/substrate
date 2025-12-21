# MCP-6: Initialized Notification Handler

## Metadata
- **blocked_by:** [MCP-4]
- **unlocks:** [MCP-7]
- **priority:** Critical (on critical path)

## Scope

Implement the `notifications/initialized` handler that completes the handshake.

## Protocol

**Notification (no response):**
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/initialized"
}
```

## Implementation

```rust
// src/mcp/handlers/initialized.rs

impl McpInterface {
    pub async fn handle_initialized(&self, _params: Value) -> Result<Value, McpError> {
        // Must be in Initializing state
        self.state.require(McpState::Initializing)?;

        // Transition to Ready
        self.state.transition(McpState::Ready)?;

        tracing::info!("MCP session initialized, now accepting requests");

        // Notifications don't return a result
        // The JSON-RPC layer should not send a response for notifications
        Ok(Value::Null)
    }
}
```

## Notes

- This is a **notification**, not a request (no `id` field)
- The MCP layer must detect notifications and not send responses
- After this, the server is fully operational

## Files to Create/Modify

- Create `src/mcp/handlers/initialized.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Only succeeds if state is `Initializing`
- [ ] Transitions state to `Ready`
- [ ] Returns no response (notification semantics)
- [ ] Logs successful initialization

# MCP-7: Ping Handler

## Metadata
- **blocked_by:** [MCP-6]
- **unlocks:** [MCP-8, MCP-9]
- **priority:** Critical (on critical path)

## Scope

Implement the `ping` request handler for health checks.

## Protocol

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "ping"
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {}
}
```

## Implementation

```rust
// src/mcp/handlers/ping.rs

impl McpInterface {
    pub async fn handle_ping(&self, _params: Value) -> Result<Value, McpError> {
        // Ping works in Ready state only
        self.state.require_ready()?;

        Ok(json!({}))
    }
}
```

## Notes

- Simple health check, returns empty object
- Can be used for keepalive in long-running connections
- Must be in Ready state

## Files to Create/Modify

- Create `src/mcp/handlers/ping.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Returns empty object `{}`
- [ ] Requires Ready state
- [ ] Fast response (no blocking operations)

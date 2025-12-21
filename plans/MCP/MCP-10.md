# MCP-10: Tools Call Handler

## Metadata
- **blocked_by:** [MCP-3, MCP-7, MCP-9]
- **unlocks:** [MCP-11, MCP-12, MCP-13]
- **priority:** Critical (on critical path)

## Scope

Implement the `tools/call` request handler that invokes Plexus methods and returns buffered results.

## Protocol

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "claudecode.chat",
    "arguments": {
      "session_name": "my-session",
      "query": "Hello, world!"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "content": [{ "type": "text", "text": "Hello! How can I help?" }],
    "isError": false
  }
}
```

## Implementation

```rust
// src/mcp/handlers/tools_call.rs

#[derive(Debug, Deserialize)]
pub struct ToolsCallParams {
    pub name: String,
    pub arguments: Value,
}

impl McpInterface {
    pub async fn handle_tools_call(&self, params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        let params: ToolsCallParams = serde_json::from_value(params)?;

        // Parse tool name: "namespace.method"
        let (namespace, method) = params.name
            .split_once('.')
            .ok_or_else(|| McpError::InvalidToolName(params.name.clone()))?;

        // Build Plexus method name
        let plexus_method = format!("{}.{}", namespace, method);

        // Call Plexus and get stream
        let stream = match self.plexus.call(&plexus_method, params.arguments).await {
            Ok(s) => s,
            Err(e) => {
                return Ok(serde_json::to_value(ToolCallResult {
                    content: vec![McpContent {
                        content_type: "text".into(),
                        text: e.to_string(),
                    }],
                    is_error: true,
                })?);
            }
        };

        // Buffer the stream into MCP response
        let result = buffer_plexus_stream(stream).await;

        Ok(serde_json::to_value(result)?)
    }
}
```

## Error Handling

Tool errors are returned as successful responses with `isError: true`:

```json
{
  "result": {
    "content": [{ "type": "text", "text": "Session 'foo' not found" }],
    "isError": true
  }
}
```

Protocol errors (invalid params, etc.) return JSON-RPC errors:

```json
{
  "error": {
    "code": -32602,
    "message": "Invalid tool name format: 'no-dot-here'"
  }
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/tools_call.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Parses `namespace.method` tool names
- [ ] Routes to correct Plexus activation
- [ ] Buffers streaming response
- [ ] Returns tool errors as `isError: true`
- [ ] Returns protocol errors as JSON-RPC errors
- [ ] Requires Ready state
- [ ] Integration test with real activation

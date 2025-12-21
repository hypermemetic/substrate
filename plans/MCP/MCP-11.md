# MCP-11: stdio MCP Transport

## Metadata
- **blocked_by:** [MCP-10]
- **unlocks:** [MCP-14, MCP-15]
- **priority:** High

## Scope

Integrate MCP protocol handling into the existing stdio transport with a `--mcp` flag.

## CLI Interface

```bash
# Existing Plexus mode (no handshake)
substrate --stdio

# MCP mode (requires initialize handshake)
substrate --stdio --mcp
```

## Implementation

```rust
// src/transport/stdio.rs (modifications)

pub struct StdioTransport {
    plexus: Arc<Plexus>,
    mcp: Option<McpInterface>,
}

impl StdioTransport {
    pub fn new(plexus: Arc<Plexus>, mcp_mode: bool) -> Self {
        Self {
            plexus: plexus.clone(),
            mcp: if mcp_mode {
                Some(McpInterface::new(plexus))
            } else {
                None
            },
        }
    }

    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        if let Some(mcp) = &self.mcp {
            // MCP mode: route through McpInterface
            match mcp.handle(&request.method, request.params).await {
                Ok(result) => JsonRpcResponse::success(request.id, result),
                Err(e) => JsonRpcResponse::error(request.id, e.into()),
            }
        } else {
            // Plexus mode: direct routing
            self.plexus.call(&request.method, request.params).await
        }
    }
}

// src/main.rs (modifications)

#[derive(Parser)]
struct Args {
    #[arg(long)]
    stdio: bool,

    #[arg(long, requires = "stdio")]
    mcp: bool,
}
```

## Message Format

Both modes use newline-delimited JSON-RPC:

```
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}\n
{"jsonrpc":"2.0","id":1,"result":{...}}\n
```

## Notification Handling

MCP notifications (no `id` field) should not generate responses:

```rust
fn is_notification(request: &JsonRpcRequest) -> bool {
    request.id.is_none()
}

async fn handle_message(&self, msg: &str) -> Option<String> {
    let request: JsonRpcRequest = serde_json::from_str(msg)?;

    if is_notification(&request) {
        // Process but don't respond
        let _ = self.handle_request(request).await;
        None
    } else {
        // Process and respond
        let response = self.handle_request(request).await;
        Some(serde_json::to_string(&response).unwrap())
    }
}
```

## Files to Create/Modify

- Modify `src/transport/stdio.rs`
- Modify `src/main.rs` for `--mcp` flag
- Add integration tests

## Acceptance Criteria

- [ ] `--mcp` flag enables MCP mode
- [ ] MCP mode requires initialize handshake
- [ ] Notifications don't generate responses
- [ ] Plexus mode continues working unchanged
- [ ] Passes MCP validator with stdio transport

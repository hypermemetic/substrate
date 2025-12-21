# MCP-3: McpInterface Struct

## Metadata
- **blocked_by:** [MCP-2]
- **unlocks:** [MCP-10]
- **priority:** High

## Scope

Create the main MCP interface that wraps Plexus and routes MCP protocol methods.

## Implementation

```rust
// src/mcp/interface.rs

pub struct McpInterface {
    plexus: Arc<Plexus>,
    state: McpStateMachine,
    server_info: ServerInfo,
}

impl McpInterface {
    pub fn new(plexus: Arc<Plexus>) -> Self {
        Self {
            plexus,
            state: McpStateMachine::new(),
            server_info: ServerInfo {
                name: "substrate".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
        }
    }

    /// Route an MCP request to the appropriate handler
    pub async fn handle(&self, method: &str, params: Value) -> Result<Value, McpError> {
        match method {
            "initialize" => self.handle_initialize(params).await,
            "notifications/initialized" => self.handle_initialized(params).await,
            "ping" => self.handle_ping(params).await,
            "tools/list" => self.handle_tools_list(params).await,
            "tools/call" => self.handle_tools_call(params).await,
            "resources/list" => self.handle_resources_list(params).await,
            "resources/read" => self.handle_resources_read(params).await,
            "prompts/list" => self.handle_prompts_list(params).await,
            "notifications/cancelled" => self.handle_cancelled(params).await,
            _ => Err(McpError::MethodNotFound(method.to_string())),
        }
    }
}
```

## Files to Create/Modify

- Create `src/mcp/interface.rs`
- Create `src/mcp/types.rs` (ServerInfo, Capabilities, etc.)
- Update `src/mcp/mod.rs`

## Acceptance Criteria

- [ ] `McpInterface` struct holding Plexus + state machine
- [ ] `handle()` method with routing to handlers (stubs initially)
- [ ] Server info populated from Cargo.toml
- [ ] All handlers return `McpError::NotImplemented` initially

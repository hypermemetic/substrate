# MCP-4: Initialize Handler

## Metadata
- **blocked_by:** [MCP-2]
- **unlocks:** [MCP-6]
- **priority:** Critical (on critical path)
- **assigned:** true

## Scope

Implement the `initialize` request handler that performs MCP handshake.

## Protocol

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": { ... },
    "clientInfo": { "name": "claude-code", "version": "1.0.0" }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {
      "tools": { "listChanged": true },
      "resources": { "subscribe": true, "listChanged": true },
      "logging": {}
    },
    "serverInfo": { "name": "substrate", "version": "0.1.0" }
  }
}
```

## Implementation

```rust
// src/mcp/handlers/initialize.rs

#[derive(Debug, Deserialize)]
pub struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
}

#[derive(Debug, Serialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
}

impl McpInterface {
    pub async fn handle_initialize(&self, params: Value) -> Result<Value, McpError> {
        // Must be in Uninitialized state
        self.state.require(McpState::Uninitialized)?;

        let params: InitializeParams = serde_json::from_value(params)?;

        // Validate protocol version
        if !SUPPORTED_VERSIONS.contains(&params.protocol_version.as_str()) {
            return Err(McpError::UnsupportedVersion(params.protocol_version));
        }

        // Transition to Initializing
        self.state.transition(McpState::Initializing)?;

        // Build capabilities based on registered activations
        let capabilities = self.build_capabilities();

        Ok(serde_json::to_value(InitializeResult {
            protocol_version: params.protocol_version,
            capabilities,
            server_info: self.server_info.clone(),
        })?)
    }

    fn build_capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: true }),
            resources: self.plexus.has_activation("arbor").then(|| {
                ResourcesCapability { subscribe: true, list_changed: true }
            }),
            prompts: None,
            logging: Some(LoggingCapability {}),
        }
    }
}

const SUPPORTED_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26"];
```

## Files to Create/Modify

- Create `src/mcp/handlers/mod.rs`
- Create `src/mcp/handlers/initialize.rs`
- Add capability types to `src/mcp/types.rs`

## Acceptance Criteria

- [x] Validates protocol version against supported list
- [x] Returns error if called in wrong state
- [x] Transitions state to `Initializing`
- [x] Returns capabilities based on registered activations
- [x] Includes serverInfo from Cargo.toml

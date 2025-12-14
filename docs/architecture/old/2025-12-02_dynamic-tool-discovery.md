# Dynamic Tool Discovery & Server Identity

**Date**: 2025-12-02
**Status**: Implemented

## Problem

Clients need to:
1. **Discover available tools at runtime** - The plugin system means tool availability is dynamic
2. **Verify server identity** - Know if they're talking to the same server configuration
3. **Access tool schemas** - Understand how to call each tool

## Solution: Three Core RPC Methods

### 1. `tools/list` - Tool Discovery

Returns all available tools with their schemas.

**Implementation**: `src/main.rs:74`
```rust
handler.add_method("tools/list", move |_params: Params| {
    let reg = registry_clone.clone();
    async move {
        let tools = reg.list_tools();
        let result = serde_json::to_value(&json!({ "tools": tools }))
            .map_err(|_| jsonrpc_core::Error::internal_error())?;
        Ok(result)
    }
});
```

**Response Format**:
```json
{
  "tools": [
    {
      "name": "agent_list",
      "description": "List all agents",
      "inputSchema": {
        "type": "object",
        "properties": {},
        "additionalProperties": false
      }
    },
    {
      "name": "read_file",
      "description": "Read file contents",
      "inputSchema": {
        "type": "object",
        "properties": {
          "path": { "type": "string" }
        },
        "required": ["path"]
      }
    }
  ]
}
```

**Why This Matters**: Each tool's `inputSchema` is a JSON Schema that describes:
- Required parameters
- Optional parameters
- Parameter types and constraints
- Validation rules

This enables:
- **Dynamic UI generation** - Clients can build forms from schemas
- **Client-side validation** - Check arguments before sending
- **Auto-completion** - IDEs can suggest valid parameters
- **Documentation** - Self-documenting API

### 2. `tools/call` - Tool Execution

Execute any tool by name with typed arguments.

**Implementation**: `src/main.rs:85`
```rust
handler.add_method("tools/call", move |params: Params| {
    let reg = registry.clone();
    async move {
        let value: Value = params.parse()
            .map_err(|_| jsonrpc_core::Error::invalid_params("Invalid parameters"))?;

        let name = value.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| jsonrpc_core::Error::invalid_params("Missing 'name'"))?;

        let args = value.get("arguments")
            .cloned()
            .unwrap_or(json!({}));

        let result = reg.call_tool(name, args).await
            .map_err(|e| {
                let mut err = jsonrpc_core::Error::invalid_request();
                err.message = e.to_string();
                err
            })?;

        serde_json::to_value(result)
            .map_err(|_| jsonrpc_core::Error::internal_error())
    }
});
```

**Request Format**:
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "read_file",
    "arguments": {
      "path": "/tmp/test.txt"
    }
  },
  "id": 1
}
```

### 3. `server/identity` - Server Fingerprint

Returns a stable UUID that identifies the server's tool configuration.

**Purpose**:
- Same UUID = Same set of tools available
- Different UUID = Tools have changed (plugins added/removed/updated)
- Enables client caching and connection pooling
- Future: Can track server state changes

**Implementation**: Generated from sorted list of tool names + descriptions
- Deterministic: Same tools → Same UUID
- Stable: UUID only changes when tools change
- Fast: Computed once, cached

**Response Format**:
```json
{
  "server_id": "550e8400-e29b-41d4-a716-446655440000",
  "tools_count": 15,
  "protocol_version": "1.0"
}
```

**Future Extensions**:
- Track server state (future: when server becomes stateful)
- Session management
- Tool version tracking
- Server capability negotiation

## Why This Architecture Works

### Plugin-Aware Binaries

Any binary using the plugin system must:
1. **Query available tools at runtime** via `tools/list`
2. **Read schemas dynamically** from the response
3. **Build UI/CLI dynamically** based on discovered tools

This is why `rpc-client` and `simple-metaphor` exist:
- They don't hardcode tool names
- They discover tools via `tools/list`
- They adapt their interface based on what's available

### Example: Dynamic CLI

```rust
// Pseudocode for dynamic CLI
async fn build_cli() -> Command {
    let tools = rpc_client.call("tools/list").await?;

    let mut cmd = Command::new("dynamic-cli");

    for tool in tools {
        let subcommand = Command::new(tool.name)
            .about(tool.description);

        // Parse inputSchema to add arguments
        for (param_name, param_schema) in tool.input_schema.properties {
            subcommand = subcommand.arg(
                Arg::new(param_name)
                    .required(param_schema.required)
                    .value_parser(parse_from_schema(param_schema))
            );
        }

        cmd = cmd.subcommand(subcommand);
    }

    cmd
}
```

### Protocol Design

**Server Identity Protocol**:
```
1. Client connects
2. Client calls tools/list → Gets all tools
3. Client calls server/identity → Gets server UUID
4. Client caches: UUID → [list of tools]

On reconnect:
1. Client calls server/identity
2. If UUID matches cache: Skip tools/list, use cached tools
3. If UUID differs: Call tools/list, update cache
```

**For now (stateless)**:
- UUID represents tool availability only
- Same UUID = Same tools
- Server has no session state

**Future (stateful)**:
- UUID represents tool availability + server state
- Different UUID = Tools changed OR state changed
- Enables connection pooling, session management

## Implementation Checklist

- [x] `tools/list` - Implemented in main.rs:79-88
- [x] `tools/call` - Implemented in main.rs:91-116
- [x] `server/identity` - Implemented in main.rs:119-159

## Files Referenced

- **JSON-RPC Handler**: `src/main.rs:70-112` (create_rpc_handler function)
- **Plugin Registry**: `src/plugin/registry.rs:32-37` (list_tools method)
- **RPC Types**: `src/rpc/types.rs:6-12` (Tool struct with name, description, inputSchema)

## RPC Client Support

The `rpc-client` binary (`bin/rpc-client`) demonstrates this architecture:

```bash
# Get server identity
rpc-client identity

# List tools
rpc-client list

# List tools with full schemas
rpc-client list --schemas

# Call any tool dynamically
rpc-client call <tool_name> --args '{"param": "value"}'
```

**Key Features**:
- ✅ Discovers tools at runtime via `tools/list`
- ✅ Shows tool schemas with `--schemas` flag
- ✅ Verifies server identity via `server/identity`
- ✅ Can call any tool without hardcoding

## Next Steps

1. ~~Implement `server/identity` tool~~ ✅ Done
2. ~~Update `rpc-client`~~ ✅ Done
3. Future: Cache server identity in client for performance
4. Future: Add `tools/describe <name>` for detailed tool docs
5. Future: Generate CLI dynamically from schemas

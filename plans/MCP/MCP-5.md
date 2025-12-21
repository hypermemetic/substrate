# MCP-5: Schema to MCP Tool Transform

## Metadata
- **blocked_by:** [MCP-2]
- **unlocks:** [MCP-8]
- **priority:** High

## Scope

Create utilities to transform Plexus activation schemas into MCP tool format.

## MCP Tool Format

```json
{
  "name": "claudecode.chat",
  "description": "Chat with a Claude Code session",
  "inputSchema": {
    "type": "object",
    "properties": {
      "session_name": { "type": "string" },
      "query": { "type": "string" }
    },
    "required": ["session_name", "query"]
  }
}
```

## Implementation

```rust
// src/mcp/schema.rs

#[derive(Debug, Serialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Transform Plexus schema to MCP tools list
pub fn plexus_schema_to_mcp_tools(schema: &Schema) -> Vec<McpTool> {
    schema.activations
        .iter()
        .flat_map(|activation| {
            activation.methods.iter().map(move |method| {
                McpTool {
                    // Use dot notation: "namespace.method"
                    name: format!("{}.{}", activation.namespace, method.name),
                    description: method.description.clone(),
                    input_schema: build_input_schema(&method.params),
                }
            })
        })
        .collect()
}

/// Convert Plexus param definitions to JSON Schema
fn build_input_schema(params: &[ParamDef]) -> Value {
    let properties: serde_json::Map<String, Value> = params
        .iter()
        .map(|p| {
            let schema = match &p.schema {
                Some(s) => s.clone(),
                None => infer_schema_from_type(&p.param_type),
            };
            (p.name.clone(), schema)
        })
        .collect();

    let required: Vec<String> = params
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name.clone())
        .collect();

    json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

fn infer_schema_from_type(type_name: &str) -> Value {
    match type_name {
        "String" | "&str" => json!({ "type": "string" }),
        "i32" | "i64" | "u32" | "u64" | "usize" => json!({ "type": "integer" }),
        "f32" | "f64" => json!({ "type": "number" }),
        "bool" => json!({ "type": "boolean" }),
        _ => json!({ "type": "object" }),  // Complex types
    }
}
```

## Files to Create/Modify

- Create `src/mcp/schema.rs`
- Update `src/mcp/mod.rs`

## Acceptance Criteria

- [ ] Transforms all activation methods to MCP tools
- [ ] Uses `namespace.method` naming convention
- [ ] Generates valid JSON Schema for inputSchema
- [ ] Handles optional parameters correctly
- [ ] Unit tests with sample Plexus schema

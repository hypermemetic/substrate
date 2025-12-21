# MCP-8: Tools List Handler

## Metadata
- **blocked_by:** [MCP-3, MCP-5, MCP-7]
- **unlocks:** [MCP-9]
- **priority:** Critical (on critical path)

## Scope

Implement the `tools/list` request handler to enumerate available tools.

## Protocol

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list",
  "params": { "cursor": null }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "tools": [
      {
        "name": "claudecode.chat",
        "description": "Chat with a Claude Code session",
        "inputSchema": { ... }
      }
    ],
    "nextCursor": null
  }
}
```

## Implementation

```rust
// src/mcp/handlers/tools_list.rs

#[derive(Debug, Deserialize)]
pub struct ToolsListParams {
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolsListResult {
    pub tools: Vec<McpTool>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

impl McpInterface {
    pub async fn handle_tools_list(&self, params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        let params: ToolsListParams = serde_json::from_value(params)
            .unwrap_or(ToolsListParams { cursor: None });

        // Get schema from Plexus and transform to MCP tools
        let schema = self.plexus.full_schema();
        let all_tools = plexus_schema_to_mcp_tools(&schema);

        // Handle pagination (50 tools per page)
        let (tools, next_cursor) = self.paginate(all_tools, params.cursor, 50);

        Ok(serde_json::to_value(ToolsListResult {
            tools,
            next_cursor,
        })?)
    }

    fn paginate<T>(
        &self,
        items: Vec<T>,
        cursor: Option<String>,
        page_size: usize,
    ) -> (Vec<T>, Option<String>) {
        let start = cursor
            .and_then(|c| c.parse::<usize>().ok())
            .unwrap_or(0);

        let page: Vec<T> = items.into_iter().skip(start).take(page_size).collect();
        let next = if page.len() == page_size {
            Some((start + page_size).to_string())
        } else {
            None
        };

        (page, next)
    }
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/tools_list.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Lists all activation methods as MCP tools
- [ ] Uses `namespace.method` naming
- [ ] Supports pagination via cursor
- [ ] Requires Ready state
- [ ] Returns valid JSON Schema in inputSchema

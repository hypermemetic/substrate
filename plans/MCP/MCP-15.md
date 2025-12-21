# MCP-15: Prompts (Cone Integration)

## Metadata
- **blocked_by:** [MCP-14]
- **unlocks:** []
- **priority:** Low (optional feature)

## Scope

Expose Cone sessions as MCP prompt templates.

## Protocol

### prompts/list

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "prompts/list"
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "result": {
    "prompts": [
      {
        "name": "cone:research-assistant",
        "description": "Research assistant with web access",
        "arguments": [
          {
            "name": "topic",
            "description": "Research topic",
            "required": true
          }
        ]
      }
    ]
  }
}
```

### prompts/get

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "prompts/get",
  "params": {
    "name": "cone:research-assistant",
    "arguments": {
      "topic": "quantum computing"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "description": "Research assistant session",
    "messages": [
      {
        "role": "user",
        "content": {
          "type": "text",
          "text": "Research the following topic: quantum computing"
        }
      }
    ]
  }
}
```

## Implementation

```rust
// src/mcp/handlers/prompts.rs

impl McpInterface {
    pub async fn handle_prompts_list(&self, _params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        // Get Cone activation if available
        let cone = match self.plexus.get_activation("cone") {
            Some(c) => c,
            None => {
                // Return empty list if Cone not available
                return Ok(serde_json::to_value(PromptsListResult { prompts: vec![] })?);
            }
        };

        // List all cones as prompt templates
        let cones = cone.list().await?;

        let prompts: Vec<McpPrompt> = cones
            .iter()
            .filter(|c| c.system_prompt.is_some())  // Only cones with system prompts
            .map(|c| McpPrompt {
                name: format!("cone:{}", c.name),
                description: c.system_prompt.clone(),
                arguments: vec![
                    PromptArgument {
                        name: "query".into(),
                        description: Some("Initial query to send".into()),
                        required: true,
                    }
                ],
            })
            .collect();

        Ok(serde_json::to_value(PromptsListResult { prompts })?)
    }

    pub async fn handle_prompts_get(&self, params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        let params: PromptsGetParams = serde_json::from_value(params)?;

        // Parse "cone:{name}" format
        let cone_name = params.name
            .strip_prefix("cone:")
            .ok_or_else(|| McpError::PromptNotFound(params.name.clone()))?;

        let cone = self.plexus.get_activation("cone")
            .ok_or(McpError::PromptsNotAvailable)?;

        let config = cone.get_by_name(cone_name).await
            .map_err(|_| McpError::PromptNotFound(params.name.clone()))?;

        // Build initial message from arguments
        let query = params.arguments
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let messages = vec![
            PromptMessage {
                role: "user".into(),
                content: MessageContent::Text {
                    text: query.to_string(),
                },
            }
        ];

        Ok(serde_json::to_value(PromptsGetResult {
            description: config.system_prompt,
            messages,
        })?)
    }
}
```

## Notes

- Prompts are derived from Cone sessions with system prompts
- The `cone:` prefix distinguishes from other potential prompt sources
- Arguments are used to construct the initial user message
- This enables Claude Code to "resume" a cone session context

## Files to Create/Modify

- Create `src/mcp/handlers/prompts.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Lists Cone sessions with system prompts as prompts
- [ ] Returns empty list if Cone not available
- [ ] `prompts/get` returns system context + initial message
- [ ] Uses `cone:` prefix for prompt names
- [ ] Handles missing arguments gracefully

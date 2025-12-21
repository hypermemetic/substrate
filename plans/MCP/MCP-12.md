# MCP-12: Prompts (Cone Integration)

## Metadata
- **blocked_by:** [MCP-11]
- **unlocks:** []
- **priority:** Low (optional feature)

## Scope

Expose Cone sessions as MCP prompt templates.

## Protocol

### prompts/list

**Response:**
```json
{
  "result": {
    "prompts": [{
      "name": "cone:research-assistant",
      "description": "Research assistant with web access",
      "arguments": [{ "name": "query", "required": true }]
    }]
  }
}
```

### prompts/get

**Request:**
```json
{ "method": "prompts/get", "params": { "name": "cone:research-assistant", "arguments": { "query": "quantum computing" } } }
```

**Response:**
```json
{
  "result": {
    "description": "Research assistant session",
    "messages": [{
      "role": "user",
      "content": { "type": "text", "text": "Research the following: quantum computing" }
    }]
  }
}
```

## Implementation

```rust
impl McpInterface {
    pub async fn handle_prompts_list(&self, _params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        let cone = match self.plexus.get_activation("cone") {
            Some(c) => c,
            None => return Ok(json!({ "prompts": [] })),
        };

        let cones = cone.list().await?;
        let prompts: Vec<McpPrompt> = cones.iter()
            .filter(|c| c.system_prompt.is_some())
            .map(|c| McpPrompt {
                name: format!("cone:{}", c.name),
                description: c.system_prompt.clone(),
                arguments: vec![PromptArgument { name: "query".into(), required: true }],
            })
            .collect();

        Ok(serde_json::to_value(PromptsListResult { prompts })?)
    }

    pub async fn handle_prompts_get(&self, params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;
        let params: PromptsGetParams = serde_json::from_value(params)?;

        let cone_name = params.name.strip_prefix("cone:")
            .ok_or(McpError::PromptNotFound(params.name.clone()))?;

        let cone = self.plexus.get_activation("cone")?;
        let config = cone.get_by_name(cone_name).await?;

        let query = params.arguments.get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        Ok(serde_json::to_value(PromptsGetResult {
            description: config.system_prompt,
            messages: vec![PromptMessage {
                role: "user".into(),
                content: MessageContent::Text { text: query.to_string() },
            }],
        })?)
    }
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/prompts.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Lists Cone sessions with system prompts
- [ ] Returns empty list if Cone not available
- [ ] `prompts/get` returns initial message
- [ ] Uses `cone:` prefix for names

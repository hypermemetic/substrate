# MCP-11: Resources (Arbor Integration)

## Metadata
- **blocked_by:** [MCP-9]
- **unlocks:** [MCP-12]
- **priority:** Low (optional feature)

## Scope

Expose Arbor trees and nodes as MCP resources.

## URI Scheme

```
arbor://tree/{tree_id}                     # Tree metadata
arbor://tree/{tree_id}/node/{node_id}      # Specific node
arbor://tree/{tree_id}/head                # Current head node
```

## Protocol

### resources/list

**Response:**
```json
{
  "result": {
    "resources": [
      {
        "uri": "arbor://tree/abc123",
        "name": "Main conversation tree",
        "mimeType": "application/json"
      }
    ]
  }
}
```

### resources/read

**Request:**
```json
{ "method": "resources/read", "params": { "uri": "arbor://tree/abc123/node/def456" } }
```

**Response:**
```json
{
  "result": {
    "contents": [{
      "uri": "arbor://tree/abc123/node/def456",
      "mimeType": "application/json",
      "text": "{\"role\":\"assistant\",\"content\":\"Hello!\"}"
    }]
  }
}
```

## Implementation

```rust
impl McpInterface {
    pub async fn handle_resources_list(&self, _params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        let arbor = self.plexus.get_activation("arbor")
            .ok_or(McpError::ResourcesNotAvailable)?;

        let trees = arbor.tree_list().await?;
        let resources: Vec<McpResource> = trees.iter()
            .flat_map(|t| vec![
                McpResource { uri: format!("arbor://tree/{}", t.id), name: t.name.clone(), mime_type: "application/json".into() },
                McpResource { uri: format!("arbor://tree/{}/head", t.id), name: format!("{} (head)", t.name), mime_type: "application/json".into() },
            ])
            .collect();

        Ok(serde_json::to_value(ResourcesListResult { resources })?)
    }

    pub async fn handle_resources_read(&self, params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;
        let params: ResourcesReadParams = serde_json::from_value(params)?;
        let (tree_id, node_spec) = parse_arbor_uri(&params.uri)?;

        let arbor = self.plexus.get_activation("arbor")?;
        let node_id = match node_spec {
            NodeSpec::Head => arbor.tree_head(&tree_id).await?,
            NodeSpec::Node(id) => id,
        };

        let node = arbor.node_get(&tree_id, &node_id).await?;
        Ok(serde_json::to_value(ResourcesReadResult {
            contents: vec![ResourceContent {
                uri: params.uri,
                mime_type: "application/json".into(),
                text: serde_json::to_string_pretty(&node)?,
            }],
        })?)
    }
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/resources.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Lists Arbor trees as resources
- [ ] Reads nodes by ID or `/head`
- [ ] Returns proper MIME types
- [ ] Returns error if Arbor not available

# MCP-14: Resources (Arbor Integration)

## Metadata
- **blocked_by:** [MCP-11 OR MCP-12]
- **unlocks:** [MCP-15]
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

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "resources/list"
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "resources": [
      {
        "uri": "arbor://tree/abc123",
        "name": "Main conversation tree",
        "mimeType": "application/json"
      },
      {
        "uri": "arbor://tree/abc123/head",
        "name": "Current head",
        "mimeType": "application/json"
      }
    ]
  }
}
```

### resources/read

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "resources/read",
  "params": {
    "uri": "arbor://tree/abc123/node/def456"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "contents": [
      {
        "uri": "arbor://tree/abc123/node/def456",
        "mimeType": "application/json",
        "text": "{\"role\":\"assistant\",\"content\":\"Hello!\"}"
      }
    ]
  }
}
```

## Implementation

```rust
// src/mcp/handlers/resources.rs

impl McpInterface {
    pub async fn handle_resources_list(&self, _params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        // Get Arbor activation if available
        let arbor = self.plexus.get_activation("arbor")
            .ok_or(McpError::ResourcesNotAvailable)?;

        // List all trees
        let trees = arbor.tree_list().await?;

        let resources: Vec<McpResource> = trees
            .iter()
            .flat_map(|tree| {
                vec![
                    McpResource {
                        uri: format!("arbor://tree/{}", tree.id),
                        name: tree.name.clone().unwrap_or_else(|| tree.id.to_string()),
                        mime_type: "application/json".into(),
                    },
                    McpResource {
                        uri: format!("arbor://tree/{}/head", tree.id),
                        name: format!("{} (head)", tree.name.as_deref().unwrap_or(&tree.id.to_string())),
                        mime_type: "application/json".into(),
                    },
                ]
            })
            .collect();

        Ok(serde_json::to_value(ResourcesListResult { resources })?)
    }

    pub async fn handle_resources_read(&self, params: Value) -> Result<Value, McpError> {
        self.state.require_ready()?;

        let params: ResourcesReadParams = serde_json::from_value(params)?;
        let (tree_id, node_spec) = parse_arbor_uri(&params.uri)?;

        let arbor = self.plexus.get_activation("arbor")
            .ok_or(McpError::ResourcesNotAvailable)?;

        let node_id = match node_spec {
            NodeSpec::Head => arbor.tree_head(&tree_id).await?,
            NodeSpec::Node(id) => id,
        };

        let node = arbor.node_get(&tree_id, &node_id).await?;
        let content = serde_json::to_string_pretty(&node)?;

        Ok(serde_json::to_value(ResourcesReadResult {
            contents: vec![ResourceContent {
                uri: params.uri,
                mime_type: "application/json".into(),
                text: content,
            }],
        })?)
    }
}

fn parse_arbor_uri(uri: &str) -> Result<(TreeId, NodeSpec), McpError> {
    // arbor://tree/{tree_id}/node/{node_id}
    // arbor://tree/{tree_id}/head
    // arbor://tree/{tree_id}
    // ...
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/resources.rs`
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Lists all Arbor trees as resources
- [ ] Reads tree metadata
- [ ] Reads specific nodes by ID
- [ ] Reads head node via `/head` suffix
- [ ] Returns proper MIME types
- [ ] Returns error if Arbor not available

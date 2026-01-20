# HANDLE-9: Handle Resolution via Plexus

**blocked_by**: [HANDLE-5, HANDLE-7]
**unlocks**: [HANDLE-6]

## Scope

Document and implement the pattern for resolving external handles through Plexus. This is the ONE valid use case for Plexus in conjunction with Arbor data.

## Context

Arbor trees contain "external" nodes that store `Handle` references to data in other plugins:

```
Tree (owned by Cone "alice")
├── [root]
│   └── [external] 550e8400...@1.0.0::chat:msg-123:user:alice  <- ConeHandle
│       └── [external] 550e8400...@1.0.0::chat:msg-456:assistant:alice
│           └── [external] 123e4567...@1.0.0::execute:cmd-789  <- ClaudeCodeHandle
```

When rendering or processing a tree, you may need to resolve what these handles point to. The handle format tells you:
- `plugin_id`: Which plugin owns this data
- `version`: Schema version for type lookup
- `method`: Which method created it
- `meta`: Plugin-specific identifiers

To actually fetch the data, you need Plexus to route to the owning plugin.

## Acceptance Criteria

- [ ] Document handle resolution flow in architecture docs
- [ ] Implement `plexus.resolve_handle(handle)` method (builds on HANDLE-5)
- [ ] Add example showing Arbor tree traversal with handle resolution
- [ ] Handle resolution returns `serde_json::Value` (schema determined by plugin)
- [ ] Graceful handling of unknown plugins/versions

## Implementation Notes

### Resolution Flow

```
┌─────────────────────────────────────────────────────────────────┐
│  Arbor Tree                                                     │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ External Node                                            │   │
│  │ handle: {plugin_id: X, method: "chat", meta: [msg-123]} │   │
│  └─────────────────────────────────────────────────────────┘   │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  Plexus.resolve_handle(handle)                                  │
│  1. Look up plugin by plugin_id                                 │
│  2. Call plugin.resolve_handle(handle)                          │
│  3. Plugin uses HandleEnum to parse meta and query storage      │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  Resolved Data (JSON)                                           │
│  {                                                              │
│    "id": "msg-123",                                             │
│    "role": "user",                                              │
│    "content": "Hello, how are you?",                            │
│    "created_at": "2024-01-15T10:30:00Z"                         │
│  }                                                              │
└─────────────────────────────────────────────────────────────────┘
```

### Example: Rich Tree Render

```rust
/// Render an Arbor tree with resolved handle content
async fn render_tree_with_content(
    arbor: &ArborStorage,
    plexus: &Plexus,
    tree_id: Uuid,
) -> String {
    let tree = arbor.tree_get(&tree_id).await.unwrap();
    let mut output = String::new();

    for (node_id, node) in &tree.nodes {
        match &node.data {
            NodeData::Text(content) => {
                output.push_str(&format!("[text] {}\n", content));
            }
            NodeData::External(handle) => {
                // THIS is where Plexus is needed - resolving cross-plugin handles
                match plexus.resolve_handle(handle).await {
                    Ok(resolved) => {
                        output.push_str(&format!("[{}::{}] {}\n",
                            handle.method,
                            handle.meta.join(":"),
                            serde_json::to_string_pretty(&resolved).unwrap()
                        ));
                    }
                    Err(e) => {
                        output.push_str(&format!("[unresolved] {} ({})\n",
                            handle.display(), e
                        ));
                    }
                }
            }
        }
    }

    output
}
```

### When to Resolve Handles

1. **Display/Rendering**: Showing tree contents to users
2. **Export**: Serializing conversation history
3. **Search**: Finding messages containing certain text
4. **Analytics**: Computing statistics across conversations

### When NOT to Resolve Handles

1. **Tree structure operations**: Adding/removing nodes, path traversal
2. **Handle creation**: The owning plugin creates handles directly
3. **Ownership checks**: Plugin ID is in the handle itself

### Error Handling

```rust
pub enum HandleResolveError {
    /// Plugin not registered in Plexus
    UnknownPlugin(Uuid),

    /// Plugin doesn't implement resolve_handle
    NotResolvable,

    /// Handle parsing failed (wrong format, missing meta)
    ParseError(HandleParseError),

    /// Data not found in plugin's storage
    NotFound,

    /// Database or I/O error
    StorageError(String),
}
```

## Relationship to Other Tickets

- **HANDLE-5** (Plexus resolve_handle RPC): Provides the infrastructure
- **HANDLE-7** (Arbor usage pattern): Clarifies when Plexus IS needed
- **HANDLE-3/4** (ConeHandle/ClaudeCodeHandle): Define the resolvable handle types

## Estimated Complexity

Medium - Implementation builds on HANDLE-5, focus is on documentation and examples

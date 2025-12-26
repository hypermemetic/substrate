# Handle Resolution Infrastructure

## Current State

Handles are now structured references that arbor nodes use to point to external content:

```rust
struct Handle {
    plugin: String,       // "cone", "bash", "claudecode"
    version: String,      // "1.0.0"
    method: String,       // "chat", "execute" - operation type
    meta: Vec<String>,    // ["msg-{uuid}", "user", "name"] - structured
}
```

**Display format**: `plugin@version::method:meta[0]:meta[1]:...`

Example:
```
cone@1.0.0::chat:msg-8c263a23-2b2a-4f17-8495-bea3fff3ff82:user:haiku-test
```

The `resolve_handle` infrastructure allows any activation to resolve handles it owns, returning the actual content as a stream.

## What Was Implemented

### Phase 1: Handle Type Refactor

Old format had unstructured `identifier`:
```rust
// Before
Handle { source: "cone", identifier: "msg-xxx:user:name", ... }
```

New format separates concerns:
```rust
// After
Handle { plugin: "cone", method: "chat", meta: ["msg-xxx", "user", "name"], ... }
```

Added `Display` and `FromStr` for the wire format.

### Phase 2: Database Migration

Column renames in arbor storage:
```sql
ALTER TABLE nodes RENAME COLUMN handle_source TO handle_plugin;
ALTER TABLE nodes RENAME COLUMN handle_identifier TO handle_method;
ALTER TABLE nodes RENAME COLUMN handle_metadata TO handle_meta;
```

Data migration transforms old format:
```
Old: handle_method = "msg-xxx:user:name"
New: handle_method = "chat", handle_meta = ["msg-xxx", "user", "name"]
```

Runs on startup via `ArborStorage::migrate_handle_columns()`.

### Phase 3: Hub Reference Pattern

Activations need hub access to resolve foreign handles. Pattern uses `OnceLock<Weak<Plexus>>`:

```rust
struct Cone {
    hub: Arc<OnceLock<Weak<Plexus>>>,
}

// In build_plexus()
Arc::new_cyclic(|weak| {
    let cone = Cone::new(...);
    cone.inject_hub(weak.clone());  // Weak before Arc exists
    Plexus::new().register(cone)
})
```

### Phase 4: Activation Trait Extension

Default implementation returns `HandleNotSupported`:
```rust
trait Activation {
    async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
        Err(PlexusError::HandleNotSupported(self.namespace().to_string()))
    }
}
```

### Phase 5: hub-macro Integration

Added `resolve_handle` flag for delegation:

```rust
#[hub_methods(namespace = "cone", resolve_handle)]
impl Cone { ... }

impl Cone {
    async fn resolve_handle_impl(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
        // Actual implementation
    }
}
```

Macro generates:
```rust
async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
    self.resolve_handle_impl(handle).await
}
```

### Phase 6: Cone's resolve_handle_impl

```rust
async fn resolve_handle_impl(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
    let msg_id = handle.meta[0].clone();
    let message = storage.resolve_message_handle(&msg_id).await?;

    yield ConeEvent::ResolvedMessage {
        id: message.id,
        role: message.role,
        content: message.content,
        model: message.model_id,
        name: handle.meta.get(2).cloned().unwrap_or("unknown"),
    };
}
```

### Phase 7: Plexus Dispatch

```rust
impl Plexus {
    pub async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
        let activation = self.activations.get(&handle.plugin)?;
        activation.resolve_handle(handle).await
    }
}
```

## Challenges Encountered

### 1. Cyclic Reference for Hub Access

Activations need `Plexus` to resolve foreign handles, but `Plexus` contains activations.

**Solution**: `Arc::new_cyclic` provides `Weak<Plexus>` during construction. Activations store in `OnceLock<Weak<Plexus>>` and upgrade when needed.

### 2. Macro Can't Know Instance Data

The hub-macro generates `Activation` impl, but `resolve_handle` needs instance-specific storage access.

**Solution**: Added `resolve_handle` flag that generates delegation to `self.resolve_handle_impl()`. The struct implements this method with full access to its fields.

### 3. Migration Ordering Bug

Data migration was inside the column-rename conditional, so it never ran if columns were already renamed (from a previous run).

**Solution**: Moved `migrate_handle_data()` outside the `if` block to always run.

## Shortcuts Taken

1. **No MCP exposure** - `resolve_handle` isn't exposed as an MCP tool yet. Need to add `plexus.resolve_handle` method.

2. **Single message resolution** - Returns one message at a time. Batch resolution would be more efficient for context assembly.

3. **No caching** - Each resolution queries the database. Frequently-accessed messages should be cached.

4. **Cone-only** - Only Cone implements `resolve_handle_impl`. ClaudeCode and future plugins need their own.

## Flow Diagram

```
┌─────────────┐                         ┌──────────────┐
│   arbor     │  tree_render shows      │    Client    │
│   tree      │  handles like:          │              │
│             │  [cone@1.0.0::chat:...] │              │
└──────┬──────┘                         └──────┬───────┘
       │                                       │
       │ get path                              │ resolve_handle(handle)
       ▼                                       ▼
┌──────────────┐                        ┌──────────────┐
│   Arbor      │                        │    Plexus    │
│  Storage     │                        │              │
│              │                        │ dispatch by  │
│ returns      │                        │ handle.plugin│
│ Handle{}     │                        └──────┬───────┘
└──────────────┘                               │
                                               ▼
                                        ┌──────────────┐
                                        │     Cone     │
                                        │              │
                                        │ resolve_     │
                                        │ handle_impl  │
                                        └──────┬───────┘
                                               │
                                               │ query messages table
                                               ▼
                                        ┌──────────────┐
                                        │ ConeStorage  │
                                        │              │
                                        │ returns      │
                                        │ Message{}    │
                                        └──────┬───────┘
                                               │
                                               │ ResolvedMessage event
                                               ▼
                                        ┌──────────────┐
                                        │   Client     │
                                        │              │
                                        │ id, role,    │
                                        │ content, ... │
                                        └──────────────┘
```

## Handle Semantics by Plugin

| Plugin | Method | Meta | Resolves To |
|--------|--------|------|-------------|
| cone | chat | [msg_id, role, name] | Message content |
| claudecode | session | [session_id, msg_idx] | Claude message |
| bash | execute | [cmd_id, exit_code] | Command output |
| arbor | node | [tree_id, node_id] | Node content |

## Files Changed

| File | Change |
|------|--------|
| `src/activations/arbor/types.rs` | Handle struct, Display/FromStr |
| `src/activations/arbor/storage.rs` | Column rename + data migration |
| `src/activations/cone/storage.rs` | `message_to_handle()` new format |
| `src/activations/cone/activation.rs` | `resolve_handle_impl`, flag |
| `src/activations/cone/types.rs` | `ResolvedMessage` event |
| `src/plexus/plexus.rs` | `Plexus::resolve_handle()` |
| `hub-macro/src/parse.rs` | `resolve_handle` flag parsing |
| `hub-macro/src/codegen/activation.rs` | Conditional generation |

## Validation: Tree Render Output

```
└──
    └── [cone@1.0.0::chat:msg-8c263a23-...:user:user]
        └── [cone@1.0.0::chat:msg-b725c8dc-...:assistant:haiku-test]
            └── [cone@1.0.0::chat:msg-5178d591-...:user:user]
                └── ...
```

Handles now show structured format: `plugin@version::method:meta...`

## Future Work

### 1. MCP Tool Exposure (Priority: High)
Add `plexus.resolve_handle` as MCP method:
```rust
#[hub_method]
async fn resolve_handle(&self, handle: String) -> impl Stream<Item = ResolvedContent> {
    let h: Handle = handle.parse()?;
    self.resolve_handle(&h).await
}
```

### 2. Batch Resolution (Priority: Medium)
Resolve multiple handles in parallel for context assembly:
```rust
async fn resolve_handles(&self, handles: Vec<Handle>) -> Vec<ResolvedContent> {
    futures::future::join_all(handles.iter().map(|h| self.resolve_handle(h))).await
}
```

### 3. Resolution Caching (Priority: Medium)
Cache resolved content with TTL:
```rust
struct ResolutionCache {
    cache: DashMap<Handle, (Instant, ResolvedContent)>,
    ttl: Duration,
}
```

### 4. Foreign Handle Resolution (Priority: High)
Use `hub()` to resolve handles from other plugins:
```rust
// In Cone
match handle.plugin.as_str() {
    "cone" => self.resolve_local(handle),
    _ => self.hub().resolve_handle(handle).await,  // Delegate to hub
}
```

### 5. ClaudeCode Implementation (Priority: Medium)
Add `resolve_handle_impl` to ClaudeCode activation for session message resolution.

### 6. Handle Validation (Priority: Low)
Validate handle format before resolution:
```rust
impl Handle {
    fn validate(&self) -> Result<(), HandleError> {
        if self.meta.is_empty() {
            return Err(HandleError::MissingMeta);
        }
        // ...
    }
}
```

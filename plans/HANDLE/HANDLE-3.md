# HANDLE-3: Define ConeHandle Enum

**blocked_by**: [HANDLE-2]
**unlocks**: [HANDLE-5]

## Scope

Define handle types for Cone activation (LLM conversation agents).

## Acceptance Criteria

- [ ] Define `ConeHandle` enum with Cone and Message variants
- [ ] Integrate with existing Cone storage tables
- [ ] Remove manual handle creation in message handling
- [ ] Update Cone activation to use enum-based resolution

## Handle Definition

```rust
#[derive(HandleEnum)]
#[handle(plugin_id = "CONE_PLUGIN_ID", version = "1.0.0")]
pub enum ConeHandle {
    #[handle(method = "cone", table = "cones", key = "id")]
    Cone { cone_id: String },

    #[handle(method = "message", table = "cone_messages", key = "id")]
    Message { message_id: String, role: String },
}
```

## Storage Tables

From `cone/storage.rs`:
- `cones` - cone configuration (model, system prompt, head position)
- `cone_messages` - message content stored for handle resolution

## Current Handle Usage

Cone already creates handles for messages stored in Arbor:
```rust
Handle::new(self.plugin_id(), "1.0.0", "chat")
    .with_meta(vec![msg_uuid.to_string(), role.to_string()])
```

## Migration

1. Add `ConeHandle` enum to `cone/types.rs`
2. Update message storage to use `ConeHandle::Message { ... }.to_handle()`
3. Implement `resolve_handle` using the enum

## Estimated Complexity

Low-Medium - Cone already has handle infrastructure

# HANDLE-6: Documentation and Examples

**blocked_by**: [HANDLE-5, HANDLE-9]
**unlocks**: []

## Scope

Update documentation to cover the HandleEnum system.

## Acceptance Criteria

- [ ] Update Plugin Development Guide with HandleEnum section
- [ ] Add examples in `examples/` directory
- [ ] Update architecture docs with final implementation details
- [ ] Add inline documentation to generated code

## Documentation Updates

### Plugin Development Guide

Add section covering:
1. When to define handles (persistent, addressable data)
2. How to define `HandleEnum`
3. Attribute reference
4. Resolution flow

### Examples

Create `examples/handle_enum.rs`:
```rust
//! Example of HandleEnum derive macro usage

#[derive(HandleEnum)]
#[handle(plugin_id = "EXAMPLE_PLUGIN_ID", version = "1.0.0")]
pub enum ExampleHandle {
    #[handle(method = "item", table = "items", key = "id")]
    Item { item_id: String },
}

// Demonstrate creation, parsing, resolution
```

### Architecture Docs

- Update [HandleEnum Codegen](../docs/architecture/16677960973459046655_handle-enum-codegen.md) with final API
- Update [Arbor Stream Map](../docs/architecture/16677963644192091647_arbor-stream-map.md) showing integration

## Estimated Complexity

Low - documentation only

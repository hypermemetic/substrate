# HANDLE-2: Implement HandleEnum Derive Macro

**blocked_by**: []
**unlocks**: [HANDLE-3, HANDLE-4, HANDLE-5, HANDLE-6]

## Scope

Create `#[derive(HandleEnum)]` proc macro in `hub-macro` crate that generates handle creation, parsing, and storage resolution.

## Acceptance Criteria

- [ ] Macro parses enum-level attributes: `#[handle(plugin_id, version)]`
- [ ] Macro parses variant-level attributes: `#[handle(method, table, key)]`
- [ ] Generates `to_handle(&self) -> Handle` method
- [ ] Generates `TryFrom<&Handle>` implementation
- [ ] Generates `resolve(&self, pool: &SqlitePool) -> Result<Value>` method
- [ ] Proper error types for parsing and resolution failures
- [ ] Unit tests for generated code

## Implementation Notes

### Location

`hub-macro/src/handle_enum.rs` (new file)

### Enum-level attributes

```rust
#[derive(HandleEnum)]
#[handle(plugin_id = "PLUGIN_CONSTANT", version = "1.0.0")]
pub enum MyHandle { ... }
```

### Variant-level attributes

```rust
#[handle(method = "event", table = "events", key = "id")]
Event { event_id: String },
```

### Generated trait

```rust
pub trait HandleResolver {
    fn to_handle(&self) -> Handle;
    async fn resolve(&self, pool: &SqlitePool) -> Result<serde_json::Value, HandleResolveError>;
}
```

### Error types

```rust
pub enum HandleParseError {
    WrongPlugin { expected: Uuid, got: Uuid },
    MissingMeta { index: usize, field: &'static str },
    UnknownMethod(String),
}

pub enum HandleResolveError {
    NotFound,
    DatabaseError(sqlx::Error),
}
```

## Testing

1. Create test enum with multiple variants
2. Verify `to_handle()` produces correct Handle structure
3. Verify `TryFrom<&Handle>` round-trips correctly
4. Verify resolution generates correct SQL (mock or in-memory SQLite)

## Estimated Complexity

Medium-High - proc macro with SQL generation

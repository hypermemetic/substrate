# HANDLE-7: Clarify Arbor Usage Pattern - Direct vs Plexus

**blocked_by**: []
**unlocks**: [HANDLE-8, HANDLE-9]

## Scope

Document and enforce the architectural distinction between:
1. **Direct ArborStorage usage** - For activations that need tree/node operations
2. **Plexus routing** - ONLY for cross-plugin handle resolution

## Context

Arbor is "infrastructure" - a foundational service that other activations depend on directly. Unlike most activations which communicate solely through Plexus, Arbor provides a shared storage layer that activations inject at construction time.

**Anti-pattern** (current state in some tests):
```rust
// Wrong: Routing Arbor operations through Plexus
plexus.route("arbor.tree_render", params).await
```

**Correct pattern**:
```rust
// Right: Direct ArborStorage usage for tree operations
let tree = arbor_storage.tree_get(&tree_id).await?;
let rendered = tree.render();
```

## Acceptance Criteria

- [x] Update architecture docs to explicitly state Arbor usage pattern
  - Created: `docs/architecture/16677890479326575615_arbor-usage-pattern.md`
- [x] Document the two categories of Arbor interaction:
  - Direct: tree/node CRUD, rendering, path traversal
  - Via Plexus: handle resolution (when you have a `Handle` pointing to external data)
- [x] Add code comments in `ArborStorage` clarifying it's meant for direct injection
  - Added comprehensive docstring to `ArborStorage` struct
- [x] Audit existing code for incorrect Plexus routing to Arbor
  - Tests in `src/activations/cone/tests.rs` demonstrate correct pattern
  - No existing code routes tree operations through Plexus
- [x] Update Plugin Development Guide with "Infrastructure Activations" section
  - Added section 7 to Plugin Development Guide

## Implementation Notes

### Why Arbor is Special

1. **Performance**: Tree operations are frequent and latency-sensitive. Plexus routing adds unnecessary serialization/deserialization overhead.

2. **Ownership**: Activations "own" their Arbor trees. Cone creates trees for conversations, ClaudeCode creates trees for sessions. They need direct control.

3. **Atomicity**: Complex tree operations may require multiple steps within a transaction. Direct storage access enables this.

### When Plexus IS Needed for Arbor

The ONLY case where Plexus routing to Arbor makes sense is handle resolution:

```rust
// Arbor tree contains external nodes with handles like:
// Handle { plugin_id: CONE_PLUGIN_ID, method: "chat", meta: ["msg-123", "user", "alice"] }

// To resolve what this handle points to, you MUST go through Plexus
// because Arbor doesn't know how to interpret Cone handles
let resolved = plexus.resolve_handle(handle).await?;
```

This is the subject of HANDLE-9.

### Documentation Updates

Create/update: `docs/architecture/*_arbor-usage-pattern.md`

Content should include:
1. Arbor as infrastructure (like a database layer)
2. Direct injection pattern
3. Comparison table: Direct vs Plexus
4. Examples of correct and incorrect usage

## Estimated Complexity

Low - Primarily documentation and minor code comments

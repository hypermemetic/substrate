# Arbor Per-Method Return Types Design

**Status**: Design Document (Phase 1 - Agent C)
**Scope**: Replace shared `ArborEvent` mega-union with per-method return types
**Updated**: 2025-12-31

## Overview

This document designs the refactoring of the Arbor activation from a shared `ArborEvent` enum (with 27+ variants) to per-method return types. This follows the pattern already established in the Cone activation refactor (see commit `a959ec2`).

The goal is to make the API more type-safe and self-documenting: each method returns only its valid outcome variants, rather than requiring clients to handle impossible cases.

---

## Current State Analysis

### Current ArborEvent Mega-Union

The current `ArborEvent` enum in `/Users/shmendez/dev/controlflow/hypermemetic/substrate/src/activations/arbor/types.rs` has **27 variants**:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum ArborEvent {
    // Tree lifecycle events
    TreeCreated { tree_id: TreeId },
    TreeDeleted { tree_id: TreeId },
    TreeUpdated { tree_id: TreeId },
    TreeList { tree_ids: Vec<TreeId> },

    // Tree reference counting events
    TreeClaimed { tree_id: TreeId, owner_id: String, new_count: i64 },
    TreeReleased { tree_id: TreeId, owner_id: String, new_count: i64 },
    TreeScheduledDeletion { tree_id: TreeId, scheduled_at: i64 },
    TreeArchived { tree_id: TreeId, archived_at: i64 },
    TreeRefs { tree_id: TreeId, refs: ResourceRefs },

    // Node lifecycle events
    NodeCreated { tree_id: TreeId, node_id: NodeId, parent: Option<NodeId> },
    NodeUpdated { tree_id: TreeId, old_id: NodeId, new_id: NodeId },
    NodeDeleted { tree_id: TreeId, node_id: NodeId },
    NodeClaimed { tree_id: TreeId, node_id: NodeId, owner_id: String, new_count: i64 },
    NodeReleased { tree_id: TreeId, node_id: NodeId, owner_id: String, new_count: i64 },
    NodeScheduledDeletion { tree_id: TreeId, node_id: NodeId, scheduled_at: i64 },
    NodeArchived { tree_id: TreeId, node_id: NodeId, archived_at: i64 },
    NodeRefs { tree_id: TreeId, node_id: NodeId, refs: ResourceRefs },

    // Data retrieval events
    TreeData { tree: Tree },
    TreeSkeleton { skeleton: TreeSkeleton },
    NodeData { tree_id: TreeId, node: Node },
    NodeChildren { tree_id: TreeId, node_id: NodeId, children: Vec<NodeId> },
    NodeParent { tree_id: TreeId, node_id: NodeId, parent: Option<NodeId> },
    ContextPath { tree_id: TreeId, path: Vec<NodeId> },
    ContextPathData { tree_id: TreeId, nodes: Vec<Node> },
    ContextHandles { tree_id: TreeId, handles: Vec<Handle> },
    ContextLeaves { tree_id: TreeId, leaves: Vec<NodeId> },

    // Scheduled/Archived queries
    TreesScheduled { tree_ids: Vec<TreeId> },
    NodesScheduled { tree_id: TreeId, node_ids: Vec<NodeId> },
    TreesArchived { tree_ids: Vec<TreeId> },

    // Render
    TreeRender { tree_id: TreeId, render: String },
}
```

### Current Methods and Their Event Usage

| Method | Returns | Success Variant | Error Handling |
|--------|---------|-----------------|----------------|
| `tree_create` | `ArborEvent` | `TreeCreated` | Returns `TreeCreated { tree_id: TreeId::nil() }` |
| `tree_get` | `ArborEvent` | `TreeData` | Returns `TreeList { tree_ids: vec![] }` |
| `tree_get_skeleton` | `ArborEvent` | `TreeSkeleton` | Returns `TreeList { tree_ids: vec![] }` |
| `tree_list` | `ArborEvent` | `TreeList` | Returns `TreeList { tree_ids: vec![] }` |
| `tree_update_metadata` | `ArborEvent` | `TreeUpdated` | Returns `TreeList { tree_ids: vec![] }` |
| `tree_claim` | `ArborEvent` | `TreeClaimed` | Returns `TreeList { tree_ids: vec![] }` |
| `tree_release` | `ArborEvent` | `TreeReleased` | Returns `TreeList { tree_ids: vec![] }` |
| `tree_list_scheduled` | `ArborEvent` | `TreesScheduled` | Returns `TreesScheduled { tree_ids: vec![] }` |
| `tree_list_archived` | `ArborEvent` | `TreesArchived` | Returns `TreesArchived { tree_ids: vec![] }` |
| `node_create_text` | `ArborEvent` | `NodeCreated` | Returns `TreeList { tree_ids: vec![] }` |
| `node_create_external` | `ArborEvent` | `NodeCreated` | Returns `TreeList { tree_ids: vec![] }` |
| `node_get` | `ArborEvent` | `NodeData` | Returns `TreeList { tree_ids: vec![] }` |
| `node_get_children` | `ArborEvent` | `NodeChildren` | Returns `TreeList { tree_ids: vec![] }` |
| `node_get_parent` | `ArborEvent` | `NodeParent` | Returns `TreeList { tree_ids: vec![] }` |
| `node_get_path` | `ArborEvent` | `ContextPath` | Returns `TreeList { tree_ids: vec![] }` |
| `context_list_leaves` | `ArborEvent` | `ContextLeaves` | Returns `TreeList { tree_ids: vec![] }` |
| `context_get_path` | `ArborEvent` | `ContextPathData` | Returns `TreeList { tree_ids: vec![] }` |
| `context_get_handles` | `ArborEvent` | `ContextHandles` | Returns `TreeList { tree_ids: vec![] }` |
| `tree_render` | `ArborEvent` | `TreeRender` | Returns `TreeRender { tree_id, render: "Error: ..." }` |

**Problems identified:**
1. Methods return unrelated variants on error (e.g., `tree_get` returns `TreeList` on error)
2. No explicit `Error` variant - errors are hidden in misused variants
3. Clients must pattern match on all 27 variants even though only 1-2 are valid for a given method
4. API schema shows all variants as possible returns for every method

---

## Proposed New Types

Following the Cone pattern, each method gets its own result type with explicit success and error variants.

### Tree Operations

```rust
/// Result of arbor.tree_create
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeCreateResult {
    TreeCreated { tree_id: TreeId },
    Error { message: String },
}

/// Result of arbor.tree_get
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeGetResult {
    TreeData { tree: Tree },
    Error { message: String },
}

/// Result of arbor.tree_get_skeleton
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeGetSkeletonResult {
    TreeSkeleton { skeleton: TreeSkeleton },
    Error { message: String },
}

/// Result of arbor.tree_list
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeListResult {
    TreeList { tree_ids: Vec<TreeId> },
    Error { message: String },
}

/// Result of arbor.tree_update_metadata
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeUpdateMetadataResult {
    TreeUpdated { tree_id: TreeId },
    Error { message: String },
}

/// Result of arbor.tree_claim
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeClaimResult {
    TreeClaimed {
        tree_id: TreeId,
        owner_id: String,
        new_count: i64
    },
    Error { message: String },
}

/// Result of arbor.tree_release
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeReleaseResult {
    TreeReleased {
        tree_id: TreeId,
        owner_id: String,
        new_count: i64
    },
    Error { message: String },
}

/// Result of arbor.tree_list_scheduled
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeListScheduledResult {
    TreesScheduled { tree_ids: Vec<TreeId> },
    Error { message: String },
}

/// Result of arbor.tree_list_archived
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeListArchivedResult {
    TreesArchived { tree_ids: Vec<TreeId> },
    Error { message: String },
}

/// Result of arbor.tree_render
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeRenderResult {
    TreeRender { tree_id: TreeId, render: String },
    Error { message: String },
}
```

### Node Operations

```rust
/// Result of arbor.node_create_text and arbor.node_create_external
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeCreateResult {
    NodeCreated {
        tree_id: TreeId,
        node_id: NodeId,
        parent: Option<NodeId>
    },
    Error { message: String },
}

/// Result of arbor.node_get
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeGetResult {
    NodeData { tree_id: TreeId, node: Node },
    Error { message: String },
}

/// Result of arbor.node_get_children
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeGetChildrenResult {
    NodeChildren {
        tree_id: TreeId,
        node_id: NodeId,
        children: Vec<NodeId>
    },
    Error { message: String },
}

/// Result of arbor.node_get_parent
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeGetParentResult {
    NodeParent {
        tree_id: TreeId,
        node_id: NodeId,
        parent: Option<NodeId>
    },
    Error { message: String },
}

/// Result of arbor.node_get_path
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeGetPathResult {
    ContextPath { tree_id: TreeId, path: Vec<NodeId> },
    Error { message: String },
}
```

### Context Operations

```rust
/// Result of arbor.context_list_leaves
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextListLeavesResult {
    ContextLeaves { tree_id: TreeId, leaves: Vec<NodeId> },
    Error { message: String },
}

/// Result of arbor.context_get_path
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextGetPathResult {
    ContextPathData { tree_id: TreeId, nodes: Vec<Node> },
    Error { message: String },
}

/// Result of arbor.context_get_handles
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextGetHandlesResult {
    ContextHandles { tree_id: TreeId, handles: Vec<Handle> },
    Error { message: String },
}
```

---

## Type Summary

| Method | New Return Type | Variants |
|--------|-----------------|----------|
| `tree_create` | `TreeCreateResult` | `tree_created`, `error` |
| `tree_get` | `TreeGetResult` | `tree_data`, `error` |
| `tree_get_skeleton` | `TreeGetSkeletonResult` | `tree_skeleton`, `error` |
| `tree_list` | `TreeListResult` | `tree_list`, `error` |
| `tree_update_metadata` | `TreeUpdateMetadataResult` | `tree_updated`, `error` |
| `tree_claim` | `TreeClaimResult` | `tree_claimed`, `error` |
| `tree_release` | `TreeReleaseResult` | `tree_released`, `error` |
| `tree_list_scheduled` | `TreeListScheduledResult` | `trees_scheduled`, `error` |
| `tree_list_archived` | `TreeListArchivedResult` | `trees_archived`, `error` |
| `tree_render` | `TreeRenderResult` | `tree_render`, `error` |
| `node_create_text` | `NodeCreateResult` | `node_created`, `error` |
| `node_create_external` | `NodeCreateResult` | `node_created`, `error` |
| `node_get` | `NodeGetResult` | `node_data`, `error` |
| `node_get_children` | `NodeGetChildrenResult` | `node_children`, `error` |
| `node_get_parent` | `NodeGetParentResult` | `node_parent`, `error` |
| `node_get_path` | `NodeGetPathResult` | `context_path`, `error` |
| `context_list_leaves` | `ContextListLeavesResult` | `context_leaves`, `error` |
| `context_get_path` | `ContextGetPathResult` | `context_path_data`, `error` |
| `context_get_handles` | `ContextGetHandlesResult` | `context_handles`, `error` |

**Total: 15 new types** (some shared, e.g., `NodeCreateResult` for both text and external)

---

## Migration Strategy

### Backwards Compatible Serialization

The key insight is that the JSON wire format remains identical. For example:

**Before (ArborEvent):**
```json
{"type": "tree_created", "tree_id": "550e8400-..."}
```

**After (TreeCreateResult):**
```json
{"type": "tree_created", "tree_id": "550e8400-..."}
```

The `#[serde(tag = "type", rename_all = "snake_case")]` annotation ensures the same JSON structure. Clients consuming the JSON API see no breaking change.

### Legacy ArborEvent Preservation

Following the Cone pattern, keep the legacy `ArborEvent` enum with a deprecation warning:

```rust
/// Events emitted by arbor operations (deprecated - use method-specific types)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
#[deprecated(note = "Use method-specific result types instead")]
pub enum ArborEvent {
    // ... all existing variants ...
}
```

This allows:
1. Existing internal code that uses `ArborEvent` to continue working
2. Gradual migration to new types
3. Clear signal to new code to use specific types

### Phased Implementation

**Phase 1: Add new types (non-breaking)**
- Add all new `*Result` enums to `types.rs`
- Export from `mod.rs`
- No changes to `activation.rs` yet

**Phase 2: Update activation methods (non-breaking)**
- Change method signatures from `-> impl Stream<Item = ArborEvent>` to `-> impl Stream<Item = TreeCreateResult>` etc.
- Update match arms to use new types
- JSON output remains identical

**Phase 3: Update consumers**
- Update any internal code using `ArborEvent` variants
- Update tests to use new types

**Phase 4: Deprecation cleanup (future)**
- After sufficient migration period, consider removing `ArborEvent`

---

## Implementation Notes

### File Organization

All new types go in `/Users/shmendez/dev/controlflow/hypermemetic/substrate/src/activations/arbor/types.rs`:

```rust
// types.rs organization:

// ============================================================================
// Core Types (existing)
// ============================================================================
// ArborId, TreeId, NodeId, NodeType, ResourceState, ResourceRefs, Node, Tree...

// ============================================================================
// Method-specific Result Types (new)
// ============================================================================

// --- Tree Operations ---
pub enum TreeCreateResult { ... }
pub enum TreeGetResult { ... }
// etc.

// --- Node Operations ---
pub enum NodeCreateResult { ... }
pub enum NodeGetResult { ... }
// etc.

// --- Context Operations ---
pub enum ContextListLeavesResult { ... }
// etc.

// ============================================================================
// Legacy Types (deprecated)
// ============================================================================
#[deprecated]
pub enum ArborEvent { ... }

// ============================================================================
// Error Types
// ============================================================================
pub struct ArborError { ... }
```

### Export Updates in mod.rs

```rust
// mod.rs
pub use types::{
    // Core types
    ArborError, Node, NodeId, NodeType, ResourceRefs, ResourceState, Tree,
    TreeId, TreeSkeleton,

    // Method result types (new)
    TreeCreateResult, TreeGetResult, TreeGetSkeletonResult, TreeListResult,
    TreeUpdateMetadataResult, TreeClaimResult, TreeReleaseResult,
    TreeListScheduledResult, TreeListArchivedResult, TreeRenderResult,
    NodeCreateResult, NodeGetResult, NodeGetChildrenResult, NodeGetParentResult,
    NodeGetPathResult,
    ContextListLeavesResult, ContextGetPathResult, ContextGetHandlesResult,

    // Legacy (deprecated)
    #[allow(deprecated)]
    ArborEvent,
};
```

### Activation.rs Changes Example

Before:
```rust
async fn tree_create(...) -> impl Stream<Item = ArborEvent> + Send + 'static {
    stream! {
        match storage.tree_create(metadata, &owner_id).await {
            Ok(tree_id) => yield ArborEvent::TreeCreated { tree_id },
            Err(e) => {
                eprintln!("Error creating tree: {}", e.message);
                yield ArborEvent::TreeCreated { tree_id: TreeId::nil() };
            }
        }
    }
}
```

After:
```rust
async fn tree_create(...) -> impl Stream<Item = TreeCreateResult> + Send + 'static {
    stream! {
        match storage.tree_create(metadata, &owner_id).await {
            Ok(tree_id) => yield TreeCreateResult::TreeCreated { tree_id },
            Err(e) => yield TreeCreateResult::Error { message: e.message },
        }
    }
}
```

### Unused ArborEvent Variants

Some `ArborEvent` variants are not currently used by any method:
- `TreeDeleted` - no delete method exposed
- `NodeUpdated` - no update method exposed
- `NodeDeleted` - no delete method exposed
- `NodeClaimed`, `NodeReleased`, `NodeScheduledDeletion`, `NodeArchived`, `NodeRefs` - no node refcount methods exposed
- `TreeScheduledDeletion`, `TreeArchived`, `TreeRefs` - lifecycle events, not direct returns
- `NodesScheduled` - not used

These will be omitted from the new per-method types. They can be added later if those methods are exposed.

---

## Validation Criteria

1. **JSON wire format unchanged**: Same `{"type": "...", ...}` structure
2. **All methods have explicit error handling**: No more hidden errors in misused variants
3. **Schema reflects actual return types**: Each method schema shows only valid variants
4. **Existing tests pass**: Migration doesn't break functionality
5. **Generated TypeScript has correct types**: Each method gets proper union type

---

## References

- Cone refactor commit: `a959ec2 refactor: split ConeEvent into method-specific result types`
- Parallel implementation plan: `16679600499658362623_parallel-implementation-plan.md`
- Cone types.rs: Example of the pattern in action

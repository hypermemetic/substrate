# Arbor

Conversation tree storage. Manages hierarchical node structures where each node
is either inline text or an external reference (Handle) to data owned by another
activation. Used by Orcha, ClaudeCode, and Cone to back agent session history.

---

## Model

A **Tree** contains **Nodes** in a parent-child hierarchy. Nodes are either:

- `Text { content }` — inline string content
- `External { handle }` — lazy reference to data in another activation

Trees have reference counts. When count hits zero, the tree is scheduled for
deletion (7-day hold, then archived, then purged after 30 days).

---

## Hub methods

**Namespace:** `arbor`

### Trees

| Method | Params | Returns |
|--------|--------|---------|
| `tree_create` | `metadata?, owner_id` | `TreeCreated { tree_id, root_node_id }` |
| `tree_get` | `tree_id` | `TreeData { tree }` — full tree with all nodes |
| `tree_get_skeleton` | `tree_id` | `TreeSkeleton` — IDs only, no content |
| `tree_list` | — | `TreeList` — active trees |
| `tree_list_scheduled` | — | `TreesScheduled` — pending deletion |
| `tree_list_archived` | — | `TreesArchived` |
| `tree_update_metadata` | `tree_id, metadata` | `TreeUpdated` |
| `tree_claim` | `tree_id, owner_id, count` | `TreeClaimed` — increment ref count |
| `tree_release` | `tree_id, owner_id, count` | `TreeReleased` — decrement ref count |
| `tree_render` | `tree_id` | `TreeRender` — ASCII art with optional handle resolution |

### Nodes

| Method | Params | Returns |
|--------|--------|---------|
| `node_create_text` | `tree_id, parent?, content, metadata?` | `NodeCreated` |
| `node_create_external` | `tree_id, parent?, handle, metadata?` | `NodeCreated` |
| `node_get` | `tree_id, node_id` | `NodeData` |
| `node_get_children` | `tree_id, node_id` | `NodeChildren` |
| `node_get_parent` | `tree_id, node_id` | `NodeParent` |

### Context (conversation path)

| Method | Params | Returns |
|--------|--------|---------|
| `context_list_leaves` | `tree_id` | `ContextLeaves` — all leaf node IDs |
| `context_get_path` | `tree_id, node_id` | `ContextPathData` — root-to-node with full data |
| `context_get_handles` | `tree_id, node_id` | `ContextHandles` — external handles in path |

---

## Lifecycle

```
Active (ref_count ≥ 1)
  → ScheduledDelete (ref_count = 0, holds for 7 days)
  → Archived (read-only)
  → Purged (after 30 days)
```

`tree_claim` increments the ref count (reactivates if scheduled).
`tree_release` decrements it.

---

## Storage

SQLite tables: `trees`, `tree_refs`, `nodes`, `node_refs`, `node_children`.

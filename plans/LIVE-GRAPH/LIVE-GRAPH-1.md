# Plan: Runtime-Augmentable Graphs + Plan Node

## Overview

Two capabilities, enabled by a small set of primitives:

1. **Any running node can inject new nodes/edges into its own graph.** The lattice
   gains live-graph awareness in `add_node` and `add_edge`: after mutation, readiness
   is re-evaluated and new `NodeReady` events fire naturally into the existing stream.

2. **A `[planner]` node type** runs Claude to produce a ticket file, compiles it,
   and spawns it as a child graph attached to the parent via `parent_graph_id`.
   The child's events stream through the parent's channel. PM can recursively
   introspect the full execution tree.

These are orthogonal. The first is a lattice primitive. The second is built on top of it
and on `NodeSpec::SubGraph`. Both are enabled by the same schema change.

---

## Layer 1 — Lattice

### Schema

**`lattice_graphs` gains one column:**

```
parent_graph_id  TEXT  NULL  REFERENCES lattice_graphs(id)
```

Migration is a single `ALTER TABLE ... ADD COLUMN`. Existing rows get `NULL`, which
means root graph. No backfill required — the semantics are correct without it.

Add an index on `parent_graph_id` for efficient child lookup.

This single column establishes the full tree structure. A graph with
`parent_graph_id IS NULL` is a root. Everything else is a descendant. You can
reconstruct the full tree by recursively following child links.

### New storage methods

- `create_child_graph(parent_id: &str, metadata: Value) → LatticeGraph`
  Wrapper around `create_graph` that sets `parent_graph_id`. Used by `dispatch_plan`
  and by any node that wants to spawn structured child work.

- `get_child_graphs(parent_id: &str) → Vec<LatticeGraph>`
  Direct children only. Used by PM recursion and cancel propagation.

### Modified storage methods

**`add_node` — live-graph awareness**

After inserting the new node row, check the graph's status. If the graph is `Running`,
immediately evaluate whether the new node is ready:

- If it has no predecessor edges: mark it `Ready`, emit `NodeReady` event.
- If it has predecessor edges where all sources are `Complete`: mark it `Ready`,
  emit `NodeReady` event.
- Otherwise: leave as `Pending`; it will be seeded normally when predecessors complete.

Implementation: call `check_and_ready(graph_id, node_id)` after insert (this function
already exists for the gather/join logic — reuse or factor it).

**`add_edge` — live-graph awareness**

After inserting the new edge row, check the graph's status. If the graph is `Running`
and the source node is `Complete`:

- Retroactively deposit the source node's output token(s) on the new edge in
  `lattice_edge_tokens`. Without this, the target node will never receive the token
  and will never become `Ready` for `JoinType::All`.
- Then call `advance_graph(graph_id, source_node_id, ...)` or directly call
  `check_and_ready` for the target node.

This is the critical correctness invariant for dynamic edges: connecting a new
downstream node to an already-complete upstream node must retroactively place tokens.

### Hub method signatures

`lattice::add_node` and `lattice::add_edge` signatures stay the same. The live-graph
behavior is transparent — callers don't need to know whether the graph is running or
not. The storage layer detects this and does the right thing.

### Invariants that are already correct

**`GraphDone` safety:** `advance_graph` checks whether ALL nodes in the graph are
`Complete` or `Failed` before emitting `GraphDone`. Dynamically added nodes start
as `Pending`, which prevents premature closure. No change needed.

**Double-dispatch prevention:** `run_graph_execution` maintains a `dispatched:
HashSet<String>` in memory. New `NodeReady` events for dynamically added nodes
arrive in the stream and are dispatched once. Replay on reconnect is also safe
since the set starts fresh and events are replayed from the log.

### Invariant that breaks

**Percentage tracking:** `total_nodes` is captured once at stream start. When new
nodes are added, the denominator is stale and percentages become optimistic.

Fix: change `total_nodes` to a variable that is re-fetched from the DB each time a
node completes (`graph.count_nodes().await`). The percentage becomes `complete /
current_total` rather than `complete / initial_total`. This is always accurate and
handles both growing and stable graphs.

---

## Layer 2 — OrchaGraph / GraphRuntime

### New `GraphRuntime` method

`create_child_graph(parent_id: &str, metadata: Value) → OrchaGraph`

Wraps `LatticeStorage::create_child_graph`. Returns a new `OrchaGraph` with
`parent_graph_id` set. Used by `dispatch_plan`.

### New `OrchaGraph` method

`add_plan(task: &str) → Result<String, String>`

Analogous to `add_task`, `add_synthesize`, etc. Adds a node with
`OrchaNodeKind::Plan { task }` serialized as the spec data.

---

## Layer 3 — Orcha Types

### `OrchaNodeKind`

Add one variant:

```
Plan { task: String }
```

Tagged `"orcha_type": "plan"`. Serializes identically to `Task` in terms of the
node spec shape — the difference is entirely in dispatch logic.

### `OrchaNodeSpec` (the user-facing input type)

Add one variant:

```
Plan { task: String }
```

Used by `run_graph_definition` and the ticket compiler. Mirrors the existing `Task`
variant.

---

## Layer 4 — Ticket Compiler

Add one new ticket type: `[planner]` → `OrchaNodeSpec::Plan { task }`.

Body format identical to `[agent]`. The `blocked_by` and `validate` metadata
directives work the same way. The `validate` directive for a `[planner]` node
would validate after the entire child graph completes, which is the natural
interpretation.

Example:

```markdown
# ANALYZE: Analyze prism modules [agent]
...

# PLAN: Generate improvement tickets [planner]
blocked_by: [ANALYZE]

You have received a detailed analysis of the prism codebase. Your task is to produce
a ticket file that addresses the issues found. Output only the ticket file, starting
with the first `# TICKET-ID: ...` heading. Do not include any other text.
```

---

## Layer 5 — Graph Runner (`dispatch_plan`)

`dispatch_plan` is a new async function structured in four phases:

**Phase 1 — Plan:** Run a Claude Code session exactly like `dispatch_task`. Collect
the full output text. The session uses the same model, working directory, and loopback
configuration as other task nodes. The planning prompt receives `<prior_work>` context
from resolved input tokens — this is how it sees the outputs of upstream analysis agents.

If Claude produces empty output, fail the node immediately (this is the empty-output
fix applied universally). If Claude produces text that doesn't parse as a valid ticket
file, fail the node with the compile error — do not attempt to recover or retry silently.

**Phase 2 — Compile:** Call `ticket_compiler::compile_tickets(output_text)`. On error,
fail the node with a descriptive message that includes the first few lines of Claude's
output (so the caller can see what Claude actually produced vs. what was expected).

**Phase 3 — Build child graph:** Call `graph_runtime.create_child_graph(parent_id,
metadata)` where `parent_id` is the current graph's ID. Then call
`build_graph_from_definition` using the compiled nodes and edges. Save the ticket map
to PM: `pm.save_ticket_map(&child_graph_id, &id_map)`.

**Phase 4 — Execute child graph:** Call `run_graph_execution` on the child graph with
the same model, working directory, and cancel receiver as the parent. Register the
child graph's cancel sender in the `cancel_registry` so it can be cancelled
independently or via parent cancellation (see cancel propagation below).

Forward all `OrchaEvent`s from the child execution through the parent's `output_tx`
channel. The parent's event stream will contain child events interleaved with its
own. Consumers can distinguish them by the `graph_id` in `Complete`/`Failed` events;
the PM layer provides the structured tree view for querying.

**Output token:** On child graph completion, return
`Token::ok_data({"child_graph_id": "...", "summary": "<first 200 chars of plan>"})`.
The `child_graph_id` in the token is what PM follows for recursive introspection.

On child graph failure: return `Err(format!("Child graph failed: {}", error))`,
which fails the Plan node in the parent. This propagates up through the lattice
normally — the parent graph fails unless there is an error-colored edge routing the
failure somewhere.

---

## Layer 6 — Cancel Propagation

Currently `cancel_graph(graph_id)` sends on the watch channel in `cancel_registry`
for that specific graph. It does not reach child graphs.

**Change:** When `cancel_graph` is called:
1. Cancel the target graph's watch channel (existing behavior).
2. Recursively fetch `get_child_graphs(graph_id)` and cancel each child.
3. Continue recursively.

This is a breadth-first traversal of the graph tree. Cycles cannot exist (graph IDs
are UUIDs generated at creation time, so a graph cannot be its own ancestor).

`dispatch_plan` registers the child graph's cancel sender in `cancel_registry` under
the child's `graph_id`, so recursive cancellation can reach it.

---

## Layer 7 — PM

### `inspect_ticket` change

When the output token of a Plan node contains `child_graph_id`:
- Fetch the child graph's status from `LatticeStorage::get_graph`.
- Fetch the child's ticket map from `pm.get_ticket_map`.
- Build a `child_status` field in the response showing each child ticket's status.

This makes a single `pm inspect_ticket` call on a Plan node give you the full picture:
what the planner produced and how far along the child execution is.

### `graph_status` change

Add optional `recursive: bool` parameter (default `false`).

When `true`: for each ticket in the graph whose output token contains `child_graph_id`,
embed the child's ticket list inline with a nesting indicator. Apply a depth limit of
3 levels to prevent runaway recursion from deeply nested plans.

### `list_graphs` change

Add optional `root_only: bool` parameter (default `true`).

When `true` (default): shows only graphs with `parent_graph_id IS NULL`. This keeps
the default view clean — you see top-level executions, not every child graph.

When `false`: shows the full set, which is useful for debugging.

---

## Interaction design: how a meta-graph runs

Concrete example — analyzing and improving prism:

```
META GRAPH
├─ T1: analyze scanner.py        [agent] ─┐
├─ T2: analyze loc.py            [agent]  │
├─ T3: analyze complexity.py     [agent]  │  all parallel
├─ T4: analyze symbols.py        [agent]  │
├─ T5: analyze imports.py        [agent]  │
├─ T6: analyze smells.py         [agent]  │
├─ T7: analyze cli.py            [agent] ─┘
│
├─ PLAN: generate fix tickets    [planner] ← blocked by T1–T7
│    │    receives all 7 analysis outputs as <prior_work>
│    │    runs Claude → emits ticket file
│    │    compiles ticket file → creates CHILD GRAPH
│    │
│    └── CHILD GRAPH (parent_graph_id = META GRAPH)
│         ├─ FIX-1: fix deep_nesting       [agent] ─┐
│         ├─ FIX-2: fix symbols agg        [agent]  │  parallel
│         ├─ FIX-3: add pyproject.toml     [agent]  │
│         ├─ FIX-4: expose --exclude flag  [agent] ─┘
│         └─ VALIDATE: pytest              [prog]  ← blocked by FIX-1–4
│
└─ REPORT: summarize all changes [agent/synthesize] ← blocked by PLAN
     receives child_graph_id in prior_work context
     can inspect what was built
```

PM view:
- `pm graph_status(META_GRAPH, recursive=true)` shows the full tree
- `pm inspect_ticket(PLAN, META_GRAPH)` shows the child graph status inline
- `pm list_graphs(root_only=true)` shows only META GRAPH, not the child

Event stream (what `subscribe_graph` or the run_tickets caller sees):
- Parent node events: NodeStarted/NodeComplete for T1–T7, PLAN, REPORT
- Child graph events interleaved: NodeStarted/NodeOutput/NodeComplete for FIX-1–4, VALIDATE
- One unified stream, ordered by time

---

## What does NOT change

- `run_tickets` / `run_tickets_async` — no changes; `[planner]` is handled by the
  ticket compiler producing a `Plan` node which `dispatch_node` routes to
  `dispatch_plan`.
- `run_graph_definition` — add `Plan` to the match arm in `dispatch_node` and a
  `Plan` arm in the node-building loop. One line each.
- `LatticeGraph` struct — no changes; `metadata` already holds JSON.
- Token routing, edge conditions, gather logic — no changes.
- Reconnect replay — no changes; child graph events are in the parent's `lattice_events`
  log (forwarded through `output_tx` → stored via the existing event log path). Actually
  this needs verification — see open question below.

---

## Open questions

**Child graph event persistence in parent log:**
Child events forwarded through `output_tx` are emitted to the parent's stream but are
NOT written to the parent graph's `lattice_events` table — they're stored in the
child's own table. On reconnect, the parent stream replays parent events; child events
would need to be re-fetched by watching the child graph separately.

This is acceptable for V1. Document it: on reconnect to a parent stream, child graph
progress must be fetched via `pm graph_status(recursive=true)` rather than replayed
from the parent event log.

**Dynamic augmentation vs. Plan node:**
The live-graph `add_node`/`add_edge` capability is exposed through the existing
`lattice::add_node` / `lattice::add_edge` hub methods. Claude Code sessions with MCP
access to the substrate can call these. This is intentional — it's the "option 2"
escape hatch, but since all nodes are added to graphs tracked by the same lattice,
they remain introspectable. The difference from a Plan node is structure and PM
visibility: Plan nodes are first-class in the ticket format; raw `add_node` calls are
ad-hoc. Both are valid and complementary.

**Planner output quality:**
The `[planner]` node relies on Claude producing a correctly-formatted ticket file.
Claude is good at this but not perfect. The failure mode (compile error → node fails →
parent graph fails) is correct but harsh. A future improvement would be a
`max_plan_retries` parameter that re-runs the planning session with the compile error
as feedback, analogous to `dispatch_validate_with_retry`. Out of scope for this change.

---

## Files to modify

| File | Change |
|------|--------|
| `activations/lattice/storage.rs` | `parent_graph_id` column, `create_child_graph`, `get_child_graphs`, live-graph behavior in `add_node`/`add_edge` |
| `activations/lattice/types.rs` | `LatticeGraph` struct gains `parent_graph_id: Option<String>` |
| `activations/lattice/activation.rs` | `get_graph` / `get_running_graph_ids` / `create` return `parent_graph_id` where relevant |
| `activations/orcha/types.rs` | `OrchaNodeKind::Plan`, `OrchaNodeSpec::Plan` |
| `activations/orcha/graph_runtime.rs` | `create_child_graph`, `add_plan` on `OrchaGraph` |
| `activations/orcha/graph_runner.rs` | `dispatch_plan`, route `Plan` in `dispatch_node`, fix percentage tracking |
| `activations/orcha/ticket_compiler.rs` | `[planner]` type |
| `activations/orcha/activation.rs` | `cancel_graph` recursive propagation, `run_graph_definition` Plan arm |
| `activations/orcha/pm/storage.rs` | `inspect_ticket` child embedding, `graph_status` `recursive` param, `list_graphs` `root_only` param |
| `activations/orcha/pm/activation.rs` | expose new PM params as hub method params |

No new files. No new tables beyond the one column. No changes to the WebSocket/RPC layer.

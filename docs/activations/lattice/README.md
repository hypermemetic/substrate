# Lattice

A colored Petri net DAG execution engine. Manages graph topology, drives node
readiness, routes typed tokens along edges, and emits a durable event log that
supports crash recovery and stream reconnect.

Lattice knows nothing about AI. It's pure orchestration — routing and sequencing.
Orcha sits on top of it.

---

## Execution model

Nodes move through states: `Pending → Ready → Running → Complete | Failed`.

When a node completes it emits a **token** (colored + optional payload) onto its
outbound edges. Edge conditions filter by color. Downstream nodes become `Ready`
when their join condition is satisfied. `Task` and `Scatter` nodes emit `NodeReady`
events for the caller to dispatch; `Gather` nodes execute automatically.

---

## Node types

```rust
enum NodeSpec {
    Task    { data: Value, handle: Option<Handle> }  // caller-dispatched
    Scatter { data: Value, handle: Option<Handle> }  // caller-dispatched, fan-out
    Gather  { strategy: GatherStrategy }             // auto-executed, fan-in
    SubGraph { graph_id: String }                    // nested graph
}
```

`GatherStrategy`: `All` (collect every inbound token) or `First { n }` (first N).

---

## Token model

```rust
struct Token {
    color:   TokenColor,           // Ok | Error | Named { name }
    payload: Option<TokenPayload>, // Data { value } | Handle(h)
}
```

Edge conditions filter which tokens route on that edge (`None` = any color).

**Join types** — how many predecessors must deliver before a node becomes Ready:
- `All` — every inbound edge must have a token (AND-join, default)
- `Any` — one inbound edge is enough (OR-join)

---

## Hub methods

**Namespace:** `lattice`

### Graph

| Method | Params | Returns |
|--------|--------|---------|
| `create` | `metadata: Value` | `CreateResult { graph_id }` |
| `cancel` | `graph_id` | `CancelResult` |
| `list` | — | `ListGraphsResult { graphs }` |
| `get` | `graph_id` | `GetGraphResult { graph, nodes }` |
| `create_child_graph` | `parent_id, metadata` | `CreateChildGraphResult { graph_id }` |
| `get_child_graphs` | `parent_id` | `GetChildGraphsResult { graphs }` |

### Nodes

| Method | Params | Returns |
|--------|--------|---------|
| `add_node` | `graph_id, spec, node_id?` | `AddNodeResult { node_id }` |
| `get_node_inputs` | `graph_id, node_id` | `GetNodeInputsResult { inputs: Vec<Token> }` |

### Edges

| Method | Params | Returns |
|--------|--------|---------|
| `add_edge` | `graph_id, from_node_id, to_node_id, condition?` | `AddEdgeResult` |

### Execution

| Method | Params | Returns |
|--------|--------|---------|
| `execute` | `graph_id, after_seq?` | `Stream<LatticeEventEnvelope>` |
| `node_complete` | `graph_id, node_id, output?` | `NodeUpdateResult` |
| `node_failed` | `graph_id, node_id, error` | `NodeUpdateResult` |

---

## `execute` stream

Long-lived stream of sequenced events. Pass `after_seq` to reconnect without
replaying the full history; all events are durably persisted.

```rust
struct LatticeEventEnvelope {
    seq:   u64,          // monotonically increasing
    event: LatticeEvent,
}

enum LatticeEvent {
    NodeReady  { node_id, spec },
    NodeStarted { node_id },
    NodeDone   { node_id, output },
    NodeFailed { node_id, error },
    GraphDone  { graph_id },
    GraphFailed { graph_id, node_id, error },
}
```

Stream closes on `GraphDone` or `GraphFailed`.

**Reconnect:** pass `after_seq = <last seq received>` — replays everything
after that point, then continues live. No events are lost.

---

## Crash recovery

On startup, any graphs left in `Running` state have stuck `Running` nodes reset
to `Pending`, `Ready` nodes re-emit `NodeReady`, and callers reconnect via
`execute` with replay.

---

## Storage

SQLite tables:

| Table | Purpose |
|-------|---------|
| `lattice_graphs` | Graph state and metadata |
| `lattice_nodes` | Node state, spec, output, join type |
| `lattice_edges` | Edge topology and color conditions |
| `lattice_edge_tokens` | Token delivery log (never deleted) |
| `lattice_events` | Durable ordered event log |

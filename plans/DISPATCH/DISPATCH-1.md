# DISPATCH-1: Thread GraphRuntime and CancelRegistry into dispatch_node [agent]

Add `graph_runtime: Arc<GraphRuntime>` and `cancel_registry: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>>` as parameters to `run_graph_execution` and `dispatch_node` in `src/activations/orcha/graph_runner.rs`.

`pm: Arc<Pm>` is already a parameter of both functions — this follows the same pattern.

## What to add to run_graph_execution

```rust
pub fn run_graph_execution<P: HubContext + 'static>(
    graph: Arc<OrchaGraph>,
    claudecode: Arc<ClaudeCode<P>>,
    arbor_storage: Arc<ArborStorage>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    graph_runtime: Arc<GraphRuntime>,          // NEW
    cancel_registry: CancelRegistry,           // NEW
    model: Model,
    working_directory: String,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    node_to_ticket: HashMap<String, String>,
)
```

In the tokio::spawn block, clone and pass both new params to dispatch_node:
```rust
let gr = graph_runtime.clone();
let cr = cancel_registry.clone();
// ...
let result = dispatch_node(cc, arbor, lb, pm_log, gr, cr, &g, &spec, ...).await;
```

## What to add to dispatch_node

Same two new params after `pm`:
```rust
async fn dispatch_node<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    arbor: Arc<ArborStorage>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    graph_runtime: Arc<GraphRuntime>,          // NEW
    cancel_registry: CancelRegistry,           // NEW
    graph: &OrchaGraph,
    ...
)
```

Pass them through to dispatch_subgraph:
```rust
if let NodeSpec::SubGraph { graph_id } = spec {
    return dispatch_subgraph(claudecode, arbor, loopback_storage, pm, graph_runtime, cancel_registry, graph, graph_id.clone(), ...).await;
}
```

Pass them to the Plan arm:
```rust
OrchaNodeKind::Plan { task } => {
    dispatch_plan(claudecode, arbor, loopback_storage, pm, graph_runtime, cancel_registry, graph, task, resolved_inputs, node_id, model, working_directory, output_tx, cancel_rx, ticket_id).await
}
```

## What to add to dispatch_subgraph

Add the same two params and pass to recursive run_graph_execution call:
```rust
async fn dispatch_subgraph<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    arbor: Arc<ArborStorage>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    graph_runtime: Arc<GraphRuntime>,          // NEW
    cancel_registry: CancelRegistry,           // NEW
    graph: &OrchaGraph,
    ...
)
```

In the recursive call:
```rust
let events = run_graph_execution(child, claudecode, arbor, loopback_storage, pm, graph_runtime, cancel_registry, model, working_directory, cancel_rx, HashMap::new());
```

## Update all callers in activation.rs

There are 5 calls to `graph_runner::run_graph_execution` in `src/activations/orcha/activation.rs`:
- `recover_running_graphs` (line ~240)
- `run_graph` / subscribe-style call (line ~1236)
- `run_tickets` tokio::spawn (line ~1692)
- `run_tickets_async` tokio::spawn (line ~1824)
- `build_and_run_graph_definition` (line ~1991)

For each, `self.graph_runtime` and `self.cancel_registry` are already available on the `Orcha` struct. In the fire-and-forget spawns, clone them before the spawn:
```rust
let graph_runtime = self.graph_runtime.clone();   // already done in most places
let cancel_registry = self.cancel_registry.clone(); // already cloned in most places
```

Also update `build_and_run_graph_definition` signature to accept both params and pass through.

## Type alias

In graph_runner.rs, define a local type alias at the top to avoid the verbose type:
```rust
type CancelRegistry = Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>>;
```

This matches the alias already defined in activation.rs.

## Imports needed in graph_runner.rs

```rust
use super::graph_runtime::GraphRuntime;
```

`GraphRuntime` is already imported (it's how `OrchaGraph` is opened). Verify the import is present. If not, add it.

## Validation

`cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error"` must produce no output.


# L5-IMPL: Implement dispatch_plan in graph_runner.rs [agent]

blocked_by: [L5-WIRE]

Add `async fn dispatch_plan` to `src/activations/orcha/graph_runner.rs` and replace the `Plan` stub in `dispatch_node`.

## Function signature

```rust
async fn dispatch_plan<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    arbor: Arc<ArborStorage>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    graph_runtime: Arc<GraphRuntime>,
    cancel_registry: CancelRegistry,
    graph: &OrchaGraph,
    task: String,
    resolved_inputs: Vec<crate::activations::lattice::ResolvedToken>,
    node_id: &str,
    model: Model,
    working_directory: String,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
) -> Result<Option<NodeOutput>, String>
```

## Phase 1 — Generate ticket source

Use `dispatch_task` to run Claude with the plan prompt. The output text IS the ticket file.

```rust
let ticket_result = dispatch_task(
    claudecode.clone(),
    loopback_storage.clone(),
    pm.clone(),
    task.clone(),
    resolved_inputs,
    node_id,
    model,
    working_directory.clone(),
    &graph.graph_id,
    output_tx.clone(),
    cancel_rx.clone(),
    ticket_id.clone(),
).await?;

let ticket_source = match &ticket_result {
    Some(NodeOutput::Single(token)) => {
        token.payload.as_ref()
            .and_then(|p| match p {
                crate::activations::lattice::TokenPayload::Data { value } => {
                    value.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                }
                _ => None,
            })
            .ok_or_else(|| "Plan task produced no text output".to_string())?
    }
    _ => return Err("Plan task produced no output".to_string()),
};
```

## Phase 2 — Compile tickets

```rust
let compiled = crate::activations::orcha::ticket_compiler::compile_tickets(&ticket_source)
    .map_err(|e| format!("Plan ticket compile error: {}", e))?;
```

## Phase 3 — Build child graph

Use `graph_runtime.create_child_graph` to create a child of the current graph, then manually add nodes and edges (same logic as `build_graph_from_definition` in activation.rs but using `create_child_graph` instead of `create_graph`).

The metadata should include `_plexus_run_config`:
```rust
let child_metadata = serde_json::json!({
    "_plexus_run_config": {
        "model": format!("{:?}", model).to_lowercase(),
        "working_directory": working_directory,
    },
    "parent_graph_id": graph.graph_id,
    "plan_node_id": node_id,
});

let child_graph = graph_runtime
    .create_child_graph(&graph.graph_id, child_metadata)
    .await
    .map_err(|e| format!("Failed to create child graph: {}", e))?;

let child_graph_id = child_graph.graph_id.clone();

// Add nodes
let mut id_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
for crate::activations::orcha::activation::OrchaNodeDef { id, spec } in compiled.nodes {
    // ... add each node to child_graph same as build_graph_from_definition
}

// Add edges
for crate::activations::orcha::activation::OrchaEdgeDef { from, to } in compiled.edges {
    // ...
}
```

NOTE: `OrchaNodeDef` and `OrchaEdgeDef` are defined in `activation.rs`. They may need to be moved to `types.rs` or made `pub(super)` so graph_runner.rs can use them, OR the node-building logic can be extracted into a helper on `OrchaGraph`/`GraphRuntime`.

**Preferred approach**: add a `build_child_graph` method to `GraphRuntime` in `graph_runtime.rs` that accepts `parent_id, metadata, nodes: Vec<OrchaNodeDef>, edges: Vec<OrchaEdgeDef>` and returns `(String, HashMap<String, String>)`. This keeps activation.rs's `build_graph_from_definition` as just a thin wrapper.

Alternatively, make `OrchaNodeDef`, `OrchaEdgeDef`, and `build_graph_from_definition` pub and import them in graph_runner.rs.

**Simplest**: move `OrchaNodeDef` and `OrchaEdgeDef` from activation.rs to types.rs, export them, and call `build_graph_from_definition` (made pub) from dispatch_plan.

Save ticket map and source:
```rust
let node_to_ticket: HashMap<String, String> = id_map
    .iter()
    .map(|(ticket, node)| (node.clone(), ticket.clone()))
    .collect();

pm.save_ticket_map(&child_graph_id, &id_map).await
    .map_err(|e| format!("Failed to save ticket map: {}", e))?;
pm.save_ticket_source(&child_graph_id, &ticket_source).await
    .map_err(|e| format!("Failed to save ticket source: {}", e))?;
```

## Phase 4 — Execute child graph

Register a cancel token for the child graph (so it can be cancelled via cancel_graph):
```rust
let (child_cancel_tx, child_cancel_rx) = tokio::sync::watch::channel(false);
cancel_registry.lock().await.insert(child_graph_id.clone(), child_cancel_tx);
```

Also propagate parent cancellation: if the parent cancel_rx fires, also cancel the child. Use a tokio::select in a spawned task or just pass `cancel_rx` as the child's cancel.

Actually the simplest correct approach: pass the parent's `cancel_rx` as the child's cancel receiver. If the parent is cancelled, the child execution also stops:
```rust
let child_arc = Arc::new(graph_runtime.open_graph(child_graph_id.clone()));
let events = run_graph_execution(
    child_arc,
    claudecode,
    arbor,
    loopback_storage,
    pm,
    graph_runtime,
    cancel_registry.clone(),
    model,
    working_directory,
    cancel_rx,         // parent's cancel propagates to child
    node_to_ticket,
);
tokio::pin!(events);
```

Forward child events to the parent's output_tx:
```rust
while let Some(event) = events.next().await {
    match &event {
        OrchaEvent::Complete { .. } => {
            cancel_registry.lock().await.remove(&child_graph_id);
            return Ok(Some(NodeOutput::Single(Token::ok_data(
                serde_json::json!({ "child_graph_id": child_graph_id }),
            ))));
        }
        OrchaEvent::Failed { error, .. } => {
            cancel_registry.lock().await.remove(&child_graph_id);
            return Err(format!("Plan child graph failed: {}", error));
        }
        _ => {
            let _ = output_tx.send(event);
        }
    }
}
cancel_registry.lock().await.remove(&child_graph_id);
Err("Plan child graph stream ended without completion".to_string())
```

## Wire up in dispatch_node

Replace:
```rust
OrchaNodeKind::Plan { .. } => {
    Err("Plan nodes are not yet implemented in this build".to_string())
}
```

With:
```rust
OrchaNodeKind::Plan { task } => {
    dispatch_plan(claudecode, arbor, loopback_storage, pm, graph_runtime, cancel_registry, graph, task, resolved_inputs, node_id, model, working_directory, output_tx, cancel_rx, ticket_id).await
}
```

## Validation

`cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error"` must produce no output.

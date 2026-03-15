# RUNPLAN: Add orcha.run_plan one-shot hub method [agent]

Add a `run_plan` hub method to `src/activations/orcha/activation.rs`.

Callers currently need four round-trips to run a plan: create_graph → add_plan_node →
run_graph. `run_plan` collapses this into a single streaming call.

## Signature

```rust
#[plexus_macros::hub_method(params(
    task = "Natural-language task — passed directly to Claude as the planning prompt",
    model = "Model for all nodes: opus, sonnet, haiku (default: sonnet)",
    working_directory = "Working directory for task nodes (default: /workspace)"
))]
async fn run_plan(
    &self,
    task: String,
    model: Option<String>,
    working_directory: Option<String>,
) -> impl Stream<Item = OrchaEvent> + Send + 'static
```

## Implementation

```rust
let model_str = model.as_deref().unwrap_or("sonnet").to_string();
let model_enum = match model_str.as_str() {
    "opus" => Model::Opus,
    "haiku" => Model::Haiku,
    _ => Model::Sonnet,
};
let wd = working_directory.unwrap_or_else(|| "/workspace".to_string());
let graph_runtime = self.graph_runtime.clone();
let cancel_registry = self.cancel_registry.clone();
let claudecode = self.claudecode.clone();
let arbor = self.arbor_storage.clone();
let lb = self.loopback.storage();
let pm = self.pm.clone();

stream! {
    // 1. Create graph with run config embedded in metadata.
    let metadata = serde_json::json!({
        "_plexus_run_config": {
            "model": model_str,
            "working_directory": wd,
        }
    });
    let graph = match graph_runtime.create_graph(metadata).await {
        Ok(g) => Arc::new(g),
        Err(e) => { yield OrchaEvent::Failed { session_id: String::new(), error: e }; return; }
    };
    let graph_id = graph.graph_id.clone();

    // 2. Add a single Plan node.
    let node_id = match graph.add_plan(task.clone()).await {
        Ok(id) => id,
        Err(e) => { yield OrchaEvent::Failed { session_id: graph_id, error: e }; return; }
    };

    // 3. Persist ticket map + source.
    let ticket_map: std::collections::HashMap<String, String> =
        [("plan".to_string(), node_id.clone())].into_iter().collect();
    let _ = pm.save_ticket_map(&graph_id, &ticket_map).await;
    let _ = pm.save_ticket_source(&graph_id, &task).await;

    // 4. Register cancel token.
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    cancel_registry.lock().await.insert(graph_id.clone(), cancel_tx);

    // 5. Execute.
    let node_to_ticket: std::collections::HashMap<String, String> =
        [(node_id, "plan".to_string())].into_iter().collect();
    let execution = graph_runner::run_graph_execution(
        graph, claudecode, arbor, lb, pm,
        graph_runtime, cancel_registry.clone(),
        model_enum, wd, cancel_rx, node_to_ticket,
    );
    tokio::pin!(execution);
    while let Some(event) = execution.next().await {
        yield event;
    }
    cancel_registry.lock().await.remove(&graph_id);
}
```

## Notes

- `graph.add_plan` is already defined in `graph_runtime.rs`.
- `pm.save_ticket_map` / `pm.save_ticket_source` follow the same pattern as
  `run_tickets` (search for `save_ticket_map` in activation.rs).
- Use `Arc::new(graph)` because `run_graph_execution` takes `Arc<OrchaGraph>`.
- `graph_runtime.open_graph(graph_id.clone())` may also be used after creation
  instead of keeping the `graph` handle from `create_graph`.
- Look at `run_graph` (around line 1220) and `run_tickets` (around line 1600)
  for the exact clone pattern and imports already in scope.

## Validation

`cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error"` must produce no output.


# WATCHTREE: Add watch_graph_tree recursive observation stream [agent]

blocked_by: [RUNPLAN]

Add `watch_graph_tree` to `src/activations/orcha/activation.rs`.

Like `subscribe_graph` but recursively follows child graphs created by Plan nodes,
multiplexing all events into one stream. Ends only when the ROOT graph completes or
fails.

## Problem

When a Plan node runs, it creates a child graph and executes it.
`subscribe_graph(root_id)` only sees the Plan node completing — all of the child
graph's NodeStarted/NodeComplete/NodeOutput events are invisible to the caller.
`watch_graph_tree` fixes this.

## Signature

```rust
#[plexus_macros::hub_method(params(
    graph_id = "Root graph ID to watch (recursively includes all child graphs)",
    after_seq = "Sequence number for the root graph to resume from (0 or omit)"
))]
async fn watch_graph_tree(
    &self,
    graph_id: String,
    after_seq: Option<u64>,
) -> impl Stream<Item = OrchaEvent> + Send + 'static
```

## Implementation

Use an unbounded mpsc channel so all graph-watcher tasks forward events to a
single receiver. A separate discovery task polls `get_child_graphs` every 500 ms
and subscribes to newly-seen graphs.

```rust
let lattice_storage = self.lattice_storage.clone();
let pm = self.pm.clone();
let graph_runtime = self.graph_runtime.clone();
let root_id = graph_id.clone();

stream! {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<OrchaEvent>();
    let known_ids: Arc<tokio::sync::Mutex<std::collections::HashSet<String>>> =
        Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new()));

    // Helper closure: spawn a watcher for `gid` that forwards events to `tx`.
    // Uses the same translation logic as subscribe_graph.
    let spawn_watcher = {
        let pm = pm.clone();
        let gr = graph_runtime.clone();
        let known_ids = known_ids.clone();
        move |gid: String, tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>| {
            let pm = pm.clone();
            let gr = gr.clone();
            let known_ids = known_ids.clone();
            tokio::spawn(async move {
                let graph = gr.open_graph(gid.clone());
                let node_to_ticket: HashMap<String, String> = pm
                    .get_ticket_map(&gid).await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(t, n)| (n, t))
                    .collect();

                let total_nodes = graph.count_nodes().await.unwrap_or(0);
                let mut complete_nodes: usize = 0;

                fn calc_pct(c: usize, t: usize) -> Option<u32> {
                    if t == 0 { None } else { Some((c as f32 / t as f32 * 100.0) as u32) }
                }

                let stream = graph.watch(None);  // always from beginning for child graphs
                tokio::pin!(stream);
                while let Some(env) = stream.next().await {
                    use crate::activations::lattice::LatticeEvent::*;
                    let evt = match env.event {
                        NodeReady { node_id, .. } => {
                            let ticket_id = node_to_ticket.get(&node_id).cloned();
                            Some(OrchaEvent::NodeStarted {
                                node_id, label: None, ticket_id,
                                percentage: calc_pct(complete_nodes, total_nodes),
                            })
                        }
                        NodeDone { node_id, .. } => {
                            complete_nodes += 1;
                            let ticket_id = node_to_ticket.get(&node_id).cloned();
                            Some(OrchaEvent::NodeComplete {
                                node_id, label: None, ticket_id,
                                output_summary: None,
                                percentage: calc_pct(complete_nodes, total_nodes),
                            })
                        }
                        NodeFailed { node_id, error } => {
                            complete_nodes += 1;
                            let ticket_id = node_to_ticket.get(&node_id).cloned();
                            Some(OrchaEvent::NodeFailed {
                                node_id, label: None, ticket_id, error,
                                percentage: calc_pct(complete_nodes, total_nodes),
                            })
                        }
                        GraphDone { graph_id } => {
                            Some(OrchaEvent::Complete { session_id: graph_id })
                        }
                        GraphFailed { graph_id, node_id, error } => {
                            Some(OrchaEvent::Failed {
                                session_id: graph_id,
                                error: format!("Node {} failed: {}", node_id, error),
                            })
                        }
                        _ => None,
                    };
                    if let Some(e) = evt {
                        // Check if this is a completion/failure and mark graph as terminal
                        if tx.send(e).is_err() { break; }
                    }
                }
                // Remove from known_ids so discovery doesn't re-subscribe if graph reappears
            });
        }
    };

    // Subscribe to root.
    known_ids.lock().await.insert(root_id.clone());
    spawn_watcher(root_id.clone(), tx.clone());

    // Discovery task: poll for new child graphs across all known graphs.
    {
        let lattice = lattice_storage.clone();
        let known = known_ids.clone();
        let tx_disc = tx.clone();
        let spawn_watcher_disc = spawn_watcher.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let current_known: Vec<String> = known.lock().await.iter().cloned().collect();
                for gid in current_known {
                    if let Ok(children) = lattice.get_child_graphs(&gid).await {
                        for child in children {
                            let mut guard = known.lock().await;
                            if !guard.contains(&child.id) {
                                guard.insert(child.id.clone());
                                drop(guard);
                                spawn_watcher_disc(child.id.clone(), tx_disc.clone());
                            }
                        }
                    }
                }
                // Stop discovery when tx is closed (stream consumer dropped).
                if tx_disc.is_closed() { break; }
            }
        });
    }

    // Forward events; only end the stream when the ROOT graph completes/fails.
    while let Some(event) = rx.recv().await {
        let is_root_terminal = matches!(&event,
            OrchaEvent::Complete { session_id } | OrchaEvent::Failed { session_id, .. }
            if session_id == &root_id
        );
        yield event;
        if is_root_terminal { break; }
    }
}
```

## Notes

- `LatticeGraph` (returned by `get_child_graphs`) has a field `id: String`.
  Verify by checking `src/activations/lattice/storage.rs` around `get_child_graphs`.
- `self.lattice_storage` is an `Arc<LatticeStorage>` already on the `Orcha` struct
  (used by `subscribe_graph`). Verify the field name.
- The `spawn_watcher` closure needs `Clone` to be used in both the initial call and
  the discovery task. Either clone it explicitly or move into the discovery task
  and call only once.
- The `after_seq` param for the root graph: pass `after_seq` to the root watcher's
  `graph.watch(after_seq)` call; child graphs always start from `None`.
- NodeOutput/Retrying events are emitted by `run_graph_execution`, not by
  `graph.watch()`. This stream only sees lattice-level events. That is acceptable
  for a reconnectable observation stream — live chunks are out of scope.

## Validation

`cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error"` must produce no output.


# RETRY-1: Add max_retries to OrchaNodeKind and OrchaNodeSpec [agent]

Add `max_retries: Option<u8>` to the `Task`, `Synthesize`, and `Validate` variants
in **both** `OrchaNodeKind` and `OrchaNodeSpec` in
`src/activations/orcha/types.rs`, and thread the field through the graph_runtime
builders.

## Changes to types.rs

```rust
pub enum OrchaNodeKind {
    Task { task: String, #[serde(default)] max_retries: Option<u8> },
    Synthesize { task: String, #[serde(default)] max_retries: Option<u8> },
    Validate { command: String, cwd: Option<String>, #[serde(default)] max_retries: Option<u8> },
    Review { prompt: String },
    Plan { task: String },
}

pub enum OrchaNodeSpec {
    Task { task: String, #[serde(default)] max_retries: Option<u8> },
    Synthesize { task: String, #[serde(default)] max_retries: Option<u8> },
    Validate { command: String, cwd: Option<String>, #[serde(default)] max_retries: Option<u8> },
    Gather { strategy: GatherStrategy },
    Review { prompt: String },
    Plan { task: String },
}
```

`#[serde(default)]` means existing stored nodes without the field deserialize
with `max_retries: None` — fully backwards compatible.

## Changes to graph_runtime.rs

Update `add_task`, `add_synthesize`, `add_validate` signatures to accept `max_retries`:

```rust
pub async fn add_task(&self, task: impl Into<String>, max_retries: Option<u8>) -> Result<String, String>
pub async fn add_synthesize(&self, task: impl Into<String>, max_retries: Option<u8>) -> Result<String, String>
pub async fn add_validate(&self, command: impl Into<String>, cwd: Option<impl Into<String>>, max_retries: Option<u8>) -> Result<String, String>
```

Pass `max_retries` into the `OrchaNodeKind` construction:
```rust
let kind = OrchaNodeKind::Task { task: task.into(), max_retries };
```

Update `build_child_graph` (which already calls `add_task` etc.) to pass
`max_retries` from `OrchaNodeSpec`:
```rust
OrchaNodeSpec::Task { task, max_retries } => graph.add_task(task, max_retries).await,
OrchaNodeSpec::Synthesize { task, max_retries } => graph.add_synthesize(task, max_retries).await,
OrchaNodeSpec::Validate { command, cwd, max_retries } => graph.add_validate(command, cwd, max_retries).await,
```

## Update all callers

All existing calls to `add_task`, `add_synthesize`, `add_validate` in
`activation.rs`, `graph_runner.rs`, and `graph_runtime.rs` must pass `None` as
the new `max_retries` argument. Search for them:
```
grep -n "add_task\|add_synthesize\|add_validate" src/activations/orcha/activation.rs src/activations/orcha/graph_runner.rs src/activations/orcha/graph_runtime.rs
```

## Validation

`cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error"` must produce no output.


# RETRY-2: Implement retry dispatch for Task and Synthesize nodes [agent]

blocked_by: [RETRY-1]

Use the `max_retries` field (added in RETRY-1) to automatically retry Task and
Synthesize nodes that produce empty output or Claude errors, and wire
`dispatch_validate_with_retry` to use the node's own `max_retries` instead of a
hardcoded constant.

## Context

`dispatch_validate_with_retry` in `src/activations/orcha/graph_runner.rs` already
implements a retry-with-fix-upstream loop using `const MAX_RETRIES: usize = 3`.
Task and Synthesize nodes currently fail immediately on empty output or error.
This ticket adds the same safety net for those node types.

## Step 1 — dispatch_validate_with_retry: accept max_retries

Change:
```rust
const MAX_RETRIES: usize = 3;
```
to a parameter:
```rust
async fn dispatch_validate_with_retry<P: HubContext + 'static>(
    ...existing params...
    max_retries: usize,   // ← NEW (was const 3 inside)
) -> Result<Option<NodeOutput>, String>
```

In `dispatch_node`, extract the value from the kind and pass it:
```rust
OrchaNodeKind::Validate { command, cwd, max_retries } => {
    dispatch_validate_with_retry(
        ..., max_retries.unwrap_or(3) as usize  // default 3 preserves existing behaviour
    ).await
}
```

## Step 2 — dispatch_task_with_retry

Add a new function that wraps `dispatch_task` with a simple retry loop:

```rust
async fn dispatch_task_with_retry<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    task: String,
    resolved_inputs: Vec<crate::activations::lattice::ResolvedToken>,
    node_id: &str,
    model: Model,
    working_directory: String,
    graph_id: &str,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
    max_retries: usize,
) -> Result<Option<NodeOutput>, String> {
    let mut last_error: String = "empty output".to_string();

    for attempt in 0..=max_retries {
        if attempt > 0 {
            let _ = output_tx.send(OrchaEvent::Retrying {
                node_id: node_id.to_string(),
                ticket_id: ticket_id.clone(),
                attempt,
                max_attempts: max_retries,
                error: last_error.clone(),
            });
            // Exponential backoff capped at 8 s: 1, 2, 4, 8, 8, ...
            let secs = 1u64 << (attempt - 1).min(3);
            tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
        }

        match dispatch_task(
            claudecode.clone(), loopback_storage.clone(), pm.clone(),
            task.clone(), resolved_inputs.clone(), node_id, model,
            working_directory.clone(), graph_id,
            output_tx.clone(), cancel_rx.clone(), ticket_id.clone(),
        ).await {
            Ok(Some(ref out)) if !is_empty_output(out) => {
                return Ok(Some(out.clone()));
            }
            Ok(_) => {
                last_error = "task produced empty output".to_string();
            }
            Err(e) => {
                last_error = e;
            }
        }
    }

    Err(format!("Task failed after {} attempt(s): {}", max_retries + 1, last_error))
}
```

`is_empty_output` uses the already-defined `output_text` helper:
```rust
fn is_empty_output(output: &NodeOutput) -> bool {
    output_text(output).map(|t| t.trim().is_empty()).unwrap_or(true)
}
```

## Step 3 — dispatch_synthesize_with_retry

Identical wrapper around `dispatch_synthesize`:

```rust
async fn dispatch_synthesize_with_retry<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    arbor: Arc<ArborStorage>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    graph: &OrchaGraph,
    task: String,
    resolved_inputs: Vec<crate::activations::lattice::ResolvedToken>,
    node_id: &str,
    model: Model,
    working_directory: String,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
    max_retries: usize,
) -> Result<Option<NodeOutput>, String>
```

Same loop structure — call `dispatch_synthesize` on each attempt and retry on
empty output or error.

## Step 4 — Wire in dispatch_node

Replace the existing `OrchaNodeKind::Task` and `OrchaNodeKind::Synthesize` arms:

```rust
OrchaNodeKind::Task { task, max_retries } => {
    dispatch_task_with_retry(
        claudecode, loopback_storage, pm, task, resolved_inputs, node_id,
        model, working_directory, &graph.graph_id,
        output_tx, cancel_rx, ticket_id,
        max_retries.unwrap_or(0) as usize,
    ).await
}
OrchaNodeKind::Synthesize { task, max_retries } => {
    dispatch_synthesize_with_retry(
        claudecode, arbor, loopback_storage, pm, graph, task, resolved_inputs,
        node_id, model, working_directory, output_tx, cancel_rx, ticket_id,
        max_retries.unwrap_or(0) as usize,
    ).await
}
```

`max_retries: None` → `unwrap_or(0)` means zero retries by default, preserving
current fail-fast behaviour for nodes that don't opt in.

## Validation

`cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error"` must produce no output.

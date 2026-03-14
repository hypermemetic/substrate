# STREAM-UX: Orcha Streaming API — Production Readiness for UI Consumers

## Overview

Orcha's core execution engine works end-to-end: tickets compile to graphs, graphs execute,
Claude agents run with auto-approval, validation gates downstream work, synthesize nodes
receive prior-work context. The engine is sound.

What doesn't work is *observing* that execution from a UI or an async client. Progress events
carry UUID node IDs instead of ticket IDs, the graph ID is buried in a freetext string, there
is no way to reconnect to a running graph after a network drop, no real-time agent output,
and approval requests have no push mechanism. These gaps are what separates the current system
from one a product UI can be built on.

## Root Cause

`run_tickets` was built as a single long-lived streaming RPC: the caller connects, gets events,
and must stay connected. This works for a CLI (synapse) but fails for any real client because:

1. Network connections drop and there is no reconnect path
2. Progress messages are human-readable strings, not structured data
3. The graph_id — the only handle for reconnection or polling — is not a typed field
4. Execution blocks the RPC stream rather than running as a detached background job

This EPIC has 18 tickets across four dependency layers.

## Dependency Graph

```
Independent (can run in parallel):
  STREAM-1   Strip eprintln! debug noise
  STREAM-8   Authentication — bearer token
  STREAM-14  pm.list_graphs
  STREAM-15  Working directory pre-validation
  STREAM-18  Loopback session routing fix

After STREAM-2:
  STREAM-2   Typed OrchaEvent variants
    ├──► STREAM-3   run_tickets_async (background job)
    │      ├──► STREAM-5   ticket_id in node events
    │      ├──► STREAM-6   Live agent output chunks
    │      ├──► STREAM-7   cancel_graph
    │      └──► STREAM-11  subscribe_approvals (push)
    │             └──► STREAM-12  Auto-approve via notifier
    │             └──► STREAM-17  Review node implementation
    ├──► STREAM-4   subscribe_graph (reconnect)
    ├──► STREAM-9   Retrying events
    ├──► STREAM-10  Output summary in NodeComplete
    └──► STREAM-16  Progress percentage

Independent of streaming:
  STREAM-13  Graph recovery on restart
```

---

# STREAM-1: Strip debug eprintln! from MCP bridge and loopback [agent]

The MCP bridge (`src/mcp/bridge.rs` in plexus-transport) and the loopback activation
(`src/activations/claudecode_loopback/activation.rs`) contain dozens of `eprintln!` statements
added during development. These go to stderr in production, pollute logs, and leak
implementation details.

## What to do

In `/workspace/hypermemetic/plexus-transport/src/mcp/bridge.rs`:
- Replace every `eprintln!(...)` with the equivalent `tracing::debug!(...)` call
- Keep the message content — just change the macro

In `/workspace/hypermemetic/plexus-substrate/src/activations/claudecode_loopback/activation.rs`:
- Replace every `eprintln!(...)` with `tracing::debug!(...)` or `tracing::trace!(...)`

Do not add or remove any logic. Do not change any other files.

validate: grep -r 'eprintln!' /workspace/hypermemetic/plexus-transport/src/mcp/bridge.rs /workspace/hypermemetic/plexus-substrate/src/activations/claudecode_loopback/activation.rs && echo "FAIL: eprintln still present" && exit 1 || echo "PASS: no eprintln"

---

# STREAM-2: Emit typed OrchaEvent variants for graph lifecycle [agent]

blocked_by: []

Progress events from `run_graph_execution` currently emit freetext strings:

```
message: Graph lattice-XXX ready, starting execution   ← graph_id buried in string
message: Node ready: 6a82bd31-...                      ← UUID, no ticket ID
message: Node complete: 6a82bd31-...                   ← UUID, no ticket ID
message: Node failed: 6a82bd31-... — error text        ← UUID, no ticket ID
```

A UI cannot extract structured data from these without string parsing.

## What to do

In `src/activations/orcha/types.rs`, add new variants to `OrchaEvent`:

```rust
GraphStarted { graph_id: String },
NodeStarted { node_id: String, label: Option<String> },
NodeComplete { node_id: String, label: Option<String>, output_summary: Option<String> },
NodeFailed  { node_id: String, label: Option<String>, error: String },
```

In `src/activations/orcha/graph_runner.rs`, replace the freetext `Progress` yields:
- `"Graph … ready, starting execution"` → `OrchaEvent::GraphStarted { graph_id }`
- `"Node ready: …"` → `OrchaEvent::NodeStarted { node_id, label: None }`
- `"Node complete: …"` → `OrchaEvent::NodeComplete { node_id, label: None, output_summary: None }`
- `"Node failed: … — …"` → `OrchaEvent::NodeFailed { node_id, label: None, error }`

Keep the existing `Progress` variant — it is still used by the orchestrator path.

In `src/activations/orcha/activation.rs`, at the start of `run_tickets` (just before
`run_graph_execution`), yield `OrchaEvent::GraphStarted { graph_id: graph_id.clone() }`.
This must be the *first* event so clients can capture the graph_id before any nodes fire.

Ensure `OrchaEvent` derives `JsonSchema` correctly after the new variants are added.
Compile with: `cargo build --package plexus-substrate --features mcp-gateway`

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep -E "^error" | head -5 && exit 1 || exit 0

---

# STREAM-3: Add run_tickets_async — background execution returning graph_id [agent]

blocked_by: [STREAM-2]

`run_tickets` is a blocking stream: the client must stay connected for the entire run.
If the connection drops, the graph keeps running (tokio tasks are detached) but the
client has no handle to reconnect to.

`run_tickets_async` solves this by returning immediately with the graph_id, then running
execution in a detached tokio task. The client uses `pm.graph_status` or `subscribe_graph`
(STREAM-4) to observe progress.

## What to do

In `src/activations/orcha/activation.rs`, add a new hub method `run_tickets_async`:

```rust
#[plexus_macros::hub_method(params(
    tickets = "Raw ticket file content",
    metadata = "Arbitrary JSON metadata",
    model = "Model: opus, sonnet, haiku (default: sonnet)",
    working_directory = "Working directory (default: /workspace)"
))]
async fn run_tickets_async(
    &self,
    tickets: String,
    metadata: Value,
    model: Option<String>,
    working_directory: Option<String>,
) -> impl Stream<Item = OrchaEvent> + Send + 'static {
    ...
}
```

Implementation:
1. Compile tickets with `ticket_compiler::compile_tickets`
2. Build the graph with `build_graph_from_definition`
3. Save the ticket map with `pm.save_ticket_map`
4. Yield `OrchaEvent::GraphStarted { graph_id }` immediately
5. `tokio::spawn` the `run_graph_execution` call (detached — drop the stream)
6. Return (stream ends after the single GraphStarted event)

The spawned task should log errors via `tracing::error!` but not propagate them (the
caller is gone).

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-4: Add subscribe_graph — reconnectable event stream from sequence [agent]

blocked_by: [STREAM-2]

After `run_tickets_async` returns a graph_id, the client needs a way to observe execution
events. The lattice already stores all events with sequence numbers and supports replay
via `execute_stream(storage, graph_id, after_seq)`. This ticket exposes that as a first-class
Orcha hub method.

## What to do

In `src/activations/orcha/activation.rs`, add:

```rust
#[plexus_macros::hub_method(params(
    graph_id = "Lattice graph ID from run_tickets_async or build_tickets",
    after_seq = "Sequence number to resume from (0 or omit to start from beginning)"
))]
async fn subscribe_graph(
    &self,
    graph_id: String,
    after_seq: Option<u64>,
) -> impl Stream<Item = OrchaEvent> + Send + 'static {
    ...
}
```

Implementation:
- Open the graph with `self.graph_runtime.open_graph(graph_id)`
- Call `graph.watch(after_seq)` to get the lattice event stream
- Map `LatticeEventEnvelope` → `OrchaEvent` using the same logic as `run_graph_execution`:
  - `NodeReady` → `NodeStarted`
  - `NodeDone` → `NodeComplete`
  - `NodeFailed` → `NodeFailed`
  - `GraphDone` → `Complete`
  - `GraphFailed` → `Failed`
- Do NOT dispatch nodes — this is observation only, execution is already running

The client workflow becomes:
```
graph_id = run_tickets_async(...)  # fires and forgets
events   = subscribe_graph(graph_id, after_seq=0)  # observe
# on disconnect: subscribe_graph(graph_id, after_seq=last_seen_seq)
```

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-5: Include ticket_id in node events via pm id_map [agent]

blocked_by: [STREAM-3]

`NodeStarted`, `NodeComplete`, and `NodeFailed` events carry a `node_id` (UUID) and an
optional `label`. A UI needs the ticket ID (`CALC-1`, `STREAM-2`, etc.) to label nodes
correctly without doing per-event lookups.

The pm id_map (ticket_id → node_id) is saved after `build_graph_from_definition` in
both `run_tickets` and `run_tickets_async`. The reverse map (node_id → ticket_id) needs
to be passed into `run_graph_execution` so it can annotate events.

## What to do

1. Add `ticket_id: Option<String>` to `OrchaEvent::NodeStarted`, `NodeComplete`, `NodeFailed`

2. Add a `node_to_ticket: HashMap<String, String>` parameter to `run_graph_execution`
   (the reverse of the pm id_map)

3. In `run_graph_execution`, on each node event, look up the node_id in `node_to_ticket`
   and populate `ticket_id`

4. In `run_tickets`, `run_tickets_async`, and `build_and_run_graph_definition`, construct
   the reverse map from the id_map returned by `build_graph_from_definition` and pass it
   through

5. In `subscribe_graph` (STREAM-4), `node_to_ticket` cannot be known without the pm
   storage; load it via `pm_storage.get_ticket_map(graph_id)` and invert it there

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-6: Stream live Claude output during node execution [agent]

blocked_by: [STREAM-3]

When an agent node is running, the UI shows nothing until it completes. Claude may run
for 30–90 seconds writing code, but the operator sees only a spinner. This ticket wires
live `ChatEvent::Content` chunks into the OrchaEvent stream so UIs can render a live
output pane.

## What to do

Add a new OrchaEvent variant in `types.rs`:

```rust
NodeOutput { node_id: String, ticket_id: Option<String>, chunk: String },
```

In `graph_runner.rs`, `dispatch_task` accumulates output via `ChatEvent::Content` into
`output_text`. Change this: for each `ChatEvent::Content { text }`, in addition to
appending to `output_text`, send the chunk through a `tokio::sync::mpsc::Sender<OrchaEvent>`.

The sender must be threaded from `run_graph_execution` into the spawned per-node task and
ultimately into `dispatch_task`. Use an `mpsc::channel` created in `run_graph_execution`;
the receiver is selected alongside the lattice event stream in the main loop.

Suggested channel signature:
```rust
let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel::<OrchaEvent>();
```

In the main `while let Some(envelope) = event_stream.next().await` loop, add a
`tokio::select!` branch draining `output_rx` and yielding those events.

`dispatch_task` signature gains `output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>`.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-7: Add cancel_graph — stop a running graph and all its agents [agent]

blocked_by: [STREAM-3]

There is no way to stop a running graph. An agent stuck in a loop, a runaway validate
retry, or a mistyped task prompt can only be killed by restarting the substrate. This
ticket adds `orcha.cancel_graph(graph_id)`.

## What to do

**Cancellation token registry** — In `GraphRuntime` or `OrchaActivation`, maintain a
`DashMap<String, CancellationToken>` keyed by graph_id. When `run_graph_execution` starts,
register a `CancellationToken`. Pass a clone into each spawned node task.

Use `tokio_util::CancellationToken` (already in the dependency tree via tokio).

**In each spawned node task** (`dispatch_task`, `dispatch_synthesize`):
- Pass `cancel: CancellationToken`
- Wrap the `claudecode.chat(...)` stream consumption in `tokio::select!`:
  ```rust
  tokio::select! {
      _ = cancel.cancelled() => return Err("Graph cancelled".to_string()),
      event = chat_stream.next() => { ... }
  }
  ```

**New hub method** in `activation.rs`:

```rust
#[plexus_macros::hub_method(params(
    graph_id = "Lattice graph ID to cancel"
))]
async fn cancel_graph(
    &self,
    graph_id: String,
) -> impl Stream<Item = OrchaEvent> + Send + 'static {
    // Look up token, call token.cancel(), yield OrchaEvent::Failed or a new Cancelled variant
}
```

**Cleanup**: Remove the token from the registry when the graph completes or fails normally.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-8: Authentication — bearer token for WebSocket and MCP endpoints [agent]

The substrate exposes two unauthenticated surfaces: the WebSocket JSON-RPC server
(port 4444) and the MCP HTTP server (`/mcp`). Any process on the network can call any
method, including `bash_execute` which runs arbitrary shell commands, or
`claudecode_create` which spawns Claude sessions at the operator's API cost.

## What to do

Add an optional `--api-key` CLI argument to `src/main.rs`. When set, all incoming
connections must supply it.

**WebSocket**: In `plexus-transport/src/websocket.rs`, wrap the jsonrpsee
`ServerBuilder` with a `tower` middleware layer that reads the `Authorization: Bearer
<key>` header on the HTTP upgrade request and rejects with 401 if absent or wrong.
jsonrpsee supports this via `ServerBuilder::set_http_middleware`.

**MCP HTTP**: In `plexus-transport/src/mcp/bridge.rs`, add a Tower middleware layer
to the axum router that validates the same `Authorization: Bearer <key>` header before
passing requests to `StreamableHttpService`. Return HTTP 401 if missing or invalid.

**Configuration**: Pass the key as `Option<String>` through `TransportConfig` /
`TransportServerBuilder`. When `None`, no auth is required (current behaviour).

**Environment variable fallback**: Also read `PLEXUS_API_KEY` from the environment so
container deployments can inject the key without CLI flags.

Do not break existing behaviour when no key is configured — all current tests must pass.

validate: cargo build --package plexus-substrate --features mcp-gateway --package plexus-transport --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-9: Emit OrchaEvent::Retrying during validate-with-retry loop [agent]

blocked_by: [STREAM-2]

When a `[prog]` validate node fails, `dispatch_validate_with_retry` in `graph_runner.rs`
silently re-runs the upstream agent task with an error context prompt, then re-runs the
validate command, up to 3 times. No event is emitted during this process.

A UI watching a validate node that stays in "running" for 3 minutes has no idea if it is:
- On first attempt, Claude is still writing
- On second attempt, first try failed validation
- Completely stuck

## What to do

Add to `OrchaEvent` in `types.rs`:

```rust
Retrying {
    node_id: String,
    ticket_id: Option<String>,
    attempt: usize,
    max_attempts: usize,
    error: String,       // the validation error that triggered the retry
},
```

In `dispatch_validate_with_retry` in `graph_runner.rs`, add an
`output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>` parameter (same channel
introduced in STREAM-6). At the top of each retry loop iteration (when
`error_context.is_some()`), send:

```rust
let _ = output_tx.send(OrchaEvent::Retrying {
    node_id: validate_node_id.to_string(),
    ticket_id: None,
    attempt,
    max_attempts: MAX_RETRIES,
    error: err.clone(),
});
```

Thread the `output_tx` through `dispatch_node` → `dispatch_validate_with_retry`.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-10: Populate output_summary in NodeComplete from agent final text [agent]

blocked_by: [STREAM-2]

`OrchaEvent::NodeComplete` has an `output_summary: Option<String>` field that STREAM-2
adds but leaves as `None`. Without it, a UI must make an extra `pm.inspect_ticket` call
after every completion event just to show what the agent produced.

## What to do

In `graph_runner.rs`, `dispatch_task` returns `NodeOutput::Single(Token::ok_data({"text": ...}))`.
The text is the full accumulated Claude output.

When `complete_node` is called in the `tokio::spawn` block inside `run_graph_execution`,
extract the text from the `NodeOutput` and pass it to the `NodeComplete` event:

```rust
let summary = output.as_ref().and_then(|o| {
    if let NodeOutput::Single(token) = o {
        token.payload.as_ref().and_then(|p| {
            if let TokenPayload::Data { value } = p {
                value.get("text").and_then(|v| v.as_str()).map(|s| {
                    // Truncate to 200 chars for summary
                    s.chars().take(200).collect::<String>()
                })
            } else { None }
        })
    } else { None }
});
```

Then yield `OrchaEvent::NodeComplete { node_id, ticket_id: None, output_summary: summary }`.

This requires that `complete_node` returns the output or it is captured before calling
`g.complete_node`. Restructure the spawn block accordingly.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-11: subscribe_approvals — push stream for pending approval requests [agent]

blocked_by: [STREAM-3]

`orcha.list_pending_approvals` is a snapshot. A UI showing "Claude wants to run Bash —
approve?" must poll it on a timer. This causes either high polling frequency (many wasted
requests when nothing is happening) or high latency before the user sees the prompt.

The loopback storage already has `get_or_create_notifier(session_id)` which returns a
`Arc<Notify>` that fires whenever a new approval arrives for that session. This ticket
exposes it as a streaming hub method.

## What to do

Add to `src/activations/orcha/activation.rs`:

```rust
#[plexus_macros::hub_method(params(
    graph_id = "Graph ID to watch for approval requests",
    timeout_secs = "How long to wait before closing (default: 300)"
))]
async fn subscribe_approvals(
    &self,
    graph_id: String,
    timeout_secs: Option<u64>,
) -> impl Stream<Item = OrchaEvent> + Send + 'static {
    ...
}
```

Implementation:
1. Get `Arc<Notify>` from `self.loopback.storage().get_or_create_notifier(&graph_id)`
2. Loop:
   a. Call `self.loopback.storage().list_pending(Some(&graph_id))`
   b. Yield an `OrchaEvent::ApprovalPending` (new variant, see below) for each one
   c. `tokio::select!` — wait on `notifier.notified()` or timeout
3. Close stream on timeout

New `OrchaEvent` variant:

```rust
ApprovalPending {
    approval_id: String,
    graph_id: String,
    tool_name: String,
    tool_input: serde_json::Value,
    created_at: String,
},
```

The existing `orcha.approve_request` / `orcha.deny_request` are unchanged — they are
still the resolution path.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-12: Replace auto-approve polling with loopback Notify wakeup [agent]

blocked_by: [STREAM-11]

The auto-approve loop added to fix graph execution (`dispatch_task` in `graph_runner.rs`)
spawns a task that polls `list_pending` every 500ms for the lifetime of each agent
invocation. For a graph with 5 nodes each spawning 3 tool calls, that is 15 concurrent
500ms poll loops hitting SQLite simultaneously.

The fix is to use the same `Arc<Notify>` mechanism that STREAM-11 surfaces publicly.
The notifier fires immediately when a new approval is inserted, so the auto-approver
wakes only when there is work to do.

## What to do

In `dispatch_task` in `graph_runner.rs`, replace the current polling loop:

```rust
// Before: 500ms poll
tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = &mut cancel_rx => break,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if let Ok(pending) = lb.list_pending(Some(&gid)).await {
                    for approval in pending { let _ = lb.resolve_approval(...).await; }
                }
            }
        }
    }
});

// After: notify-driven
tokio::spawn(async move {
    let notifier = lb.get_or_create_notifier(&gid);
    loop {
        tokio::select! {
            _ = &mut cancel_rx => break,
            _ = notifier.notified() => {
                if let Ok(pending) = lb.list_pending(Some(&gid)).await {
                    for approval in pending { let _ = lb.resolve_approval(...).await; }
                }
            }
        }
    }
});
```

The notifier is already created in `LoopbackStorage::create_approval` (check that
`notify_one()` is called there — add it if not).

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-13: Graph recovery on substrate restart [agent]

When the substrate process restarts, all running graphs are lost. The lattice DB still
has nodes in `Running` or `Ready` state, but the `tokio::spawn` tasks that drive them
are gone and the in-memory `Arc<Notify>` notifiers are fresh. No one calls
`advance_graph` for those stuck nodes ever again.

## What to do

In `src/builder.rs` (or a new `src/activations/orcha/recovery.rs`), after all
activations are constructed, add a startup recovery pass:

1. Query `lattice_graphs` for graphs with `status = 'running'`
2. For each running graph, query its nodes for any in `status = 'running'` or `'ready'`
3. For nodes stuck in `'running'`: mark them `failed` with error
   `"interrupted: substrate restarted"` via `advance_graph`
4. For nodes stuck in `'ready'`: reset them to `'pending'` so the lattice can re-advance
5. After resetting nodes, call `start_graph` again — this will re-advance the lattice
   and emit new `NodeReady` events, but nobody is watching the event stream yet

The harder part: re-attach a `run_graph_execution` watcher. Add a method on `Orcha`
called `recover_running_graphs()` that is called from `builder.rs` after construction.
It queries for running lattice graphs whose graph_id is also in the pm storage (i.e.,
were started by `run_tickets`), then spawns `run_graph_execution` for each one.

This is a best-effort recovery: graphs that were mid-LLM-call lose that progress and
restart the node from scratch. Idempotency is fine — the lattice de-duplicates.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-14: Add pm.list_graphs — query graphs by project metadata [agent]

blocked_by: []

There is no way to list existing graphs or find them by project name. After calling
`run_tickets_async`, the client gets back a `graph_id`. If the client loses that ID
(page reload, new session, etc.), there is no recovery path short of `lattice_list`.

`lattice_list` returns all graphs but speaks in lattice terms, not ticket vocabulary.
`pm.graph_status` requires you to already know the `graph_id`.

## What to do

In `src/activations/orcha/pm/activation.rs`, add a new method:

```rust
#[plexus_macros::hub_method(params(
    project = "Optional: filter by metadata.project string",
    limit   = "Optional: max results (default 20)"
))]
async fn list_graphs(
    &self,
    project: Option<String>,
    limit: Option<usize>,
) -> impl Stream<Item = PmListGraphsResult> + Send + 'static {
    ...
}
```

Implementation:
1. Call `pm_storage.list_ticket_maps()` — add this method to `PmStorage` / `PmStorageConfig`;
   it returns `Vec<(graph_id, created_at)>` from the `pm_ticket_maps` table
2. For each graph_id, call `lattice_storage.get_graph(graph_id)` to get metadata and status
3. If `project` filter is set, check `graph.metadata["project"] == project`
4. Return `PmListGraphsResult::Ok { graphs: Vec<PmGraphSummary> }` where:

```rust
pub struct PmGraphSummary {
    pub graph_id: String,
    pub status: String,
    pub metadata: serde_json::Value,
    pub ticket_count: usize,
    pub created_at: i64,
}
```

Add `pm_storage.list_ticket_maps()` as a SQL `SELECT graph_id, created_at FROM pm_ticket_maps ORDER BY created_at DESC LIMIT ?`.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-15: Working directory pre-validation before agent dispatch [agent]

blocked_by: []

When `dispatch_task` creates a Claude session, it passes `working_directory` directly.
If the directory does not exist, the Claude CLI process exits immediately with an error
that surfaces as a terse `ChatEvent::Err` or a timeout. The graph marks the node as
failed but the error message gives no hint that the cause was a missing directory.

Worse: `run_tickets` accepts `--working-directory` from the caller with no validation.
A typo silently fails every node in the graph with an opaque error.

## What to do

In `dispatch_task` in `graph_runner.rs`, before calling `claudecode.create(...)`,
check that the working directory exists:

```rust
if !std::path::Path::new(&working_directory).is_dir() {
    return Err(format!(
        "Working directory does not exist: '{}'. \
         Create it before running tickets or pass an existing path.",
        working_directory
    ));
}
```

In `src/activations/orcha/activation.rs`, in `run_tickets` and `run_tickets_async`,
after resolving `wd`, apply the same check and yield `OrchaEvent::Failed` immediately
if it fails — before building the graph.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-16: OrchaEvent progress percentage from graph completion fraction [agent]

blocked_by: [STREAM-2]

Every `OrchaEvent::Progress` has `percentage: null`. A UI progress bar stays empty for
the entire run. The graph's completion fraction is knowable at any point: it is
`complete_nodes / total_nodes`.

## What to do

In `run_graph_execution` in `graph_runner.rs`, maintain two counters:
- `total_nodes: usize` — set once at graph start by calling
  `graph.storage.get_all_nodes(&graph_id)` and counting them
- `complete_nodes: usize` — incremented each time a `NodeDone` event arrives

On each `NodeComplete` / `NodeFailed` yield, set `percentage` to:
```rust
Some((complete_nodes as f32 / total_nodes as f32 * 100.0) as u32)
```

Also populate `percentage` in the `OrchaEvent::NodeStarted` event as the current
fraction (pre-completion) so the UI can show a continuous ramp rather than step jumps.

`graph.storage.get_all_nodes(graph_id)` may not exist yet — add it to `LatticeStorage`
as a simple `SELECT COUNT(*) FROM lattice_nodes WHERE graph_id = ?`.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-17: Implement [review] node type — human-in-the-loop gate [agent]

blocked_by: [STREAM-11]

`OrchaNodeKind::Review` is defined in `types.rs` and handled in `dispatch_node` with:
```rust
OrchaNodeKind::Review { .. } => Err("review nodes are not yet implemented".to_string())
```

It is the last unimplemented node type. A `[review]` node pauses graph execution at that
point and waits for an explicit human approval before the downstream chain can proceed.
This is different from tool-level approval — it gates entire workflow transitions.

## What to do

`Review` node behaviour:
1. When dispatched, emit `OrchaEvent::ApprovalPending` (from STREAM-11) with the review
   prompt as `tool_input`
2. Insert a loopback approval record keyed on the graph_id with `tool_name = "review"`
   and `input = {"prompt": "<review prompt>"}
3. Block (poll `get_approval` every 1s, same as loopback permit) until resolved
4. On approval → return `Ok(Some(NodeOutput::Single(Token::ok())))`
5. On denial → return `Err("Review denied: <message>")`

In the ticket compiler (`ticket_compiler.rs`), add `[review]` as a recognized type tag
that maps to `OrchaNodeSpec::Review` (needs to be added to `types.rs` alongside the
existing `OrchaNodeSpec` variants if not already there).

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

# STREAM-18: Loopback session routing — remove URL query-param dependency [agent]

blocked_by: []

The loopback permit handler extracts `session_id` from `_connection.query.session_id`,
which is injected by the MCP bridge from the HTTP request URL:
`http://127.0.0.1:4444/mcp?session_id=<id>`.

This is a layering violation: the session identity is encoded in the URL because there
was no other way to thread it through the MCP protocol. The bridge parses query params
and injects them as `_connection` fields. This works but is fragile — any MCP client
that does not preserve query strings (e.g., many standard MCP libraries) silently breaks
the routing, and `session_id` falls back to `tool_use_id` → `"unknown"`.

## What to do

The MCP protocol allows servers to include arbitrary metadata in tool definitions.
Use this to make session routing explicit at the MCP layer instead of the URL layer.

**Option A (simpler):** In `executor.rs`, include the `loopback_session_id` in the
`--session-id` Claude CLI argument if Claude Code supports it, removing the URL
dependency entirely. Check if `claude --session-id` is a valid flag in the Claude Code
CLI (`claude --help`). If so, pass it and read it from Claude's tool call metadata
rather than the URL.

**Option B (correct):** Add a `PLEXUS_SESSION_ID` environment variable to the MCP
config that `executor.rs` sets when writing the temp MCP config JSON. In the loopback
permit handler, read `std::env::var("PLEXUS_SESSION_ID")` as a fallback after the
connection metadata. Since the MCP config is written per-session and includes the
session_id in the env block, the loopback handler always has access to it.

Implement Option B. Update `executor.rs` `write_mcp_config_sync` to include:
```json
{ "env": { "PLEXUS_SESSION_ID": "<session_id>" } }
```
Update `loopback/activation.rs` to read `std::env::var("PLEXUS_SESSION_ID")` as a
second fallback after `_connection.query.session_id`.

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0

---

## Implementation Notes

### What the pm module already has

`pm.graph_status`, `pm.what_next`, `pm.inspect_ticket`, `pm.why_blocked` are solid
poll-based endpoints. A UI that polls `pm.graph_status` every 2–3 seconds during execution
can already build a reasonable dashboard with the current code. These do not need to change.

### What a UI needs that pm doesn't provide

- Push: know *when* to poll (currently must poll on a timer)
- Reconnect: after a network drop, resume the event stream from the last seen sequence
- Live output: see what Claude is writing while a node runs
- Approval notifications: "Claude wants to write a file — approve?" without polling
- Cancellation: a stop button

### Testing approach

Each ticket compiles cleanly. Behavioral verification can be done with:

```bash
# Start substrate
cd /workspace/hypermemetic/plexus-substrate
./target/debug/plexus-substrate >> /tmp/substrate.log 2>&1 &

# Run a ticket set and pipe output through jq to verify structure
synapse substrate orcha run_tickets \
  --tickets "$(cat /tmp/tickets.md)" \
  --metadata '{}' --model haiku \
  | jq 'select(.type == "graph_started") | .graph_id'
```

### Sequence diagram: target state after all tickets

```
Client                    Substrate
  │                           │
  │  run_tickets_async(...)   │
  ├──────────────────────────►│
  │  {type:"graph_started",   │  ← STREAM-3
  │   graph_id:"lattice-XXX"} │
  │◄──────────────────────────┤
  │  (stream closes)          │
  │                           │  tokio::spawn → run_graph_execution
  │                           │    dispatch CALC-1 → claudecode.chat()
  │  subscribe_graph(gid, 0)  │
  ├──────────────────────────►│  ← STREAM-4
  │  {type:"node_started",    │
  │   ticket_id:"CALC-1"}     │  ← STREAM-5
  │◄──────────────────────────┤
  │  {type:"node_output",     │
  │   chunk:"def add(a,b):"}  │  ← STREAM-6
  │◄──────────────────────────┤
  │  ... (network drops) ...  │
  │                           │  graph keeps running
  │  subscribe_graph(gid, 14) │  ← reconnect from seq 14
  ├──────────────────────────►│
  │  {type:"node_complete",   │
  │   ticket_id:"CALC-1"}     │
  │◄──────────────────────────┤
  │  {type:"complete",        │
  │   session_id:"lattice-X"} │
  │◄──────────────────────────┤
```

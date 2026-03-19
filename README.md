# Substrate

An AI orchestration server. You describe work as a graph of agents, validators,
and human gates — Substrate runs it, streams events, and handles failures.

The core primitive is **Orcha**: a multi-agent execution engine where each
node in a DAG is a typed agent action. Nodes can run Claude, execute shell
commands, wait for human approval, spawn child graphs, or run a full
contract-first TDD loop. The graph runs until it completes, fails, or reaches
a human gate.

---

## What you can do with it

**Run a ticket plan from markdown:**

```markdown
# TASK-1: Analyze the codebase [agent]
Summarize the architecture of /workspace/myproject

# TASK-2: Write the implementation [agent]
blocked_by: [TASK-1]
Implement the feature described in the analysis above

# TASK-3: Validate [prog]
blocked_by: [TASK-2]
validate: cargo test --package myproject 2>&1
```

```bash
synapse substrate orcha run_tickets_files \
  --ticket_files '["plan.md"]' \
  --model sonnet \
  --working_directory /workspace/myproject
```

Node types: `[agent]` runs Claude, `[agent/synthesize]` runs Claude with upstream
outputs stitched in as context, `[prog]` runs a shell command, `[review]` pauses
for human approval, `[planner]` asks Claude to generate a new ticket plan which
runs as a child graph.

**Build a graph programmatically:**

```bash
synapse substrate orcha create_graph --metadata '{"name":"deploy"}'
# → graph_id: abc123

synapse substrate orcha add_task_node --graph_id abc123 --task "build the binary"
# → node_id: node_1

synapse substrate orcha add_validate_node --graph_id abc123 --command "cargo build --release"
# → node_id: node_2

synapse substrate orcha add_dependency --graph_id abc123 --from_node node_1 --to_node node_2
synapse substrate orcha run_graph --graph_id abc123 --model sonnet
```

**Wait for human approval mid-graph:**

When a `Review` node fires, the graph pauses. Other independent nodes still run.
You get an `ApprovalPending` event, review the context, and approve or deny via
the API. The graph resumes.

```bash
synapse substrate orcha list_pending_approvals --graph_id abc123
synapse substrate orcha approve_request --approval_id xyz --message "looks good"
```

---

## Architecture

Three layers. Each knows only about the layer below it.

```
┌────────────────────────────────────────────────────────┐
│  Orcha                                                 │
│  Multi-agent orchestration. Typed node dispatch,       │
│  ticket compiler, repair loops, human gates.           │
├────────────────────────────────────────────────────────┤
│  Lattice                                               │
│  DAG execution engine. Nodes, edges, tokens.           │
│  Knows nothing about AI — just routing and sequencing. │
├────────────────────────────────────────────────────────┤
│  Plexus RPC                                            │
│  Self-describing, streaming-first RPC protocol.        │
│  Code is schema. No drift. Language-agnostic clients.  │
└────────────────────────────────────────────────────────┘
```

See `docs/architecture/intro-lattice-orcha-tdd.md` for the full walkthrough.

### Orcha node types

```rust
enum OrchaNodeKind {
    Task      { task: String }          // run Claude on this prompt
    Synthesize { task: String }         // Task + upstream outputs stitched in as context
    Validate  { command: String }       // run a shell command; exit 0 → ok token, else error
    Review    { prompt: String }        // pause and wait for human approval via API
    Plan      { task: String }          // Claude produces a ticket plan, spawns as child graph
    // Tdd — planned, not yet implemented
}
```

### Lattice token model

Nodes produce typed tokens that flow along edges:

- **Color**: `Ok` | `Error` | `Named("x")` — controls routing; edges can be conditional
- **Payload**: `Data { value }` | `Handle { method }` — the result or a lazy stream reference
- **JoinType**: `All` (wait for every upstream) | `Any` (fire on first)

Fan-out via `Scatter`. Fan-in via `Gather`. Nested work via `SubGraph`. The
same graph engine that drives simple sequential plans also drives recursive
agent trees.

### Plexus RPC

Substrate exposes everything through Plexus RPC — a protocol where every method
has a runtime JSON Schema, every call returns a stream, and namespaces organize
as a tree. Clients can discover the full API by querying it.

Access via:
- **WebSocket** — `ws://localhost:4444`
- **MCP** — `http://localhost:4444/mcp` (Orcha methods appear as MCP tools)
- **Synapse CLI** — `synapse substrate <namespace> <method> [--param value]`
- **In-process Rust** — `DynamicHub::call(method, params)`

---

## What works today

**Ticket plans execute as parallel agent DAGs.**
Write a markdown file. Agents with no `blocked_by` run simultaneously. Results
flow forward as tokens. A `[agent/validate]` node runs `cargo test` and routes
the exit code — ok token on pass, error token on failure — to whatever you wired
downstream. You watch it all in real time via `subscribe_graph`.

```markdown
# ANALYZE-1: Read the module [agent]
Summarize the architecture of src/activations/orcha/

# ANALYZE-2: Read the tests [agent]
List what is and isn't tested in src/activations/orcha/

# SYNTHESIZE: Identify gaps [agent/synthesize]
blocked_by: [ANALYZE-1, ANALYZE-2]
Given the analysis and test coverage above, list the top 5 untested behaviors.

# VALIDATE: Check it compiles [prog]
blocked_by: [SYNTHESIZE]
validate: cargo check --package plexus-substrate 2>&1
```

ANALYZE-1 and ANALYZE-2 run in parallel. SYNTHESIZE receives both outputs as
`<prior_work>` context. VALIDATE runs the command; if it fails, the node gets
an error token and the graph fails with the output.

The ticket format works but has rough edges: `blocked_by` is the only dependency
syntax, there's no way to express conditional branching in the ticket file itself
(that requires building the graph programmatically), and error handling between
nodes is all-or-nothing unless you wire it manually.

---

**Human approval gates that don't block the graph.**
A `Review` node pauses at a checkpoint. Any independent branches in the same
graph keep running. You get an `ApprovalPending` event, read the context
(whatever Claude produced), and approve or deny via the API. The graph resumes
from exactly that node. Deny it and the node gets an error token — wired
however you want.

This is how you build workflows where a human reviews a spec before code is
written, or reviews a PR before it's merged, without freezing everything else.

---

**A graph can spawn child graphs.**
The `Plan` node type runs Claude, which generates a ticket file, compiles it
into a new graph, and executes it as a child. The child's events stream through
the parent channel. Cancel the parent and the child cancels. `pm graph_status`
with `recursive=true` shows the full tree.

Concrete example from `LIVE-GRAPH-1.md`:

```
META GRAPH
├─ T1–T7: analyze 7 modules simultaneously      [agent]
├─ PLAN: generate fix tickets                   [planner]  ← blocked by T1–T7
│    receives all 7 analyses as prior_work
│    runs Claude → emits a ticket file
│    compiles it → spawns CHILD GRAPH
│
└─ CHILD GRAPH
     ├─ FIX-1 through FIX-4: fixes              [agent]  (parallel)
     └─ VALIDATE: pytest                        [validate]  ← blocked by fixes
```

The meta graph never knew upfront how many fixes there would be. Claude decided.

---

**The whole API is MCP tools.**
Every Orcha method — `run_graph`, `list_pending_approvals`, `approve_request`,
`subscribe_graph` — is an MCP tool. Claude Desktop, Claude Code, or any agent
with MCP access can orchestrate other agents. An agent running inside a graph
can call `orcha.add_task_node` to inject new work into its own graph. This is
how you get graphs that rewrite themselves at runtime.

---

**What else ships today:**

| | |
|---|---|
| Lattice DAG engine | Scatter/Gather, colored token routing, edge conditions, JoinType All/Any |
| Child graphs | Nested graphs with parent-linked cancel propagation and PM tree introspection |
| Graph recovery | Reconnect to a running graph without double-dispatching any node |
| Model selection | Opus/Sonnet/Haiku per graph — heavy reasoning vs. fast validation loops |
| Loopback tool approval | Claude sessions request tool use; routed through the approval API |
| Arbor | Conversation tree storage backing agent session history |
| Synapse CLI | `synapse substrate orcha run_tickets_files --ticket_files '[...]'` |
| hub-codegen | TypeScript clients generated from runtime schema; no drift possible |

---

## Roadmap

### Runtime graph mutation (live graph)

**Status:** `LIVE-GRAPH-1.md` — designed, not yet implemented.

The `[planner]` node exists and works: Claude generates a ticket file, it
compiles into a child graph, the child graph runs. The parent graph never
knew upfront how many nodes there would be — the planner decided.

What's not implemented: mutating a graph *while it's running*. Today the
child graph is fully compiled before any node executes. A running `[planner]`
node cannot call `lattice.add_node` mid-flight to inject work after seeing
partial results.

What live graph enables: a planner that reads partial analysis output from
sibling nodes as they complete, decides the fix is actually 9 tasks not 3,
and injects them into the same running graph — without a child graph boundary.

The critical correctness constraint for this: when you add an edge to a node
that's already `Complete`, the storage layer must retroactively deposit the
source token on the new edge. Otherwise the downstream node never fires.

---

### Contract-verified code (`[agent/tdd]` node)

**Status:** `plans/TDD/TDD-1.md` — TDD-1 through TDD-7, designed, not yet implemented.

The problem with "write this code and test it": if the same agent writes both,
the tests pass because they were written to match the implementation. That's
circular verification, not a test suite.

The TDD node breaks this by introducing a spec as the shared source of truth:

1. **Spec agent** writes a `BehavioralSpec` — preconditions, postconditions,
   invariants, examples, edge cases. No file paths. No commands. Just behavior.

2. **In parallel:** a spec review agent checks it for internal contradictions;
   a project analysis agent reads the codebase and figures out where the files go
   and what test command to run.

3. **Impl agent and test agent work simultaneously from the same spec, never
   communicating.** If they both honor it, the tests pass.

4. If the tests fail, a repair agent reads the contract, the impl output, the
   test code, and the failure log, and classifies: impl wrong? tests wrong?
   spec ambiguous? impossible? Environmental problem? Each routes differently.

What you write:

```markdown
# TDD-1: Implement advance_graph [agent/tdd]
The `advance_graph` function in lattice/storage.rs should accept a completed
node ID and route its output token to all qualifying downstream nodes, updating
their readiness state and emitting NodeReady events where appropriate.
```

What you get back: a token containing the contract, the implementation, the test
file, and how many repair cycles it took.

---

### Autonomous approval with human override (`ORCHA-1`)

**Status:** `ORCHA-1.md` — designed, not yet implemented.

Currently all tool-use in Claude sessions is auto-approved by a Haiku instance.
There's no way to turn this off without modifying code.

The plan adds an `approval_mode` to each session:
- `autonomous` — Haiku auto-approves everything (current default)
- `gated` — every tool use creates an `ApprovalPending` event; a human approves
- `interactive` — human watches the stream and approves in real time

A production deploy graph runs `autonomous`. A staged rollout to prod runs
`gated` — the graph pauses before every file write for a human sign-off.
You switch modes without changing the graph definition.

---

### Discord as a development surface (`DISCORD/`)

**Status:** `DISCORD-1.md` through `DISCORD-11.md` — designed, partially implemented.

Discord activation exposes the full Discord API as Plexus RPC methods:
`discord.guilds.{id}.channels.list`, `discord.guilds.{id}.channels.{id}.messages.create`,
etc. Hierarchical namespaces matching Discord's own API structure.

What this enables concretely: a graph triggered by a Discord message in `#work`.
The message is the task. The graph runs. When it completes, the result is posted
back to the channel. Your whole team shares one Discord channel as a shared
interface to Orcha. No dashboard, no webapp — just messages.

Or: a Review node that posts "ready to deploy to prod — approve?" in `#deploys`
and waits for a thumbs-up reaction before continuing.

---

### Interactive mid-stream redirection (`BIDIR/`)

**Status:** `BIDIR/` — designed, not yet implemented.

Right now you call a method and get a stream back. That's one direction.
Bidirectional streaming adds the reverse: the server pushes events, the client
pushes commands back on the same connection.

What this changes for agent workflows: you're watching an agent work via
`subscribe_graph`. You see it heading in the wrong direction halfway through.
Today your options are: let it finish and repair, or cancel and restart.

With bidirectional streaming: you push a message into the running session stream
— "actually, focus on the public API surface only, ignore internals" — and the
agent incorporates it. The session is interactive without polling. Human feedback
goes in; agent output comes back. Same connection.

---

### Bigger bets

**TLA+-verified behavioral specs.** The spec review agent in the TDD node is
semantic — Claude checking Claude's spec for contradictions. `DispatchTdd.tla`
already exists and TLC already checks its invariants. The interface is designed
for drop-in replacement: `SpecReview.method` is `"semantic_review" | "tlc"`.
When a spec fails TLC, the counterexample is a concrete execution trace — a
specific input that violates a specific postcondition — fed directly back to
the spec agent as refinement guidance. This is the difference between "your
spec seems contradictory" and "here is the input that proves it."

**Persistent agent memory.** Agents currently start fresh on every graph run.
Add an Arbor-backed memory layer keyed by project + task type. After a TDD
graph succeeds, the `TddContractArtifact` and repair history are written to
memory. A repair agent working on a similar task next week reads that history
first. Recurring `ImplBug` patterns in a codebase get recognized. Orcha stops
being stateless.

**Multi-hub graph federation.** Run a graph where the impl node runs on a
powerful cloud Substrate instance and the validate node runs locally against
your dev database. The graph is the unit of composition; which machine runs
which node is a routing decision. Same ticket format, same event stream,
distributed execution.

**Issue-to-PR pipeline.** A GitHub webhook triggers a Plan node that reads the
issue, generates a TDD ticket for the bug, runs it, and opens a PR with the
implementation and test output as the PR description. The human files the issue
and reviews the PR. Everything between is Orcha.

---

## Project structure

```
src/activations/
├── lattice/         DAG engine — types, storage, activation
├── orcha/           Multi-agent orchestration
│   ├── types.rs     OrchaNodeKind, BehavioralSpec, TddContractArtifact, ...
│   ├── graph_runner.rs  run_graph_execution, all dispatch_* functions
│   ├── graph_runtime.rs OrchaGraph builder
│   ├── ticket_compiler.rs  markdown → graph
│   ├── activation.rs    Plexus RPC methods
│   └── pm/          Project manager — TDD contract storage
├── claudecode/      Claude Code session wrapper
├── claudecode_loopback/  Tool-use approval routing
├── arbor/           Conversation tree storage
├── bash/            Shell command execution
├── changelog/       API hash tracking
└── mustache/        Template rendering

plans/               Implementation plans (epics + tickets)
├── LIVE-GRAPH/
├── DISCORD/
├── ORCHA/
├── BIDIR/
├── TDD/
│   └── TDD-1.md
└── DispatchTdd.tla  Formal TLA+ spec of the TDD node

docs/architecture/   Design documents (newest-first filename ordering)
```

---

## Quickstart

```bash
# Start substrate
substrate-start

# Run a ticket plan
synapse substrate orcha run_tickets_files \
  --ticket_files '["plans/TDD/TDD-1.md"]' \
  --model sonnet \
  --working_directory /workspace/hypermemetic/plexus-substrate

# Watch a running graph
synapse substrate orcha subscribe_graph --graph_id <id>

# Check pending approvals
synapse substrate orcha list_pending_approvals --graph_id <id>

# Approve
synapse substrate orcha approve_request --approval_id <id>
```

Substrate port: `4444` — WebSocket and MCP on the same port.

---

## See also

- `docs/architecture/intro-lattice-orcha-tdd.md` — full architectural introduction
- `docs/architecture/16678373036159325695_plugin-development-guide.md` — how to write a new activation
- `plans/DispatchTdd.tla` — formal spec of the TDD node control flow
- `plans/TDD/TDD-1.md` — implementation plan for TDD-1 through TDD-7

## License

MIT

# Introduction: Lattice, Orcha, and the TDD Node

This document is for engineers joining the project. It covers the full stack
from the bottom up: the Plexus RPC protocol, the Lattice execution engine,
the Orcha orchestration layer, and the TDD node — a new kind of agent node
that runs contract-first development inside a single graph step.

---

## The Big Picture

Substrate is a server. You call methods on it — create a graph, add nodes,
run it — and it orchestrates AI agents on your behalf. The result comes back
as a stream.

The whole thing is built in three layers that sit on top of each other:

```
┌────────────────────────────────────────────────────────┐
│  Orcha                                                 │
│  Multi-agent orchestration. AI nodes, human gates,     │
│  validation steps, repair loops.                       │
├────────────────────────────────────────────────────────┤
│  Lattice                                               │
│  DAG execution engine. Nodes, edges, tokens.           │
│  Knows nothing about AI — just routing and sequencing. │
├────────────────────────────────────────────────────────┤
│  Plexus RPC                                            │
│  The transport. Self-describing, streaming-first RPC.  │
│  How clients call methods and receive results.         │
└────────────────────────────────────────────────────────┘
```

Each layer depends only on the layer below it. Lattice doesn't know about
Claude. Orcha doesn't know about Plexus wire encoding. The TDD node doesn't
know about lattice token routing.

---

## Layer 1: Plexus RPC

Plexus is the wire protocol. The unusual property: **code is schema**. Every
method exposes a JSON Schema at runtime describing its parameters and return
type. Clients can discover the full API by querying it — no separate IDL file,
no drift between schema and implementation.

Two other properties matter for understanding the layers above:

**Streaming by default.** Every method returns a stream. A single call can
yield dozens of events before the stream closes. This is not an afterthought —
it's why you can watch a graph execute node by node in real time.

**Hierarchical namespaces.** Plugins organize as a tree: `arbor.tree_create`,
`orcha.run_graph`, `lattice.execute`. Each plugin implements a single trait:

```rust
trait Activation {
    fn namespace(&self) -> &str;
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;
    fn plugin_schema(&self) -> PluginSchema;  // the self-describing part
}
```

A `DynamicHub` routes incoming calls to the right `Activation` by namespace.
That's all Plexus RPC does at this level — it routes and streams.

---

## Layer 2: Lattice

Lattice is a DAG execution engine. You define a graph of nodes with directed
edges, then execute it. Nodes run when their inputs are ready. Results flow
forward as tokens.

### Why a DAG engine?

The naive way to orchestrate several tasks is to write sequential async code:

```rust
let a = do_thing_a().await?;
let b = do_thing_b(a).await?;
let c = do_thing_c().await?;
```

This works until you need: parallelism, conditional routing, fan-out to
multiple workers, fan-in to collect results, or subgraphs. At that point
you're building a DAG engine anyway. Lattice makes that explicit and reusable.

### Nodes

Every node has a `NodeSpec` that describes what kind of work it does:

```
Task      — run something (impl provided by the caller — in Orcha, that's Claude)
Scatter   — fan out: send one input to many downstream nodes
Gather    — fan in:  wait for N upstream nodes, collect their outputs
SubGraph  — delegate to a nested graph, return its result
```

### Tokens

When a node completes, it produces a `NodeOutput` — one or more `Token`s. Each
token has two parts:

**Color** — routing metadata:
- `Ok` — success, route to normal downstream nodes
- `Error` — failure, route to error-handler edges
- `Named("x")` — semantic label, used for conditional routing

**Payload** — the data:
- `Data { value }` — a JSON value (the result)
- `Handle { method, meta }` — a lazy reference to a streaming method call

Tokens flow along edges. An edge can have a condition: only pass this token
if its color matches. This is how you build branching graphs — one path for
`Ok`, another for `Error`.

### Execution

When a node completes and produces a token, Lattice evaluates its outgoing
edges and routes the token to qualifying downstream nodes. A downstream node
becomes ready when its inputs satisfy its `JoinType`:

- `All` — wait for all upstream edges (default)
- `Any` — fire as soon as any upstream edge delivers

Gather nodes collect all tokens from a fan-out and present them as a list to
the next node. This is how you do "run N things in parallel, then do something
with all their results."

```
         ┌──────────┐
         │  Task A  │
         └────┬─────┘
              │ token
        ┌─────▼──────┐
        │   Scatter  │
        └──┬─────┬───┘
           │     │
    ┌──────▼┐   ┌▼──────┐
    │ Task B│   │ Task C│   (run in parallel)
    └──────┬┘   └┬──────┘
           │     │
        ┌──▼─────▼───┐
        │   Gather   │   (wait for both)
        └─────┬──────┘
              │ [token_B, token_C]
         ┌────▼─────┐
         │  Task D  │
         └──────────┘
```

---

## Layer 3: Orcha

Lattice knows nothing about AI. It just routes tokens. Orcha is the layer that
gives nodes meaning. It adds a type system on top of Lattice's `NodeSpec::Task`
— a tagged enum called `OrchaNodeKind` that tells the executor what to actually
do when a node fires:

```rust
enum OrchaNodeKind {
    Task      { task: String, .. }    // run Claude on this prompt
    Synthesize { task: String, .. }   // like Task but with upstream context stitched in
    Validate  { command: String, .. } // run a shell command, pass/fail as token color
    Review    { prompt: String }      // pause and wait for human approval
    Plan      { task: String }        // ask Claude to produce a structured plan
    Tdd       { task: String, .. }    // contract-first dev loop (described below)
}
```

When Orcha executes a graph, it subscribes to Lattice's event stream. When
Lattice emits `NodeReady`, Orcha reads the node's `data` field, deserializes
the `OrchaNodeKind`, and dispatches to the appropriate handler:

```
Lattice: "node abc123 is ready"
           │
           ▼
Orcha: deserialize OrchaNodeKind from node data
           │
           ├─ Task      → dispatch_task      (create ClaudeCode session, stream result)
           ├─ Validate  → dispatch_validate  (run shell command, return ok/error token)
           ├─ Review    → dispatch_review    (create loopback approval, poll until resolved)
           └─ Tdd       → dispatch_tdd       (see below)
           │
           ▼
Lattice: node_complete(token) or node_fail(error)
```

### Building a graph

You don't write node IDs and edge tables by hand. Orcha provides two ways to
build graphs:

**Builder API** — programmatic, from Rust or via RPC calls:
```
graph.add_task("summarize the file")
graph.add_validate("cargo test")
graph.add_edge(node_a, node_b)
```

**Ticket compiler** — markdown format that compiles to a graph:
```markdown
# TASK-1: Summarize the file [agent/task]
blocked_by: []

# TASK-2: Run tests [agent/validate]
blocked_by: [TASK-1]
validate: cargo test 2>&1
```

The ticket format is how you write multi-step plans that execute automatically.
`[agent/task]`, `[agent/validate]`, `[agent/tdd]` — these tags select the node
type. `blocked_by` becomes a Lattice edge.

### Loopback

When a `Review` node fires, Orcha doesn't block a thread. It creates an
approval record in the loopback database, emits an `ApprovalPending` event,
and polls until a human responds via the API. The graph is paused at that
node; other independent nodes in the same graph can still run.

This is the same mechanism TDD nodes use for human escalation when a repair
loop exhausts its retries.

---

## Layer 4: The TDD Node

### The problem

When an AI agent writes code, how do you know it's right? The naive answer:
write tests. But if the same agent writes both the code and the tests, you
have circular verification — the tests pass because they were written to match
the implementation, not because the implementation is correct.

The TDD node breaks this circle.

### The solution: a behavioral spec as shared source of truth

Before any code is written, a spec agent defines the behavior in abstract
terms — preconditions, postconditions, invariants, examples, edge cases. No
file paths. No implementation details. Just: "what must be true."

Then two agents work independently from that spec:
- An **impl agent** writes code that satisfies it
- A **test agent** writes tests that verify it

They never communicate. If they both honor the spec, the tests pass. Agreement
proves the spec was precise enough — not that the agents collaborated.

```
                     BehavioralSpec
                          │
          ┌───────────────┴──────────────┐
          │                              │
   IMPL AGENT                      TEST AGENT
   "write code that                "write tests that
    satisfies this spec"            verify this spec"
          │                              │
          └───────────────┬──────────────┘
                          │
                     run the tests
                          │
                    pass? → done
                    fail? → repair
```

### The phases

A TDD node runs five phases internally. The parent graph sees one node.

#### Phase 1 — ContractPhase

A spec agent receives the task description and outputs a `BehavioralSpec`:

```json
{
  "preconditions":  ["graph is non-empty", "all node IDs are unique"],
  "postconditions": ["all nodes executed", "topological order respected"],
  "properties":     ["idempotent on retry", "output length equals node count"],
  "examples":       [{"input": {"nodes": 3, "edges": [[0,1],[1,2]]}, "expected": [0,1,2]}],
  "edge_cases":     ["single node graph", "disconnected subgraphs", "cycle detection"],
  "out_of_scope":   ["node cancellation", "partial execution resumption"]
}
```

Notice what's missing: no file paths, no test commands. Those come next.

#### Phase 2 — ContractValidating

Two agents run in parallel:

**Spec review agent** — checks the spec for internal consistency:
- Do any postconditions contradict preconditions?
- Are the examples consistent with the properties?
- Are any invariants vacuously true?
- Do the examples cover the stated edge cases?

Returns `{ consistent: bool, issues: [...] }`. If inconsistent, the issues are
fed back to the spec agent and Phase 1 reruns. `spec_cycle` is bounded by
`max_spec_cycles` to prevent infinite loops.

**Project analysis agent** — reads the actual codebase and derives execution
context:
- Which files should the impl agent modify?
- Where should the test file go, following project conventions?
- What command runs exactly these tests?
- What test framework is in use (drives property-based test library selection)?

Returns `ExecutionContext { impl_targets, test_path, validate_command, test_framework }`.

When both return, they're composed into a `TddContractArtifact`:

```rust
struct TddContractArtifact {
    spec:    BehavioralSpec,    // the what
    context: ExecutionContext,  // the where and how
}
```

The separation matters. The spec agent doesn't need to know where files live.
The project analysis agent doesn't need to understand the behavior. Each does
one job.

#### Phase 3 — Branching

Both agents receive the full `TddContractArtifact` and work in parallel:

```
TddContractArtifact
        │
  ┌─────┴──────┐
  │            │
IMPL          TEST
agent         agent
"modify        "write
 these files"   tests here"
  │            │
  └─────┬──────┘
        │
  (both finish)
```

The impl agent is told: modify `impl_targets`, do not write tests.
The test agent is told: write to `test_path`, do not touch implementation files.
For each `property` in the spec, the test agent writes at least one
property-based test using the framework from `ExecutionContext.test_framework`
(`proptest` for Rust, `hypothesis` for Python, `fast-check` for JS).

#### Phase 4 — Validating

`validate_command` runs. Exit 0 means done.

#### Phase 5 — Repairing

If validation fails, a repair agent reads everything — the contract, the impl
output, the test code, and the failure log — and classifies the root cause:

| Diagnosis | Meaning | Action |
|---|---|---|
| `ImplBug` | Contract and tests agree; impl is wrong | Redo impl |
| `TestBug` | Impl is correct per contract; tests have wrong expectations | Redo tests |
| `ImplTestMismatch` | Both diverged from contract incompatibly | Redo both |
| `ContractAmbiguity` | Spec didn't pin down a behavior | Refine spec (back to Phase 1) |
| `Impossible` | Spec cannot be satisfied in this codebase | Escalate to human |
| `Environmental` | Missing dependency, wrong path | Fix with context |

The repair loop is bounded by `max_repair_cycles`. `ContractAmbiguity` also
increments `spec_cycle` — if both bounds are exhausted, the node escalates to
human via a loopback approval gate.

### Control flow

```
start
  │
  ▼
ContractPhase ◄─────────────────────────┐
  │                                      │ SpecFail
  ▼                                      │ (spec_cycle < max)
ContractValidating ─── SpecFail ─────────┘
  │                                      │ SpecExhausted → Failed
  │ SpecPass
  ▼
Branching (impl + test in parallel)
  │
  ▼
Validating
  │
  ├─ pass → Complete ✓
  │
  └─ fail
       │
       ▼
     Repairing ── ImplBug/TestBug/Mismatch/Environmental ──► Branching (repair_cycle++)
       │
       ├─ ContractAmbiguity ──► ContractPhase (spec_cycle++, repair_cycle++)
       │
       ├─ Impossible ──► EscalatingToHuman
       │                     │
       │                     ├─ approved → Branching
       │                     └─ denied → Failed
       │
       └─ repair_cycle >= max → Failed
```

This control flow is formally specified in `plans/DispatchTdd.tla`. TLC verifies
the invariants: branches never run before a validated contract exists, impossible
specs always escalate rather than silently retry, cycles are bounded, and the
node always terminates.

---

## Putting it together

Here's what happens when you submit a ticket file with an `[agent/tdd]` node
to `orcha.run_graph`:

1. **Ticket compiler** parses the markdown, emits `OrchaNodeSpec::Tdd { task, .. }`
2. **Graph runtime** creates a Lattice node with the spec serialized as `NodeSpec::Task { data }`
3. **Lattice** adds the node to the DAG, resolves dependencies, waits for predecessors
4. When predecessors complete, Lattice emits `NodeReady`
5. **Orcha's run_graph_execution** deserializes `OrchaNodeKind::Tdd` from the node data
6. `dispatch_tdd` runs — five phases, internally managed, invisible to the graph
7. On success, `graph.complete_node(ok_token)` — Lattice routes the result forward
8. Downstream nodes in the parent graph fire with the contract + impl + test as their input

The parent graph never knew a TDD loop happened. It just sees a node that
eventually produced a token.

---

## File map

```
src/activations/
├── lattice/
│   ├── types.rs          NodeSpec, NodeOutput, Token, TokenColor, TokenPayload
│   ├── storage.rs        DAG storage, advance_graph, token routing
│   ├── activation.rs     Plexus RPC methods: create, add_node, add_edge, execute
│   └── mod.rs
│
└── orcha/
    ├── types.rs          OrchaNodeKind, BehavioralSpec, ExecutionContext,
    │                     TddContractArtifact, SpecReview, TddRepairDecision
    ├── graph_runtime.rs  OrchaGraph builder (add_task, add_tdd, add_edge, ...)
    ├── graph_runner.rs   run_graph_execution, dispatch_task, dispatch_tdd,
    │                     dispatch_spec_review, dispatch_execution_context,
    │                     dispatch_tdd_branches, dispatch_tdd_repair
    ├── ticket_compiler.rs  markdown → OrchaNodeSpec graph
    ├── activation.rs     Plexus RPC methods: run_graph, create_session, ...
    ├── orchestrator.rs   session management
    ├── storage.rs        graph + session persistence
    ├── mod.rs
    └── pm/               project manager — TDD contract storage
        ├── storage.rs    orcha_tdd_contracts, orcha_tdd_behavioral_specs tables
        └── activation.rs Pm wrapper

plans/
├── DispatchTdd.tla       Formal TLA+ spec of the TDD node control flow
└── TDD/
    └── TDD-1.md          Implementation plan (7 tickets, TDD-1 through TDD-7)
```

# Activation Integration — How the Pieces Connect

Activations in Substrate are independent services that integrate through four mechanisms: the DynamicHub routing layer, shared storage (ArborStorage), the handle system for cross-activation references, and parent context injection for runtime coupling without compile-time dependencies.

## Activation Dependency Graph

```
                    DynamicHub
                        │
        ┌───────────────┼───────────────┐
        │               │               │
      Orcha           Cone        ClaudeCode
   (orchestrator)    (LLM chat)   (Claude sessions)
        │               │               │
   ┌────┼────┐     ┌────┘          ┌────┘
   │    │    │     │               │
Lattice CC  Loopback  Arbor ◄──────┘
 (DAG) (exec) (approval) (shared tree storage)
```

Arrows indicate runtime dependency. Orcha depends on Lattice for graph execution, ClaudeCode for subprocess management, and Loopback for approval gating. Both Cone and ClaudeCode depend on Arbor for persistent tree storage.

## The Orchestration Pipeline: Orcha -> Lattice -> ClaudeCode -> Loopback

Full flow:

1. User calls `orcha.run_task_async` or `orcha.run_tickets_files`
2. Orcha creates a Lattice graph with task nodes
3. Lattice drives topological execution, emitting `NodeReady` events
4. For each ready node, Orcha spawns a ClaudeCode session (claude subprocess)
5. Claude hits a tool use -> Claude CLI calls `loopback.permit()` via `--permission-prompt-tool` MCP hook
6. Loopback blocks (poll loop with `Notify`) until `respond()` called
7. Orcha's auto-approval agent (or human) calls `loopback.respond()`
8. Claude continues or stops based on approval
9. ClaudeCode captures events, writes to Arbor trees
10. Node completes -> Lattice advances graph -> next nodes become ready

The pipeline is fully async. Multiple nodes execute concurrently when the DAG permits it. Lattice handles fan-out: completing one node can unblock several downstream nodes simultaneously.

## Shared Arbor Storage

Arbor is the universal conversation tree store. Three activations share a single `ArborStorage` instance:

| Activation | Relationship | Usage |
|------------|-------------|-------|
| **Arbor** | Owns it | Exposes tree/node CRUD methods |
| **Cone** | Receives `Arc<ArborStorage>` at init | Creates trees per conversation |
| **ClaudeCode** | Receives `Arc<ArborStorage>` at init | Creates trees per session |

Each activation creates its own trees but the storage pool is shared. This means any activation can walk trees created by another — Orcha can inspect ClaudeCode session trees, Cone conversations are visible to Arbor queries, etc.

The sharing happens during hub construction in `builder.rs`: ArborStorage is created once, then cloned (`Arc`) into each activation that needs it before registration.

## Handle System

Handles are type-erased cross-activation references stored in Arbor tree nodes.

**Format:** `{plugin}@{version}::{method}:{meta[0]}:{meta[1]}:...`

**Example:** `cone@1.0.0::chat:msg-550e8400:user:alice`

### Handle Lifecycle

1. **Producer creates handle** — e.g., Cone stores a message, creates a handle pointing to it
2. **Handle stored** as `NodeType::External` in an Arbor tree node
3. **Consumer walks tree**, encounters foreign handle
4. **Consumer calls** `parent.resolve_handle(handle)` -> routes through DynamicHub -> dispatched to owning activation
5. **Owning activation** extracts meta fields, looks up data in its own storage, returns content

Handles decouple storage from access. The consumer doesn't need to know how the producer stores data — it only needs the handle string and a parent context to resolve it.

## Parent Context Injection

**Problem:** Activations need to resolve handles from other activations without compile-time coupling.

**Solution:** Generic `P: HubContext` parameter + `Arc::new_cyclic` injection.

```rust
pub struct Cone<P: HubContext = NoParent> {
    hub: Arc<OnceLock<P>>,  // Weak<DynamicHub> injected here
}
```

During hub construction in `builder.rs`:

```rust
let hub = Arc::new_cyclic(|weak_hub: &Weak<DynamicHub>| {
    arbor.inject_parent(weak_hub.clone());
    cone.inject_parent(weak_hub.clone());
    claudecode.inject_parent(weak_hub.clone());
    DynamicHub::new("substrate").register(arbor).register(cone)...
});
```

The `OnceLock` pattern allows activations to be constructed before the hub exists. The weak reference is injected inside `Arc::new_cyclic`'s closure, which runs after allocation but before the `Arc` is returned. This breaks the circular dependency: the hub owns the activations, and the activations hold a weak reference back to the hub.

Four activations use parent context: **Arbor**, **Cone**, **ClaudeCode**, and **Orcha** (indirectly via its wrapped ClaudeCode instance).

## Cone -> LLM Flow

Cone uses the `cllient` crate's `ModelRegistry` for LLM access:

1. **At init:** `ModelRegistry::new()` reads env vars (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`)
2. **On chat:** looks up model in registry, gets endpoint + credentials
3. **Calls LLM**, streams tokens back via Plexus RPC streaming
4. **Stores messages** as handles in Arbor tree

The registry is internal to Cone — no other activation accesses LLM providers directly. Orcha gets LLM capabilities indirectly by spawning ClaudeCode sessions (which run the `claude` CLI binary, not the Cone activation).

## Registry Backend Discovery

The Registry activation (from `plexus-registry` crate) tracks remote Plexus RPC backends:

- **Storage:** SQLite table `backends` with columns: `id`, `name`, `host`, `port`, `protocol`, `is_active`, `last_seen`
- **Methods:** `list`, `info`, `register`, `ping`
- **Sources:** `auto`, `file`, `manual`, `env`

Synapse uses Registry for multi-backend discovery: it queries `registry.list` to find available backends, then connects directly to each backend's host:port for RPC calls. Registry is passive — it tracks what's available but doesn't route traffic.

## Integration Boundaries

What crosses activation boundaries:
- **Handle strings** — opaque references resolved via parent context
- **ArborStorage** — shared `Arc`, no RPC overhead for tree access
- **Lattice events** — `NodeReady`/`NodeComplete` drive Orcha's execution loop
- **Loopback tokens** — approval IDs passed from ClaudeCode's MCP hook to Orcha's approval logic

What stays internal:
- LLM credentials and provider logic (Cone only)
- Subprocess management (ClaudeCode only)
- Graph topology and scheduling (Lattice only)
- DAG persistence and recovery (Orcha only)

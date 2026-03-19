# Substrate

A Plexus RPC server. Write Rust methods with `#[hub_method]`, get a
self-describing streaming RPC server with WebSocket, MCP, and CLI access
— no separate schema files, no drift.

---

## Architecture

Three layers. Each knows only about the layer below it.

```
┌────────────────────────────────────────────────────────┐
│  Activations                                           │
│  Pluggable modules. Each exposes typed, streaming      │
│  methods via the hub macro. Orcha, Lattice, Arbor, ... │
├────────────────────────────────────────────────────────┤
│  Plexus RPC                                            │
│  Self-describing, streaming-first RPC protocol.        │
│  Code is schema. Runtime JSON Schema per method.       │
│  Language-agnostic clients via hub-codegen.            │
├────────────────────────────────────────────────────────┤
│  Transport                                             │
│  WebSocket + MCP on the same port (4444).              │
│  Synapse CLI — dynamic, schema-driven command line.    │
└────────────────────────────────────────────────────────┘
```

---

## Activations

| Activation | Purpose |
|---|---|
| **[orcha](docs/activations/orcha/README.md)** | Multi-agent orchestration — ticket plans as parallel agent DAGs, human approval gates, child graphs. |
| **[lattice](docs/activations/lattice/README.md)** | Colored Petri net DAG execution engine. Nodes, edges, typed tokens, scatter/gather, join types, crash recovery. |
| **[arbor](docs/activations/arbor/README.md)** | Conversation tree storage with reference counting and lifecycle management. Backs session history. |
| **[claudecode](docs/activations/claudecode/README.md)** | Claude Code CLI session wrapper with Arbor-backed history, forking, async polling, and JSONL import/export. |
| **[claudecode_loopback](docs/activations/claudecode_loopback/README.md)** | Tool-use approval gating. Claude sessions block on permission requests until a parent approves or denies. |
| **[cone](docs/activations/cone/README.md)** | Multi-model LLM agent (Claude, OpenAI, others) with Arbor-backed conversation history and branching. |
| **[bash](docs/activations/bash/README.md)** | Shell command execution with real-time stdout/stderr streaming. |
| **[changelog](docs/activations/changelog/README.md)** | API hash tracking — detects undocumented schema changes between restarts. |
| **[mustache](docs/activations/mustache/README.md)** | Template rendering. Other activations register and call named Mustache templates. |
| **[chaos](docs/activations/chaos/README.md)** | Fault injection — force-fail/complete running nodes, kill processes, crash substrate. For testing. |
| **[registry](docs/activations/registry/README.md)** | Backend discovery and health checking. Maintains a registry of Plexus RPC backends. |
| **[interactive](docs/activations/interactive/README.md)** | Bidirectional communication demo (prompts, selections, confirmations). Protocol not yet fully shipped. |
| **[echo](docs/activations/echo/README.md)** | Echo service. Reference implementation of the `#[hub_methods]` macro pattern. |
| **[health](docs/activations/health/README.md)** | Reports server uptime. |
| **[solar](docs/activations/solar/README.md)** | Nested plugin hierarchy demo modelling the solar system. Shows `ChildRouter` pattern. |

---

## Access

Everything is exposed on port `4444`:

- **WebSocket** — `ws://localhost:4444`
- **MCP** — `http://localhost:4444/mcp` (all methods appear as MCP tools)
- **Synapse CLI** — `synapse substrate <namespace> <method> [--param value]`
- **In-process Rust** — `DynamicHub::call(method, params)`

---

## Quickstart

```bash
# Start
substrate-start

# Explore available methods
LANG=C.UTF-8 synapse substrate

# Run an agent graph from a ticket plan
LANG=C.UTF-8 synapse substrate orcha run_tickets_files \
  --ticket_files '["plans/TDD/TDD-1.md"]' \
  --model sonnet \
  --working_directory /workspace/hypermemetic/plexus-substrate
```

---

## See also

- [`docs/activations/orcha/README.md`](docs/activations/orcha/README.md) — Orcha: multi-agent orchestration
- [`docs/architecture/intro-lattice-orcha-tdd.md`](docs/architecture/intro-lattice-orcha-tdd.md) — full stack walkthrough
- [`docs/architecture/__index.md`](docs/architecture/__index.md) — architecture doc index
- [`docs/QUICKSTART.md`](docs/QUICKSTART.md) — getting started guide
- [`docs/architecture/16678373036159325695_plugin-development-guide.md`](docs/architecture/16678373036159325695_plugin-development-guide.md) — how to write a new activation

## License

MIT

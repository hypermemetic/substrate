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
| **orcha** | Multi-agent orchestration — run ticket plans as parallel agent DAGs, human approval gates, child graphs. See [`docs/activations/orcha/README.md`](docs/activations/orcha/README.md). |
| **lattice** | DAG execution engine underlying Orcha. Nodes, edges, typed tokens, scatter/gather, join types. |
| **arbor** | Conversation tree storage. Backs agent session history. |
| **claudecode** | Claude Code CLI session wrapper. Spawns and manages Claude sessions. |
| **claudecode_loopback** | Tool-use approval routing. Claude sessions request permission; routed through the approval API. |
| **bash** | Shell command execution. |
| **changelog** | API hash tracking — logs when the method schema changes between restarts. |
| **mustache** | Template rendering. |

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
# Start in background (default - daemonizes after startup)
cargo run

# Start in foreground (for debugging)
cargo run -- --fg

# Development mode (auto-restart on file changes)
# First install cargo-watch: cargo install cargo-watch
cargo dev

# Explore available methods
LANG=C.UTF-8 synapse substrate

# Run an agent graph from a ticket plan
LANG=C.UTF-8 synapse substrate orcha run_tickets_files \
  --ticket_files '["plans/TDD/TDD-1.md"]' \
  --model sonnet \
  --working_directory /workspace/hypermemetic/plexus-substrate
```

### Running Modes

- **Background (default)**: `cargo run` — Shows startup logs, then daemonizes
- **Foreground**: `cargo run -- --fg` — Stays attached to terminal
- **Development**: `cargo dev` — Auto-restarts on code changes (requires `cargo-watch`)
- **Stdio/MCP**: `cargo run -- --stdio` — Line-delimited JSON-RPC for MCP integration

---

## See also

- [`docs/activations/orcha/README.md`](docs/activations/orcha/README.md) — Orcha: multi-agent orchestration
- [`docs/architecture/intro-lattice-orcha-tdd.md`](docs/architecture/intro-lattice-orcha-tdd.md) — full stack walkthrough
- [`docs/architecture/__index.md`](docs/architecture/__index.md) — architecture doc index
- [`docs/QUICKSTART.md`](docs/QUICKSTART.md) — getting started guide
- [`docs/architecture/16678373036159325695_plugin-development-guide.md`](docs/architecture/16678373036159325695_plugin-development-guide.md) — how to write a new activation

## License

MIT

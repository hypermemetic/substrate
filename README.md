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

| Activation | Namespace | What it does |
|---|---|---|
| **Arbor** | `arbor` | Persistent conversation trees with context tracking, handles, scheduling, archival |
| **Cone** | `cone` | LLM conversation orchestration — model registry, streaming chat, Arbor-integrated context |
| **ClaudeCode** | `claudecode` | Claude Code session management — create/execute/interrupt sessions with Arbor-backed history |
| **Loopback** | `loopback` | Bidirectional permission routing — parent approves/denies tool calls from child sessions |
| **Orcha** | `orcha` | Multi-agent orchestration with approval loops, crash recovery, Lattice DAG execution. See [`docs/activations/orcha/README.md`](docs/activations/orcha/README.md). |
| **Lattice** | `lattice` | DAG execution engine — topological ordering, dependency resolution, node state machines |
| **Chaos** | `chaos` | Fault injection & observability — force-fail nodes, kill processes, crash substrate for testing |
| **Bash** | `bash` | Execute shell commands with streaming stdout/stderr |
| **Registry** | `registry` | Backend discovery — register/list/ping other Plexus RPC servers |
| **Mustache** | `mustache` | Template rendering for handle values |
| **Changelog** | `changelog` | Track plexus schema hash changes with documentation enforcement |
| **Interactive** | `interactive` | Bidirectional UI prompts — wizards, confirmations, selections |
| **Health** | `health` | Health checks and uptime |
| **Echo** | `echo` | Echo/ping test activation |
| **Ping** | `ping` | Ping/pong test activation |
| **Solar** | `solar` | Demo of nested hub activation hierarchy (solar system model) |

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
synapse substrate

# Run an agent graph from a ticket plan
synapse substrate orcha run_tickets_files \
  --ticket_files '["plans/TDD/TDD-1.md"]' \
  --model sonnet \
  --working_directory .
```

### Running Modes

- **Background (default)**: `cargo run` — Shows startup logs, then daemonizes
- **Foreground**: `cargo run -- --fg` — Stays attached to terminal
- **Development**: `cargo dev` — Auto-restarts on code changes (requires `cargo-watch`)
- **Stdio/MCP**: `cargo run -- --stdio` — Line-delimited JSON-RPC for MCP integration

---

## See also

- [`docs/QUICKSTART.md`](docs/QUICKSTART.md) — getting started guide
- [`docs/architecture/__index.md`](docs/architecture/__index.md) — architecture doc index (start here)
- [`docs/architecture/16671569470654229503_system-overview.md`](docs/architecture/16671569470654229503_system-overview.md) — system overview
- [`docs/architecture/16671569470654229502_activation-integration.md`](docs/architecture/16671569470654229502_activation-integration.md) — how activations connect
- [`docs/architecture/16671569470654229501_activation-reference.md`](docs/architecture/16671569470654229501_activation-reference.md) — per-activation API reference
- [`docs/architecture/16671569470654229500_transport-mcp-gateway.md`](docs/architecture/16671569470654229500_transport-mcp-gateway.md) — transport & MCP gateway
- [`docs/activations/orcha/README.md`](docs/activations/orcha/README.md) — Orcha: multi-agent orchestration
- [`docs/architecture/16678373036159325695_plugin-development-guide.md`](docs/architecture/16678373036159325695_plugin-development-guide.md) — how to write a new activation

## License

MIT

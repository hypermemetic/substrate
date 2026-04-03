# Quickstart

Get from zero to running your first agent graph in 5 minutes.

## Prerequisites

- Rust toolchain installed
- Substrate built: `cargo build`
- Synapse CLI installed: `cd ../synapse && cabal install`

## Start the server

```bash
# Start in foreground (see logs)
cargo run -- --fg

# Or start as daemon (background)
cargo run
```

Port 4444 — WebSocket and MCP HTTP on the same port.

## Explore the API

```bash
# List all activations
synapse substrate

# Show methods for an activation
synapse substrate arbor

# Get help for a specific method
synapse substrate arbor tree_create --help
```

## Run a ticket plan

Write a markdown file with `[agent/task]`, `[agent/validate]`, or `[agent/synthesize]` nodes:

```markdown
# ANALYZE-1: Read the module [agent/task]
Summarize the architecture of src/activations/orcha/

# ANALYZE-2: Find the tests [agent/task]
List what is and isn't tested in src/activations/orcha/

# SYNTHESIZE: Identify gaps [agent/synthesize]
blocked_by: [ANALYZE-1, ANALYZE-2]
Given the analyses above, list the top 5 untested behaviors.

# VALIDATE: Check it compiles [agent/validate]
blocked_by: [SYNTHESIZE]
validate: cargo check --package plexus-substrate 2>&1
```

```bash
synapse substrate orcha run_tickets_files \
  --ticket_files '["plans/my-plan.md"]' \
  --model sonnet \
  --working_directory .
```

ANALYZE-1 and ANALYZE-2 run in parallel. SYNTHESIZE receives both as `<prior_work>` context. VALIDATE runs the command and routes exit code.

## Watch a running graph

```bash
synapse substrate orcha subscribe_graph --graph_id <id>
```

Events stream in real time: `NodeReady`, `NodeComplete`, `NodeFailed`, `ApprovalPending`.

## Build a graph programmatically

```bash
# Create
synapse substrate orcha create_graph --metadata '{"name":"my-graph"}'
# → graph_id: abc123

# Add nodes
synapse substrate orcha add_task_node --graph_id abc123 --task "analyze the codebase"
# → node_id: node_1

synapse substrate orcha add_validate_node --graph_id abc123 --command "cargo test 2>&1"
# → node_id: node_2

# Wire
synapse substrate orcha add_dependency --graph_id abc123 --from_node node_1 --to_node node_2

# Run
synapse substrate orcha run_graph --graph_id abc123 --model sonnet
```

## Human approval gates

When an agent hits a tool requiring approval, the graph pauses that node. Independent branches keep running. Approve via:

```bash
synapse substrate orcha list_pending_approvals
synapse substrate orcha approve_request --approval_id <id>
```

## MCP access

Substrate exposes all methods as MCP tools at `http://localhost:4444/mcp`. Configure Claude Code or Claude Desktop to point there and every activation method becomes a tool call.

## Other useful commands

```bash
# Chat with an LLM via Cone
synapse substrate cone create --name demo --model claude-sonnet
synapse substrate cone chat --identifier.type by_name --identifier.name demo --prompt "Hello"

# Execute a shell command
synapse substrate bash execute --command "ls -la"

# Check system health
synapse substrate health check

# Inspect the schema hash (for cache invalidation)
synapse substrate hash
```

## See also

- [`README.md`](../README.md) — Full activation table and architecture overview
- [`docs/architecture/__index.md`](architecture/__index.md) — Architecture doc index
- [`docs/architecture/16671569470654229503_system-overview.md`](architecture/16671569470654229503_system-overview.md) — System overview
- [`docs/architecture/intro-lattice-orcha-tdd.md`](architecture/intro-lattice-orcha-tdd.md) — Full stack introduction

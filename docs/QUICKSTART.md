# Quickstart

Get from zero to running your first agent graph in 5 minutes.

## Prerequisites

- Substrate built: `cargo build --package plexus-substrate`
- Synapse CLI installed: `cd /workspace/hypermemetic/synapse && cabal install --installdir=~/.local/bin --allow-newer`

## Start the server

```bash
substrate-start          # uses the helper in ~/.bashrc
# or: cd /workspace/hypermemetic/plexus-substrate && nohup ./target/debug/plexus-substrate > /tmp/substrate.log 2>&1 &
```

Port 4444 — WebSocket and MCP on the same port.

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
LANG=C.UTF-8 synapse substrate orcha run_tickets_files \
  --ticket_files '["plans/my-plan.md"]' \
  --model sonnet \
  --working_directory /workspace/hypermemetic/plexus-substrate
```

ANALYZE-1 and ANALYZE-2 run in parallel. SYNTHESIZE receives both as `<prior_work>` context. VALIDATE runs the command and routes exit code.

## Watch a running graph

```bash
LANG=C.UTF-8 synapse substrate orcha subscribe_graph --graph_id <id>
```

Events stream in real time: `NodeReady`, `NodeComplete`, `NodeFailed`, `ApprovalPending`.

## Build a graph programmatically

```bash
# Create
LANG=C.UTF-8 synapse substrate orcha create_graph --metadata '{"name":"my-graph"}'
# → graph_id: abc123

# Add nodes
LANG=C.UTF-8 synapse substrate orcha add_task_node --graph_id abc123 --task "analyze the codebase"
# → node_id: node_1

LANG=C.UTF-8 synapse substrate orcha add_validate_node --graph_id abc123 --command "cargo test 2>&1"
# → node_id: node_2

# Wire
LANG=C.UTF-8 synapse substrate orcha add_dependency --graph_id abc123 --from_node node_1 --to_node node_2

# Run
LANG=C.UTF-8 synapse substrate orcha run_graph --graph_id abc123 --model sonnet
```

## Human approval gates

When a `Review` node fires, the graph pauses. Independent branches keep running. Approve via:

```bash
LANG=C.UTF-8 synapse substrate orcha list_pending_approvals --graph_id abc123
LANG=C.UTF-8 synapse substrate orcha approve_request --approval_id <id>
```

## MCP access

Substrate exposes all Orcha methods as MCP tools at `http://localhost:4444/mcp`. Configure Claude Code or Claude Desktop to point there and every `orcha.*` method becomes a tool call.

## See also

- `README.md` — full architecture and roadmap
- `docs/architecture/__index.md` — architecture doc index
- `docs/architecture/intro-lattice-orcha-tdd.md` — full stack introduction

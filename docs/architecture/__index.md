# Architecture Documents

## Start Here

- **[intro-lattice-orcha-tdd.md](intro-lattice-orcha-tdd.md)** — Full stack introduction: Plexus RPC → Lattice → Orcha → TDD node. Read this first.

## Lattice

- See `src/activations/lattice/` — the DAG execution engine is self-documenting via types. No standalone design doc yet.

## Orcha

- [Orcha Approval Workflow](./16772656563233000000_orcha-approval-workflow.md) - Manual and auto-approval flows, session ID topology, notification propagation, Arbor integration

## TDD Node

- **[plans/TDD/TDD-1.md](../../plans/TDD/TDD-1.md)** — Implementation plan (TDD-1 through TDD-7)
- **[plans/DispatchTdd.tla](../../plans/DispatchTdd.tla)** — TLA+ formal specification

## ClaudeCode & Loopback

- [ClaudeCode Loopback Integration](./16677965632570341631_claudecode-loopback-integration.md) - Async chat + loopback approval flow (session_id-based API)
- [Loopback MCP Conformance](./16678130375925173503_loopback-mcp-conformance-analysis.md) - Permission routing via `--permission-prompt-tool` and streaming JSON output
- [Loopback Findings](./16672867147228287743_loopback-findings.md) - Root cause analysis of MCP response double-serialization bug
- [Arbor-Buffered Streaming](./16678111153768723711_arbor-buffered-streaming.md) - Original design doc (superseded by Loopback Integration above)

## Handle System

- [HandleEnum Codegen](./16677960973459046655_handle-enum-codegen.md) - Declarative handle definition with automatic storage resolution
- [Arbor Stream Map](./16677963644192091647_arbor-stream-map.md) - Persistent event streams with handle resolution through Plexus

## Development Process

- [Worktree Stack Development](./16677958939404136703_worktree-stack-development.md) - Working on multiple interdependent crates in isolation using git worktrees

## Mobile & Web Clients

- [Mobile Client Architecture](./16677366925482194687_mobile-client-architecture.md) - Building iOS/Android apps with Tauri + generated TypeScript client (remote backend pattern)

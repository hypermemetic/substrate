# Architecture Documents

## Start Here

- **[intro-lattice-orcha-tdd.md](intro-lattice-orcha-tdd.md)** — Full stack introduction: Plexus RPC → Lattice → Orcha → TDD node. Start here.

## Development Process

- [Worktree Stack Development](./16677958939404136703_worktree-stack-development.md) - **NEW**: How to work on multiple interdependent crates in isolation using git worktrees

## Handle System

- [HandleEnum Codegen](./16677960973459046655_handle-enum-codegen.md) - **NEW**: Declarative handle definition with automatic storage resolution
- [Arbor Stream Map](./16677963644192091647_arbor-stream-map.md) - Persistent event streams with handle resolution through Plexus

## ClaudeCode & Loopback

- [ClaudeCode Loopback Integration](./16677965632570341631_claudecode-loopback-integration.md) - Complete guide to async chat + loopback approval flow (session_id-based API)
- [Loopback MCP Conformance](./16678130375925173503_loopback-mcp-conformance-analysis.md) - Permission routing via `--permission-prompt-tool` and streaming JSON output
- [Arbor-Buffered Streaming](./16678111153768723711_arbor-buffered-streaming.md) - Original design doc (superseded by above)

## Orcha

- [Orcha Approval Workflow](./16772656563233000000_orcha-approval-workflow.md) - Manual and auto-approval flows, session ID topology, notification propagation, Arbor integration

## TDD Node

- **[plans/TDD/TDD-1.md](../../plans/TDD/TDD-1.md)** — Implementation plan for the TDD (Test-Driven Dispatch) node activation
- **[plans/DispatchTdd.tla](../../plans/DispatchTdd.tla)** — TLA+ formal specification for TDD dispatch correctness

## JsExec

- **NEW** activation added in `7fa6a8d` - JavaScript execution in sandboxed V8 isolates via Cloudflare workerd
- See `jsexec/docs/architecture/` for module loading system documentation

## Mobile & Web Clients

- [Mobile Client Architecture](./16677366925482194687_mobile-client-architecture.md) - **NEW**: Building iOS/Android apps with Tauri + generated TypeScript client (remote backend pattern)

# BIDIR-1: Bidirectional Streaming Epic

**Status:** Planning
**Date:** 2026-01-13

## Prerequisites

Before starting this project, create a git worktree to isolate the work:

```bash
# From substrate root
git worktree add ../substrate-bidir feature/bidirectional-streaming -b feature/bidirectional-streaming

# Work in the new worktree
cd ../substrate-bidir

# Also create worktrees for dependent repos if needed
cd ../hub-core
git worktree add ../hub-core-bidir feature/bidirectional-streaming -b feature/bidirectional-streaming

cd ../hub-macro
git worktree add ../hub-macro-bidir feature/bidirectional-streaming -b feature/bidirectional-streaming
```

This keeps the main development tree clean while working on this feature.

---

## Goal

Extend substrate's streaming architecture to support bidirectional communication while maintaining full backward compatibility with existing unidirectional methods.

## Context

### Current Architecture

The current streaming system is **unidirectional** (server→client):

```
Client                           Server
  │                                │
  │──── tools/call request ───────>│
  │                                │
  │<──── Progress notification ────│
  │<──── Progress notification ────│
  │<──── Data notification ────────│
  │<──── Done notification ────────│
  │<──── Final result ─────────────│
  │                                │
```

**Key components:**
- `PlexusStream`: `Pin<Box<dyn Stream<Item = PlexusStreamItem> + Send>>`
- `PlexusStreamItem`: Enum with `Data`, `Progress`, `Error`, `Done` variants
- `Activation::call()`: Takes `params: Value`, returns `Result<PlexusStream, PlexusError>`
- Transport: MCP (SSE notifications) and WebSocket (jsonrpsee subscriptions)

### Desired Architecture

Enable **bidirectional** communication for interactive workflows:

```
Client                           Server
  │                                │
  │──── tools/call request ───────>│
  │                                │
  │<──── Progress notification ────│
  │<──── Request (needs input) ────│  ← NEW: Server asks client
  │                                │
  │──── Response (user input) ────>│  ← NEW: Client responds
  │                                │
  │<──── Data notification ────────│
  │<──── Done notification ────────│
  │<──── Final result ─────────────│
  │                                │
```

### Use Cases

1. **Interactive prompts**: Activation asks user yes/no questions
2. **Confirmation dialogs**: "About to delete 5 repos, continue?"
3. **Progressive refinement**: "Found 100 matches, filter by X or Y?"
4. **Input collection**: "Enter API token for GitHub:"
5. **Long-running approvals**: Multi-step workflows with checkpoints

## Dependency DAG

```
              BIDIR-2 (Core Types)
                     │
          ┌─────────┼─────────┐
          ▼         ▼         ▼
      BIDIR-3   BIDIR-4   BIDIR-5
      (Channel) (Macro)   (Helpers)
          │         │         │
          └─────────┼─────────┘
                    ▼
              BIDIR-6 (MCP)
                    │
                    ▼
              BIDIR-7 (WebSocket)
                    │
          ┌─────────┼─────────┐
          ▼         ▼         ▼
      BIDIR-8   BIDIR-9   BIDIR-10
     (Example) (Tests)   (Docs)
```

## Phase Breakdown

### Phase 1: Core Infrastructure (BIDIR-2 through BIDIR-5)
- Define new stream item types for bidirectional messages
- Create channel-based context for activations
- Update hub-macro to support bidirectional methods
- Build helper functions for common patterns

### Phase 2: Transport Integration (BIDIR-6, BIDIR-7)
- Map bidirectional protocol to MCP notifications
- Implement WebSocket bidirectional handlers

### Phase 3: Validation (BIDIR-8 through BIDIR-10)
- Build example interactive activation
- Comprehensive test coverage
- Architecture documentation

## Design Principles

1. **Backward Compatible**: All existing methods continue to work unchanged
2. **Opt-in**: Bidirectional is a method-level feature, not required
3. **Transport Agnostic**: Works over MCP and WebSocket
4. **Type Safe**: Compile-time guarantees for request/response matching
5. **Cancellation Aware**: Respects existing cancellation mechanisms

## Tickets

| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| BIDIR-2 | Core types and traits | - | BIDIR-3, BIDIR-4, BIDIR-5 |
| BIDIR-3 | Channel-based activation context | BIDIR-2 | BIDIR-6, BIDIR-7 |
| BIDIR-4 | Hub-macro bidirectional support | BIDIR-2 | BIDIR-6, BIDIR-7 |
| BIDIR-5 | Helper functions and patterns | BIDIR-2 | BIDIR-6, BIDIR-7 |
| BIDIR-6 | MCP transport mapping | BIDIR-3, BIDIR-4, BIDIR-5 | BIDIR-8 |
| BIDIR-7 | WebSocket transport mapping | BIDIR-3, BIDIR-4, BIDIR-5 | BIDIR-8 |
| BIDIR-8 | Example interactive activation | BIDIR-6, BIDIR-7 | BIDIR-9, BIDIR-10 |
| BIDIR-9 | Test coverage | BIDIR-8 | - |
| BIDIR-10 | Architecture documentation | BIDIR-8 | - |

## Critical Path

```
BIDIR-2 → BIDIR-3 → BIDIR-6 → BIDIR-8 → BIDIR-9
```

Minimum viable: Types → Channels → MCP → Example → Tests

## Open Questions

1. **Request timeout**: How long should server wait for client response?
2. **Multiple requests**: Can server have multiple outstanding requests?
3. **MCP mapping**: Use progress notifications or custom notification type?
4. **Cancellation**: If client doesn't respond, auto-cancel or wait indefinitely?
5. **Session state**: Where to store pending request state?

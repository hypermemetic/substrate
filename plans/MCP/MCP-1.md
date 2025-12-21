# MCP-1: MCP Compatibility Epic

## Goal

Make Substrate fully compatible with Model Context Protocol (MCP) 2024-11-05 and 2025-03-26, enabling Claude Code and other MCP clients to use Plexus activations as tools.

## Architecture

MCP is a **view layer** over Plexus, not a replacement. Same activations, different delivery:

```
┌─────────────────────────────────────────────────────────────┐
│                      MCP Layer                               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ McpState    │  │ McpInterface│  │ Stream Buffering    │  │
│  │ Machine     │  │ (router)    │  │ (for tools/call)    │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         └────────────────┴───────────────────┬┘             │
│                                              ▼              │
│                    ┌─────────────────────────────┐          │
│                    │         Plexus              │          │
│                    │   (existing activations)    │          │
│                    └─────────────────────────────┘          │
└─────────────────────────────────────────────────────────────┘
```

## Dependency DAG

```
                         MCP-2 (McpState)
                              │
    ┌─────────────────────────┼─────────────────────────┬─────────────┐
    ▼                         ▼                         ▼             ▼
 MCP-3                     MCP-4                     MCP-5         MCP-9
(McpInterface)          (initialize)            (schema)       (buffering)
    │                         │                      │             │
    │                         ▼                      │             │
    │                      MCP-6                     │             │
    │                   (initialized)                │             │
    │                         │                      │             │
    │                         ▼                      │             │
    │                      MCP-7 (ping)              │             │
    │                         │                      │             │
    │              ┌──────────┴──────────┐           │             │
    │              ▼                     ▼           │             │
    │           MCP-8 ◄──────────────────┘           │             │
    │        (tools/list)                            │             │
    │                                                │             │
    └────────────────────────┬───────────────────────┴─────────────┘
                             ▼
                         MCP-10 (tools/call)
                              │
         ┌────────────────────┼────────────────────┐
         ▼                    ▼                    ▼
      MCP-11              MCP-12               MCP-13
   (stdio transport)  (HTTP SSE transport)  (cancellation)
         │                    │
         └────────────────────┼────────────────────┐
                              ▼                    ▼
                         MCP-14               MCP-15
                      (resources/*)         (prompts/*)
```

**Key insight:** `tools/list` and `tools/call` are independent - buffering (MCP-9) doesn't need schema (MCP-5).

## Phases

### Phase 1: Foundation
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-2  | McpState enum and state machine | - | MCP-3, MCP-4, MCP-5, MCP-9 |

### Phase 2: Core Handlers (Parallel)
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-3  | McpInterface struct wrapping Plexus | MCP-2 | MCP-10 |
| MCP-4  | `initialize` request handler | MCP-2 | MCP-6 |
| MCP-5  | Schema → MCP tool transform | MCP-2 | MCP-8 |
| MCP-9  | Stream buffering utilities | MCP-2 | MCP-10 |

### Phase 3: Lifecycle Completion
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-6  | `notifications/initialized` handler | MCP-4 | MCP-7 |
| MCP-7  | `ping` handler | MCP-6 | MCP-8, MCP-10 |

### Phase 4: Tools (Parallel)
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-8  | `tools/list` implementation | MCP-5, MCP-7 | - |
| MCP-10 | `tools/call` implementation | MCP-3, MCP-7, MCP-9 | MCP-11, MCP-12, MCP-13 |

### Phase 5: Transports & Extensions (Parallel)
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-11 | stdio MCP transport | MCP-10 | MCP-14, MCP-15 |
| MCP-12 | Streamable HTTP transport (SSE) | MCP-10 | MCP-14, MCP-15 |
| MCP-13 | `notifications/cancelled` support | MCP-10 | - |

### Phase 6: Optional Features (Parallel)
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-14 | `resources/*` (Arbor integration) | MCP-11 or MCP-12 | - |
| MCP-15 | `prompts/*` (Cone integration) | MCP-14 | - |

## Critical Path

```
MCP-2 → MCP-4 → MCP-6 → MCP-7 → MCP-10 → MCP-11
```

**Length: 6 tickets** (reduced from 8)

Parallelism opportunities:
- After MCP-2: 4 tickets can run concurrently (MCP-3, MCP-4, MCP-5, MCP-9)
- After MCP-7: 2 tickets can run concurrently (MCP-8, MCP-10)
- After MCP-10: 3 tickets can run concurrently (MCP-11, MCP-12, MCP-13)
- After MCP-11/12: 2 tickets can run concurrently (MCP-14, MCP-15)

## Success Criteria

1. `substrate --stdio --mcp` passes MCP validator tests
2. Claude Code can discover and call Plexus tools
3. Streamable HTTP mode enables real-time progress for long operations
4. Existing Plexus clients continue working unchanged

## References

- [MCP Compatibility Spec](../docs/architecture/16680473255155665663_mcp-compatibility.md)
- [MCP Specification 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26)

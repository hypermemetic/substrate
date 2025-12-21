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
    ┌─────────────────────────┼─────────────────────────┐
    ▼                         ▼                         ▼
 MCP-3                     MCP-4                     MCP-5
(McpInterface)          (initialize)              (schema)
    │                         │                      │
    │                         ▼                      │
    │                      MCP-6                     │
    │                   (initialized)                │
    │                         │                      │
    │                         ▼                      │
    │                      MCP-7 (ping)              │
    │                         │                      │
    │              ┌──────────┴──────────┐           │
    │              ▼                     ▼           │
    │           MCP-8 ◄──────────────────┘           │
    │        (tools/list)                            │
    │                                                │
    └────────────────────────┬───────────────────────┘
                             ▼
                         MCP-9 (tools/call + SSE streaming)
                              │
                   ┌──────────┴──────────┐
                   ▼                     ▼
                MCP-10               MCP-11
             (cancellation)       (resources/*)
                                      │
                                      ▼
                                   MCP-12
                                 (prompts/*)
```

**Key insight:** Streamable HTTP only - no stdio buffering. Clients needing buffered responses implement it client-side.

## Phases

### Phase 1: Foundation
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-2  | McpState enum and state machine | - | MCP-3, MCP-4, MCP-5 |

### Phase 2: Core Handlers (Parallel)
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-3  | McpInterface struct wrapping Plexus | MCP-2 | MCP-9 |
| MCP-4  | `initialize` request handler | MCP-2 | MCP-6 |
| MCP-5  | Schema → MCP tool transform | MCP-2 | MCP-8 |

### Phase 3: Lifecycle Completion
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-6  | `notifications/initialized` handler | MCP-4 | MCP-7 |
| MCP-7  | `ping` handler | MCP-6 | MCP-8, MCP-9 |

### Phase 4: Tools (Parallel)
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-8  | `tools/list` implementation | MCP-5, MCP-7 | - |
| MCP-9  | `tools/call` + SSE streaming | MCP-3, MCP-7 | MCP-10, MCP-11 |

### Phase 5: Extensions (Parallel)
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-10 | `notifications/cancelled` support | MCP-9 | - |
| MCP-11 | `resources/*` (Arbor integration) | MCP-9 | MCP-12 |

### Phase 6: Optional
| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| MCP-12 | `prompts/*` (Cone integration) | MCP-11 | - |

## Critical Path

```
MCP-2 → MCP-4 → MCP-6 → MCP-7 → MCP-9 → MCP-11 → MCP-12
```

**Length: 7 tickets** (12 total, down from 15)

Parallelism opportunities:
- After MCP-2: 3 tickets concurrently (MCP-3, MCP-4, MCP-5)
- After MCP-7: 2 tickets concurrently (MCP-8, MCP-9)
- After MCP-9: 2 tickets concurrently (MCP-10, MCP-11)

## Success Criteria

1. Streamable HTTP endpoint passes MCP validator tests
2. Claude Code can discover and call Plexus tools via HTTP
3. SSE streaming enables real-time progress for long operations
4. Existing Plexus clients continue working unchanged
5. No buffering layer needed - clients handle buffering if required

## References

- [MCP Compatibility Spec](../docs/architecture/16680473255155665663_mcp-compatibility.md)
- [MCP Specification 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26)

# HANDLE-1: HandleEnum Codegen Epic

## Overview

Implement declarative handle definition with automatic storage resolution via a `#[derive(HandleEnum)]` macro.

## Goal

Replace ad-hoc handle creation and manual resolution with a single enum definition that generates:
1. Handle creation (`enum → Handle`)
2. Handle parsing (`Handle → enum`)
3. Storage resolution (`enum → SQL query → data`)

## Dependency DAG

```
HANDLE-2 (derive macro)           HANDLE-7 (Arbor usage pattern)
    │                                  │
    ├────────────────┐                 ├────────────────┐
    ▼                ▼                 ▼                │
HANDLE-3         HANDLE-4         HANDLE-8             │
(Cone)           (ClaudeCode)     (Cone tests)         │
    │                │                                 │
    └────────────────┘                                 │
              │                                        │
              ▼                                        │
        HANDLE-5 ◄─────────────────────────────────────┘
    (Plexus integration)
              │
              ├────────────────┐
              ▼                ▼
        HANDLE-6          HANDLE-9
      (Documentation)  (Handle resolution docs)
```

## Tickets

| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| HANDLE-2 | Implement `#[derive(HandleEnum)]` macro | - | 3,4 |
| HANDLE-3 | Define ConeHandle enum | 2 | 5 |
| HANDLE-4 | Define ClaudeCodeHandle enum | 2 | 5 |
| HANDLE-5 | Plexus resolve_handle RPC method | 3,4 | 6,9 |
| HANDLE-6 | Update docs and examples | 5,9 | - |
| HANDLE-7 | Clarify Arbor usage pattern (direct vs Plexus) | - | 8,9 |
| HANDLE-8 | Update Cone tests to use direct ArborStorage | 7 | - |
| HANDLE-9 | Document handle resolution via Plexus | 5,7 | 6 |

## Activations

| Activation | Handle Types |
|------------|--------------|
| Cone | Cone, Message |
| ClaudeCode | Session, ChatEvent |

## Success Criteria

1. `#[derive(HandleEnum)]` generates all boilerplate
2. Cone and ClaudeCode define handle enums
3. Resolution works through Plexus via plugin registry
4. No manual `resolve_handle` implementations needed
5. Handles are type-safe and self-documenting

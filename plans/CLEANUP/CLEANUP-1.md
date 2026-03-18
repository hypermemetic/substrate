# CLEANUP-1: Repository Hygiene — Master Plan

## Goal

Clean up everything in the repository that isn't source code: tracked runtime
artifacts, transient TLC outputs, superseded plans, misplaced ticket files, and
stale documentation. No code changes.

## What was found

**Tracked artifacts that should never be in git:**
- 7 SQLite `.db` files at the root (1.6M arbor.db alone). `.gitignore` has `*.db`
  on line 34 but these predate the rule — they're force-tracked.
- `.DS_Store` macOS artifact — tracked despite `.gitignore`
- `plans/states/` — TLC model checker intermediate state files (~8 timestamped
  directories, hundreds of `.st`/`.fp`/`nodes_*`/`ptrs_*` files). In `.gitignore`
  but tracked.
- `plans/*.bin` — TLC trace files (DispatchTdd, MediumBatch, SubstrateResilience
  traces). In `.gitignore` but tracked.

**Superseded plans:**
- `plans/tdd-node.tickets.md` — explicitly replaced by `tdd-node-v2.tickets.md`
  ("This document replaces tdd-node.tickets.md")

**Plans not following EPIC convention:**
These files sit flat at `plans/` root instead of `plans/<EPIC>/<EPIC>-N.md`:
- `plans/dispatch-plan.tickets.md` → should be `plans/DISPATCH/DISPATCH-1.md`
- `plans/medium-batch.tickets.md` → should be `plans/RUNPLAN/RUNPLAN-1.md`
- `plans/tdd-node-v2.tickets.md` → should be under a TDD epic

**Docs to review:**
- `docs/LOOPBACK_BLOCKING_APPROVAL.md` — unusual permissions (600 not 644);
  may be a stale working note
- `docs/COMPARISON.md` and `docs/REBRAND.md` — old branding docs, may be archivable
- `docs/architecture/16680205403394519551_mcp-to-arbor-flow.md` and
  `docs/architecture/16680205403394519551_nested-plugin-rpc-mismatch.md` — same
  numeric prefix (prefix collision or intentional?)
- `docs/architecture/__index.md` — needs update to include recent additions

## Dependency DAG

```
CLEANUP-2 (untrack artifacts)
    │
    └─► CLEANUP-3 (strengthen .gitignore)

CLEANUP-4 (delete superseded plan)
    │
    └─► CLEANUP-5 (reorganize root ticket files into EPIC folders)

CLEANUP-6 (triage docs/)
    │
    └─► CLEANUP-7 (update __index.md)
```

CLEANUP-2/3, CLEANUP-4/5, and CLEANUP-6/7 are three independent tracks.
All can start simultaneously once CLEANUP-1 is read.

## Constraints

- **No code changes.** Do not touch anything in `src/`.
- **No Cargo.toml or Cargo.lock changes.**
- **No changes to `.tla` or `.cfg` files** — the formal specs are valuable.
- Keep `.db` files on disk; only untrack them from git. Substrate needs them at runtime.
- Keep TLA+ specs and configs (`.tla`, `.cfg`). Only remove transient outputs
  (`.bin` traces, `states/` directories).
- Keep all docs in `docs/architecture/old/` — they're properly archived.

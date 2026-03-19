# CLEANUP-7: Update docs/architecture/__index.md [agent]

blocked_by: [CLEANUP-6]
unlocks: []

## Goal

`__index.md` is the curated entry point for architecture docs. After CLEANUP-6
potentially archives or removes some docs, and given recent additions
(`intro-lattice-orcha-tdd.md`, `orcha-approval-workflow.md`), update the index
to reflect current state.

## Read the current index first

```bash
cat /workspace/hypermemetic/plexus-substrate/docs/architecture/__index.md
```

## Changes to make

### 1. Add the new onboarding doc

`intro-lattice-orcha-tdd.md` (written today) is the canonical entry point for
new engineers. It should be the first entry in the index, prominently placed.

Add under a "Start Here" section or at the top of the existing first section:

```markdown
## Start Here

- **[intro-lattice-orcha-tdd.md](intro-lattice-orcha-tdd.md)** — Full stack
  introduction: Plexus RPC → Lattice → Orcha → TDD node. Read this first.
```

### 2. Add the orcha-approval-workflow doc if missing

`16772656563233000000_orcha-approval-workflow.md` was added in early March.
If it's not in the index, add it under the Orcha section.

### 3. Remove any entries pointing to docs deleted in CLEANUP-6

If CLEANUP-6 deleted or archived any docs that are referenced in `__index.md`,
remove or update those entries.

### 4. Check section coverage

Scan the index for sections. Verify the following topics have at least one entry:
- Lattice / DAG engine
- Orcha orchestration
- TDD node (new — may need adding)
- Handle system
- Schema / codegen
- ClaudeCode / loopback

If the TDD node design is not represented, add a pointer to:
- `plans/TDD/TDD-1.md` — implementation plan
- `plans/DispatchTdd.tla` — formal spec

These aren't in `docs/architecture/` but the index can link to plans.

## Commit

```bash
git commit -m "docs: update __index.md — add onboarding doc, reflect recent additions

Add intro-lattice-orcha-tdd.md as first entry under 'Start Here'.
Add orcha-approval-workflow.md to Orcha section.
Add TDD node references.
Remove any entries invalidated by CLEANUP-6 doc triage.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

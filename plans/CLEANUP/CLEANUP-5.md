# CLEANUP-5: Move root-level ticket files into EPIC folders [agent]

blocked_by: [CLEANUP-4]
unlocks: []

## Goal

Three ticket files sit at `plans/` root instead of following the
`plans/<EPIC>/<EPIC>-N.md` convention defined in `CLAUDE.md`. Move them.

## Files to move

| Current path | New path | Notes |
|---|---|---|
| `plans/dispatch-plan.tickets.md` | `plans/DISPATCH/DISPATCH-1.md` | L5-WIRE: thread GraphRuntime/CancelRegistry into dispatch_node |
| `plans/medium-batch.tickets.md` | `plans/RUNPLAN/RUNPLAN-1.md` | run_plan hub method implementation |
| `plans/tdd-node-v2.tickets.md` | `plans/TDD/TDD-1.md` | TDD node v2 implementation plan, 7 tickets |

## Operations

```bash
cd /workspace/hypermemetic/plexus-substrate

mkdir -p plans/DISPATCH plans/RUNPLAN plans/TDD

git mv plans/dispatch-plan.tickets.md plans/DISPATCH/DISPATCH-1.md
git mv plans/medium-batch.tickets.md  plans/RUNPLAN/RUNPLAN-1.md
git mv plans/tdd-node-v2.tickets.md   plans/TDD/TDD-1.md
```

## Update internal references

After moving, check if any file references these old paths by name and update:

```bash
grep -r "tdd-node-v2.tickets.md\|dispatch-plan.tickets.md\|medium-batch.tickets.md" \
  /workspace/hypermemetic/plexus-substrate \
  --include="*.md" \
  --exclude-dir=".git"
```

For each match found: update the reference to the new path. The most likely
locations are:
- `README.md` (references `tdd-node-v2.tickets.md` in the roadmap section)
- `docs/architecture/__index.md` (if it indexes these)
- `CLAUDE.md` (unlikely but check)

**In README.md specifically:** the roadmap section references
`tdd-node-v2.tickets.md` — update to `plans/TDD/TDD-1.md`.

## Commit

```bash
git commit -m "chore: move root-level ticket files into EPIC folders

dispatch-plan.tickets.md → DISPATCH/DISPATCH-1.md
medium-batch.tickets.md  → RUNPLAN/RUNPLAN-1.md
tdd-node-v2.tickets.md   → TDD/TDD-1.md

Follows the plans/<EPIC>/<EPIC>-N.md convention in CLAUDE.md.
Update path references in README.md.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

## Verification

```bash
# New paths exist
git ls-files plans/DISPATCH/DISPATCH-1.md
git ls-files plans/RUNPLAN/RUNPLAN-1.md
git ls-files plans/TDD/TDD-1.md

# Old paths gone
git ls-files plans/dispatch-plan.tickets.md  # nothing
git ls-files plans/medium-batch.tickets.md   # nothing
git ls-files plans/tdd-node-v2.tickets.md    # nothing

# No stale path references in markdown
grep -r "tdd-node-v2.tickets.md\|dispatch-plan.tickets.md\|medium-batch.tickets.md" \
  /workspace/hypermemetic/plexus-substrate --include="*.md" --exclude-dir=".git"
# should return nothing
```

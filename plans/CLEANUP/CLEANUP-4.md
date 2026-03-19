# CLEANUP-4: Delete superseded tdd-node.tickets.md [agent]

blocked_by: []
unlocks: [CLEANUP-5]

## Goal

`plans/tdd-node.tickets.md` is explicitly superseded. The first line of
`plans/tdd-node-v2.tickets.md` says:
> "This document replaces `tdd-node.tickets.md`."

Delete it from git. It is not archived — it's replaced, not just old.

## Operation

```bash
cd /workspace/hypermemetic/plexus-substrate
git rm plans/tdd-node.tickets.md
git commit -m "chore: delete superseded tdd-node.tickets.md

Replaced by tdd-node-v2.tickets.md per the v2 document's own declaration.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

## Verification

```bash
git ls-files plans/tdd-node.tickets.md  # should return nothing
git ls-files plans/tdd-node-v2.tickets.md  # should still be present
```

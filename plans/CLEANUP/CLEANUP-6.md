# CLEANUP-6: Triage docs/ and docs/architecture/ [agent]

blocked_by: []
unlocks: [CLEANUP-7]

## Goal

Review specific documentation artifacts identified as potentially stale, misplaced,
or anomalous. Make a disposition decision on each and act on it.

## Items to triage

### 1. docs/LOOPBACK_BLOCKING_APPROVAL.md

Read the full file. Determine:
- Is it a design doc for a feature that shipped? If yes, archive to `docs/architecture/`
  with the correct naming convention (`(u64::MAX - nanotime)_loopback-blocking-approval.md`).
- Is it a stale working note? If yes, delete it.
- Is it still actively relevant as a top-level doc? If yes, leave it and fix permissions
  (`chmod 644 docs/LOOPBACK_BLOCKING_APPROVAL.md`).

To generate the correct archive filename:
```python
import time
nanotime = int(time.time() * 1_000_000_000)
prefix = (2**64 - 1) - nanotime
print(f'{prefix}_loopback-blocking-approval.md')
```

### 2. docs/COMPARISON.md

Read the file. Determine:
- What is being compared? Old design vs new? If it's a historical comparison that
  informed a decision now made, archive or delete.
- If still useful as reference, leave it.

### 3. docs/REBRAND.md

Read the file. Determine:
- Is the rebrand complete? If the decision has been made and terminology is now
  consistent, this is a historical artifact. Archive or delete.
- If still informing active decisions, leave it.

### 4. Duplicate-prefix architecture docs

Two docs share the exact same numeric prefix `16680205403394519551`:
- `docs/architecture/16680205403394519551_mcp-to-arbor-flow.md`
- `docs/architecture/16680205403394519551_nested-plugin-rpc-mismatch.md`

Read both. Determine if they're actually distinct documents covering different topics
(likely — same timestamp, written in the same session) or if one is a duplicate/
early draft of the other.

If both are distinct and valid: the collision is harmless (both sort together),
no action needed.

If one supersedes the other: delete the older one.

## Decision log

After reading each file, document your decision in this format at the bottom of
this ticket (edit the file in place):

```
LOOPBACK_BLOCKING_APPROVAL.md: [archive | delete | keep — reason]
COMPARISON.md: [archive | delete | keep — reason]
REBRAND.md: [archive | delete | keep — reason]
prefix-collision docs: [both keep | delete X — reason]
```

Then execute the decisions.

## Commit

```bash
git commit -m "chore: triage docs/ — archive or delete stale documentation

[List what was actually done here based on decisions above]

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

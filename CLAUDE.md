Often I will say "open this" in reference to code use `codium --goto` to take me there

if you need to see the dir structure use the `tre` command

## Architecture Documentation Naming Convention

Architecture documents in `docs/architecture/` use reverse-chronological naming to ensure newest documents appear first in alphabetical sorting.

**Naming formula**: `(u64::MAX - nanotime)_title.md`

Where:
- `nanotime` = current Unix timestamp in nanoseconds
- This creates a descending numeric prefix (newer = smaller number = sorts first)
- Example: `16681577588676290559_type-system.md`

**To generate a filename**:
```python
import time
nanotime = int(time.time() * 1_000_000_000)
filename = (2**64 - 1) - nanotime
print(f'{filename}_your-title.md')
```

This "chronological bubbling" helps prioritize recent architectural decisions.


This project consists of a few pieces: Substrate, Symbols, and Cllient. All should be in the parent which should be called `controlflow`
When I say "create an architecture doc and link it" I mean to create the same file in whichever subprojects it makes sense to add to. We then have a claude instance which reads them there and this steers cross project development. Ideally when I say "write a doc" that implicitly includes linking it and also mentioning it in the commit you add it to. usually we write a doc when we are going to commit anyway. if I'm asking for one determine if this is at a commit boundary

## Planning Documents Convention

Planning docs live in `plans/<EPIC>/` where EPIC is a short prefix (e.g., MCP, ARBOR, CONE).

**Structure:**
```
plans/
  MCP/
    MCP-1.md   # Epic overview (the "master plan")
    MCP-2.md   # Individual ticket
    MCP-3.md   # Individual ticket
    ...
```

**Ticket format:** `<EPIC>-<N>.md` where N is sequential within the epic.

**Epic overview (X-1.md)** contains:
- High-level goal and context
- Dependency DAG showing which tickets unlock others
- Phase breakdown

**Individual tickets (X-N.md)** contain:
- `blocked_by: [X-2, X-3]` - tickets that must complete first
- `unlocks: [X-5, X-6]` - tickets that can start once this completes
- Scope, acceptance criteria, implementation notes

**Design for parallelism:** Structure tickets so completing one unlocks multiple concurrent tickets. The DAG should fan out, not be linear. Identify the critical path and minimize its length.

```
     X-2 (foundation)
      │
  ┌───┼───┬───┐
  ▼   ▼   ▼   ▼
 X-3 X-4 X-5 X-6  ← parallel work
  │   │   │   │
  └───┴───┼───┘
          ▼
        X-7 (integration)
```

When a ticket is completed, check its `unlocks` field to identify newly unblocked work.
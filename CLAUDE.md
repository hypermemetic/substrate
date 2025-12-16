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
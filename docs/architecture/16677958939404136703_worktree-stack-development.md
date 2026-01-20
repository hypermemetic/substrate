# Worktree Stack Development

> How to work on multiple interdependent crates in isolation using git worktrees

## Problem

The hypermemetic project has a stack of interdependent crates:

```
substrate (main application)
    ├── hub-core (core infrastructure)
    │       └── hub-macro (proc macros)
    └── hyperforge (plugin system)
```

When developing a feature that touches multiple crates:
- Modifying `hub-core` triggers rebuilds of `substrate`
- Changes to the main crates affect other developers
- Hard to test changes in isolation

## Solution: Worktree Stack

Create git worktrees for all crates you need to modify, with paths configured so they depend on each other.

### Directory Structure

```
/Users/shmendez/dev/controlflow/hypermemetic/
├── substrate/              # Main crate (don't modify)
├── hub-core/               # Core crate (don't modify)
├── hub-macro/              # Macro crate (don't modify)
├── hyperforge/             # Plugin crate (don't modify)
└── worktrees/
    └── generic-hub-context/    # Feature worktree
        ├── Cargo.toml          # Substrate (modified paths)
        ├── src/                # Substrate source
        ├── hub-core/           # Hub-core worktree
        │   ├── Cargo.toml
        │   └── src/
        └── hub-macro/          # Hub-macro worktree
            ├── Cargo.toml
            └── src/
```

## Setup Process

### 1. Create the base worktree (substrate)

```bash
cd /Users/shmendez/dev/controlflow/hypermemetic/substrate
mkdir -p ../worktrees
git worktree add ../worktrees/my-feature feature/my-feature
```

### 2. Create worktrees for dependent crates

For each crate you need to modify:

```bash
# Hub-core
cd /Users/shmendez/dev/controlflow/hypermemetic/hub-core
git checkout -b feature/my-feature  # Create branch if needed
git checkout main                    # Switch back
git worktree add ../worktrees/my-feature/hub-core feature/my-feature

# Hub-macro
cd /Users/shmendez/dev/controlflow/hypermemetic/hub-macro
git checkout -b feature/my-feature
git checkout main
git worktree add ../worktrees/my-feature/hub-macro feature/my-feature
```

### 3. Update Cargo.toml paths

The substrate `Cargo.toml` needs paths pointing to local worktree copies:

**Before (original):**
```toml
hub-macro = { path = "../hub-macro" }
hub-core = { path = "../hub-core" }
hyperforge = { path = "../hyperforge" }
cllient = { path = "../../juggernautlabs/cllient" }
```

**After (worktree):**
```toml
hub-macro = { path = "./hub-macro" }
hub-core = { path = "./hub-core" }
hyperforge = { path = "../../hyperforge" }  # Not in worktree, adjust path
cllient = { path = "../../../juggernautlabs/cllient" }  # Adjust for depth
```

### 4. Fix workspace conflicts

If dependent crates have their own `[workspace]` section, remove it and add them to substrate's workspace:

**hub-core/Cargo.toml** - Remove:
```toml
[workspace]
members = ["."]
```

**substrate/Cargo.toml** - Update:
```toml
[workspace]
members = [".", "hub-core", "hub-macro"]
```

### 5. Fix dev-dependency paths

If hub-macro has a dev-dependency on substrate:

**hub-macro/Cargo.toml:**
```toml
[dev-dependencies]
# Before: substrate = { path = "../substrate", package = "substrate-hub" }
substrate = { path = "..", package = "substrate-hub" }
```

### 6. Verify compilation

```bash
cd /Users/shmendez/dev/controlflow/hypermemetic/worktrees/my-feature
cargo check
```

## Path Reference

From the worktree at `/Users/shmendez/dev/controlflow/hypermemetic/worktrees/my-feature/`:

| Target | Relative Path |
|--------|---------------|
| Local hub-macro | `./hub-macro` |
| Local hub-core | `./hub-core` |
| Main hyperforge | `../../hyperforge` |
| Main substrate | `../../substrate` |
| juggernautlabs/cllient | `../../../juggernautlabs/cllient` |

## Common Pitfalls

### 1. sqlx version conflicts

If one crate uses sqlx 0.8 and another uses 0.6, you'll get:
```
package `libsqlite3-sys` links to the native library `sqlite3`, but it conflicts...
```

**Solution:** Keep sqlx versions consistent across the stack, or don't add sqlx to hub-core (keep it in substrate only).

### 2. Multiple workspace roots

```
error: multiple workspace roots found in the same workspace
```

**Solution:** Remove `[workspace]` from dependent crates and add them to the main workspace.

### 3. Branch doesn't exist in dependent crate

```
fatal: invalid reference: feature/my-feature
```

**Solution:** Create the branch first:
```bash
git checkout -b feature/my-feature
git checkout main
git worktree add ...
```

### 4. Branch already checked out

```
fatal: 'feature/my-feature' is already used by worktree at '...'
```

**Solution:** You're on that branch. Switch to main first:
```bash
git checkout main
git worktree add ...
```

## Subagent Instructions

When spawning subagents to work on the stack:

```
**IMPORTANT: Work ONLY in the worktree at /Users/shmendez/dev/controlflow/hypermemetic/worktrees/my-feature/**

Files to modify:
- /Users/shmendez/dev/controlflow/hypermemetic/worktrees/my-feature/hub-macro/src/...
- /Users/shmendez/dev/controlflow/hypermemetic/worktrees/my-feature/hub-core/src/...
- /Users/shmendez/dev/controlflow/hypermemetic/worktrees/my-feature/src/...

DO NOT modify files in:
- /Users/shmendez/dev/controlflow/hypermemetic/hub-macro/
- /Users/shmendez/dev/controlflow/hypermemetic/hub-core/
- /Users/shmendez/dev/controlflow/hypermemetic/substrate/

Verify compilation: cd /Users/shmendez/dev/controlflow/hypermemetic/worktrees/my-feature && cargo check
```

## Cleanup

When done with the feature:

```bash
# Remove worktrees
git worktree remove ../worktrees/my-feature/hub-macro
git worktree remove ../worktrees/my-feature/hub-core
git worktree remove ../worktrees/my-feature

# Or force remove if dirty
git worktree remove --force ../worktrees/my-feature
```

## Current Worktree: generic-hub-context

The `generic-hub-context` worktree is set up for the HandleEnum feature:

```
/Users/shmendez/dev/controlflow/hypermemetic/worktrees/generic-hub-context/
├── Cargo.toml              # workspace: [".", "hub-core", "hub-macro"]
├── hub-core/               # feature/generic-hub-context branch
├── hub-macro/              # feature/generic-hub-context branch
├── plans/HANDLE/           # Implementation plan
└── docs/architecture/      # Design docs
```

All HANDLE ticket work should happen in this worktree.

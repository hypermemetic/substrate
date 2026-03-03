# DEVEX-1: Developer Experience & Setup Improvements

## Overview

Improve developer experience when setting up and using substrate, synapse, and the codegen toolchain. Address confusion around binary names, error messages, documentation, and version mismatches that create friction during onboarding and development.

## Problem Statement

Current pain points identified during benchmark testing:
- **Cryptic database errors** that don't explain how to fix them
- **Binary name confusion** (substrate vs plexus-substrate in scripts)
- **Outdated local synapse** (v0.3.0.0 → 101x slower than published v3.5.0)
- **Missing quick start** documentation
- **Package naming inconsistency** (hub-codegen vs plexus_codegen_typescript)
- **No health check commands** to verify system state

These issues add ~40 minutes of debugging time for a task that should take 2 minutes.

## Goals

1. Make setup/first-run experience clear and fast
2. Provide actionable error messages that tell users what to do
3. Ensure documentation matches actual binary names and commands
4. Add health check and diagnostic commands
5. Catch common issues in CI before they reach developers

## Recommendations

### 1. Improve Database Error Messages ⭐ HIGH PRIORITY

**Current Error:**
```
Failed to initialize Orcha storage: "Failed to get table info: error occurred
while decoding column 0: mismatched types; Rust type `alloc::string::String`
(as SQL type `TEXT`) is not compatible with SQL type `INTEGER`"
```

**Improved Error:**
```
❌ Substrate database schema is incompatible with current version.

This usually happens after upgrading substrate with existing databases.

To fix, choose one:
  1. Reset all databases:
     plexus-substrate --reset-db -p 4444

  2. Manual cleanup:
     rm -rf ~/.plexus/substrate/activations
     plexus-substrate -p 4444

  3. Backup and reset:
     cp -r ~/.plexus/substrate/activations ~/.plexus/substrate/activations.backup
     plexus-substrate --reset-db -p 4444

For more information: https://docs.plexus.dev/troubleshooting/database-errors
```

**Implementation:**
- Catch SQLite schema mismatch errors in `src/activations/storage.rs`
- Wrap in custom error type with user-friendly message
- Add `--reset-db` flag to plexus-substrate CLI
- Include link to troubleshooting docs

### 2. Fix Script Binary Names

**Issue:** Scripts reference `substrate` but actual binary is `plexus-substrate`

**Files to fix:**
- `scripts/start-substrate.sh`
- Any other scripts or docs referencing the binary

**Change:**
```bash
# Before
SUBSTRATE_BIN="../plexus-substrate/target/debug/substrate"

# After
SUBSTRATE_BIN="../plexus-substrate/target/debug/plexus-substrate"
```

**Alternative:** Create symlink during build:
```bash
# In build.rs or Cargo.toml
ln -sf plexus-substrate substrate
```

### 3. Update Synapse to v3.5.0

**Issue:** Local synapse build is v0.3.0.0 (8.5s), published is v3.5.0 (0.08s)
- **101x performance difference**

**Action:**
- Update synapse repository code to match published v3.5.0
- Verify local build matches published performance
- Document in README to use `cabal install plexus-synapse` for latest version

### 4. Add Quick Start Documentation

**Add to README.md (or QUICKSTART.md):**

```markdown
## Quick Start

### 1. Start Substrate

```bash
cd plexus-substrate
cargo build --release
./target/release/plexus-substrate -p 4444
```

**If substrate fails with database error:**
```bash
plexus-substrate --reset-db -p 4444
```

### 2. Generate IR from Substrate

```bash
# Install latest synapse (recommended)
cabal install plexus-synapse

# Generate IR (takes ~0.08 seconds)
synapse -i > ir.json
```

### 3. Generate TypeScript Code

```bash
cd hub-codegen
cargo run --release -- -t typescript -o ./output < ir.json
```

## Troubleshooting

**Substrate won't start:**
- Check if already running: `ps aux | grep plexus-substrate`
- Kill existing: `pkill plexus-substrate`
- Reset databases: `plexus-substrate --reset-db`

**Synapse can't find backend:**
- Ensure substrate is running (should see "substrate" in `synapse` output)
- Default port is 4444
- Check substrate logs: `tail -f /tmp/substrate.log`

**Tests fail to compile:**
- Run `cargo clean && cargo build`
- Ensure imports match package name in Cargo.toml
```

### 5. Add Health Check Commands

**New CLI flags for plexus-substrate:**

```rust
// Add to src/main.rs CLI args
#[derive(Parser)]
struct Args {
    // ... existing args ...

    /// Display health status and exit
    #[arg(long)]
    health: bool,

    /// Reset all databases (with confirmation)
    #[arg(long)]
    reset_db: bool,

    /// Display version information
    #[arg(short = 'V', long)]
    version: bool,
}
```

**Usage:**
```bash
plexus-substrate --health
# Output: ✅ Substrate running on port 4444 (39 plugins, 166 methods)

plexus-substrate --version
# Output: plexus-substrate 0.2.7 (ce2029c84e46951b)

plexus-substrate --reset-db
# Prompt: This will delete all databases. Continue? [y/N]
```

### 6. CI Tests for Common Issues

**Add to `.github/workflows/test.yml`:**

```yaml
name: Test

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Build
        run: cargo build --release

      - name: Test
        run: cargo test --all

      - name: Check for broken imports
        run: |
          # Fail if old package names still referenced
          ! grep -r "use hub_codegen::" src/ tests/ || \
            (echo "ERROR: Found old package name 'hub_codegen'" && exit 1)

      - name: Verify binary names
        run: |
          test -f target/release/plexus-substrate || \
            (echo "ERROR: Binary not found at expected location" && exit 1)
```

### 7. Document Package Naming Consistency

**Add to README.md:**

```markdown
## Project Structure

This project has multiple related components:

| Component | Name | Usage |
|-----------|------|-------|
| Substrate (this repo) | `plexus-substrate` | Runtime server |
| Synapse | `synapse` | IR generation CLI |
| Codegen | `hub-codegen` (repo)<br>`plexus_codegen_typescript` (lib) | Code generator |

**When using in code:**
```rust
// Substrate
use plexus_substrate::*;

// Codegen
use plexus_codegen_typescript::*;
```

**When running:**
```bash
plexus-substrate -p 4444
synapse -i > ir.json
cargo run --bin hub-codegen
```
```

### 8. Add Makefile for Common Tasks

**Create `Makefile` in substrate root:**

```makefile
.PHONY: start stop health reset test clean help

help:
	@echo "Common substrate tasks:"
	@echo "  make start   - Start substrate on port 4444"
	@echo "  make stop    - Stop substrate"
	@echo "  make health  - Check substrate health"
	@echo "  make reset   - Reset databases"
	@echo "  make test    - Run tests"
	@echo "  make clean   - Clean build and databases"

start:
	@echo "Starting substrate..."
	@cargo build --release
	@./target/release/plexus-substrate -p 4444 > /tmp/substrate.log 2>&1 &
	@sleep 2
	@if pgrep -f plexus-substrate > /dev/null; then \
		echo "✅ Substrate started on port 4444"; \
	else \
		echo "❌ Failed to start substrate. Check /tmp/substrate.log"; \
		exit 1; \
	fi

stop:
	@pkill plexus-substrate && echo "✅ Stopped substrate" || echo "⚠️  No substrate process found"

health:
	@./target/release/plexus-substrate --health || \
		echo "❌ Substrate not running or health check failed"

reset:
	@echo "This will delete all databases. Are you sure? [y/N] " && read ans && [ $${ans:-N} = y ]
	@rm -rf ~/.plexus/substrate/activations
	@echo "✅ Databases reset"

test:
	@cargo test --all

clean: stop reset
	@cargo clean
	@echo "✅ Cleaned build artifacts and databases"
```

## Implementation Plan

### Phase 1: Critical Fixes (Week 1)
1. Improve database error messages with actionable guidance
2. Fix script binary names (plexus-substrate)
3. Add `--reset-db` flag to substrate
4. Update README with Quick Start section

### Phase 2: Developer Tools (Week 2)
5. Add `--health` and `--version` flags
6. Create Makefile for common tasks
7. Update synapse to v3.5.0
8. Add CI checks for common issues

### Phase 3: Documentation (Week 3)
9. Document package naming conventions
10. Add troubleshooting section to docs
11. Create troubleshooting wiki/docs page

## Success Metrics

- **Time to first run:** < 5 minutes (from git clone to working IR generation)
- **Database errors:** Users know how to fix them without debugging
- **Script failures:** 0 (all scripts use correct binary names)
- **CI failures:** Catch import/naming issues before merge

## Dependencies

- None (can implement independently)

## Related Issues

- Package rename from hub-codegen → plexus-codegen-typescript left broken imports
- Synapse repository version mismatch (v0.3.0.0 vs v3.5.0 published)
- No automated way to detect/fix database schema issues

## Testing Plan

1. Fresh VM test: Clone repos, follow Quick Start, verify it works
2. Database error test: Corrupt database, verify error message is helpful
3. Script test: Run all scripts, verify they find correct binaries
4. CI test: Verify CI catches broken imports and naming issues

## Notes

- The 101x performance difference in synapse versions suggests the local build is very stale
- Most pain points are documentation/UX issues, not fundamental architecture problems
- Quick wins can dramatically improve developer experience

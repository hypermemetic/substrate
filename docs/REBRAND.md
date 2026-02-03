# Hypermemetic Rebranding Guide

This document explains how to use claude-container to execute the rebranding plan.

## Quick Start

```bash
# First time: build the container with Rust + Haskell
claude-container -s rebrand \
  --config .claude-projects-rebrand.yml \
  -f Dockerfile.claude-container \
  --build

# Subsequent sessions: reuse cached image
claude-container -s rebrand \
  --config .claude-projects-rebrand.yml \
  -f Dockerfile.claude-container
```

## Session Workflow

### 1. Start a Phase
```bash
# Phase 1: Core README rewrites
claude-container -s rebrand-phase1 --config .claude-projects-rebrand.yml -f Dockerfile.claude-container

# Phase 2: Examples
claude-container -s rebrand-phase2 --config .claude-projects-rebrand.yml -f Dockerfile.claude-container
```

### 2. Review Changes
```bash
# See what Claude changed
claude-container --diff-session rebrand-phase1

# List all sessions
claude-container --list-sessions
```

### 3. Merge or Discard
```bash
# Happy with changes? Merge them
claude-container --merge-session rebrand-phase1

# Not happy? Delete and start over
claude-container --delete-session rebrand-phase1
```

## The Rebranding Plan

### Phase 1: Naming & Root README (Week 1)

**Goal**: Create the "front door" experience

Tasks for Claude:
1. Create `/README.md` (root) with:
   - One-liner: "Write Rust, get TypeScript SDKs. Zero drift, instant streaming."
   - 30-second code example
   - Comparison table (vs gRPC, OpenAPI, tRPC)
   - Quick start link

2. Update `Cargo.toml` descriptions in:
   - hub-core
   - hub-macro
   - hub-transport
   - hub-codegen
   - substrate

3. Create `/docs/QUICKSTART.md`

### Phase 2: Individual READMEs (Week 2)

**Goal**: Each crate tells its story

Tasks for Claude:
1. Rewrite `hub-core/README.md` - focus on Activation trait
2. Rewrite `hub-macro/README.md` - "code IS schema" message
3. Rewrite `hub-transport/README.md` - protocol matrix
4. Rewrite `hub-codegen/README.md` - SDK generation story
5. Rewrite `substrate/README.md` - orchestrator framing

### Phase 3: Examples (Week 3)

**Goal**: Production-ready examples

Tasks for Claude:
1. Create `examples/todo-api/` - full CRUD example
2. Create `examples/streaming/` - progress reporting
3. Create `examples/typescript-client/` - SDK usage patterns

### Phase 4: Docs Restructure (Week 4)

**Goal**: Move academic content, create practical guides

Tasks for Claude:
1. Create `/docs/CONCEPTS.md` - Activation, Hub, Schema, Streaming
2. Create `/docs/COMPARISON.md` - vs alternatives
3. Move architecture docs to `/docs/internals/`
4. Strip category theory from front matter of synapse/README.md

## Prompt Templates

### For Phase 1 (Root README)
```
Read the rebranding plan in REBRAND.md, then create the root README.md
following the structure outlined in Phase 1.

Key messaging:
- "Write Rust, get TypeScript SDKs"
- Schema comes from code, not YAML/proto files
- Streaming is built-in, not bolted-on
- Services compose into trees

Show a real code example from hub-macro, then the generated TypeScript.
```

### For README Rewrites
```
Rewrite hub-core/README.md with practical framing:
1. What problem does this solve? (composable RPC services)
2. 10-line example showing Activation trait
3. When to use this vs hub-transport vs substrate
4. Link to detailed docs

Remove academic terminology from the intro. Keep technical depth
in "Architecture" section for contributors.
```

### For Examples
```
Create examples/todo-api/ with:
- src/main.rs: TodoService activation with CRUD methods
- src/types.rs: Todo struct, TodoEvent enum
- README.md: How to run, how to call from curl, how to use TS client
- tests/: Basic integration tests

Use SQLite for storage. Show proper error handling with PlexusStreamItem::Error.
```

## Useful Commands Inside Container

```bash
# Check all Rust crates compile
check-all

# Build synapse (Haskell)
build-synapse

# Test TypeScript SDK
test-ts

# Run substrate server
cd substrate && cargo run --release

# Generate TypeScript client
cd substrate && cargo run --release &
cd ../hub-codegen && cargo run -- --typescript ../substrate-sandbox-ts/src/generated
```

## Files to Focus On

**High Priority (user-facing)**:
- `/README.md` (create)
- `/docs/QUICKSTART.md` (create)
- `hub-core/README.md`
- `hub-macro/README.md`
- `substrate/README.md`
- `substrate-sandbox-ts/README.md`

**Medium Priority**:
- `hub-transport/README.md`
- `hub-codegen/README.md`
- `synapse/README.md`

**Low Priority (move to internals)**:
- `substrate/docs/architecture/*.md`
- `synapse/docs/architecture/*.md`

## Success Criteria

After rebranding:
1. New user can understand "what is this?" in 30 seconds
2. Comparison with gRPC/OpenAPI is clear
3. "Hello World" works in 5 minutes
4. Academic terminology is in internals, not front matter
5. All crate descriptions are practical, not abstract

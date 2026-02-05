# Plexus RPC: Ecosystem Naming Strategy

## Decision: Protocol Name = "Plexus RPC"

With "Plexus RPC" as the protocol name, here's the complete naming strategy for all projects in the ecosystem.

---

## Current State (Before Rebrand)

```
Protocol (unnamed explicitly):
  - Referred to as "the protocol", "substrate protocol", "hub protocol"
  - RPC methods: plexus_schema, plexus_call, plexus_hash

Rust Libraries:
  - hub-core      (Activation trait, DynamicHub)
  - hub-macro     (Proc macros)
  - hub-transport (WebSocket, HTTP/SSE)
  - hub-codegen   (TypeScript codegen)

Haskell:
  - synapse       (CLI binary name)
  - hub-synapse   (Cabal package name)

Servers:
  - substrate     (Reference server with arbor, cone, etc.)

Other:
  - substrate-protocol      (Protocol docs?)
  - substrate-rust-codegen  (Rust client codegen)
  - substrate-sandbox-ts    (TypeScript testing)
```

---

## New State (After Rebrand)

### The Naming Principles

1. **"plexus-*"** = Protocol-level tools (spec, transports, core protocol)
2. **"hub-*"** = Higher-level framework libraries (keep for compatibility)
3. **"synapse"** = The CLI (keep the name, it's distinctive)
4. **"substrate"** = The reference server (keep the name)
5. **Language-specific naming** = `plexus-{language}`, `plexus-{language}-{tool}`

---

## Proposed Naming Structure

### 1. Protocol & Specification

```
plexus-rpc/          (or just "plexus-protocol/")
├── docs/
│   ├── protocol-spec.md
│   ├── streaming.md
│   ├── schemas.md
│   ├── comparison.md
│   └── quickstart.md
├── README.md        "Plexus RPC protocol specification"
└── examples/
    └── minimal-server.rs
```

**What it is:** The protocol specification, documentation, and reference examples.

**Current project:** `substrate-protocol/` → rename to `plexus-protocol/`

---

### 2. Rust Ecosystem

#### Core Libraries (Keep "hub-*" for Compatibility)

```
hub-core/            → KEEP NAME
  description: "Core infrastructure for Plexus RPC: Activation trait, DynamicHub, schemas"

hub-macro/           → KEEP NAME
  description: "Procedural macros for Plexus RPC activations"

hub-transport/       → KEEP NAME
  description: "Transport implementations for Plexus RPC: WebSocket, HTTP/SSE"
```

**Why keep "hub-*"?**
- Existing crates.io names (breaking change to rename)
- Existing import paths in user code
- Technical debt is manageable
- Can add `#[doc(alias = "plexus")]` for discoverability

**Update descriptions** to mention "Plexus RPC" so users understand what these are for.

#### Codegen Tools

```
hub-codegen/         → RENAME to plexus-codegen-typescript
  description: "TypeScript client generator for Plexus RPC"
  binary: plexus-codegen-typescript

substrate-rust-codegen/ → RENAME to plexus-codegen-rust
  description: "Rust client generator for Plexus RPC"
  binary: plexus-codegen-rust
```

**Why rename codegen?**
- Not tied to "hub" framework internals
- Users invoke these as CLI tools (breakage is minimal)
- Clear naming: `plexus-codegen-{target}`
- Future: `plexus-codegen-python`, `plexus-codegen-go`, etc.

**Migration:**
```bash
# Before
hub-codegen --output ./client

# After
plexus-codegen-typescript --output ./client

# Or unified CLI with subcommands
plexus-codegen typescript --output ./client
plexus-codegen rust --output ./client
```

---

### 3. Haskell Ecosystem

#### Synapse CLI

```
synapse/
├── hub-synapse.cabal    → RENAME to plexus-synapse.cabal
│   name: plexus-synapse
│   description: "Schema-driven CLI for Plexus RPC servers"
│   binary: synapse
└── README.md            "Synapse: CLI for Plexus RPC"
```

**Package name:** `plexus-synapse` (cabal package)
**Binary name:** `synapse` (what users type)
**Why?**
- Binary name "synapse" is established, distinctive, works well
- Package name can be `plexus-synapse` for ecosystem clarity
- Users never see the cabal package name, only the binary

**Current:** `hub-synapse` → **New:** `plexus-synapse`

---

### 4. Reference Server

```
substrate/
├── Cargo.toml
│   name: substrate
│   description: "Reference Plexus RPC server with conversation trees and LLM orchestration"
└── README.md
    "Substrate: A Plexus RPC server"
```

**Keep the name "substrate"** - it's the reference implementation, strong brand identity.

**Update description** to mention Plexus RPC.

---

### 5. Language-Specific Implementations

#### Python (Future)

```
plexus-python/
├── plexus_rpc/          (Python package)
│   ├── core.py
│   ├── activation.py
│   └── client.py
└── README.md

plexus-codegen-python/   (Generates Python clients)
```

#### TypeScript/JavaScript (Future)

```
plexus-typescript/       or @plexus-rpc/core
├── src/
│   ├── activation.ts
│   ├── client.ts
│   └── server.ts
└── package.json
    name: "@plexus-rpc/core"
```

#### Go (Future)

```
plexus-go/
└── plexus/
    ├── activation.go
    ├── client.go
    └── server.go
```

---

### 6. Testing & Examples

```
substrate-sandbox-ts/    → RENAME to plexus-examples-typescript
  description: "Example TypeScript clients for Plexus RPC"

examples/                (in plexus-protocol/)
├── hello-world/         Minimal Plexus RPC server (Rust)
├── streaming/           Streaming with progress (Rust)
├── python-server/       Python implementation
└── client-examples/
    ├── typescript/
    ├── python/
    └── rust/
```

---

## Complete Naming Matrix

| Project | Current Name | New Name | Reasoning |
|---------|-------------|----------|-----------|
| **Protocol Spec** | substrate-protocol | plexus-protocol | Clear protocol naming |
| **Rust Core** | hub-core | hub-core (keep) | Breaking change, technical debt OK |
| **Rust Macro** | hub-macro | hub-macro (keep) | Breaking change, technical debt OK |
| **Rust Transport** | hub-transport | hub-transport (keep) | Breaking change, technical debt OK |
| **TS Codegen** | hub-codegen | plexus-codegen-typescript | Clearer, enables plexus-codegen-* family |
| **Rust Codegen** | substrate-rust-codegen | plexus-codegen-rust | Clearer, consistent with TS codegen |
| **Haskell Package** | hub-synapse | plexus-synapse | Aligns with protocol |
| **CLI Binary** | synapse | synapse (keep) | Strong brand, users know it |
| **Reference Server** | substrate | substrate (keep) | Strong brand, clear identity |
| **TS Examples** | substrate-sandbox-ts | plexus-examples-typescript | Clearer purpose |

---

## Unified CLI Strategy (Optional Future Enhancement)

### Option A: Keep Separate Binaries

```bash
plexus-codegen-typescript --server localhost:4445 --output ./client
plexus-codegen-rust --server localhost:4445 --output ./client
synapse arbor tree-list
```

**Pros:** Simple, each tool is independent
**Cons:** Multiple binaries to install

### Option B: Unified "plexus" CLI

```bash
plexus codegen typescript --server localhost:4445 --output ./client
plexus codegen rust --server localhost:4445 --output ./client
plexus connect substrate  (alias for synapse)
```

**Pros:** Single entry point, consistent UX
**Cons:** Requires building a CLI orchestrator

**Recommendation:** Start with Option A (keep separate), consider Option B later.

---

## NPM Package Naming

### For TypeScript Users

```json
{
  "name": "@plexus-rpc/core",
  "description": "Core library for building Plexus RPC servers in TypeScript"
}

{
  "name": "@plexus-rpc/client",
  "description": "Client library for connecting to Plexus RPC servers"
}

{
  "name": "@plexus-rpc/codegen",
  "description": "Code generator for TypeScript Plexus RPC clients",
  "bin": {
    "plexus-codegen-typescript": "./bin/codegen.js"
  }
}
```

**Install:**
```bash
npm install @plexus-rpc/core
npm install -D @plexus-rpc/codegen
```

---

## PyPI Package Naming

### For Python Users

```python
# setup.py or pyproject.toml
name = "plexus-rpc"
description = "Core library for building Plexus RPC servers in Python"

# Usage
from plexus_rpc import Activation, Server

# Codegen package
name = "plexus-codegen-python"
description = "Python client generator for Plexus RPC"
```

**Install:**
```bash
pip install plexus-rpc
pip install plexus-codegen-python
```

---

## Cargo Package Naming

### For Rust Users

```toml
# Existing (keep)
[dependencies]
hub-core = "0.3"       # Core Plexus RPC library
hub-macro = "0.3"      # Macros
hub-transport = "0.3"  # Transports

# New codegen
[dev-dependencies]
plexus-codegen-rust = "0.1"  # Rust client generator
```

**Why keep hub-*?**
- Already published on crates.io
- Breaking change affects all existing users
- Technical debt is manageable
- Can add better docs pointing to "Plexus RPC"

---

## Documentation Structure

### Main Documentation Site

```
plexus-rpc.dev/              (or docs.plexus-rpc.dev)
├── /docs
│   ├── getting-started/
│   ├── protocol/
│   ├── comparison/          (vs gRPC, OpenAPI, tRPC)
│   └── api-reference/
├── /implementations
│   ├── rust/                (hub-core docs)
│   ├── python/
│   └── typescript/
├── /tools
│   ├── synapse/             (CLI docs)
│   ├── codegen/
│   └── transports/
└── /examples
```

### Repository READMEs

**plexus-protocol/README.md:**
```markdown
# Plexus RPC

Build services where code IS schema. Zero drift, instant streaming.

## What is Plexus RPC?

Plexus RPC is a protocol for building services with runtime schema introspection...

## Implementations

- **Rust:** [hub-core](../hub-core)
- **Python:** Coming soon
- **TypeScript:** Coming soon

## Tools

- **synapse:** CLI for Plexus RPC servers
- **plexus-codegen-typescript:** Generate TypeScript clients
- **plexus-codegen-rust:** Generate Rust clients

## Reference Server

- **substrate:** Reference Plexus RPC server with conversation trees and LLM orchestration
```

**substrate/README.md:**
```markdown
# Substrate

A Plexus RPC server providing conversation trees and LLM orchestration.

## What is Plexus RPC?

Plexus RPC is a protocol... [brief explanation + link to plexus-protocol/]

## Activations

- **arbor:** Conversation tree storage
- **cone:** LLM orchestration
- **bash:** Shell execution
- **claudecode:** Claude Code CLI integration

## Quick Start

```bash
cargo run --release
# Server starts on ws://localhost:4445
```

Connect with synapse:
```bash
synapse arbor tree-list
```
```

---

## Migration Path

### Phase 1: Documentation Updates (Non-Breaking)

1. Update all README.md files to mention "Plexus RPC"
2. Update Cargo.toml descriptions
3. Update hub-synapse.cabal description
4. Add "Plexus RPC" to all user-facing docs

**No code changes, no breaking changes**

### Phase 2: Rename Non-Breaking Projects

1. `substrate-protocol/` → `plexus-protocol/`
2. `substrate-sandbox-ts/` → `plexus-examples-typescript/`

**No user impact (these aren't published packages)**

### Phase 3: Codegen Renames (Minor Breaking)

1. `hub-codegen` → `plexus-codegen-typescript`
2. `substrate-rust-codegen` → `plexus-codegen-rust`
3. Publish both old and new for transition period
4. Update docs to reference new names

**Minor impact: users update CLI tool name**

### Phase 4: Haskell Package Rename (Minor Breaking)

1. `hub-synapse.cabal` → `plexus-synapse.cabal`
2. Keep binary name as "synapse"
3. Publish to Hackage as `plexus-synapse`

**Minor impact: cabal package name changes, binary stays same**

### Phase 5: Consider Rust Library Renames (Post-1.0, Major Breaking)

Only if benefits outweigh costs:
1. `hub-core` → `plexus-core`
2. `hub-macro` → `plexus-macro`
3. `hub-transport` → `plexus-transport`

**Major impact: all Rust users update imports**

**Recommendation: SKIP this phase** - keep hub-* names for stability

---

## Search & Discovery

### GitHub Topics

Add to all repos:
- `plexus-rpc`
- `rpc`
- `schema-driven`
- `streaming`
- `codegen`

### Crates.io Keywords

```toml
[package]
keywords = ["plexus-rpc", "rpc", "streaming", "schema", "codegen"]
categories = ["network-programming", "web-programming"]
```

### NPM Keywords

```json
{
  "keywords": ["plexus-rpc", "rpc", "streaming", "schema", "codegen"]
}
```

### PyPI Classifiers

```python
classifiers = [
    "Topic :: Internet :: WWW/HTTP :: HTTP Servers",
    "Topic :: Software Development :: Code Generators",
]
```

---

## Brand Hierarchy

```
Plexus RPC
├── Protocol Specification (plexus-protocol)
├── Implementations
│   ├── Rust (hub-core, hub-macro, hub-transport)
│   ├── Python (plexus-python) [future]
│   └── TypeScript (plexus-typescript) [future]
├── Tools
│   ├── synapse (CLI)
│   ├── plexus-codegen-typescript
│   ├── plexus-codegen-rust
│   ├── plexus-codegen-python [future]
│   └── plexus-codegen-go [future]
└── Reference Server
    └── substrate
```

---

## Recommended Action Plan

### Immediate (Phase 1)

1. ✅ Update all README.md to say "Plexus RPC"
2. ✅ Update Cargo.toml descriptions: "for Plexus RPC"
3. ✅ Update hub-synapse.cabal description
4. ✅ Create plexus-protocol/ docs

### Short-term (Phase 2-3)

1. Rename `substrate-protocol/` → `plexus-protocol/`
2. Rename `hub-codegen` → `plexus-codegen-typescript`
3. Rename `substrate-rust-codegen` → `plexus-codegen-rust`
4. Update synapse to say "CLI for Plexus RPC servers"

### Long-term (Phase 4-5)

1. Rename `hub-synapse` package → `plexus-synapse`
2. Consider unified `plexus` CLI (optional)
3. Consider Rust library renames (optional, breaking)

---

## Summary

**Protocol:** Plexus RPC
**Core Libraries:** hub-* (keep for compatibility)
**Codegen:** plexus-codegen-{language}
**CLI:** synapse (binary name), plexus-synapse (package name)
**Server:** substrate (reference implementation)
**Future:** plexus-{language} for new language implementations

Clean, consistent, minimal breaking changes.

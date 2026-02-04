# Hypermemetic Rebranding Guide

## The Problem

The current documentation is too academic. Terms like "Activation", "Plexus", "Handle" make sense to framework authors but not to users trying to solve problems.

**Current state:** "A categorical approach to composable RPC with streaming semantics"
**Target state:** "Write Rust, get TypeScript SDKs. Zero config, instant streaming."

## Core Messaging

### One-Liner
> Write Rust services, get TypeScript clients. Zero drift, instant streaming.

### Elevator Pitch
> Define your API in Rust with regular functions. The framework extracts the schema from your code (no YAML, no proto files), generates type-safe clients for TypeScript/Rust, and gives you streaming by default. Services compose into trees - call child services like local functions.

### Comparison Hook
| Feature | gRPC | OpenAPI | tRPC | **Hypermemetic** |
|---------|------|---------|------|------------------|
| Schema source | .proto files | YAML/JSON | TypeScript | **Rust code** |
| Streaming | Complex setup | Webhooks | Limited | **Built-in** |
| Type safety | Generated | Optional | Full | **Full** |
| Multi-language | Yes | Yes | TS only | **Yes** |
| Compose services | Manual | Manual | No | **Native trees** |

---

## Phase 1: Root README (Priority: Highest)

### Create `/README.md`

Structure:
1. **One-liner** (the hook)
2. **30-second code example** - Rust service → TypeScript client
3. **Why this exists** - 3 bullet points max
4. **Quick start link** - `/docs/QUICKSTART.md`
5. **Comparison table** - vs gRPC, OpenAPI, tRPC
6. **Project structure** - what each crate does (one line each)

### Code Example to Use

```rust
// server.rs - Define your service
use hub_macro::activation;

#[activation]
impl TodoService {
    pub async fn create(&self, title: String) -> Todo {
        // Your logic here
    }

    pub async fn list(&self) -> Vec<Todo> {
        // Streaming happens automatically
    }
}
```

```typescript
// client.ts - Generated automatically
const client = new TodoClient("ws://localhost:4444");

const todo = await client.create({ title: "Buy milk" });
const todos = await client.list(); // Streams results
```

---

## Phase 2: Crate READMEs

### hub-core/README.md

**Current problem:** "Categorical composition of streaming activations"
**Target:** "The core traits for building composable services"

Structure:
1. What this crate provides (Activation trait, DynamicHub, Handle system)
2. When to use this vs hub-macro vs substrate
3. 10-line example showing Activation implementation
4. Link to detailed architecture docs

### hub-macro/README.md

**Key message:** "Your code IS the schema"

Structure:
1. The problem: schema drift between code and API docs
2. The solution: derive schema from Rust function signatures
3. Before/after comparison (gRPC proto vs hub-macro)
4. All the attributes (#[activation], #[streaming], etc.)

### hub-transport/README.md

**Key message:** "One service, any transport"

Structure:
1. Supported transports: WebSocket, HTTP, stdio (MCP)
2. How to configure each
3. Protocol matrix (what features work where)
4. Deployment patterns

### hub-codegen/README.md

**Key message:** "Instant SDKs, zero config"

Structure:
1. Supported languages (TypeScript, Rust)
2. How generation works (from Synapse IR)
3. CLI usage
4. Customization options

### substrate/README.md

**Current:** Good architecture docs but buried
**Target:** Lead with "what can I build?" then architecture

Structure:
1. What you can build (examples: API gateway, agent orchestrator, plugin system)
2. 5-minute quickstart
3. Built-in services (arbor, cone, registry, etc.)
4. Architecture overview (move current content here)
5. Link to `/docs/architecture/` for deep dives

### synapse/README.md

**Current problem:** Leads with category theory
**Target:** Lead with "CLI that discovers APIs"

Structure:
1. What it does: "Point at any Hypermemetic service, get a CLI"
2. Demo GIF or transcript
3. Installation
4. Usage patterns
5. Move "Algebraic Foundations" to `/docs/internals/`

---

## Phase 3: Documentation Structure

### Create These Files

```
/docs/
├── QUICKSTART.md          # 5-minute tutorial
├── CONCEPTS.md            # Activation, Hub, Handle, Schema
├── COMPARISON.md          # vs gRPC, OpenAPI, tRPC, etc.
├── DEPLOYMENT.md          # Docker, K8s, systemd
├── STREAMING.md           # How streaming works
└── internals/
    ├── ARCHITECTURE.md    # Deep dive for contributors
    ├── CATEGORY-THEORY.md # Move synapse foundations here
    └── PROTOCOL.md        # Wire protocol details
```

### /docs/QUICKSTART.md

1. Prerequisites (Rust, Node.js)
2. Create new project
3. Define a service (copy-paste code)
4. Run it
5. Generate TypeScript client
6. Call from TypeScript
7. Next steps

### /docs/CONCEPTS.md

Define each term practically:
- **Activation** = A service that handles requests (like a controller)
- **Hub** = A router that dispatches to activations (like an app)
- **Handle** = A reference to another service (like dependency injection)
- **Schema** = Auto-generated API description (like OpenAPI but from code)
- **Streaming** = Every response is a stream (like Server-Sent Events but better)

---

## Phase 4: Cargo.toml Updates

Update `description` field in each crate:

```toml
# hub-core
description = "Core traits for composable streaming services"

# hub-macro
description = "Derive API schemas from Rust function signatures"

# hub-transport
description = "Transport adapters for WebSocket, HTTP, and stdio"

# hub-codegen
description = "Generate TypeScript and Rust SDKs from service schemas"

# substrate
description = "Orchestrator for composable Rust services with auto-generated clients"
```

---

## Phase 5: Examples

### Create /examples/todo-api/

A complete, runnable example:
```
examples/todo-api/
├── Cargo.toml
├── src/
│   ├── main.rs      # Server setup
│   └── service.rs   # TodoService activation
├── client/
│   ├── package.json
│   └── src/
│       └── index.ts # Generated client usage
└── README.md        # How to run
```

### Create /examples/streaming/

Show the streaming story:
```
examples/streaming/
├── src/main.rs      # Service that yields progress updates
└── README.md        # Explains streaming model
```

---

## Files to NOT Touch (Keep as-is)

- `/substrate/docs/architecture/*.md` - Good internal docs
- Test files
- CI/CD configs
- Cargo.lock files

---

## Success Criteria

After rebranding:
1. New user understands "what is this?" in 30 seconds from root README
2. Comparison with gRPC/OpenAPI is clear and compelling
3. "Hello World" works in 5 minutes following QUICKSTART.md
4. Academic terminology is in `/docs/internals/`, not front matter
5. All crate descriptions are practical, not abstract
6. At least one complete runnable example exists

---

## Running the Rebrand

```bash
# Start container session
claude-container -s rebrand --config .claude-projects.yml --dockerfile Dockerfile.claude-container

# Inside container, projects are at:
# /workspace/hub-core
# /workspace/hub-macro
# /workspace/substrate
# etc.

# When done, extract changes:
claude-container -s rebrand --extract
```

---

## Notes for Claude

When rewriting READMEs:
- Lead with the problem being solved, not the solution's architecture
- Use concrete examples, not abstract descriptions
- "You can..." not "This enables..."
- Show code before explaining it
- Compare to familiar tools (gRPC, Express, tRPC)
- Keep technical depth but put it after the practical intro

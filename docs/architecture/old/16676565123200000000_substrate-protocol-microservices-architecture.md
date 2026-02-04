# Substrate Protocol: Microservices Architecture

## The Key Insight

**"We would eliminate the substrate server"**

This resolves the namespace collision completely. Here's the new architecture:

```
┌─────────────────────────────────────────────────────────┐
│              Substrate Protocol (the spec)              │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  Each activation is its OWN Substrate Protocol server: │
│                                                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │   Arbor     │  │    Cone     │  │  Registry   │    │
│  │   :4445     │  │   :4446     │  │   :4444     │    │
│  │             │  │             │  │             │    │
│  │ Substrate   │  │ Substrate   │  │ Substrate   │    │
│  │ Protocol    │  │ Protocol    │  │ Protocol    │    │
│  │ server      │  │ server      │  │ server      │    │
│  └─────────────┘  └─────────────┘  └─────────────┘    │
│                                                         │
│  ┌─────────────┐  ┌─────────────┐                     │
│  │    Bash     │  │ ClaudeCode  │  ...                │
│  │   :4447     │  │   :4448     │                     │
│  └─────────────┘  └─────────────┘                     │
│                                                         │
└─────────────────────────────────────────────────────────┘
           ↑
           │
     ┌─────┴──────┐
     │  Synapse   │  (discovers and connects to all)
     └────────────┘
```

---

## What Changes

### Before (Current)

```rust
// One monolithic server
substrate/
├── src/
│   ├── activations/
│   │   ├── arbor/
│   │   ├── cone/
│   │   ├── bash/
│   │   └── ...
│   └── main.rs  // Starts ONE server with all activations

// main.rs
let plexus = DynamicHub::new("substrate")
    .register(Arbor::new())
    .register(Cone::new())
    .register(Bash::new())
    .register(ClaudeCode::new());

// One server on port 4445 with all activations
PlexusRpcServer::new(plexus).bind("0.0.0.0:4445").serve().await
```

### After (Microservices)

```rust
// Each activation is its own server

arbor/
├── src/
│   ├── activation.rs  // Arbor activation implementation
│   └── main.rs        // Standalone server
└── Cargo.toml

// arbor/main.rs
let arbor = Arc::new(Arbor::new(db).await?);
SubstrateServer::new(arbor)
    .bind("0.0.0.0:4445")
    .serve()
    .await

// cone/main.rs
let cone = Arc::new(Cone::new(config).await?);
SubstrateServer::new(cone)
    .bind("0.0.0.0:4446")
    .serve()
    .await

// registry/main.rs (the coordinator)
let registry = Arc::new(Registry::new());
SubstrateServer::new(registry)
    .bind("0.0.0.0:4444")
    .serve()
    .await
```

---

## New Terminology (Crystal Clear)

| Term | Meaning | Example |
|------|---------|---------|
| **Substrate Protocol** | The RPC protocol spec | "Implements Substrate Protocol v1" |
| **Substrate server** | Any server implementing the protocol | "Arbor is a Substrate server" |
| **Arbor** | Specific server for conversation trees | "Start Arbor on port 4445" |
| **Cone** | Specific server for LLM orchestration | "Cone server handles chat" |
| **Registry** | Discovery/coordination server | "Registry tracks available servers" |
| **Activation** | Still used for the core trait/implementation | "Arbor implements Activation" |

---

## Natural Language Examples

### "The thing is available over the network"

✅ "**Arbor** is running on port 4445"
✅ "Connect to the **Arbor server** at localhost:4445"
✅ "**Cone** is available via Substrate Protocol on port 4446"
✅ "All servers implement **Substrate Protocol**"

**No confusion:** Each server has its own name. "Substrate" only refers to the protocol.

### "The thing has a few other things as plugins"

✅ "**Registry** knows about Arbor, Cone, and Bash servers"
✅ "Arbor doesn't have plugins - it's a single-purpose server"
✅ "If you need composition, use **Registry** to discover servers"

**No DynamicHub needed:** Each server is focused. Registry handles discovery.

---

## Architecture Benefits

### 1. Clear Separation of Concerns

**Each server does ONE thing:**
- **Arbor**: Conversation trees (SQLite backend)
- **Cone**: LLM orchestration (stateful agents)
- **Bash**: Shell execution (sandboxed)
- **ClaudeCode**: Claude Code integration
- **Registry**: Service discovery

No monolithic server trying to do everything.

### 2. Independent Deployment

```bash
# Deploy only what you need
docker run arbor:latest -p 4445:4445

# Or run everything
docker-compose up

# Scale independently
docker-compose scale cone=3 arbor=1
```

### 3. Independent Versioning

```
arbor:1.2.3      implements Substrate Protocol v1
cone:2.0.1       implements Substrate Protocol v1
bash:1.0.0       implements Substrate Protocol v1
registry:0.9.0   implements Substrate Protocol v1
```

Protocol is stable. Implementations evolve independently.

### 4. Language Flexibility

```
arbor/          (Rust)
cone/           (Rust)
bash/           (Rust)
myservice/      (Python, using substrate-protocol-py)
another/        (TypeScript, using substrate-protocol-ts)
```

Each server can be in any language that implements Substrate Protocol.

---

## Registry as the Coordinator

### Registry's Job

```rust
// Registry activation
#[hub_methods(namespace = "registry", ...)]
impl Registry {
    /// Register a server
    async fn register(&self, url: String, info: ServerInfo) -> ...

    /// List all servers
    async fn list(&self) -> ...

    /// Get server info
    async fn info(&self, name: String) -> ...

    /// Health check all servers
    async fn health(&self) -> ...
}
```

### Synapse Uses Registry for Discovery

```bash
# Synapse connects to registry first (port 4444)
$ synapse

# Registry tells synapse about available servers
Available servers:
  arbor       127.0.0.1:4445 ✓ - Conversation trees
  cone        127.0.0.1:4446 ✓ - LLM orchestration
  bash        127.0.0.1:4447 ✓ - Shell execution
  claudecode  127.0.0.1:4448 ✓ - Claude Code integration

# Call methods on any server
$ synapse arbor tree-list
$ synapse cone chat --name agent1 --prompt "Hello"
```

### Registry Schema

```json
{
  "servers": [
    {
      "name": "arbor",
      "url": "ws://127.0.0.1:4445",
      "namespace": "arbor",
      "version": "1.2.3",
      "description": "Conversation tree storage",
      "health": "healthy",
      "methods": ["tree_create", "tree_get", ...]
    },
    {
      "name": "cone",
      "url": "ws://127.0.0.1:4446",
      "namespace": "cone",
      "version": "2.0.1",
      "description": "LLM orchestration",
      "health": "healthy",
      "methods": ["create", "chat", ...]
    }
  ]
}
```

---

## Directory Structure

### New Repository Layout

```
hypermemetic/
├── substrate-protocol/        # The protocol specification (docs)
│   ├── docs/
│   │   ├── protocol-spec.md
│   │   ├── streaming.md
│   │   ├── schemas.md
│   │   └── transports.md
│   └── README.md
│
├── hub-core/                  # Rust implementation library
├── hub-macro/
├── hub-transport/
├── hub-codegen/
│
├── arbor/                     # Standalone server
│   ├── src/
│   │   ├── activation.rs
│   │   └── main.rs
│   ├── Cargo.toml
│   └── README.md
│
├── cone/                      # Standalone server
│   ├── src/
│   │   ├── activation.rs
│   │   └── main.rs
│   └── Cargo.toml
│
├── registry/                  # Standalone server
│   ├── src/
│   │   ├── activation.rs
│   │   └── main.rs
│   └── Cargo.toml
│
├── bash/                      # Standalone server
├── claudecode/                # Standalone server
│
├── synapse/                   # CLI (discovers via registry)
│
└── examples/
    ├── hello-world/           # Minimal Substrate Protocol server
    └── python-server/         # Python implementation
```

### What happens to "substrate" repo?

**Option 1: Split it up**
- substrate/ → arbor/, cone/, bash/, claudecode/ (separate repos)
- Keep substrate-protocol/ for the spec

**Option 2: Keep as monorepo**
- substrate/ becomes the monorepo containing all servers
- Each server builds independently
- Shared CI/CD

**Option 3: Rename substrate → hypermemetic**
- hypermemetic/ is the monorepo
- Contains all Substrate Protocol servers
- "substrate" name freed for the protocol

---

## Deployment Scenarios

### Scenario 1: All-in-One (Docker Compose)

```yaml
# docker-compose.yml
services:
  registry:
    image: substrate-protocol/registry
    ports: ["4444:4444"]

  arbor:
    image: substrate-protocol/arbor
    ports: ["4445:4445"]
    environment:
      REGISTRY_URL: "ws://registry:4444"

  cone:
    image: substrate-protocol/cone
    ports: ["4446:4446"]
    environment:
      REGISTRY_URL: "ws://registry:4444"

  bash:
    image: substrate-protocol/bash
    ports: ["4447:4447"]
    environment:
      REGISTRY_URL: "ws://registry:4444"
```

```bash
docker-compose up
# All servers start, register with registry
# Synapse connects to registry:4444, discovers all
```

### Scenario 2: Distributed (Cloud)

```
Production:
  registry.prod.example.com:443  (global)
  arbor.prod.example.com:443     (replicated 3x)
  cone.prod.example.com:443      (replicated 5x, GPU instances)
  bash.prod.example.com:443      (sandboxed, isolated)

Synapse config:
  registry: wss://registry.prod.example.com
```

### Scenario 3: Minimal (Single Server)

```bash
# Just run Arbor for conversation trees
docker run -p 4445:4445 substrate-protocol/arbor

# Synapse can connect directly (no registry)
synapse --server localhost:4445 tree-list
```

---

## Documentation Flow

### substrate-protocol/README.md

```markdown
# Substrate Protocol

**Build services where code IS schema.**

Substrate Protocol is an RPC protocol for building services with:
- Runtime schema introspection
- Streaming-first design
- Tree-structured namespaces
- Type-safe client generation

## Implementations

**Rust:**
- [hub-core](../hub-core) - Core library
- [arbor](../arbor) - Conversation tree server
- [cone](../cone) - LLM orchestration server
- [registry](../registry) - Service discovery server

**Python:**
- substrate-protocol-py (coming soon)

**TypeScript:**
- substrate-protocol-ts (coming soon)

## Quick Start

Run all servers:
```bash
docker-compose up
```

Connect with Synapse:
```bash
synapse
# Discovers servers via registry
```
```

### arbor/README.md

```markdown
# Arbor

**Conversation tree storage server**

Arbor is a Substrate Protocol server that provides persistent conversation trees with branching support.

## Running

```bash
cargo run --release
# Listens on ws://localhost:4445
```

## API

```bash
# Create a tree
synapse arbor tree-create --metadata '{...}'

# Get a tree
synapse arbor tree-get --tree-id <UUID>
```

Or use the generated TypeScript client:

```typescript
import { ArborClient } from '@substrate-protocol/arbor-client'

const client = new ArborClient('ws://localhost:4445')
const tree = await client.treeCreate({...})
```

## Deployment

See [deployment guide](./docs/deployment.md) for Docker, Kubernetes, and cloud options.
```

---

## Migration Path

### Phase 1: Split the Codebase (Non-Breaking)

```bash
# Current
substrate/
  src/activations/arbor/
  src/activations/cone/
  ...

# New (still in same repo)
substrate/
  arbor/src/    (becomes own crate)
  cone/src/     (becomes own crate)
  registry/src/ (becomes own crate)
  ...
```

Each activation becomes a workspace member:

```toml
# substrate/Cargo.toml
[workspace]
members = ["arbor", "cone", "registry", "bash", "claudecode"]
```

### Phase 2: Update Entry Points

```rust
// arbor/src/main.rs (NEW)
async fn main() -> Result<()> {
    let arbor = Arc::new(Arbor::new(db).await?);

    // Register with registry if configured
    if let Some(registry_url) = env::var("REGISTRY_URL").ok() {
        register_with_registry(registry_url, "arbor", "ws://0.0.0.0:4445").await?;
    }

    SubstrateServer::new(arbor)
        .bind("0.0.0.0:4445")
        .serve()
        .await
}
```

### Phase 3: Update Documentation

- Update all READMEs to refer to "Substrate Protocol"
- Each server gets its own README
- Create substrate-protocol/ repo with spec docs

### Phase 4: Update Synapse

```rust
// Synapse connects to registry first
let registry = connect("ws://localhost:4444").await?;
let servers = registry.list().await?;

// Build CLI from discovered servers
for server in servers {
    add_subcommand(&server.name, &server.url, &server.schema);
}
```

---

## Advantages of This Architecture

### 1. Clean Terminology

✅ "Substrate Protocol" = protocol (no confusion)
✅ "Arbor server" = specific server
✅ "Cone server" = different specific server
✅ No namespace collision

### 2. Scalability

✅ Deploy only what you need
✅ Scale services independently
✅ Update services independently

### 3. Clarity

✅ Each server has clear responsibility
✅ No "god object" substrate server
✅ Easy to understand boundaries

### 4. Extensibility

✅ Add new servers without modifying existing ones
✅ Third-party servers can implement Substrate Protocol
✅ Language-agnostic

### 5. Operations

✅ Monitor services independently
✅ Debug issues in isolation
✅ Roll back individual services

---

## Open Questions

1. **Does Registry implement Substrate Protocol?**
   - Yes - it's a server like any other
   - Has methods: `register`, `list`, `info`, `health`
   - Synapse calls it like any Substrate Protocol server

2. **Can servers talk to each other?**
   - Yes - Cone might call Arbor to store conversation state
   - Use Substrate Protocol for inter-server communication
   - Registry provides service discovery

3. **What about the monorepo?**
   - Keep substrate/ as monorepo? Rename to hypermemetic/?
   - Split into separate repos per server?
   - Recommendation: Monorepo for now, can split later

4. **Backward compatibility?**
   - Can we provide a "substrate-all" Docker image?
   - Runs all servers in one container for easy migration
   - Recommendation: Yes, ease migration path

---

## Recommendation

**Use "Substrate Protocol" with microservices architecture:**

1. ✅ No namespace collision (no "substrate server" exists)
2. ✅ Clean separation of concerns
3. ✅ Independent deployment and scaling
4. ✅ Clear positioning: "Substrate Protocol" is the spec
5. ✅ Natural language: "Arbor is a Substrate Protocol server"
6. ✅ Aligns with modern microservices best practices

The elimination of the monolithic substrate server solves the naming problem completely.

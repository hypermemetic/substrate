# Plexus RPC: Terminology Usage Examples

## Core Terminology

**Plexus RPC** = The protocol itself
**Plexus server** = A server implementing Plexus RPC
**Activation** = A service/capability/plugin
**Plexus router** = Routes calls to activations (what we call DynamicHub)

---

## Level 1: User-Facing (Explaining to Someone New)

### "The thing is available over the network"

**Natural language:**
- "Substrate is a **Plexus server** running on port 4445"
- "Your service is **available via Plexus RPC** on localhost:4444"
- "Connect to the **Plexus server** at ws://example.com:4444"
- "Synapse talks to **Plexus servers** over WebSockets"

**In documentation:**
```
Substrate exposes its activations via Plexus RPC on WebSocket.
By default, it listens on localhost:4445.

$ synapse connect substrate.example.com:4445
Connected to Plexus server (substrate v1.2.3)
```

### "The thing has a few other things as plugins"

**Natural language:**
- "Substrate includes several **activations**: Arbor, Cone, and Bash"
- "You can add your own **activations** to the server"
- "This **Plexus server** hosts 15 activations"
- "Arbor is an **activation** that provides conversation trees"

**In documentation:**
```
A Plexus server can host multiple activations. Substrate ships with:
- Arbor: Conversation tree storage
- Cone: LLM orchestration
- Bash: Shell command execution

Each activation implements the Activation trait and registers with
the Plexus router.
```

---

## Level 2: Technical Documentation

### README.md style

```markdown
# Substrate

A Plexus server providing conversation trees, LLM orchestration, and shell execution.

## What is Plexus RPC?

Plexus RPC is a protocol for building services with:
- Runtime schema introspection
- Streaming-first design
- Tree-structured namespaces
- Type-safe client generation

## Architecture

Substrate is a **Plexus server** with the following activations:

- **arbor**: Conversation tree storage
- **cone**: LLM orchestration
- **bash**: Shell execution

Connect via WebSocket on port 4445 and call methods like:
- `arbor.tree_create`
- `cone.chat`
- `bash.execute`
```

### Quick Start

```markdown
## Running a Plexus Server

```bash
# Start the server
cargo run --release

# Server listens on ws://localhost:4445
# Activations: arbor, cone, bash, health
```

## Calling Methods

Use the Synapse CLI (connects to Plexus servers):

```bash
synapse arbor tree-list
synapse cone chat --name "my-agent" --prompt "Hello"
```

Or use the generated TypeScript client:

```typescript
import { PlexusClient } from './generated/substrate-client'

const client = new PlexusClient('ws://localhost:4445')
await client.arbor.treeCreate({ metadata: {...} })
```
```

### API Reference style

```markdown
## Plexus RPC Protocol

### Connection

Plexus RPC uses WebSocket with JSON-RPC 2.0 messages.

```javascript
// Connect to a Plexus server
const ws = new WebSocket('ws://localhost:4445')

// Call an activation method
ws.send({
  jsonrpc: "2.0",
  id: 1,
  method: "plexus_call",
  params: {
    method: "arbor.tree_create",
    params: { metadata: {...} }
  }
})
```

### Schema Discovery

Query available activations:

```javascript
// Get all activations on this server
ws.send({
  method: "plexus_schema",
  params: {}
})
// Returns: { activations: [{namespace: "arbor", methods: [...]}] }

// Get detailed schema for one activation
ws.send({
  method: "plexus_activation_schema",
  params: ["arbor"]
})
```
```

---

## Level 3: Developer Documentation (Building Your Own)

### Building a Plexus Server

```markdown
# Building Your First Plexus Server

## 1. Create an Activation

```rust
use hub_core::{Activation, hub_macro::hub_methods};

#[derive(Clone)]
pub struct MyService;

#[hub_methods(
    namespace = "myservice",
    version = "1.0.0",
    description = "My service"
)]
impl MyService {
    #[hub_method]
    async fn hello(&self, name: String) -> impl Stream<Item = MyEvent> {
        stream! {
            yield MyEvent::Greeting {
                message: format!("Hello, {}!", name)
            };
        }
    }
}
```

## 2. Host it with a Plexus Router

```rust
use hub_core::plexus::DynamicHub;

// Create a Plexus router and register your activation
let router = DynamicHub::new("myapp")
    .register(MyService);

// Expose it over the network
let server = PlexusRpcServer::new(router)
    .bind("127.0.0.1:4444")
    .await?;

server.serve().await?;
```

Now you have a **Plexus server** at `localhost:4444` with the
`myservice` activation available.

## 3. Generate Clients

```bash
# Start your server
cargo run

# Generate TypeScript client
plexus-codegen --server localhost:4444 --output ./client
```

Your **Plexus server** now exposes type-safe TypeScript clients automatically.
```

---

## Level 4: Marketing / Positioning

### Landing Page Copy

```markdown
# Plexus RPC

Write Rust services, get type-safe clients. Zero drift, instant streaming.

## What is Plexus RPC?

Plexus RPC is a protocol for building services where your code **is** the schema.
Write methods in Rust with regular types and doc comments. Plexus RPC automatically:

- Exposes runtime JSON schemas
- Generates type-safe TypeScript/Python clients
- Provides streaming progress and error handling
- Enables self-documenting CLIs

## How It Works

1. **Write your service in Rust**
   ```rust
   #[hub_method]
   async fn create_user(&self, email: String) -> impl Stream<Item = UserEvent>
   ```

2. **Start your Plexus server**
   ```bash
   cargo run
   # Listening on ws://localhost:4444
   ```

3. **Generate clients, use CLIs, done**
   ```bash
   plexus-codegen --output ./client
   plexus-cli user create --email test@example.com
   ```

No separate schema files. No drift. Just code.
```

### Comparison

```markdown
## Plexus RPC vs Alternatives

| Feature | Plexus RPC | gRPC | OpenAPI/REST |
|---------|-----------|------|--------------|
| Schema source | Rust code | .proto files | YAML files |
| Schema drift | Impossible | Easy | Very easy |
| Streaming | Built-in | Bolt-on | SSE (separate) |
| Client generation | Auto | protoc | openapi-generator |
| Runtime introspection | Full | Limited | Swagger UI |
```

---

## Level 5: Internal Code Documentation

### In hub-core/src/plexus/mod.rs

```rust
//! Plexus RPC routing and coordination
//!
//! This module implements the routing layer for Plexus RPC servers.
//!
//! # Overview
//!
//! A Plexus server routes RPC calls to activations. The [`DynamicHub`]
//! type provides dynamic registration:
//!
//! ```rust
//! let router = DynamicHub::new("myapp")
//!     .register(MyActivation)
//!     .register(AnotherActivation);
//! ```
//!
//! Calls are routed by namespace:
//! - `myactivation.method` → calls MyActivation
//! - `anotheractivation.method` → calls AnotherActivation
//!
//! # Terminology
//!
//! - **Plexus RPC**: The protocol itself
//! - **Plexus server**: A server implementing the protocol
//! - **Activation**: A service/capability that can be called
//! - **DynamicHub**: Routes calls to registered activations

/// Routes RPC calls to registered activations.
///
/// This is the core routing layer for a Plexus server. Register
/// activations with [`.register()`](Self::register), then route
/// calls with [`.call()`](Self::call).
pub struct DynamicHub {
    // ...
}
```

### In substrate/src/main.rs

```rust
//! Substrate: A Plexus server with conversation trees and LLM orchestration
//!
//! This Plexus server provides several activations:
//! - `arbor`: Conversation tree storage
//! - `cone`: LLM orchestration
//! - `bash`: Shell command execution
//! - `health`: Health check endpoint
//!
//! The server exposes Plexus RPC on:
//! - WebSocket (JSON-RPC): ws://localhost:4445
//! - MCP: stdio (via MCP bridge)

async fn main() -> Result<()> {
    // Create Plexus router and register activations
    let router = DynamicHub::new("substrate")
        .register(Arbor::new(db).await?)
        .register(Cone::new(config).await?)
        .register(Bash::new())
        .register(Health::new());

    // Start Plexus server
    info!("Starting Plexus server on ws://localhost:4445");
    let server = PlexusRpcServer::new(router)
        .bind("127.0.0.1:4445")
        .await?;

    server.serve().await
}
```

---

## Level 6: Casual Conversation / Slack / Issues

### Discussing infrastructure

```
dev1: "We need to spin up another Plexus server for the staging environment"
dev2: "What activations should it run?"
dev1: "Just arbor and cone, no bash access on staging"

dev2: "Got it, deploying Plexus server with arbor + cone activations"
```

### Discussing features

```
user: "How do I call the tree_create method?"
dev: "Substrate exposes that via Plexus RPC. Use synapse:
     synapse arbor tree-create --metadata '{...}'

     Or if you're using the TS client:
     await client.arbor.treeCreate({...})"
```

### Discussing architecture

```
dev1: "Should we make this a separate Plexus server or add it as an
       activation to substrate?"
dev2: "If it's tightly coupled with arbor, add it as an activation.
       If it needs different deployment/scaling, separate server."
```

### Debugging

```
dev: "The Plexus server on staging is returning 404 for cone.chat"
ops: "Checked the activations list? Maybe cone didn't register?"
dev: "Yep, checking: synapse plexus_schema
     ...only seeing arbor and health. Cone activation failed to start."
```

---

## Level 7: Cargo.toml / Package Descriptions

### hub-core

```toml
[package]
name = "hub-core"
description = "Core infrastructure for building Plexus RPC services: Activation trait, routing, and schema introspection"
```

### hub-transport

```toml
[package]
name = "hub-transport"
description = "Transport implementations for Plexus RPC: WebSocket, HTTP/SSE, and in-process"
```

### substrate

```toml
[package]
name = "substrate"
description = "Reference Plexus server with conversation trees (Arbor) and LLM orchestration (Cone)"
```

### synapse

```toml
[package]
name = "synapse"
description = "Schema-driven CLI for Plexus RPC servers"
```

---

## Level 8: Git Commit Messages

```
feat: add retry logic to Plexus RPC client

Added exponential backoff when connecting to Plexus servers.
Helps with flaky network conditions.

fix: arbor activation not handling large payloads

The arbor activation was timing out on trees with >1000 nodes.
Increased stream buffer size.

docs: update Plexus RPC protocol specification

Added section on streaming error handling to the Plexus RPC spec.

refactor: rename Plexus -> DynamicHub in routing code

The DynamicHub type is the router implementation. "Plexus" now
refers only to the protocol name (Plexus RPC).
```

---

## Level 9: Error Messages

```rust
// Good error messages using the terminology

return Err(PlexusError::ActivationNotFound {
    namespace: "missing",
    available: vec!["arbor", "cone"],
    message: "Activation 'missing' not found on this Plexus server. \
              Available activations: arbor, cone"
});

return Err(PlexusError::ConnectionFailed {
    address: "localhost:4445",
    message: "Failed to connect to Plexus server at localhost:4445. \
              Is the server running?"
});

return Err(PlexusError::SchemaValidation {
    method: "arbor.tree_create",
    message: "Invalid parameters for arbor.tree_create. \
              Expected field 'metadata' of type object."
});
```

---

## Summary: Natural Language Patterns

### Common phrases that work well:

✅ "Connect to the **Plexus server** at..."
✅ "This **Plexus server** provides..."
✅ "Available via **Plexus RPC**"
✅ "The **arbor activation** handles..."
✅ "Register your **activation** with..."
✅ "Call methods using the **Plexus RPC protocol**"
✅ "Synapse connects to **Plexus servers**"
✅ "Build a **Plexus server** with..."

### Phrases to avoid:

❌ "The Plexus" (ambiguous - Plexus what?)
❌ "A Plexus" (noun needs specifier: server, router, client, etc.)
❌ "Plexus implementation" (say "Plexus server" or "Plexus client")
❌ "Hub" without qualifier (say "Plexus server" or "DynamicHub router")

---

## Quick Reference Card

| When you mean... | Say this... |
|-----------------|-------------|
| The protocol | Plexus RPC |
| A server instance | Plexus server |
| The routing layer | Plexus router or DynamicHub |
| A service/plugin | Activation |
| Substrate specifically | Substrate (a Plexus server) |
| The CLI tool | Synapse |
| A method call | `activation.method` |
| Multiple services | Activations (plural) |
| The library suite | hub-core, hub-macro, hub-transport |
| Schema introspection | Plexus RPC schemas |
| Client SDK | Plexus RPC client or TypeScript/Python client |

---

## Does This Feel Natural?

The key test: Can you say these naturally?

- "I'm running a **Plexus server** on port 4444"
- "What **activations** does your server have?"
- "Connect to the **Plexus server** and call arbor.tree_list"
- "Substrate is a **Plexus server** with Arbor and Cone"
- "Use Synapse to talk to any **Plexus server**"
- "Build your own **activation** and host it"

If these feel natural, the terminology works. If they feel clunky, we adjust.

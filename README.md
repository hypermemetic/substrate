# Substrate

A Plexus RPC server providing conversation trees and LLM orchestration.

## What is Plexus RPC?

Plexus RPC is a protocol for building services where code IS schema. Services expose JSON schemas at runtime for all methods, enabling zero-drift type-safe client generation and instant streaming. The protocol supports tree-structured namespacing, where plugins organize hierarchically via dot-separated paths (`arbor.tree_create`, `cone.chat`).

Key features:
- **Self-describing**: Query any method's schema at runtime
- **Streaming-first**: All methods return streams by default
- **Tree-structured**: Organize methods in hierarchical namespaces
- **Language-agnostic**: Generate type-safe clients for any language

## Abstract

Substrate is a reference Plexus RPC server implementing conversation tree storage (Arbor), LLM orchestration (Cone), and development tools (ClaudeCode, Bash). It demonstrates the full Plexus RPC architecture: hierarchical plugin structure, runtime schema introspection, streaming by default, and cross-language client generation.

This document describes the Plexus RPC architecture as implemented in Substrate. Server-specific activations (Arbor, Cone, ClaudeCode) are documented separately.

## Architecture

### Layer Structure

```
┌────────────────────────────────────────────────────────────────┐
│                     Plexus RPC Backends                        │
│  (DynamicHub instance, future: remote hubs via URL)           │
├────────────────────────────────────────────────────────────────┤
│                         hub-macro                              │
│  #[hub_methods] #[hub_method(streaming)]                      │
│  → generates method enums, schemas, streaming annotations     │
├────────────────────────────────────────────────────────────────┤
│                         hub-core                               │
│  Activation trait, DynamicHub routing, PluginSchema types     │
│  ChildRouter trait, streaming infrastructure                  │
├────────────────────────────────────────────────────────────────┤
│                         substrate                              │
│  Foundation types (Handle, Value), serialization              │
└────────────────────────────────────────────────────────────────┘
```

### Activation Trait

The unified interface for all Plexus RPC plugins:

```rust
#[async_trait]
pub trait Activation: Send + Sync + 'static {
    type Methods: MethodEnumSchema;

    fn namespace(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;
    fn methods(&self) -> Vec<&str>;

    async fn call(&self, method: &str, params: Value)
        -> Result<PlexusStream, PlexusError>;

    fn plugin_schema(&self) -> PluginSchema;
}
```

All plugins implement `Activation`. The `plugin_schema()` method returns a JSON Schema describing available methods, parameters, and return types.

### Tree-Structured Namespace

Plexus RPC organizes methods hierarchically via dot-separated paths:

```
plexus
├── arbor.tree_create
├── arbor.node_create_text
├── cone.create
├── cone.chat
├── echo.echo
└── health.check
```

Nested plugins implement `ChildRouter` to delegate calls to children. Plexus RPC supports arbitrary nesting depth.

### Schema System

Every Plexus RPC activation exposes a `schema` method:

```rust
// Query any plugin's schema
plexus.call("echo.schema", {})
plexus.call("arbor.schema", {})
```

Schemas include:
- Method names and descriptions
- Parameter types (JSON Schema)
- Return types (JSON Schema)
- Streaming annotation
- Child plugin summaries (namespace, description, hash)

Child schemas are **not included recursively**. Clients fetch child schemas individually via `{namespace}.schema`, enabling lazy traversal of large plugin trees.

### Hash-Based Versioning

Each method schema has a content hash. Parent hashes incorporate child hashes. The root hash changes when any descendant changes. This enables:

- Cache invalidation
- Client version detection
- Schema drift warnings

### Streaming by Default

All Plexus RPC methods return `PlexusStream`, a stream of `PlexusStreamItem`:

```rust
pub enum PlexusStreamItem {
    Content { metadata, content_type, data },
    Progress { metadata, message, percentage },
    Error { metadata, message, code, recoverable },
    Done { metadata },
}
```

Non-streaming methods emit a single `Content` item followed by `Done`. Streaming methods emit multiple items.

## Implementation Patterns

### Leaf Activation (Macro-Generated)

Simple plugins with methods, no children. Use `#[hub_methods]`:

```rust
#[derive(Clone)]
pub struct Echo;

#[hub_macro::hub_methods(
    namespace = "echo",
    version = "1.0.0",
    description = "Echo messages back"
)]
impl Echo {
    #[hub_macro::hub_method(
        streaming,
        params(
            message = "The message to echo",
            count = "Number of times to repeat"
        )
    )]
    async fn echo(
        &self,
        message: String,
        count: u32
    ) -> impl Stream<Item = EchoEvent> {
        stream! {
            for _ in 0..count {
                yield EchoEvent::Echo { message: message.clone() };
            }
        }
    }
}
```

The macro generates:
- `EchoMethod` enum with JSON Schema
- `Activation` trait implementation
- Automatic `schema` method dispatch

### Hub Activation (Macro-Generated with Children)

Activations containing other activations (hubs). Add `hub` flag and implement `plugin_children()`:

```rust
#[hub_macro::hub_methods(
    namespace = "solar",
    version = "1.0.0",
    description = "Solar system model",
    hub
)]
impl Solar {
    async fn observe(&self) -> impl Stream<Item = SolarEvent> { /* ... */ }

    pub fn plugin_children(&self) -> Vec<PluginSchema> {
        self.planets.iter()
            .map(|p| p.to_plugin_schema())
            .collect()
    }
}

#[async_trait]
impl ChildRouter for Solar {
    fn router_namespace(&self) -> &str { "solar" }

    async fn router_call(&self, method: &str, params: Value)
        -> Result<PlexusStream, PlexusError>
    {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        self.planets.iter()
            .find(|p| p.name == name)
            .map(|p| Box::new(p.clone()) as Box<dyn ChildRouter>)
    }
}
```

Register hubs with `register_hub()`:

```rust
let plexus = DynamicHub::new()
    .register(Echo)
    .register_hub(Solar::new());
```

### Dynamic Activation (Hand-Implemented)

When activations are created from runtime data, manually implement `Activation`:

```rust
#[async_trait]
impl Activation for Planet {
    type Methods = PlanetMethod;

    fn namespace(&self) -> &str { &self.name }
    fn version(&self) -> &str { "1.0.0" }
    fn description(&self) -> &str { &self.description }
    fn methods(&self) -> Vec<&str> { vec!["info", "schema"] }

    async fn call(&self, method: &str, params: Value)
        -> Result<PlexusStream, PlexusError>
    {
        match method {
            "info" => Ok(self.info_stream()),
            "schema" => {
                let schema = self.plugin_schema();
                Ok(wrap_stream(futures::stream::once(async { schema })))
            }
            _ => route_to_child(self, method, params).await
        }
    }

    fn plugin_schema(&self) -> PluginSchema {
        PluginSchema {
            plugin_id: self.id,
            namespace: self.name.clone(),
            version: "1.0.0".into(),
            description: self.description.clone(),
            methods: vec![/* method schemas */],
            children: vec![],
            hash: compute_hash(/* ... */),
        }
    }
}
```

Dynamic activations must manually:
- Include `"schema"` in `methods()`
- Handle `"schema"` in `call()`
- Implement `ChildRouter` if they have children

## Code Generation Pipeline

```
   Rust Activation            hub-macro              Runtime Schema
  ┌──────────┐              ┌──────────┐             ┌──────────┐
  │ impl Foo │──────────────│ proc-    │─────────────│ Plugin   │
  │ {        │  #[hub_      │ macro    │  generates  │ Schema   │
  │   fn x() │  methods]    │ expand   │  schema()   │ JSON     │
  │ }        │              │          │  method     │          │
  └──────────┘              └──────────┘             └──────────┘
                                                          │
                                                          ▼
                            ┌──────────────────────────────────────┐
                            │           Synapse (Haskell)          │
                            │  Parses schema, emits IR             │
                            │  synapse --emit-ir                   │
                            └──────────────────────────────────────┘
                                                          │
                                                          ▼
                            ┌──────────────────────────────────────┐
                            │  plexus-codegen-typescript (Rust)    │
                            │  Consumes IR, generates TypeScript   │
                            └──────────────────────────────────────┘
                                                          │
                                                          ▼
                            ┌──────────────────────────────────────┐
                            │       TypeScript Client              │
                            │  Type-safe Plexus RPC calls          │
                            └──────────────────────────────────────┘
```

The pipeline is language-agnostic at the IR level. Adding Python support requires implementing a Python backend in `plexus-codegen-python`.

## Accessing Plexus RPC

### WebSocket Transport

```bash
# Start Substrate server
cargo run

# Connect via WebSocket
wscat -c ws://localhost:4444

# Call Plexus RPC methods
{"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"echo.echo","params":{"message":"hello","count":3}}}

# Get Plexus RPC schemas
{"jsonrpc":"2.0","id":1,"method":"plexus_schema"}
{"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"arbor.schema"}}
```

### MCP Bridge

Substrate exposes an MCP server that presents Plexus RPC methods as MCP tools using dot notation:

```
echo.echo(message, count)
arbor.tree_create(metadata)
cone.chat(name, prompt)
```

The MCP bridge automatically converts all registered Plexus RPC activation methods into callable MCP tools. Tool names mirror the Plexus RPC namespace structure directly.

### In-Process (Rust)

```rust
use substrate::{DynamicHub, activations::Echo};

let plexus = DynamicHub::new().register(Echo);

let mut stream = plexus.call(
    "echo.echo",
    json!({"message": "test", "count": 1})
).await?;

while let Some(item) = stream.next().await {
    println!("{:?}", item);
}
```

## Current State

| Component                     | Status  | Notes                                       |
|-------------------------------|---------|---------------------------------------------|
| hub-core                      | Stable  | Activation, DynamicHub, ChildRouter, schemas|
| hub-macro                     | Stable  | Streaming attribute works                   |
| synapse                       | Stable  | IR emission complete                        |
| plexus-codegen-typescript     | Partial | Types done, namespace generator pending     |
| Multi-hub (remote references) | Planned | Remote hub references not implemented       |

## Multi-Hub Vision

Current: All activations in-process, single DynamicHub instance.

Future: Plexus RPC hubs reference other hubs as activations via URL.

```
┌─────────────────┐          ┌─────────────────┐
│   Local Hub     │          │   Remote Hub    │
│  ┌───────────┐  │  HTTP/   │  ┌───────────┐  │
│  │ local.*   │  │  SSE     │  │ remote.*  │  │
│  └───────────┘  │◄────────►│  └───────────┘  │
│  ┌───────────┐  │          └─────────────────┘
│  │ remote@url│──┼──────────────────┘
│  │ (proxy)   │  │
│  └───────────┘  │
└─────────────────┘
```

Requirements for multi-hub Plexus RPC:
- Transport envelope for cross-hub calls
- Schema federation (remote schemas appear local)
- Streaming across network boundary
- Authentication/authorization

See `docs/architecture/16679517135570018559_multi-hub-transport-envelope.md`.

## Project Structure

```
src/
├── plexus/              # Re-exports from hub-core
├── activations/         # Substrate-specific Plexus RPC activations
│   ├── arbor/          # Conversation tree storage
│   ├── cone/           # Generic LLM orchestration
│   ├── claudecode/     # Claude Code CLI wrapper
│   ├── bash/           # Shell command execution
│   ├── changelog/      # Change tracking
│   ├── mustache/       # Template rendering
│   ├── echo/           # Example leaf activation
│   ├── health/         # Example minimal activation
│   └── solar/          # Example hub activation
├── mcp_bridge.rs       # MCP protocol adapter for Plexus RPC
└── main.rs             # Plexus RPC server entry point
```

## See Also

- `docs/architecture/16676565123400000000_plexus-rpc-ecosystem-naming.md` - Plexus RPC naming strategy
- `docs/architecture/16679477965835151615_hub-architecture-layering.md` - Detailed Plexus RPC architecture
- `docs/architecture/16679613932789736703_compiler-architecture.md` - Code generation pipeline
- `docs/architecture/16680807091363337727_introspective-rpc-protocol.md` - Plexus RPC protocol design
- `docs/architecture/16680343462706939647_schema-as-membrane.md` - Schema philosophy

## License

MIT

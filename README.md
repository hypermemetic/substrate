# Substrate

A pluggable activation system with hierarchical routing and schema introspection.

## Architecture

```
                         ┌─────────────────────────────────────┐
                         │              Plexus                 │
                         │  - Routes calls to activations      │
                         │  - Unified stream output            │
                         │  - Schema introspection             │
                         └─────────────────────────────────────┘
                                         │
             ┌───────────────────────────┼───────────────────────┐
             │                           │                       │
             ▼                           ▼                       ▼
     ┌───────────────┐           ┌───────────────┐       ┌───────────────┐
     │    Health     │           │     Echo      │       │     Solar     │
     │   (leaf)      │           │    (leaf)     │       │     (hub)     │
     │ - check()     │           │ - echo()      │       │ - observe()   │
     └───────────────┘           │ - once()      │       │ - info()      │
                                 └───────────────┘       └───────┬───────┘
                                                                 │
                                         ┌───────────────────────┼───────────┐
                                         │                       │           │
                                         ▼                       ▼           ▼
                                 ┌───────────────┐       ┌───────────┐  ┌─────────┐
                                 │    Mercury    │       │   Earth   │  │   ...   │
                                 │    (leaf)     │       │   (hub)   │  │         │
                                 │ - info()      │       │ - info()  │  │         │
                                 └───────────────┘       └─────┬─────┘  └─────────┘
                                                               │
                                                               ▼
                                                       ┌───────────────┐
                                                       │     Luna      │
                                                       │    (leaf)     │
                                                       │ - info()      │
                                                       └───────────────┘
```

## Core Concepts

### Activation Trait

The unified interface for all plugins:

```rust
#[async_trait]
pub trait Activation: Send + Sync + 'static {
    type Methods: MethodEnumSchema;

    fn namespace(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;
    fn methods(&self) -> Vec<&str>;

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;

    fn plugin_schema(&self) -> PluginSchema;
}
```

### PlexusStreamItem

Unified output type for all activation streams:

```rust
pub enum PlexusStreamItem {
    Content { metadata, content_type, data },
    Progress { metadata, message, percentage },
    Error { metadata, message, code, recoverable },
    Done { metadata },
}
```

### Schema System

Every activation exposes a `schema` method automatically:

```bash
# Get any activation's schema
plexus_call("echo.schema")
plexus_call("solar.schema")
plexus_call("solar.earth.schema")
plexus_call("solar.earth.luna.schema")
```

Children are listed as summaries (`ChildSummary`: namespace, description, hash), not full recursive schemas. This enables lazy traversal - fetch child schemas individually via `{namespace}.schema`.

## Plugin Patterns

### 1. Leaf Activation (Macro-Generated)

Simple plugins with methods, no children. Use `#[hub_methods]` macro:

```rust
#[derive(Clone)]
pub struct Echo;

#[hub_macro::hub_methods(
    namespace = "echo",
    version = "1.0.0",
    description = "Echo messages back"
)]
impl Echo {
    /// Echo a message back
    #[hub_macro::hub_method(
        description = "Echo a message",
        params(message = "The message to echo")
    )]
    async fn echo(&self, message: String) -> impl Stream<Item = EchoEvent> {
        stream! {
            yield EchoEvent::Echo { message };
        }
    }
}
```

**What the macro generates:**
- `EchoMethod` enum with JSON Schema
- `Activation` trait implementation
- RPC trait and server implementation
- Automatic `schema` method dispatch

### 2. Hub Activation (Macro-Generated with Children)

Plugins that contain other plugins. Add `hub` flag and implement `plugin_children()`:

```rust
#[derive(Clone)]
pub struct Solar {
    system: CelestialBody,
}

#[hub_macro::hub_methods(
    namespace = "solar",
    version = "1.0.0",
    description = "Solar system model",
    hub  // <-- marks this as a hub
)]
impl Solar {
    /// Observe the solar system
    async fn observe(&self) -> impl Stream<Item = SolarEvent> {
        // ...
    }

    /// Required for hubs: return child plugin schemas
    pub fn plugin_children(&self) -> Vec<PluginSchema> {
        self.system.children.iter()
            .map(|planet| planet.to_plugin_schema())
            .collect()
    }
}

// Required for nested routing
#[async_trait]
impl ChildRouter for Solar {
    fn router_namespace(&self) -> &str { "solar" }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        // Return the child activation for routing
        self.system.children.iter()
            .find(|c| c.name.to_lowercase() == name.to_lowercase())
            .map(|c| Box::new(CelestialBodyActivation::new(c.clone())) as Box<dyn ChildRouter>)
    }
}
```

**Hub registration:**
```rust
let plexus = Plexus::new()
    .register(Echo)           // leaf - use register()
    .register_hub(Solar::new());  // hub - use register_hub()
```

### 3. Dynamic Activation (Hand-Implemented)

When plugins are created from runtime data (not compile-time structs), manually implement `Activation`:

```rust
pub struct CelestialBodyActivation {
    body: CelestialBody,
    namespace: String,
}

#[async_trait]
impl Activation for CelestialBodyActivation {
    type Methods = CelestialBodyMethod;

    fn namespace(&self) -> &str { &self.namespace }
    fn version(&self) -> &str { "1.0.0" }
    fn description(&self) -> &str { "Celestial body" }
    fn methods(&self) -> Vec<&str> { vec!["info", "schema"] }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        match method {
            "info" => { /* return info stream */ }
            "schema" => {
                // Must manually handle schema for dynamic plugins
                let schema = self.plugin_schema();
                Ok(wrap_stream(futures::stream::once(async { schema }), ...))
            }
            _ => route_to_child(self, method, params).await
        }
    }

    fn plugin_schema(&self) -> PluginSchema {
        self.body.to_plugin_schema()
    }
}
```

**Key difference:** Dynamic activations must manually:
- Add `"schema"` to `methods()`
- Handle `"schema"` in `call()`
- Implement `ChildRouter` if they have children

### Pattern Comparison

| Pattern | Use Case | Schema | ChildRouter | Registration |
|---------|----------|--------|-------------|--------------|
| Leaf (macro) | Simple plugins | Auto | N/A | `register()` |
| Hub (macro) | Static nested plugins | Auto | Manual | `register_hub()` |
| Dynamic | Runtime-created plugins | Manual | Manual | Via parent |

## Usage

### RPC Access

```bash
# Start the server
cargo run

# Connect via WebSocket
wscat -c ws://localhost:4444

# Call methods
> {"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"echo.echo","params":{"message":"hello","count":3}}}

# Get schemas
> {"jsonrpc":"2.0","id":1,"method":"plexus_schema"}
> {"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"solar.earth.schema"}}
```

### MCP Bridge

Substrate exposes an MCP server that transforms Plexus methods into MCP tools:

```
plexus_call(method, params)  →  mcp__plexus__plexus_call
plexus_schema()              →  mcp__plexus__plexus_schema
echo.echo(message, count)    →  mcp__plexus__echo_echo
solar.observe()              →  mcp__plexus__solar_observe
```

### In Code

```rust
use substrate::{Plexus, activations::{Echo, Health, Solar}};
use futures::StreamExt;

let plexus = Plexus::new()
    .register(Health::new())
    .register(Echo)
    .register_hub(Solar::new());

// Direct call
let mut stream = plexus.call("echo.echo", json!({"message": "hello", "count": 1})).await?;
while let Some(item) = stream.next().await {
    println!("{:?}", item);
}

// Nested call
let mut stream = plexus.call("solar.earth.luna.info", json!({})).await?;
```

## Project Structure

```
src/
├── plexus/
│   ├── plexus.rs       # Plexus struct, Activation trait, routing
│   ├── schema.rs       # PluginSchema, MethodSchema, ChildSummary
│   ├── streaming.rs    # PlexusStream, wrap_stream helpers
│   └── types.rs        # PlexusStreamItem
├── activations/
│   ├── echo/           # Simple leaf activation example
│   ├── health/         # Minimal leaf activation
│   └── solar/          # Hub activation with nested children
│       ├── activation.rs   # Solar hub implementation
│       ├── celestial.rs    # Dynamic CelestialBodyActivation
│       └── types.rs        # SolarEvent, BodyType
├── mcp/                # MCP bridge (Plexus → MCP tools)
└── main.rs             # Server entry point
```

## Building

```bash
cargo build
cargo test
cargo run
```

## License

MIT

# hub-core

Core infrastructure for building hub-based systems with Plexus routing.

## Overview

hub-core provides the foundation for building pluggable systems with hierarchical routing and schema introspection:

- **Plexus** - Central routing and method dispatch
- **Activation** - Trait for implementing plugins
- **PlexusMcpBridge** - MCP server integration via rmcp
- **Handle** - Typed references to plugin method results
- **hub-macro** - Procedural macro for generating activation boilerplate

## Quick Start

```rust
use hub_core::plexus::Plexus;
use hub_core::{Activation, PlexusError};
use std::sync::Arc;

// Create a plexus and register your activations
let plexus = Arc::new(
    Plexus::new()
        .register(MyActivation::new())
);

// Route calls to activations
let stream = plexus.route("myactivation.method", json!({})).await?;
```

## Creating Activations

Use the `hub-macro` crate to generate activation implementations:

```rust
use hub_macro::hub_methods;
use async_stream::stream;

#[derive(Clone)]
pub struct MyApp;

#[hub_methods(
    namespace = "myapp",
    version = "1.0.0",
    description = "My application"
)]
impl MyApp {
    /// Say hello
    #[hub_method]
    async fn hello(&self, name: String) -> impl Stream<Item = MyEvent> + Send + 'static {
        stream! {
            yield MyEvent::Greeting { message: format!("Hello, {}!", name) };
        }
    }
}
```

## MCP Bridge

hub-core includes an MCP server bridge that exposes Plexus activations as MCP tools:

```rust
use hub_core::{Plexus, PlexusMcpBridge};

let plexus = Arc::new(Plexus::new().register(MyApp::new()));
let bridge = PlexusMcpBridge::new(plexus);

// Use with rmcp server
```

## Example Activations

hub-core includes two minimal example activations:

- **health** - Health check endpoint (manual Activation impl)
- **echo** - Echo messages back (hub-macro generated)

See `src/activations/` for implementation examples.

## License

AGPL-3.0-only

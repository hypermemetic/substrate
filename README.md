# Cognition Pipeline

A pluggable agent context management system with hub-and-spoke architecture.

## Architecture

```
                    ┌─────────────────────────────────────┐
                    │              Hub                    │
                    │  - Routes calls to plugins          │
                    │  - Unified stream output            │
                    │  - Dual access: programmatic + RPC  │
                    └─────────────────────────────────────┘
                                    │
            ┌───────────────────────┼───────────────────────┐
            │                       │                       │
            ▼                       ▼                       ▼
    ┌───────────────┐       ┌───────────────┐       ┌───────────────┐
    │ HealthPlugin  │       │  BashPlugin   │       │   YourPlugin  │
    │               │       │               │       │               │
    │ - check()     │       │ - execute()   │       │ - method()    │
    └───────────────┘       └───────────────┘       └───────────────┘
```

### Core Concepts

**Plugin Trait**: Single unified interface for all plugins
```rust
#[async_trait]
pub trait Plugin: Send + Sync + Clone + 'static {
    fn namespace(&self) -> &str;
    fn methods(&self) -> Vec<&str>;
    async fn call(&self, method: &str, params: Value) -> Result<HubStream, HubError>;
    fn into_rpc_methods(self) -> Methods;
}
```

**HubStreamItem**: Unified output type for all plugin streams
```rust
pub enum HubStreamItem {
    Progress { path, message, percentage },
    Data { path, content_type, data },
    Error { path, error, recoverable },
    Done { path },
}
```

**PluginPath**: Tracks nested plugin call chains
```rust
let path = PluginPath::root("bash");           // ["bash"]
let nested = path.extend("subprocess");         // ["bash", "subprocess"]
```

## Usage

### Programmatic Access (hub-cli)

```bash
# Health check
./target/debug/hub-cli health.check

# Execute bash command
./target/debug/hub-cli bash.execute 'echo hello && ls -la'
```

### JSON-RPC Server

```bash
# Start the server
./target/debug/cognition-rpc

# Server listens on ws://127.0.0.1:9944
# Endpoints:
#   - health_check (subscription)
#   - bash_execute (subscription)
```

### In Code

```rust
use cognition_pipeline::{
    hub::Hub,
    plugins::{bash::BashPlugin, health::HealthPlugin},
};
use futures::StreamExt;
use serde_json::json;

// Build hub with plugins
let hub = Hub::new()
    .register(HealthPlugin::new())
    .register(BashPlugin::new());

// Programmatic access - call and stream results
let mut stream = hub.call("bash.execute", json!("echo hello")).await?;
while let Some(item) = stream.next().await {
    println!("{:?}", item);
}

// Or convert to JSON-RPC server (consumes hub)
let module = hub.into_rpc_module()?;
let server = Server::builder().build("127.0.0.1:9944").await?;
server.start(module);
```

## Project Structure

```
src/
├── hub/
│   ├── hub.rs          # Hub struct and Plugin trait
│   ├── path.rs         # PluginPath for tracking call chains
│   └── types.rs        # HubStreamItem unified output type
├── plugin_system/
│   ├── conversion.rs   # IntoSubscription trait for RPC
│   └── types.rs        # PluginStreamItem trait
├── plugins/
│   ├── README.md       # Plugin development guide
│   ├── health/         # Simple plugin example
│   │   ├── plugin.rs   # HealthPlugin implementation
│   │   └── types.rs    # HealthStatus type
│   └── bash/           # Complex plugin example
│       ├── executor/   # Core system (standalone)
│       ├── plugin.rs   # BashPlugin (RPC adapter)
│       └── types.rs    # BashOutput, BashError
├── bin/
│   └── hub-cli.rs      # CLI for programmatic hub access
└── main.rs             # JSON-RPC server entry point
```

## Writing Plugins

See `src/plugins/README.md` for the complete plugin development guide.

**Key principle**: Plugins are standalone systems first, RPC interfaces second.

1. Define domain types implementing `PluginStreamItem`
2. Implement core business logic (independent of RPC)
3. Implement `Plugin` trait to expose via Hub
4. The `into_rpc_methods()` bridges to JSON-RPC

### Simple Plugin (health)

```rust
#[derive(Clone)]
pub struct HealthPlugin { start_time: Instant }

#[async_trait]
impl Plugin for HealthPlugin {
    fn namespace(&self) -> &str { "health" }
    fn methods(&self) -> Vec<&str> { vec!["check"] }

    async fn call(&self, method: &str, _params: Value) -> Result<HubStream, HubError> {
        match method {
            "check" => {
                let stream = self.check_stream().await;
                Ok(into_hub_stream(stream, PluginPath::root("health")))
            }
            _ => Err(HubError::MethodNotFound { ... })
        }
    }

    fn into_rpc_methods(self) -> Methods {
        self.into_rpc().into()  // Uses jsonrpsee macro
    }
}
```

### Complex Plugin (bash)

```
bash/
├── executor/mod.rs   # BashExecutor - standalone, testable
├── plugin.rs         # BashPlugin - thin wrapper + Plugin impl
└── types.rs          # BashOutput enum
```

The core `BashExecutor` can be used directly without any RPC:

```rust
let executor = BashExecutor::new();
let outputs = executor.execute_collect("echo hello").await;
```

## Building

```bash
cargo build

# Run tests
cargo test

# Run the JSON-RPC server
cargo run --bin cognition-rpc

# Run the CLI
cargo run --bin hub-cli -- health.check
```

## License

MIT

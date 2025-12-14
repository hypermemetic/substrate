# Plugin Architecture Guide

## Philosophy

**Plugins are RPC adapters over standalone systems.**

Every plugin follows a two-layer architecture:

1. **Core System Layer**: A standalone struct/module that implements the actual functionality
2. **RPC Adapter Layer**: A thin wrapper that exposes the core system over JSON-RPC

This separation ensures:
- Core functionality can be used programmatically without RPC overhead
- Business logic is testable independent of RPC concerns
- Multiple RPC interfaces could expose the same core system differently

## File Structure

### Simple Plugin (Single File)

For simple plugins where the core system fits in one file:

```
src/plugins/health/
├── mod.rs          # Public exports
├── types.rs        # Domain types (HealthStatus, etc.)
└── plugin.rs       # Core system + RPC adapter
```

**`types.rs`**: Domain-specific types
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub uptime_seconds: u64,
    pub timestamp: i64,
}

// Implement PluginStreamItem to convert to HubStreamItem
impl PluginStreamItem for HealthStatus {
    fn into_hub_item(self, path: PluginPath) -> HubStreamItem {
        HubStreamItem::Data {
            path,
            content_type: std::any::type_name::<Self>().to_string(),
            data: serde_json::to_value(self).unwrap(),
        }
    }
}
```

**`plugin.rs`**: Core system implementation
```rust
/// Core system - can be used programmatically
pub struct HealthPlugin {
    start_time: Instant,
}

impl HealthPlugin {
    pub fn new() -> Self {
        Self { start_time: Instant::now() }
    }

    /// Business logic method - returns tightly-typed stream
    async fn check_stream(&self)
        -> Pin<Box<dyn Stream<Item = HealthStatus> + Send + 'static>>
    {
        let uptime = self.start_time.elapsed().as_secs();
        Box::pin(stream! {
            yield HealthStatus {
                status: "healthy".to_string(),
                uptime_seconds: uptime,
                timestamp: chrono::Utc::now().timestamp(),
            };
        })
    }
}

/// RPC adapter trait - defines the JSON-RPC interface
#[rpc(server, namespace = "health")]
pub trait HealthRpc {
    #[subscription(
        name = "check",
        unsubscribe = "unsubscribe_check",
        item = serde_json::Value
    )]
    async fn check(&self) -> SubscriptionResult;
}

/// RPC adapter implementation - bridges core system to RPC
#[async_trait]
impl HealthRpcServer for HealthPlugin {
    async fn check(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.check_stream().await;
        let path = PluginPath::root("health");
        stream.into_subscription(pending, path).await
    }
}
```

**`mod.rs`**: Public exports
```rust
mod plugin;
mod types;

pub use plugin::{HealthPlugin, HealthRpcServer};
pub use types::HealthStatus;
```

### Complex Plugin (Multi-Module)

For complex plugins where the core system needs multiple files:

```
src/plugins/bash/
├── mod.rs              # Public exports
├── types.rs            # Domain types (BashOutput, BashError, etc.)
├── plugin.rs           # RPC adapter only
└── executor/           # Core system submodule
    ├── mod.rs          # Executor public exports
    ├── process.rs      # Process management
    ├── stream.rs       # Output streaming
    └── session.rs      # Session state
```

**Pattern**: Core system lives in its own submodule, RPC adapter stays thin.

**`executor/mod.rs`**: Core system exports
```rust
mod process;
mod session;
mod stream;

pub use process::BashProcess;
pub use session::BashSession;
pub use stream::OutputStream;

/// Core system - standalone bash executor
pub struct BashExecutor {
    sessions: HashMap<String, BashSession>,
}

impl BashExecutor {
    pub fn new() -> Self { /* ... */ }

    /// Business logic - programmatic API
    pub async fn execute(&mut self, cmd: &str) -> OutputStream { /* ... */ }
    pub async fn create_session(&mut self) -> String { /* ... */ }
    pub async fn kill_session(&mut self, id: &str) -> Result<()> { /* ... */ }
}
```

**`plugin.rs`**: RPC adapter wraps core system
```rust
use super::executor::BashExecutor;
use super::types::*;

/// RPC adapter trait
#[rpc(server, namespace = "bash")]
pub trait BashRpc {
    #[subscription(name = "execute", ...)]
    async fn execute(&self, command: String) -> SubscriptionResult;
}

/// RPC adapter struct - wraps core system
pub struct BashPlugin {
    executor: Arc<Mutex<BashExecutor>>,
}

impl BashPlugin {
    pub fn new() -> Self {
        Self {
            executor: Arc::new(Mutex::new(BashExecutor::new())),
        }
    }

    /// Thin wrapper - delegates to core system
    async fn execute_stream(&self, command: String)
        -> Pin<Box<dyn Stream<Item = BashOutput> + Send + 'static>>
    {
        let executor = self.executor.clone();
        Box::pin(stream! {
            let mut exec = executor.lock().await;
            let mut output_stream = exec.execute(&command).await;

            while let Some(output) = output_stream.next().await {
                yield output;
            }
        })
    }
}

/// RPC adapter implementation
#[async_trait]
impl BashRpcServer for BashPlugin {
    async fn execute(&self, pending: PendingSubscriptionSink, command: String)
        -> SubscriptionResult
    {
        let stream = self.execute_stream(command).await;
        let path = PluginPath::root("bash");
        stream.into_subscription(pending, path).await
    }
}
```

## Step-by-Step Plugin Creation

### 1. Design Your Domain Types

Create `types.rs` with:
- Input/output types for your system
- Implement `PluginStreamItem` for any type you'll stream

```rust
use crate::plugin_system::types::PluginStreamItem;
use crate::hub::{path::PluginPath, types::HubStreamItem};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyOutput {
    pub data: String,
}

impl PluginStreamItem for MyOutput {
    fn into_hub_item(self, path: PluginPath) -> HubStreamItem {
        HubStreamItem::Data {
            path,
            content_type: std::any::type_name::<Self>().to_string(),
            data: serde_json::to_value(self).unwrap(),
        }
    }
}
```

### 2. Implement Core System

**Simple case**: Add to `plugin.rs`
**Complex case**: Create `executor/` or `core/` submodule

Core system should:
- Have no RPC dependencies
- Return native Rust types
- Be independently testable
- Use standard async Rust patterns

```rust
pub struct MySystem {
    // Internal state
}

impl MySystem {
    pub fn new() -> Self { /* ... */ }

    /// Business logic - pure Rust
    pub async fn do_work(&self, input: &str)
        -> Pin<Box<dyn Stream<Item = MyOutput> + Send + 'static>>
    {
        Box::pin(stream! {
            // Your logic here
            yield MyOutput { data: input.to_string() };
        })
    }
}
```

### 3. Create RPC Adapter

In `plugin.rs`, define the RPC interface:

```rust
use jsonrpsee::proc_macros::rpc;
use crate::plugin_system::conversion::SubscriptionResult;

#[rpc(server, namespace = "myplugin")]
pub trait MyRpc {
    #[subscription(
        name = "do_work",
        unsubscribe = "unsubscribe_do_work",
        item = serde_json::Value
    )]
    async fn do_work(&self, input: String) -> SubscriptionResult;
}
```

### 4. Implement RPC Adapter

Bridge your core system to the RPC trait:

```rust
use async_trait::async_trait;
use jsonrpsee::PendingSubscriptionSink;
use crate::{
    hub::path::PluginPath,
    plugin_system::conversion::IntoSubscription,
};

pub struct MyPlugin {
    system: MySystem,
}

impl MyPlugin {
    pub fn new() -> Self {
        Self { system: MySystem::new() }
    }
}

#[async_trait]
impl MyRpcServer for MyPlugin {
    async fn do_work(&self, pending: PendingSubscriptionSink, input: String)
        -> SubscriptionResult
    {
        let stream = self.system.do_work(&input).await;
        let path = PluginPath::root("myplugin");
        stream.into_subscription(pending, path).await
    }
}
```

### 5. Export from Module

In `mod.rs`:

```rust
mod plugin;
mod types;

// For complex plugins:
// mod executor;

pub use plugin::{MyPlugin, MyRpcServer};
pub use types::MyOutput;
```

### 6. Register in Main

In `src/main.rs`:

```rust
use cognition_pipeline::plugins::myplugin::MyPlugin;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut module = RpcModule::new(());

    let my_plugin = MyPlugin::new();
    module.merge(my_plugin.into_rpc())?;

    // Start server...
}
```

## Key Patterns

### Always Return Streams

Even for single-value responses, use streams for consistency:

```rust
async fn single_value(&self) -> Pin<Box<dyn Stream<Item = MyType> + Send + 'static>> {
    Box::pin(stream! {
        yield MyType { /* ... */ };
    })
}
```

### Path Tracking for Nested Calls

If your plugin calls other plugins:

```rust
async fn nested_call(&self, pending: PendingSubscriptionSink)
    -> SubscriptionResult
{
    let stream = self.inner_stream().await;
    let path = PluginPath::root("outer").extend("inner");
    stream.into_subscription(pending, path).await
}
```

### Use HubStreamItem Variants

Your streams can yield different event types:

```rust
impl PluginStreamItem for MyProgress {
    fn into_hub_item(self, path: PluginPath) -> HubStreamItem {
        HubStreamItem::Progress {
            path,
            message: self.message,
            percentage: Some(self.percent),
        }
    }
}

impl PluginStreamItem for MyError {
    fn into_hub_item(self, path: PluginPath) -> HubStreamItem {
        HubStreamItem::Error {
            path,
            error: self.message,
            recoverable: self.can_retry,
        }
    }
}
```

## Testing Strategy

### Test Core System Independently

```rust
#[tokio::test]
async fn test_core_system() {
    let system = MySystem::new();
    let mut stream = system.do_work("test").await;

    let output = stream.next().await.unwrap();
    assert_eq!(output.data, "test");
}
```

### Test RPC Integration

```rust
#[tokio::test]
async fn test_rpc_adapter() {
    let plugin = MyPlugin::new();
    let module = plugin.into_rpc();
    // Use jsonrpsee test client
}
```

## Common Mistakes to Avoid

1. **Don't put RPC types in core system** - Keep jsonrpsee types only in the adapter layer
2. **Don't forget PluginStreamItem** - All stream items must implement this trait
3. **Don't block the executor** - Use async/await, never blocking calls
4. **Don't forget path tracking** - Always create a PluginPath for your streams
5. **Don't skip error handling** - Use HubStreamItem::Error for recoverable errors

## Decision Tree: Simple vs Complex

**Use simple structure (plugin.rs only) when:**
- Core system is < 200 lines
- Single responsibility (health check, ping, etc.)
- Minimal state management

**Use complex structure (executor/ submodule) when:**
- Core system is > 200 lines
- Multiple related components (process, session, stream)
- Needs internal module organization
- Could be extracted as a separate crate

## Next Steps

After creating your plugin:
1. Test core system independently
2. Test RPC integration
3. Add to `src/plugins/mod.rs`
4. Register in `src/main.rs`
5. Create test client in `bin/test-<plugin>/`
6. Document wire format and example usage

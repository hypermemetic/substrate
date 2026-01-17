# Plugin Development Guide for Plexus Hub

This document provides a complete guide for developing plugins (activations) for the Plexus hub system. After reading this guide, you should be able to create plugins without reviewing the codebase, and another developer should be able to integrate your plugin into Plexus without issue.

## Table of Contents

1. [Core Concepts](#core-concepts)
2. [Quick Start: Minimal Plugin](#quick-start-minimal-plugin)
3. [The Hub Macro System](#the-hub-macro-system)
4. [Manual Implementation (No Macros)](#manual-implementation-no-macros)
5. [Hub Plugins with Children](#hub-plugins-with-children)
6. [Parent Context Injection](#parent-context-injection)
7. [Handle System](#handle-system)
8. [Synapse CLI Impact](#synapse-cli-impact)
9. [Registration and Integration](#registration-and-integration)
10. [Best Practices](#best-practices)
11. [Reference: Complete Examples](#reference-complete-examples)

---

## Core Concepts

### What is an Activation?

An **Activation** is a plugin that registers with the Plexus hub. Each activation:

- Has a unique namespace (e.g., `"echo"`, `"health"`, `"solar"`)
- Exposes methods that can be called via RPC
- Returns streaming responses wrapped in `PlexusStreamItem`
- Has a deterministic UUID for handle routing

### The Caller-Wraps Architecture

Plexus uses a "caller-wraps" streaming architecture:

1. **Activations return typed domain events** - Your methods return `impl Stream<Item = YourEvent>`
2. **The framework wraps with metadata** - `wrap_stream()` adds provenance, timestamps, and Done events
3. **All responses are `PlexusStreamItem`** - A universal envelope with Data, Progress, Error, and Done variants

```
Your Plugin                    Plexus Framework
    │                                │
    │ Stream<HealthEvent>            │
    ├────────────────────────────────►│
    │                                │ wrap_stream()
    │                                ├─────────────────►
    │                                │ PlexusStream
```

### Streaming Response Types

All plugin responses ultimately become `PlexusStreamItem`:

```rust
pub enum PlexusStreamItem {
    Data {
        metadata: StreamMetadata,
        content_type: String,    // e.g., "health.status"
        content: Value,          // Your serialized event
    },
    Progress {
        metadata: StreamMetadata,
        message: String,
        percentage: Option<f32>,
    },
    Error {
        metadata: StreamMetadata,
        message: String,
        code: Option<String>,
        recoverable: bool,
    },
    Done {
        metadata: StreamMetadata,
    },
}
```

---

## Quick Start: Minimal Plugin

Here's the simplest possible plugin using the `hub_methods` macro:

### Step 1: Create the Event Type

```rust
// src/activations/mywidget/types.rs
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WidgetEvent {
    Created { id: String, name: String },
    Updated { id: String, field: String, value: String },
}
```

### Step 2: Create the Activation

```rust
// src/activations/mywidget/activation.rs
use super::types::WidgetEvent;
use async_stream::stream;
use futures::Stream;

#[derive(Clone)]
pub struct MyWidget;

impl MyWidget {
    pub fn new() -> Self {
        MyWidget
    }
}

impl Default for MyWidget {
    fn default() -> Self {
        Self::new()
    }
}

#[hub_macro::hub_methods(
    namespace = "mywidget",
    version = "1.0.0",
    description = "Widget management - short description (max 15 words)"
)]
impl MyWidget {
    #[hub_macro::hub_method(
        description = "Create a new widget with the given name",
        params(name = "Human-readable widget name")
    )]
    async fn create(
        &self,
        name: String,
    ) -> impl Stream<Item = WidgetEvent> + Send + 'static {
        let id = uuid::Uuid::new_v4().to_string();
        stream! {
            yield WidgetEvent::Created { id, name };
        }
    }
}
```

### Step 3: Create the Module

```rust
// src/activations/mywidget/mod.rs
mod activation;
mod types;

pub use activation::MyWidget;
pub use types::WidgetEvent;
```

### Step 4: Register with Plexus

```rust
// In builder.rs or your initialization code
use crate::activations::mywidget::MyWidget;

let plexus = Plexus::new()
    .register(MyWidget::new())
    // ... other activations
```

---

## The Hub Macro System

### `#[hub_methods]` Attribute

This attribute on an `impl` block generates the complete Activation implementation:

```rust
#[hub_macro::hub_methods(
    namespace = "mywidget",           // Required: unique namespace
    version = "1.0.0",                // Optional: semantic version (default: "1.0.0")
    description = "Short description", // Optional: max 15 words
    long_description = "Detailed...",  // Optional: full documentation
    hub,                              // Optional flag: enables child routing
    resolve_handle,                   // Optional flag: enables handle resolution
    plugin_id = "uuid-string",        // Optional: override deterministic UUID
    namespace_fn = "method_name"      // Optional: dynamic namespace via method
)]
impl MyWidget {
    // Methods go here...
}
```

**What it generates:**
- `MyWidgetMethod` enum for method dispatch
- `MyWidgetRpc` trait for JSON-RPC
- `MyWidgetRpcServer` implementation
- `impl Activation for MyWidget`

### `#[hub_method]` Attribute

Marks individual methods for exposure:

```rust
#[hub_macro::hub_method(
    description = "Human-readable description",
    params(
        name = "Parameter description",
        count = "Another parameter"
    ),
    streaming,                        // Optional flag: indicates multiple events
    returns(VariantA, VariantB)       // Optional: filter return schema variants
)]
async fn method_name(
    &self,
    name: String,
    count: u32,
) -> impl Stream<Item = MyEvent> + Send + 'static {
    // Implementation
}
```

### Event Type Requirements

Event types need only standard derives:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]  // Recommended tagging
pub enum MyEvent {
    Success { data: String },
    Progress { percent: f32 },
}
```

**Key requirements:**
- `Serialize` - for JSON output
- `Deserialize` - for schema generation
- `JsonSchema` - for automatic documentation
- Use `#[serde(tag = "type")]` for discriminated unions

### Optional Parameters

Use `Option<T>` for optional parameters:

```rust
#[hub_macro::hub_method(
    params(
        required_param = "This is required",
        optional_param = "This is optional (can be null or omitted)"
    )
)]
async fn my_method(
    &self,
    required_param: String,
    optional_param: Option<i32>,  // Accepts null or missing field
) -> impl Stream<Item = MyEvent> + Send + 'static {
    // ...
}
```

---

## Manual Implementation (No Macros)

If you need more control, you can implement the `Activation` trait manually. This is the pattern used by the `Health` activation as a reference implementation.

### Step 1: Define the Method Enum

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum MyWidgetMethod {
    Create { name: String },
    Update { id: String, field: String, value: String },
}

impl MyWidgetMethod {
    pub fn description(method: &str) -> Option<&'static str> {
        match method {
            "create" => Some("Create a new widget"),
            "update" => Some("Update an existing widget"),
            _ => None,
        }
    }
}
```

### Step 2: Implement the RPC Trait (Optional)

```rust
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::PendingSubscriptionSink;

#[rpc(server, namespace = "mywidget")]
pub trait MyWidgetRpc {
    #[subscription(name = "create", unsubscribe = "unsubscribe_create", item = serde_json::Value)]
    async fn create(&self, name: String) -> SubscriptionResult;
}
```

### Step 3: Implement the Activation Trait

```rust
use crate::plexus::{
    wrap_stream, Activation, MethodSchema, PlexusError, PlexusStream, PluginSchema,
};
use async_trait::async_trait;
use jsonrpsee::core::server::Methods;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[async_trait]
impl Activation for MyWidget {
    type Methods = MyWidgetMethod;

    fn namespace(&self) -> &str {
        "mywidget"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Widget management plugin"
    }

    fn methods(&self) -> Vec<&str> {
        vec!["create", "update", "schema"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        MyWidgetMethod::description(method).map(|s| s.to_string())
    }

    // Optional: override for custom plugin ID
    fn plugin_id(&self) -> uuid::Uuid {
        // Default formula: Uuid::new_v5(&Uuid::NAMESPACE_OID, "mywidget@1".as_bytes())
        uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("{}@{}", self.namespace(), "1").as_bytes()
        )
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        match method {
            "create" => {
                let name: String = params.get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| PlexusError::InvalidParams("missing field: name".into()))?;

                let stream = self.create_internal(name);
                Ok(wrap_stream(stream, "mywidget.create", vec!["mywidget".into()]))
            }
            "schema" => {
                // Schema method is standard - return plugin schema
                let plugin_schema = self.plugin_schema();
                Ok(wrap_stream(
                    futures::stream::once(async move {
                        crate::plexus::SchemaResult::Plugin(plugin_schema)
                    }),
                    "mywidget.schema",
                    vec!["mywidget".into()]
                ))
            }
            _ => Err(PlexusError::MethodNotFound {
                activation: "mywidget".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn into_rpc_methods(self) -> Methods {
        self.into_rpc().into()
    }

    fn plugin_schema(&self) -> PluginSchema {
        // Compute method hashes for cache invalidation
        let methods = vec![
            MethodSchema::new("create", "Create a new widget", compute_hash("create", "Create a new widget"))
                .with_returns(schemars::schema_for!(WidgetEvent)),
            MethodSchema::new("schema", "Get plugin schema", compute_hash("schema", "Get plugin schema")),
        ];

        PluginSchema::leaf(self.namespace(), self.version(), self.description(), methods)
    }
}

fn compute_hash(name: &str, desc: &str) -> String {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    desc.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
```

### The `wrap_stream` Helper

This is the key function for the caller-wraps architecture:

```rust
use crate::plexus::wrap_stream;

// Signature:
pub fn wrap_stream<T: Serialize + Send + 'static>(
    stream: impl Stream<Item = T> + Send + 'static,
    content_type: &'static str,   // e.g., "mywidget.create"
    provenance: Vec<String>,      // e.g., vec!["mywidget".into()]
) -> PlexusStream;
```

It automatically:
- Wraps each event in `PlexusStreamItem::Data`
- Adds metadata (provenance, plexus hash, timestamp)
- Appends a `Done` event when the stream completes

---

## Hub Plugins with Children

Hub plugins support nested routing to child plugins. This is useful for hierarchical structures like the Solar system example.

### Step 1: Add the `hub` Flag

```rust
#[hub_macro::hub_methods(
    namespace = "myparent",
    version = "1.0.0",
    description = "Parent plugin with children",
    hub  // This enables child routing
)]
impl MyParent {
    // Methods...

    /// Get child summaries for schema generation
    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        self.children.iter()
            .map(|child| ChildSummary {
                namespace: child.name.clone(),
                description: child.description.clone(),
                hash: child.compute_hash(),
            })
            .collect()
    }
}
```

### Step 2: Implement ChildRouter

```rust
use crate::plexus::{ChildRouter, PlexusError, PlexusStream};
use async_trait::async_trait;

#[async_trait]
impl ChildRouter for MyParent {
    fn router_namespace(&self) -> &str {
        "myparent"
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        // Delegate to Activation::call for local methods + nested routing
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        self.children.iter()
            .find(|c| c.name == name)
            .map(|c| Box::new(ChildActivation::new(c.clone())) as Box<dyn ChildRouter>)
    }
}
```

### Step 3: Register with `register_hub`

```rust
let plexus = Plexus::new()
    .register_hub(MyParent::new())  // Use register_hub, not register
    // ...
```

### Nested Routing Flow

When a call like `myparent.child1.method` is received:

1. Plexus routes to `myparent`
2. `myparent.call("child1.method", params)` is invoked
3. The macro-generated code sees the dot and calls `get_child("child1")`
4. The child receives `method` as its method name

---

## Parent Context Injection

Some plugins need to call other plugins or resolve handles. This requires a reference to the parent Plexus, which is achieved via generic context injection.

### The HubContext Trait

```rust
#[async_trait]
pub trait HubContext: Clone + Send + Sync + 'static {
    async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError>;
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;
    fn is_valid(&self) -> bool;
}
```

### Making Your Plugin Generic Over Context

```rust
use crate::plexus::hub_context::{HubContext, NoParent};
use std::sync::OnceLock;

#[derive(Clone)]
pub struct MyPlugin<P: HubContext = NoParent> {
    // Your state...
    parent: OnceLock<P>,
}

impl<P: HubContext> MyPlugin<P> {
    /// Create with explicit context type (for injection later)
    pub fn with_context_type() -> Self {
        Self {
            parent: OnceLock::new(),
        }
    }

    /// Inject parent context (called during Plexus construction)
    pub fn inject_parent(&self, parent: P) {
        let _ = self.parent.set(parent);
    }

    /// Get parent context for making calls
    fn parent(&self) -> Option<&P> {
        self.parent.get().filter(|p| p.is_valid())
    }

    /// Call another plugin
    async fn call_sibling(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        let parent = self.parent()
            .ok_or_else(|| PlexusError::ExecutionError("No parent context".into()))?;
        parent.call(method, params).await
    }
}
```

### Registration with Context Injection

```rust
use std::sync::{Arc, Weak};

// Use Arc::new_cyclic to inject Weak<Plexus> during construction
let plexus = Arc::new_cyclic(|weak_plexus: &Weak<Plexus>| {
    let my_plugin: MyPlugin<Weak<Plexus>> = MyPlugin::with_context_type();
    my_plugin.inject_parent(weak_plexus.clone());

    Plexus::new()
        .register(my_plugin)
});
```

### NoParent for Testing

For unit tests without a full Plexus:

```rust
use crate::plexus::hub_context::NoParent;

#[tokio::test]
async fn test_my_plugin() {
    let plugin: MyPlugin<NoParent> = MyPlugin::with_context_type();
    // Test methods that don't need parent context
}
```

---

## Handle System

Handles are references to data owned by plugins. They enable cross-plugin data access.

### Handle Format

```
{plugin_id}@{version}::{method}:meta[0]:meta[1]:...

Example: 550e8400-e29b-41d4-a716-446655440000@1.0.0::chat:msg-123:user:bob
```

### Creating Handles

```rust
use crate::types::Handle;

// Basic handle
let handle = Handle::new(
    self.plugin_id(),
    self.version(),
    "create"
);

// Handle with metadata
let handle = Handle::new(self.plugin_id(), "1.0.0", "chat")
    .push_meta(&message_id)
    .push_meta(&role);
```

### Plugin IDs

Plugin IDs are **deterministic UUIDs** generated from `namespace@major_version`:

```rust
// Formula (DO NOT CHANGE - breaks existing handles):
uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, "mywidget@1".as_bytes())
```

This ensures:
- Same plugin always has same UUID
- Minor/patch version changes don't break handles
- Different namespaces have different UUIDs

### Implementing Handle Resolution

To support handle resolution, add the `resolve_handle` flag:

```rust
#[hub_macro::hub_methods(
    namespace = "mywidget",
    version = "1.0.0",
    resolve_handle  // Enable this flag
)]
impl MyWidget {
    /// Resolve a handle to its data
    pub async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
        match handle.method.as_str() {
            "create" => {
                let id = handle.meta.first()
                    .ok_or_else(|| PlexusError::InvalidParams("missing id in handle".into()))?;

                // Look up and return the data
                let data = self.storage.get(id).await?;
                Ok(wrap_stream(
                    futures::stream::once(async move { data }),
                    "mywidget.resolve",
                    vec!["mywidget".into()]
                ))
            }
            _ => Err(PlexusError::HandleNotSupported(self.namespace().to_string())),
        }
    }
}
```

---

## Synapse CLI Impact

Synapse is the CLI client that discovers and navigates plugins dynamically from their schemas. Your plugin architecture decisions directly impact how users interact with your plugin via CLI.

### Schema Discovery

Synapse fetches schemas via `plexus.schema` and generates CLI commands automatically:

```
$ synapse plexus mywidget create --name "Test Widget"
```

The CLI structure mirrors your plugin structure:
- `plexus` → the backend
- `mywidget` → your namespace
- `create` → your method
- `--name` → your parameter

### Impact of Plugin Decisions

| Decision | Synapse Impact |
|----------|----------------|
| **Namespace** | Becomes the command group name |
| **Method names** | Become subcommands |
| **Parameter names** | Become CLI flags (`--name`, `--id`) |
| **Description** | Shown in `--help` output |
| **Hub plugin** | Creates nested command groups |
| **Child plugins** | Adds sub-subcommands |

### Hub Plugin CLI Structure

For a hub plugin like Solar:

```
$ synapse plexus solar observe          # Call solar.observe
$ synapse plexus solar mercury info     # Call solar.mercury.info
$ synapse plexus solar jupiter io info  # Call solar.jupiter.io.info
```

### Best Practices for CLI

1. **Use clear, short namespace names** - They become commands
2. **Use lowercase method names** - `create` not `createWidget`
3. **Keep descriptions concise** - They appear in `--help`
4. **Use meaningful parameter names** - `--name` not `--n`
5. **Document optional parameters** - Users need to know what's required

### Template Rendering

Synapse uses Mustache templates to render responses. Template resolution order:

1. `.substrate/templates/{namespace}/{method}.mustache` (project-local)
2. `~/.config/synapse/templates/{namespace}/{method}.mustache` (user global)
3. `{namespace}/default.mustache` (namespace default)
4. `default.mustache` (global default)

Your event types should have meaningful field names for template rendering:

```rust
// Good - clear field names for templates
pub enum WidgetEvent {
    Created { id: String, name: String, created_at: String },
}

// Bad - unclear field names
pub enum WidgetEvent {
    Created { a: String, b: String, c: String },
}
```

---

## Registration and Integration

### Adding Your Plugin to Plexus

1. **Add to activations module:**
   ```rust
   // src/activations/mod.rs
   pub mod mywidget;
   ```

2. **Update builder.rs:**
   ```rust
   use crate::activations::mywidget::MyWidget;

   pub async fn build_plexus() -> Arc<Plexus> {
       Plexus::new()
           .register(MyWidget::new())
           // For hub plugins:
           // .register_hub(MyHubPlugin::new())
   }
   ```

3. **Export if needed:**
   ```rust
   // src/lib.rs (if plugin should be public API)
   pub use activations::mywidget::{MyWidget, WidgetEvent};
   ```

### Registration Methods

| Method | Use Case |
|--------|----------|
| `.register(plugin)` | Simple leaf plugins |
| `.register_hub(plugin)` | Hub plugins with children |

### Async Initialization

If your plugin needs async setup (database, network, etc.):

```rust
impl MyWidget {
    pub async fn new(config: MyConfig) -> Result<Self, MyError> {
        let storage = Storage::connect(&config.db_url).await?;
        Ok(Self { storage })
    }
}

// In builder.rs:
pub async fn build_plexus() -> Arc<Plexus> {
    let my_widget = MyWidget::new(MyConfig::default())
        .await
        .expect("Failed to initialize MyWidget");

    Plexus::new()
        .register(my_widget)
}
```

---

## Best Practices

### Naming Conventions

- **Namespace**: lowercase, singular noun (`widget`, `user`, `solar`)
- **Methods**: lowercase, verb (`create`, `update`, `observe`)
- **Event types**: PascalCase, descriptive (`WidgetCreated`, `UserUpdated`)
- **Event variants**: Action-based (`Created`, `Updated`, `Deleted`)

### Error Handling

Use `PlexusError` variants:

```rust
Err(PlexusError::InvalidParams("name cannot be empty".into()))
Err(PlexusError::ExecutionError("database connection failed".into()))
Err(PlexusError::MethodNotFound { activation: "mywidget".into(), method: "invalid".into() })
```

For recoverable errors, use the error stream:

```rust
use crate::plexus::{error_stream, error_stream_with_code};

// Simple error
error_stream("Something failed".into(), vec!["mywidget".into()], true)

// Error with code
error_stream_with_code(
    "Item not found".into(),
    "NOT_FOUND".into(),
    vec!["mywidget".into()],
    true  // recoverable
)
```

### Streaming Best Practices

1. **Yield early, yield often** - Don't buffer everything
2. **Use progress events** for long operations
3. **Keep event types small** - Large payloads should be handles
4. **Document what events a method emits**

### Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_create() {
        let widget = MyWidget::new();
        let stream = widget.create("Test".into()).await;

        let items: Vec<_> = stream.collect().await;
        assert!(!items.is_empty());
    }
}
```

---

## Reference: Complete Examples

### Minimal Plugin (Echo Pattern)

```rust
// types.rs
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EchoEvent {
    Echo { message: String, count: u32 },
}

// activation.rs
use super::types::EchoEvent;
use async_stream::stream;
use futures::Stream;

#[derive(Clone)]
pub struct Echo;

impl Echo {
    pub fn new() -> Self { Echo }
}

impl Default for Echo {
    fn default() -> Self { Self::new() }
}

#[hub_macro::hub_methods(
    namespace = "echo",
    version = "1.0.0",
    description = "Echo messages back"
)]
impl Echo {
    #[hub_macro::hub_method(
        description = "Echo a message",
        params(message = "The message to echo", count = "Times to repeat")
    )]
    async fn echo(
        &self,
        message: String,
        count: u32,
    ) -> impl Stream<Item = EchoEvent> + Send + 'static {
        stream! {
            for i in 0..count {
                yield EchoEvent::Echo { message: message.clone(), count: i + 1 };
            }
        }
    }
}
```

### Hub Plugin (Solar Pattern)

See `/substrate/src/activations/solar/` for a complete example of:
- Hub plugin with nested children
- `ChildRouter` implementation
- Dynamic child discovery
- Nested method routing

### Context-Aware Plugin (Cone Pattern)

See `/substrate/src/activations/cone/` for a complete example of:
- Generic `HubContext` parameter
- Parent context injection
- Handle resolution
- Cross-plugin calls

### Manual Implementation (Health Pattern)

See `/substrate/src/activations/health/` for a complete example of:
- Manual `Activation` trait implementation
- Hand-written RPC trait
- Explicit `wrap_stream` usage
- Custom `plugin_schema` generation

---

## Summary

| Approach | Complexity | Use When |
|----------|------------|----------|
| Hub macro | Low | Most plugins - let the macro do the work |
| Manual impl | Medium | Need custom behavior or learning the internals |
| Hub plugin | Medium | Hierarchical structure with children |
| Context injection | High | Need to call other plugins or resolve handles |

**Key files to reference:**
- `hub-macro/src/lib.rs` - Macro definitions
- `hub-core/src/plexus/plexus.rs` - Activation trait
- `hub-core/src/plexus/streaming.rs` - wrap_stream helper
- `substrate/src/activations/echo/` - Minimal example
- `substrate/src/activations/health/` - Manual implementation
- `substrate/src/activations/solar/` - Hub plugin example
- `substrate/src/builder.rs` - Registration patterns

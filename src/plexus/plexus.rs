//! Plexus - the central routing layer for activations
//!
//! Plexus IS an activation that also serves as the registry for other activations.
//! It uses hub-macro with `override_call` for the routing method.

use super::{
    context::PlexusContext,
    method_enum::MethodEnumSchema,
    schema::{MethodSchema, PluginSchema, Schema, ShallowPluginSchema},
    streaming::PlexusStream,
};
use crate::types::Handle;
use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use jsonrpsee::core::server::Methods;
use jsonrpsee::RpcModule;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Clone)]
pub enum PlexusError {
    ActivationNotFound(String),
    MethodNotFound { activation: String, method: String },
    InvalidParams(String),
    ExecutionError(String),
    HandleNotSupported(String),
}

impl std::fmt::Display for PlexusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlexusError::ActivationNotFound(name) => write!(f, "Activation not found: {}", name),
            PlexusError::MethodNotFound { activation, method } => {
                write!(f, "Method not found: {}.{}", activation, method)
            }
            PlexusError::InvalidParams(msg) => write!(f, "Invalid params: {}", msg),
            PlexusError::ExecutionError(msg) => write!(f, "Execution error: {}", msg),
            PlexusError::HandleNotSupported(activation) => {
                write!(f, "Handle resolution not supported by activation: {}", activation)
            }
        }
    }
}

impl std::error::Error for PlexusError {}

// ============================================================================
// Schema Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationInfo {
    pub namespace: String,
    pub version: String,
    pub description: String,
    pub methods: Vec<String>,
}

/// Full schema for an activation (deprecated - use PluginSchema)
#[deprecated(note = "Use PluginSchema instead")]
pub type ActivationFullSchema = PluginSchema;

// ============================================================================
// Activation Trait
// ============================================================================

#[async_trait]
pub trait Activation: Send + Sync + 'static {
    type Methods: MethodEnumSchema;

    fn namespace(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str { "No description available" }
    fn methods(&self) -> Vec<&str>;
    fn method_help(&self, _method: &str) -> Option<String> { None }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;
    async fn resolve_handle(&self, _handle: &Handle) -> Result<PlexusStream, PlexusError> {
        Err(PlexusError::HandleNotSupported(self.namespace().to_string()))
    }

    fn into_rpc_methods(self) -> Methods where Self: Sized;

    /// Return this plugin's schema (methods + optional children)
    fn plugin_schema(&self) -> PluginSchema {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        PluginSchema::leaf(
            self.namespace(),
            self.version(),
            self.description(),
            self.methods().iter().map(|name| {
                let desc = self.method_help(name).unwrap_or_default();
                // Compute a simple hash for methods not using hub-macro
                let mut hasher = DefaultHasher::new();
                name.hash(&mut hasher);
                desc.hash(&mut hasher);
                let hash = format!("{:016x}", hasher.finish());
                MethodSchema::new(name.to_string(), desc, hash)
            }).collect(),
        )
    }
}

// ============================================================================
// Child Routing for Hub Plugins
// ============================================================================

/// Trait for plugins that can route to child plugins
///
/// Hub plugins implement this to support nested method routing.
/// When a method like "mercury.info" is called on a solar plugin,
/// this trait enables routing to the mercury child.
///
/// This trait is separate from Activation to avoid associated type issues
/// with dynamic dispatch.
#[async_trait]
pub trait ChildRouter: Send + Sync {
    /// Get the namespace of this router (for error messages)
    fn router_namespace(&self) -> &str;

    /// Call a method on this router
    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;

    /// Get a child plugin instance by name for nested routing
    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>>;
}

/// Route a method call to a child plugin
///
/// This is called from generated code when a hub plugin receives
/// a method that doesn't match its local methods. If the method
/// contains a dot (e.g., "mercury.info"), it routes to the child.
pub async fn route_to_child<T: ChildRouter + ?Sized>(
    parent: &T,
    method: &str,
    params: Value,
) -> Result<PlexusStream, PlexusError> {
    // Try to split on first dot for nested routing
    if let Some((child_name, rest)) = method.split_once('.') {
        if let Some(child) = parent.get_child(child_name).await {
            return child.router_call(rest, params).await;
        }
        return Err(PlexusError::ActivationNotFound(child_name.to_string()));
    }

    // No dot - method simply not found
    Err(PlexusError::MethodNotFound {
        activation: parent.router_namespace().to_string(),
        method: method.to_string(),
    })
}

/// Wrapper to implement ChildRouter for Arc<dyn ChildRouter>
///
/// This allows Plexus to return its stored Arc<dyn ChildRouter> from get_child()
struct ArcChildRouter(Arc<dyn ChildRouter>);

#[async_trait]
impl ChildRouter for ArcChildRouter {
    fn router_namespace(&self) -> &str {
        self.0.router_namespace()
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        self.0.router_call(method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        self.0.get_child(name).await
    }
}

// ============================================================================
// Internal Type-Erased Activation
// ============================================================================

#[async_trait]
trait ActivationObject: Send + Sync + 'static {
    fn namespace(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;
    fn methods(&self) -> Vec<&str>;
    fn method_help(&self, method: &str) -> Option<String>;
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;
    async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError>;
    fn plugin_schema(&self) -> PluginSchema;
    fn schema(&self) -> Schema;
}

struct ActivationWrapper<A: Activation> {
    inner: A,
}

#[async_trait]
impl<A: Activation> ActivationObject for ActivationWrapper<A> {
    fn namespace(&self) -> &str { self.inner.namespace() }
    fn version(&self) -> &str { self.inner.version() }
    fn description(&self) -> &str { self.inner.description() }
    fn methods(&self) -> Vec<&str> { self.inner.methods() }
    fn method_help(&self, method: &str) -> Option<String> { self.inner.method_help(method) }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        self.inner.call(method, params).await
    }

    async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
        self.inner.resolve_handle(handle).await
    }

    fn plugin_schema(&self) -> PluginSchema { self.inner.plugin_schema() }

    fn schema(&self) -> Schema {
        let schema = schemars::schema_for!(A::Methods);
        serde_json::from_value(serde_json::to_value(schema).expect("serialize"))
            .expect("parse schema")
    }
}

// ============================================================================
// Plexus Event Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HashEvent {
    Hash { value: String },
}

/// Event for schema() RPC method - returns shallow plugin schema
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SchemaEvent {
    /// This plugin's schema (children listed by namespace only)
    Schema(ShallowPluginSchema),
}

// ============================================================================
// Plexus
// ============================================================================

struct PlexusInner {
    activations: HashMap<String, Arc<dyn ActivationObject>>,
    /// Child routers for direct nested routing (e.g., plexus.solar.mercury.info)
    child_routers: HashMap<String, Arc<dyn ChildRouter>>,
    pending_rpc: std::sync::Mutex<Vec<Box<dyn FnOnce() -> Methods + Send>>>,
}

/// Plexus - the central hub that IS an activation and routes to other activations
#[derive(Clone)]
pub struct Plexus {
    inner: Arc<PlexusInner>,
}

impl Default for Plexus {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Plexus Infrastructure (non-RPC methods)
// ============================================================================

impl Plexus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PlexusInner {
                activations: HashMap::new(),
                child_routers: HashMap::new(),
                pending_rpc: std::sync::Mutex::new(Vec::new()),
            }),
        }
    }

    /// Register an activation
    pub fn register<A: Activation + Clone>(mut self, activation: A) -> Self {
        let namespace = activation.namespace().to_string();
        let activation_for_rpc = activation.clone();

        let inner = Arc::get_mut(&mut self.inner)
            .expect("Cannot register: Plexus has multiple references");

        inner.activations.insert(namespace, Arc::new(ActivationWrapper { inner: activation }));
        inner.pending_rpc.lock().unwrap()
            .push(Box::new(move || activation_for_rpc.into_rpc_methods()));
        self
    }

    /// Register a hub activation that supports nested routing
    ///
    /// Hub activations implement `ChildRouter`, enabling direct nested method calls
    /// like `plexus.solar.mercury.info` at the RPC layer (no plexus.call indirection).
    pub fn register_hub<A: Activation + ChildRouter + Clone + 'static>(mut self, activation: A) -> Self {
        let namespace = activation.namespace().to_string();
        let activation_for_rpc = activation.clone();
        let activation_for_router = activation.clone();

        let inner = Arc::get_mut(&mut self.inner)
            .expect("Cannot register: Plexus has multiple references");

        inner.activations.insert(namespace.clone(), Arc::new(ActivationWrapper { inner: activation }));
        inner.child_routers.insert(namespace, Arc::new(activation_for_router));
        inner.pending_rpc.lock().unwrap()
            .push(Box::new(move || activation_for_rpc.into_rpc_methods()));
        self
    }

    /// List all methods across all activations
    pub fn list_methods(&self) -> Vec<String> {
        let mut methods = Vec::new();

        // Include plexus's own methods
        for m in Activation::methods(self) {
            methods.push(format!("plexus.{}", m));
        }

        // Include registered activation methods
        for (ns, act) in &self.inner.activations {
            for m in act.methods() {
                methods.push(format!("{}.{}", ns, m));
            }
        }
        methods.sort();
        methods
    }

    /// List all activations (including plexus itself)
    pub fn list_activations_info(&self) -> Vec<ActivationInfo> {
        let mut activations = Vec::new();

        // Include plexus itself
        activations.push(ActivationInfo {
            namespace: Activation::namespace(self).to_string(),
            version: Activation::version(self).to_string(),
            description: Activation::description(self).to_string(),
            methods: Activation::methods(self).iter().map(|s| s.to_string()).collect(),
        });

        // Include registered activations
        for a in self.inner.activations.values() {
            activations.push(ActivationInfo {
                namespace: a.namespace().to_string(),
                version: a.version().to_string(),
                description: a.description().to_string(),
                methods: a.methods().iter().map(|s| s.to_string()).collect(),
            });
        }

        activations
    }

    /// Compute hash for cache invalidation
    ///
    /// Returns the hash from the recursive plugin schema. This hash changes
    /// whenever any method definition or child plugin changes.
    pub fn compute_hash(&self) -> String {
        Activation::plugin_schema(self).hash
    }

    /// Route a call to the appropriate activation
    pub async fn route(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        let (namespace, method_name) = self.parse_method(method)?;

        // Handle plexus's own methods
        if namespace == "plexus" {
            return Activation::call(self, method_name, params).await;
        }

        let activation = self.inner.activations.get(namespace)
            .ok_or_else(|| PlexusError::ActivationNotFound(namespace.to_string()))?;

        activation.call(method_name, params).await
    }

    /// Resolve a handle
    pub async fn do_resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
        let activation = self.inner.activations.get(&handle.plugin)
            .ok_or_else(|| PlexusError::ActivationNotFound(handle.plugin.clone()))?;
        activation.resolve_handle(handle).await
    }

    /// Get activation schema
    pub fn get_activation_schema(&self, namespace: &str) -> Option<Schema> {
        self.inner.activations.get(namespace).map(|a| a.schema())
    }

    /// Get plugin schemas for all activations (including plexus itself)
    pub fn list_plugin_schemas(&self) -> Vec<PluginSchema> {
        let mut schemas = Vec::new();

        // Include plexus itself
        schemas.push(Activation::plugin_schema(self));

        // Include registered activations
        for a in self.inner.activations.values() {
            schemas.push(a.plugin_schema());
        }

        schemas
    }

    /// Deprecated: use list_plugin_schemas instead
    #[deprecated(note = "Use list_plugin_schemas instead")]
    pub fn list_full_schemas(&self) -> Vec<PluginSchema> {
        self.list_plugin_schemas()
    }

    /// Get help for a method
    pub fn get_method_help(&self, method: &str) -> Option<String> {
        let (namespace, method_name) = self.parse_method(method).ok()?;
        let activation = self.inner.activations.get(namespace)?;
        activation.method_help(method_name)
    }

    fn parse_method<'a>(&self, method: &'a str) -> Result<(&'a str, &'a str), PlexusError> {
        let parts: Vec<&str> = method.splitn(2, '.').collect();
        if parts.len() != 2 {
            return Err(PlexusError::InvalidParams(format!("Invalid method format: {}", method)));
        }
        Ok((parts[0], parts[1]))
    }

    /// Get child plugin schemas (for hub functionality)
    /// Called by hub-macro when `hub` flag is set
    pub fn plugin_children(&self) -> Vec<PluginSchema> {
        self.inner.activations.values()
            .map(|a| a.plugin_schema())
            .collect()
    }

    /// Convert to RPC module
    pub fn into_rpc_module(self) -> Result<RpcModule<()>, jsonrpsee::core::RegisterMethodError> {
        let mut module = RpcModule::new(());

        PlexusContext::init(self.compute_hash());

        // Add plexus's own RPC methods
        let plexus_methods: Methods = self.clone().into_rpc().into();
        module.merge(plexus_methods)?;

        // Add all registered activation RPC methods
        let pending = std::mem::take(&mut *self.inner.pending_rpc.lock().unwrap());
        for factory in pending {
            module.merge(factory())?;
        }

        Ok(module)
    }
}

// ============================================================================
// Plexus RPC Methods (via hub-macro)
// ============================================================================

#[hub_macro::hub_methods(
    namespace = "plexus",
    version = "1.0.0",
    description = "Central routing and introspection",
    hub
)]
impl Plexus {
    /// Route a call to a registered activation
    #[hub_macro::hub_method(
        override_call,
        description = "Route a call to a registered activation",
        params(
            method = "The method to call (format: namespace.method)",
            params = "Parameters to pass to the method (optional, defaults to {})"
        )
    )]
    async fn call(
        &self,
        method: String,
        params: Option<Value>,
    ) -> Result<PlexusStream, PlexusError> {
        self.route(&method, params.unwrap_or_default()).await
    }

    /// Get plexus configuration hash (from the recursive schema)
    ///
    /// This hash changes whenever any method or child plugin changes.
    /// It's computed from the method hashes rolled up through the schema tree.
    #[hub_macro::hub_method(description = "Get plexus configuration hash")]
    async fn hash(&self) -> impl Stream<Item = HashEvent> + Send + 'static {
        // Get hash from the schema itself (recursive hash of all methods + children)
        let schema = Activation::plugin_schema(self);
        stream! { yield HashEvent::Hash { value: schema.hash }; }
    }

    /// Get plexus schema (shallow - children listed by namespace only)
    #[hub_macro::hub_method(description = "Get plexus schema (children as namespaces only)")]
    async fn schema(&self) -> impl Stream<Item = SchemaEvent> + Send + 'static {
        let schema = Activation::plugin_schema(self).shallow();
        stream! {
            yield SchemaEvent::Schema(schema);
        }
    }
}

/// ChildRouter implementation for Plexus
///
/// This enables nested routing through registered activations.
/// e.g., plexus.call("solar.mercury.info") routes to solar → mercury → info
#[async_trait]
impl ChildRouter for Plexus {
    fn router_namespace(&self) -> &str {
        "plexus"
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        // Plexus routes via its registered activations
        // Method format: "activation.method" or "activation.child.method"
        self.route(method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        // Look up registered hub activations that implement ChildRouter
        self.inner.child_routers.get(name)
            .map(|router| {
                // Clone the Arc and wrap in Box for the trait object
                Box::new(ArcChildRouter(router.clone())) as Box<dyn ChildRouter>
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plexus_implements_activation() {
        fn assert_activation<T: Activation>() {}
        assert_activation::<Plexus>();
    }

    #[test]
    fn plexus_methods() {
        let plexus = Plexus::new();
        let methods = plexus.methods();
        assert!(methods.contains(&"call"));
        assert!(methods.contains(&"hash"));
        assert!(methods.contains(&"schema"));
        // list_activations was removed - use schema() instead
    }

    #[test]
    fn plexus_hash_stable() {
        let p1 = Plexus::new();
        let p2 = Plexus::new();
        assert_eq!(p1.compute_hash(), p2.compute_hash());
    }

    #[test]
    fn plexus_is_hub() {
        use crate::activations::health::Health;
        let plexus = Plexus::new().register(Health::new());
        let schema = plexus.plugin_schema();

        // Plexus should be a hub (has children)
        assert!(schema.is_hub(), "plexus should be a hub");
        assert!(!schema.is_leaf(), "plexus should not be a leaf");

        // Should have children
        let children = schema.children.expect("plexus should have children");
        assert!(!children.is_empty(), "plexus should have at least one child");

        // Health should be a leaf
        let health = children.iter().find(|c| c.namespace == "health").expect("should have health child");
        assert!(health.is_leaf(), "health should be a leaf plugin");
    }

    #[test]
    fn plexus_recursive_schema() {
        use crate::activations::health::Health;
        let plexus = Plexus::new().register(Health::new());
        let schema = plexus.plugin_schema();

        // Pretty print the recursive schema
        let json = serde_json::to_string_pretty(&schema).unwrap();
        println!("Plexus recursive schema:\n{}", json);

        // Verify structure
        assert_eq!(schema.namespace, "plexus");
        assert!(schema.methods.iter().any(|m| m.name == "call"));
        assert!(schema.children.is_some());
    }

    /// Test direct nested routing via plexus.call("solar.mercury.info")
    ///
    /// This tests the full path: Plexus → Solar → Mercury without using
    /// the plexus.call RPC wrapper - just the Activation::call trait method.
    #[tokio::test]
    async fn plexus_direct_nested_routing() {
        use crate::activations::solar::Solar;

        // Register Solar as a hub (enables ChildRouter lookup)
        let plexus = Plexus::new().register_hub(Solar::new());

        // Call directly via Activation trait - this should route:
        // plexus.call("solar.mercury.info") →
        //   doesn't match local methods →
        //   route_to_child("solar.mercury.info") →
        //   get_child("solar") returns Solar →
        //   solar.router_call("mercury.info") →
        //   solar.call("mercury.info") →
        //   route_to_child("mercury.info") →
        //   get_child("mercury") returns CelestialBodyActivation →
        //   mercury.router_call("info") →
        //   returns Mercury info
        let result = Activation::call(&plexus, "solar.mercury.info", serde_json::json!({})).await;
        assert!(result.is_ok(), "plexus.solar.mercury.info should work: {:?}", result.err());
    }

    /// Test 3-level nested routing: plexus → solar → jupiter → io
    #[tokio::test]
    async fn plexus_deep_nested_routing() {
        use crate::activations::solar::Solar;

        let plexus = Plexus::new().register_hub(Solar::new());

        // Call plexus.solar.jupiter.io.info
        let result = Activation::call(&plexus, "solar.jupiter.io.info", serde_json::json!({})).await;
        assert!(result.is_ok(), "plexus.solar.jupiter.io.info should work: {:?}", result.err());
    }
}

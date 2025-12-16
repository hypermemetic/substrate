use super::{context::PlexusContext, path::Provenance, schema::Schema, types::PlexusStreamItem};
use crate::plugin_system::types::ActivationStreamItem;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use jsonrpsee::{core::server::Methods, RpcModule, SubscriptionMessage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, pin::Pin, sync::Arc};

/// Error type for plexus operations
#[derive(Debug, Clone)]
pub enum PlexusError {
    ActivationNotFound(String),
    MethodNotFound { activation: String, method: String },
    InvalidParams(String),
    ExecutionError(String),
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
        }
    }
}

impl std::error::Error for PlexusError {}

/// Type alias for plexus streams
pub type PlexusStream = Pin<Box<dyn Stream<Item = PlexusStreamItem> + Send + 'static>>;

/// Information about an activation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationInfo {
    pub namespace: String,
    pub version: String,
    pub description: String,
    pub methods: Vec<String>,
}

/// Plexus schema response - all activations and their methods
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexusSchema {
    pub activations: Vec<ActivationInfo>,
    pub total_methods: usize,
}

/// Activation trait - implement this to create an activation
///
/// This is the main trait that activation implementations provide. It includes
/// an associated `Methods` type which enables automatic schema generation.
///
/// # Example
/// ```ignore
/// #[derive(Clone)]
/// pub struct MyActivation;
///
/// #[async_trait]
/// impl Activation for MyActivation {
///     type Methods = MyMethod;  // Enum with #[derive(JsonSchema, Serialize)]
///
///     fn namespace(&self) -> &str { "my_activation" }
///     fn version(&self) -> &str { "1.0.0" }
///     async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
///         // ... implementation
///     }
///     fn into_rpc_methods(self) -> Methods {
///         self.into_rpc().into()
///     }
/// }
/// ```
#[async_trait]
pub trait Activation: Send + Sync + Clone + 'static {
    /// The Method enum type defining all methods this activation supports
    ///
    /// This type must implement JsonSchema and Serialize. The schema will be
    /// automatically generated from this type.
    type Methods: schemars::JsonSchema + serde::Serialize;

    /// Activation namespace (e.g., "health", "bash", "arbor")
    fn namespace(&self) -> &str;

    /// Activation version (semantic versioning: "MAJOR.MINOR.PATCH")
    fn version(&self) -> &str;

    /// Activation description (one-line summary)
    fn description(&self) -> &str {
        "No description available"
    }

    /// List available methods
    fn methods(&self) -> Vec<&str>;

    /// Get help text for a specific method
    fn method_help(&self, _method: &str) -> Option<String> {
        None
    }

    /// Call a method by name with JSON params, returns a stream
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;

    /// Convert this activation into RPC methods for JSON-RPC server
    fn into_rpc_methods(self) -> Methods;
}

/// Internal wrapper that type-erases Activation into a trait object
///
/// This wrapper converts `Activation` (which has an associated type) into
/// `ActivationObject` (which is trait-object-safe). Users don't interact with
/// this directly - `Plexus::register()` handles the wrapping automatically.
struct ActivationWrapper<A: Activation> {
    inner: A,
}

impl<A: Activation> ActivationWrapper<A> {
    fn new(activation: A) -> Self {
        Self { inner: activation }
    }
}

/// Implement trait-object-safe ActivationObject for the wrapper
#[async_trait]
impl<A: Activation> ActivationObject for ActivationWrapper<A> {
    fn namespace(&self) -> &str {
        self.inner.namespace()
    }

    fn version(&self) -> &str {
        self.inner.version()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn methods(&self) -> Vec<&str> {
        self.inner.methods()
    }

    fn method_help(&self, method: &str) -> Option<String> {
        self.inner.method_help(method)
    }

    fn schema(&self) -> Schema {
        // Automatically generate schema from A::Methods
        let schema = schemars::schema_for!(A::Methods);
        serde_json::from_value(serde_json::to_value(schema).expect("Failed to serialize schema"))
            .expect("Failed to parse schema - Methods type incorrectly defined")
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        self.inner.call(method, params).await
    }

    fn into_rpc_methods(self) -> Methods {
        self.inner.into_rpc_methods()
    }
}

/// Internal trait-object-safe activation interface (no associated types)
///
/// This trait is implemented automatically by `ActivationWrapper` to enable
/// storing activations as trait objects. Users should implement `Activation` instead.
#[async_trait]
trait ActivationObject: Send + Sync + 'static {
    fn namespace(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;
    fn methods(&self) -> Vec<&str>;
    fn method_help(&self, method: &str) -> Option<String>;
    fn schema(&self) -> Schema;
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;
    fn into_rpc_methods(self) -> Methods;
}

/// The Plexus - routes calls to registered activations
pub struct Plexus {
    activations: HashMap<String, Arc<dyn ActivationObject>>,
    /// Pending activations that haven't been converted to RPC yet
    pending_rpc: Vec<Box<dyn FnOnce() -> Methods + Send>>,
}

impl Plexus {
    pub fn new() -> Self {
        Self {
            activations: HashMap::new(),
            pending_rpc: Vec::new(),
        }
    }

    /// Register an activation with the plexus
    ///
    /// The activation is automatically wrapped to enable storage as a trait object
    /// while preserving type-driven schema generation.
    pub fn register<A: Activation>(mut self, activation: A) -> Self {
        let namespace = activation.namespace().to_string();
        let activation_for_rpc = activation.clone();

        // Wrap the activation to make it trait-object safe
        let wrapped = ActivationWrapper::new(activation);
        self.activations.insert(namespace, Arc::new(wrapped));
        self.pending_rpc
            .push(Box::new(move || activation_for_rpc.into_rpc_methods()));
        self
    }

    /// List all registered activations and their methods
    pub fn list_methods(&self) -> Vec<String> {
        let mut methods = Vec::new();
        for (namespace, activation) in &self.activations {
            for method in activation.methods() {
                methods.push(format!("{}.{}", namespace, method));
            }
        }
        methods.sort();
        methods
    }

    /// List plexus-level methods (not activation methods)
    pub fn list_plexus_methods(&self) -> Vec<&'static str> {
        vec!["plexus_schema", "plexus_activation_schema", "plexus_hash"]
    }

    /// Compute a hash of all activations and their methods for cache invalidation
    ///
    /// The hash is computed from a deterministic string of all activation
    /// namespaces, versions, and method names sorted alphabetically.
    pub fn compute_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Build deterministic string: "namespace:version:method1,method2,..."
        let mut activation_strings: Vec<String> = self
            .activations
            .values()
            .map(|a| {
                let mut methods: Vec<&str> = a.methods();
                methods.sort();
                format!("{}:{}:{}", a.namespace(), a.version(), methods.join(","))
            })
            .collect();
        activation_strings.sort();

        let combined = activation_strings.join(";");

        let mut hasher = DefaultHasher::new();
        combined.hash(&mut hasher);
        let hash = hasher.finish();

        // Return as hex string
        format!("{:016x}", hash)
    }

    /// Get information about all registered activations
    pub fn list_activations(&self) -> Vec<ActivationInfo> {
        let mut activations: Vec<ActivationInfo> = self
            .activations
            .values()
            .map(|a| ActivationInfo {
                namespace: a.namespace().to_string(),
                version: a.version().to_string(),
                description: a.description().to_string(),
                methods: a.methods().iter().map(|s| s.to_string()).collect(),
            })
            .collect();
        activations.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        activations
    }

    /// Get help for a specific method
    pub fn get_method_help(&self, method: &str) -> Option<String> {
        let (namespace, method_name) = self.parse_method(method).ok()?;
        let activation = self.activations.get(namespace)?;
        activation.method_help(method_name)
    }

    /// Get the schema for an activation
    pub fn get_activation_schema(&self, namespace: &str) -> Option<Schema> {
        let activation = self.activations.get(namespace)?;
        Some(activation.schema())
    }

    /// Call a method on an activation
    ///
    /// Method format: "namespace.method" (e.g., "bash.execute", "health.check")
    pub async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        let (namespace, method_name) = self.parse_method(method)?;

        let activation = self
            .activations
            .get(namespace)
            .ok_or_else(|| PlexusError::ActivationNotFound(namespace.to_string()))?;

        activation.call(method_name, params).await
    }

    /// Convert the plexus into an RPC module for JSON-RPC server
    ///
    /// Consumes the plexus. Clone first if you need to keep programmatic access.
    pub fn into_rpc_module(self) -> Result<RpcModule<()>, jsonrpsee::core::RegisterMethodError> {
        let mut module = RpcModule::new(());

        // Add plexus-level methods
        let plexus_schema = PlexusSchema {
            activations: self.list_activations(),
            total_methods: self.list_methods().len(),
        };

        // Compute hash for cache invalidation and initialize global context
        let plexus_hash = self.compute_hash();
        PlexusContext::init(plexus_hash.clone());

        // plexus_hash subscription - returns hash for cache invalidation
        let hash_for_hash_sub = plexus_hash.clone();
        module.register_subscription(
            "plexus_hash",
            "plexus_hash",
            "plexus_unsubscribe_hash",
            move |_params, pending, _ctx| {
                let hash = hash_for_hash_sub.clone();
                async move {
                    let sink = pending.accept().await?;
                    let response = PlexusStreamItem::data(
                        hash.clone(),
                        Provenance::root("plexus"),
                        "plexus.hash".to_string(),
                        serde_json::json!({ "hash": hash }),
                    );
                    if let Ok(msg) = SubscriptionMessage::from_json(&response) {
                        let _ = sink.send(msg).await;
                    }
                    let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                    if let Ok(msg) = SubscriptionMessage::from_json(&done) {
                        let _ = sink.send(msg).await;
                    }
                    Ok(())
                }
            },
        )?;

        // plexus_schema subscription - returns all activations and methods
        let hash_for_schema = plexus_hash.clone();
        module.register_subscription(
            "plexus_schema",
            "plexus_schema",
            "plexus_unsubscribe_schema",
            move |_params, pending, _ctx| {
                let schema = plexus_schema.clone();
                let hash = hash_for_schema.clone();
                async move {
                    let sink = pending.accept().await?;
                    let response = PlexusStreamItem::data(
                        hash.clone(),
                        Provenance::root("plexus"),
                        "plexus.schema".to_string(),
                        serde_json::to_value(&schema).unwrap(),
                    );
                    if let Ok(msg) = SubscriptionMessage::from_json(&response) {
                        let _ = sink.send(msg).await;
                    }
                    let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                    if let Ok(msg) = SubscriptionMessage::from_json(&done) {
                        let _ = sink.send(msg).await;
                    }
                    Ok(())
                }
            },
        )?;

        // plexus_activation_schema subscription - returns enriched schema for a specific activation
        // Optional second parameter specifies a method to get schema for just that method
        // Clone activations for the closure
        let activations_for_schema: HashMap<String, Arc<dyn ActivationObject>> = self
            .activations
            .iter()
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect();
        let hash_for_activation_schema = plexus_hash.clone();

        module.register_subscription(
            "plexus_activation_schema",
            "plexus_activation_schema",
            "plexus_unsubscribe_activation_schema",
            move |params, pending, _ctx| {
                let activations = activations_for_schema.clone();
                let hash = hash_for_activation_schema.clone();
                async move {
                    // Parse parameters: namespace (required), method (optional)
                    let mut seq = params.sequence();
                    let namespace: String = seq.next()?;
                    let method: Option<String> = seq.optional_next()?;
                    let sink = pending.accept().await?;

                    if let Some(activation) = activations.get(&namespace) {
                        let full_schema = activation.schema();

                        // If method specified, extract just that method's schema
                        let (schema_value, content_type) = if let Some(method_name) = method {
                            if let Some(method_schema) = full_schema.get_method_schema(&method_name) {
                                (
                                    serde_json::to_value(&method_schema).unwrap(),
                                    "plexus.method_schema".to_string(),
                                )
                            } else {
                                // Method not found in schema
                                let available = full_schema.list_methods();
                                let error = PlexusStreamItem::error(
                                    hash.clone(),
                                    Provenance::root("plexus"),
                                    format!(
                                        "Method '{}' not found in activation '{}'. Available: {}",
                                        method_name,
                                        namespace,
                                        available.join(", ")
                                    ),
                                    false,
                                );
                                if let Ok(msg) = SubscriptionMessage::from_json(&error) {
                                    let _ = sink.send(msg).await;
                                }
                                let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                                if let Ok(msg) = SubscriptionMessage::from_json(&done) {
                                    let _ = sink.send(msg).await;
                                }
                                return Ok(());
                            }
                        } else {
                            (
                                serde_json::to_value(&full_schema).unwrap(),
                                "plexus.activation_schema".to_string(),
                            )
                        };

                        let response = PlexusStreamItem::data(
                            hash.clone(),
                            Provenance::root("plexus"),
                            content_type,
                            schema_value,
                        );
                        if let Ok(msg) = SubscriptionMessage::from_json(&response) {
                            let _ = sink.send(msg).await;
                        }
                    } else {
                        let error = PlexusStreamItem::error(
                            hash.clone(),
                            Provenance::root("plexus"),
                            format!("Activation not found: {}", namespace),
                            false,
                        );
                        if let Ok(msg) = SubscriptionMessage::from_json(&error) {
                            let _ = sink.send(msg).await;
                        }
                    }

                    let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                    if let Ok(msg) = SubscriptionMessage::from_json(&done) {
                        let _ = sink.send(msg).await;
                    }
                    Ok(())
                }
            },
        )?;

        // Merge activation methods
        for factory in self.pending_rpc {
            module.merge(factory())?;
        }
        Ok(module)
    }

    /// Parse "namespace.method" into parts
    fn parse_method<'a>(&self, method: &'a str) -> Result<(&'a str, &'a str), PlexusError> {
        let parts: Vec<&str> = method.splitn(2, '.').collect();
        if parts.len() != 2 {
            return Err(PlexusError::MethodNotFound {
                activation: method.to_string(),
                method: String::new(),
            });
        }
        Ok((parts[0], parts[1]))
    }
}

impl Default for Plexus {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to convert a typed stream to PlexusStream
pub fn into_plexus_stream<S, T>(stream: S, provenance: Provenance) -> PlexusStream
where
    S: Stream<Item = T> + Send + 'static,
    T: ActivationStreamItem,
{
    let plexus_hash = PlexusContext::hash();
    Box::pin(stream.map(move |item| item.into_plexus_item(provenance.clone(), &plexus_hash)))
}

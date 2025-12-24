use super::{
    context::PlexusContext,
    guidance::{error_stream_with_guidance, ActivationGuidanceInfo, CustomGuidance},
    path::Provenance,
    schema::Schema,
    types::{GuidanceSuggestion, PlexusStreamItem},
};
use crate::activations::arbor::Handle;
use crate::plugin_system::types::ActivationStreamItem;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use jsonrpsee::{core::server::Methods, RpcModule};
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

/// Schema for a single method including params and return type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodSchemaInfo {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<schemars::Schema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returns: Option<schemars::Schema>,
}

/// Full schema for an activation including all methods with their input/output types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationFullSchema {
    pub namespace: String,
    pub version: String,
    pub description: String,
    pub methods: Vec<MethodSchemaInfo>,
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
use super::method_enum::MethodEnumSchema;

#[async_trait]
pub trait Activation: Send + Sync + Clone + 'static {
    /// The Method enum type defining all methods this activation supports
    ///
    /// This type must implement JsonSchema, Serialize, and MethodEnumSchema.
    /// The schema will be automatically generated with proper const discriminators.
    type Methods: MethodEnumSchema;

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

    /// Provide custom guidance for an error (optional override)
    ///
    /// Activations can override this to provide method-specific suggestions,
    /// example parameters, or custom recovery steps.
    ///
    /// # Example
    /// ```ignore
    /// fn custom_guidance(&self, method: &str, error: &PlexusError) -> Option<GuidanceSuggestion> {
    ///     match (method, error) {
    ///         ("execute", PlexusError::InvalidParams(_)) => {
    ///             Some(GuidanceSuggestion::TryMethod {
    ///                 method: "bash.execute".to_string(),
    ///                 example_params: Some(json!("echo 'Hello!'")),
    ///             })
    ///         }
    ///         _ => None,
    ///     }
    /// }
    /// ```
    fn custom_guidance(&self, _method: &str, _error: &PlexusError) -> Option<GuidanceSuggestion> {
        None // Default: no custom guidance
    }

    /// Call a method by name with JSON params, returns a stream
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;

    /// Resolve a handle created by this activation
    ///
    /// Default implementation returns HandleNotSupported error.
    /// Activations that create handles should override this to resolve them.
    ///
    /// The `hub` parameter provides access to the Plexus for resolving foreign handles.
    async fn resolve_handle(&self, _handle: &Handle, _hub: &Plexus) -> Result<PlexusStream, PlexusError> {
        Err(PlexusError::HandleNotSupported(self.namespace().to_string()))
    }

    /// Convert this activation into RPC methods for JSON-RPC server
    fn into_rpc_methods(self) -> Methods;

    /// Get full schema including method params and return types
    ///
    /// Default implementation returns basic info without param/return schemas.
    /// Macro-generated activations override this with full type information.
    fn full_schema(&self) -> ActivationFullSchema {
        ActivationFullSchema {
            namespace: self.namespace().to_string(),
            version: self.version().to_string(),
            description: self.description().to_string(),
            methods: self.methods().iter().map(|name| {
                MethodSchemaInfo {
                    name: name.to_string(),
                    description: self.method_help(name).unwrap_or_default(),
                    params: None,
                    returns: None,
                }
            }).collect(),
        }
    }
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

    fn method_help(&self, method: &str) -> Option<String> {
        self.inner.method_help(method)
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        self.inner.call(method, params).await
    }

    async fn resolve_handle(&self, handle: &Handle, hub: &Plexus) -> Result<PlexusStream, PlexusError> {
        self.inner.resolve_handle(handle, hub).await
    }

    fn into_rpc_methods(self) -> Methods {
        self.inner.into_rpc_methods()
    }

    fn full_schema(&self) -> ActivationFullSchema {
        self.inner.full_schema()
    }
}

/// Implement CustomGuidance by delegating to inner Activation
impl<A: Activation> CustomGuidance for ActivationWrapper<A> {
    fn custom_guidance(&self, method: &str, error: &PlexusError) -> Option<GuidanceSuggestion> {
        self.inner.custom_guidance(method, error)
    }
}

/// Implement ActivationGuidanceInfo by delegating to inner Activation
impl<A: Activation> ActivationGuidanceInfo for ActivationWrapper<A> {
    fn methods(&self) -> Vec<&str> {
        self.inner.methods()
    }

    fn schema(&self) -> Schema {
        // Automatically generate schema from A::Methods
        let schema = schemars::schema_for!(A::Methods);
        serde_json::from_value(serde_json::to_value(schema).expect("Failed to serialize schema"))
            .expect("Failed to parse schema - Methods type incorrectly defined")
    }
}

/// Internal trait-object-safe activation interface (no associated types)
///
/// This trait is implemented automatically by `ActivationWrapper` to enable
/// storing activations as trait objects. Users should implement `Activation` instead.
#[async_trait]
trait ActivationObject: Send + Sync + ActivationGuidanceInfo + 'static {
    fn namespace(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;
    fn method_help(&self, method: &str) -> Option<String>;
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;
    async fn resolve_handle(&self, handle: &Handle, hub: &Plexus) -> Result<PlexusStream, PlexusError>;
    fn into_rpc_methods(self) -> Methods;
    fn full_schema(&self) -> ActivationFullSchema;
}

/// The Plexus - routes calls to registered activations
pub struct Plexus {
    activations: HashMap<String, Arc<dyn ActivationObject>>,
    /// Pending activations that haven't been converted to RPC yet
    /// Wrapped in Mutex to make Plexus Sync (required for MCP HTTP transport)
    pending_rpc: std::sync::Mutex<Vec<Box<dyn FnOnce() -> Methods + Send>>>,
}

impl Plexus {
    pub fn new() -> Self {
        Self {
            activations: HashMap::new(),
            pending_rpc: std::sync::Mutex::new(Vec::new()),
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
            .lock()
            .unwrap()
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
        vec!["plexus_schema", "plexus_activation_schema", "plexus_full_schema", "plexus_hash"]
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

    /// Get full schemas for all registered activations including method param/return types
    pub fn list_full_schemas(&self) -> Vec<ActivationFullSchema> {
        let mut schemas: Vec<ActivationFullSchema> = self
            .activations
            .values()
            .map(|a| a.full_schema())
            .collect();
        schemas.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        schemas
    }

    /// Call a method on an activation
    ///
    /// Method format: "namespace.method" (e.g., "bash.execute", "health.check")
    ///
    /// Note: This method always returns Ok(PlexusStream). Errors are returned as
    /// stream events (Guidance → Error → Done) rather than Err results.
    pub async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        let plexus_hash = self.compute_hash();
        let provenance = Provenance::root("plexus");

        // Parse method (format: "namespace.method")
        let (namespace, method_name) = match self.parse_method(method) {
            Ok(parts) => parts,
            Err(error) => {
                // Method parse error - return guidance stream
                return Ok(error_stream_with_guidance::<dyn ActivationGuidanceInfo>(
                    plexus_hash,
                    provenance,
                    error,
                    None,
                    None,
                ));
            }
        };

        // Find activation
        let activation = match self.activations.get(namespace) {
            Some(a) => a,
            None => {
                // Activation not found - return guidance stream
                let error = PlexusError::ActivationNotFound(namespace.to_string());
                return Ok(error_stream_with_guidance::<dyn ActivationGuidanceInfo>(
                    plexus_hash,
                    provenance,
                    error,
                    None,
                    None,
                ));
            }
        };

        // Update provenance with activation namespace
        let activation_provenance = provenance.extend(namespace.to_string());

        // Call activation method
        match activation.call(method_name, params).await {
            Ok(stream) => Ok(stream),
            Err(error) => {
                // Method call error - return guidance stream with activation info
                // ActivationObject implements ActivationGuidanceInfo, so we can cast it
                let activation_clone = Arc::clone(activation);
                Ok(error_stream_with_guidance(
                    plexus_hash,
                    activation_provenance,
                    error,
                    Some(&activation_clone),
                    Some(method_name),
                ))
            }
        }
    }

    /// Resolve a handle by dispatching to the appropriate activation
    ///
    /// Looks up the activation by `handle.source` and delegates to its `resolve_handle` method.
    /// This allows activations to resolve their own handles while having access to the hub
    /// for resolving foreign handles.
    pub async fn resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
        // Find activation by handle source
        let activation = match self.activations.get(&handle.source) {
            Some(a) => a,
            None => {
                return Err(PlexusError::ActivationNotFound(handle.source.clone()));
            }
        };

        // Delegate to activation's resolve_handle, passing self as hub
        activation.resolve_handle(handle, self).await
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
            move |_params, pending, _ctx, _ext| {
                let hash = hash_for_hash_sub.clone();
                async move {
                    let sink = pending.accept().await?;
                    let response = PlexusStreamItem::data(
                        hash.clone(),
                        Provenance::root("plexus"),
                        "plexus.hash".to_string(),
                        serde_json::json!({ "hash": hash }),
                    );
                    if let Ok(raw_value) = serde_json::value::to_raw_value(&response) {
                        let _ = sink.send(raw_value).await;
                    }
                    let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                    if let Ok(raw_value) = serde_json::value::to_raw_value(&done) {
                        let _ = sink.send(raw_value).await;
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
            move |_params, pending, _ctx, _ext| {
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
                    if let Ok(raw_value) = serde_json::value::to_raw_value(&response) {
                        let _ = sink.send(raw_value).await;
                    }
                    let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                    if let Ok(raw_value) = serde_json::value::to_raw_value(&done) {
                        let _ = sink.send(raw_value).await;
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
            move |params, pending, _ctx, _ext| {
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
                                if let Ok(raw_value) = serde_json::value::to_raw_value(&error) {
                                    let _ = sink.send(raw_value).await;
                                }
                                let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                                if let Ok(raw_value) = serde_json::value::to_raw_value(&done) {
                        let _ = sink.send(raw_value).await;
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
                        if let Ok(raw_value) = serde_json::value::to_raw_value(&response) {
                        let _ = sink.send(raw_value).await;
                    }
                    } else {
                        let error = PlexusStreamItem::error(
                            hash.clone(),
                            Provenance::root("plexus"),
                            format!("Activation not found: {}", namespace),
                            false,
                        );
                        if let Ok(raw_value) = serde_json::value::to_raw_value(&error) {
                            let _ = sink.send(raw_value).await;
                        }
                    }

                    let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                    if let Ok(raw_value) = serde_json::value::to_raw_value(&done) {
                        let _ = sink.send(raw_value).await;
                    }
                    Ok(())
                }
            },
        )?;

        // plexus_full_schema subscription - returns full schema with params and return types
        let activations_for_full_schema: HashMap<String, Arc<dyn ActivationObject>> = self
            .activations
            .iter()
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect();
        let hash_for_full_schema = plexus_hash.clone();

        module.register_subscription(
            "plexus_full_schema",
            "plexus_full_schema",
            "plexus_unsubscribe_full_schema",
            move |params, pending, _ctx, _ext| {
                let activations = activations_for_full_schema.clone();
                let hash = hash_for_full_schema.clone();
                async move {
                    let mut seq = params.sequence();
                    let namespace: String = seq.next()?;
                    let sink = pending.accept().await?;

                    if let Some(activation) = activations.get(&namespace) {
                        let full_schema = activation.full_schema();
                        let response = PlexusStreamItem::data(
                            hash.clone(),
                            Provenance::root("plexus"),
                            "plexus.full_schema".to_string(),
                            serde_json::to_value(&full_schema).unwrap(),
                        );
                        if let Ok(raw_value) = serde_json::value::to_raw_value(&response) {
                        let _ = sink.send(raw_value).await;
                    }
                    } else {
                        let error = PlexusStreamItem::error(
                            hash.clone(),
                            Provenance::root("plexus"),
                            format!("Activation not found: {}", namespace),
                            false,
                        );
                        if let Ok(raw_value) = serde_json::value::to_raw_value(&error) {
                            let _ = sink.send(raw_value).await;
                        }
                    }

                    let done = PlexusStreamItem::done(hash, Provenance::root("plexus"));
                    if let Ok(raw_value) = serde_json::value::to_raw_value(&done) {
                        let _ = sink.send(raw_value).await;
                    }
                    Ok(())
                }
            },
        )?;

        // Merge activation methods (drain from mutex)
        let pending = std::mem::take(&mut *self.pending_rpc.lock().unwrap());
        for factory in pending {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexus::types::GuidanceSuggestion;

    #[test]
    fn test_custom_guidance_contract() {
        // Create test activation
        #[derive(Clone)]
        struct TestActivation;

        #[derive(serde::Serialize, schemars::JsonSchema)]
        enum TestMethod {}

        impl MethodEnumSchema for TestMethod {
            fn method_names() -> &'static [&'static str] { &[] }
            fn schema_with_consts() -> Value { Value::Null }
        }

        #[async_trait]
        impl Activation for TestActivation {
            type Methods = TestMethod;

            fn namespace(&self) -> &str {
                "test"
            }
            fn version(&self) -> &str {
                "1.0.0"
            }
            fn methods(&self) -> Vec<&str> {
                vec![]
            }
            async fn call(&self, _: &str, _: Value) -> Result<PlexusStream, PlexusError> {
                Err(PlexusError::MethodNotFound {
                    activation: "test".to_string(),
                    method: "unknown".to_string(),
                })
            }
            fn into_rpc_methods(self) -> Methods {
                Methods::new()
            }

            // Override custom_guidance
            fn custom_guidance(
                &self,
                method: &str,
                _error: &PlexusError,
            ) -> Option<GuidanceSuggestion> {
                if method == "test" {
                    Some(GuidanceSuggestion::Custom {
                        message: "custom".to_string(),
                    })
                } else {
                    None
                }
            }
        }

        let activation = TestActivation;
        let error = PlexusError::InvalidParams("test".to_string());

        // Contract: Method returns Some for specific cases, None otherwise
        assert!(activation.custom_guidance("test", &error).is_some());
        assert!(activation.custom_guidance("other", &error).is_none());
    }

    #[test]
    fn test_default_custom_guidance_returns_none() {
        // Verify default implementation returns None
        #[derive(Clone)]
        struct MinimalActivation;

        #[derive(serde::Serialize, schemars::JsonSchema)]
        enum MinimalMethod {}

        impl MethodEnumSchema for MinimalMethod {
            fn method_names() -> &'static [&'static str] { &[] }
            fn schema_with_consts() -> Value { Value::Null }
        }

        #[async_trait]
        impl Activation for MinimalActivation {
            type Methods = MinimalMethod;

            fn namespace(&self) -> &str {
                "minimal"
            }
            fn version(&self) -> &str {
                "1.0.0"
            }
            fn methods(&self) -> Vec<&str> {
                vec![]
            }
            async fn call(&self, _: &str, _: Value) -> Result<PlexusStream, PlexusError> {
                Err(PlexusError::MethodNotFound {
                    activation: "minimal".to_string(),
                    method: "unknown".to_string(),
                })
            }
            fn into_rpc_methods(self) -> Methods {
                Methods::new()
            }
            // Don't override custom_guidance - use default
        }

        let activation = MinimalActivation;
        let error = PlexusError::InvalidParams("test".to_string());

        // Default implementation should return None
        assert!(activation.custom_guidance("any_method", &error).is_none());
    }

    #[tokio::test]
    async fn test_plexus_call_behavior_contract() {
        use futures::StreamExt;

        // Create a minimal plexus (no activations)
        let plexus = Plexus::new();

        // Contract: call() always returns Ok, never Err
        let result = plexus.call("foo.bar", serde_json::json!({})).await;
        assert!(result.is_ok(), "call() must always return Ok(stream)");

        // Contract: Error case yields stream with events
        let stream = result.unwrap();
        let items: Vec<_> = stream.collect().await;
        assert!(!items.is_empty(), "Error stream must yield events");

        // Should have Guidance event (for ActivationNotFound)
        assert_eq!(items.len(), 3, "Should yield Guidance → Error → Done");
    }

    #[tokio::test]
    async fn test_plexus_call_returns_guidance_for_activation_not_found() {
        use crate::plexus::types::PlexusStreamEvent;
        use futures::StreamExt;

        let plexus = Plexus::new();

        let stream = plexus
            .call("nonexistent.method", serde_json::json!({}))
            .await
            .unwrap();
        let items: Vec<_> = stream.collect().await;

        // First event should be Guidance
        match &items[0].event {
            PlexusStreamEvent::Guidance {
                error_type,
                suggestion,
                ..
            } => {
                // Should indicate activation not found
                assert!(
                    matches!(
                        error_type,
                        crate::plexus::types::GuidanceErrorType::ActivationNotFound { .. }
                    ),
                    "Should be ActivationNotFound error"
                );
                // Should suggest calling plexus_schema
                assert!(
                    matches!(
                        suggestion,
                        crate::plexus::types::GuidanceSuggestion::CallPlexusSchema
                    ),
                    "Should suggest CallPlexusSchema"
                );
            }
            _ => panic!("First event should be Guidance"),
        }
    }
}

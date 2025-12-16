use super::{path::Provenance, schema::Schema, types::PlexusStreamItem};
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

/// Trait that activations implement to be usable through the plexus
///
/// This is the primary activation interface. Activations implement this trait
/// to provide both programmatic access (via `call`) and RPC access
/// (via `into_rpc_methods`).
#[async_trait]
pub trait Activation: Send + Sync + 'static {
    /// Activation namespace (e.g., "health", "bash", "loom")
    fn namespace(&self) -> &str;

    /// Activation version (semantic versioning: "MAJOR.MINOR.PATCH")
    ///
    /// This version is included in handles created by the activation,
    /// allowing frontends to select appropriate renderers for different
    /// activation versions.
    ///
    /// Example: "1.0.0", "2.1.3"
    fn version(&self) -> &str;

    /// Activation description (one-line summary of what the activation does)
    fn description(&self) -> &str {
        "No description available"
    }

    /// List available methods
    fn methods(&self) -> Vec<&str>;

    /// Get help text for a specific method
    fn method_help(&self, _method: &str) -> Option<String> {
        None
    }

    /// Get the enriched JSON schema for all methods
    ///
    /// This method generates the complete schema with full type information.
    /// It should:
    /// 1. Generate base JSON schema from method enum (auto-generated, may skip complex types)
    /// 2. Convert to strongly-typed Schema representation
    /// 3. For each method variant, call .describe() to get enrichment data
    /// 4. Apply enrichments (UUID formats, descriptions, etc.)
    ///
    /// **CRITICAL**: This function MUST always succeed. If it cannot generate
    /// the schema, it means the activation is incorrectly defined.
    ///
    /// Returns the fully enriched schema ready for documentation and validation.
    fn enrich_schema(&self) -> Schema {
        // Default implementation: create empty schema
        Schema::new(
            self.namespace(),
            format!("{} activation schema", self.namespace()),
        )
    }

    /// Call a method by name with JSON params, returns a stream
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;

    /// Convert this activation into RPC methods for JSON-RPC server
    /// Consumes self since jsonrpsee's into_rpc() requires ownership
    fn into_rpc_methods(self) -> Methods;
}

/// The Plexus - routes calls to registered activations
pub struct Plexus {
    activations: HashMap<String, Arc<dyn Activation>>,
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
    pub fn register<A: Activation + Clone>(mut self, activation: A) -> Self {
        let namespace = activation.namespace().to_string();
        let activation_for_rpc = activation.clone();
        self.activations.insert(namespace, Arc::new(activation));
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

    /// Get the enriched schema for an activation
    pub fn get_activation_schema(&self, namespace: &str) -> Option<Schema> {
        let activation = self.activations.get(namespace)?;
        Some(activation.enrich_schema())
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

        // Compute hash for cache invalidation
        let plexus_hash = self.compute_hash();

        // plexus_hash subscription - returns hash for cache invalidation
        let hash_for_closure = plexus_hash.clone();
        module.register_subscription(
            "plexus_hash",
            "plexus_hash",
            "plexus_unsubscribe_hash",
            move |_params, pending, _ctx| {
                let hash = hash_for_closure.clone();
                async move {
                    let sink = pending.accept().await?;
                    let response = PlexusStreamItem::Data {
                        provenance: Provenance::root("plexus"),
                        content_type: "plexus.hash".to_string(),
                        data: serde_json::json!({ "hash": hash }),
                    };
                    if let Ok(msg) = SubscriptionMessage::from_json(&response) {
                        let _ = sink.send(msg).await;
                    }
                    let done = PlexusStreamItem::Done {
                        provenance: Provenance::root("plexus"),
                    };
                    if let Ok(msg) = SubscriptionMessage::from_json(&done) {
                        let _ = sink.send(msg).await;
                    }
                    Ok(())
                }
            },
        )?;

        // plexus_schema subscription - returns all activations and methods
        module.register_subscription(
            "plexus_schema",
            "plexus_schema",
            "plexus_unsubscribe_schema",
            move |_params, pending, _ctx| {
                let schema = plexus_schema.clone();
                async move {
                    let sink = pending.accept().await?;
                    let response = PlexusStreamItem::Data {
                        provenance: Provenance::root("plexus"),
                        content_type: "plexus.schema".to_string(),
                        data: serde_json::to_value(&schema).unwrap(),
                    };
                    if let Ok(msg) = SubscriptionMessage::from_json(&response) {
                        let _ = sink.send(msg).await;
                    }
                    let done = PlexusStreamItem::Done {
                        provenance: Provenance::root("plexus"),
                    };
                    if let Ok(msg) = SubscriptionMessage::from_json(&done) {
                        let _ = sink.send(msg).await;
                    }
                    Ok(())
                }
            },
        )?;

        // plexus_activation_schema subscription - returns enriched schema for a specific activation
        // Clone activations for the closure
        let activations_for_schema: HashMap<String, Arc<dyn Activation>> = self
            .activations
            .iter()
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect();

        module.register_subscription(
            "plexus_activation_schema",
            "plexus_activation_schema",
            "plexus_unsubscribe_activation_schema",
            move |params, pending, _ctx| {
                let activations = activations_for_schema.clone();
                async move {
                    // Parse namespace parameter
                    let namespace: String = params.one()?;
                    let sink = pending.accept().await?;

                    if let Some(activation) = activations.get(&namespace) {
                        let schema = activation.enrich_schema();
                        let response = PlexusStreamItem::Data {
                            provenance: Provenance::root("plexus"),
                            content_type: "plexus.activation_schema".to_string(),
                            data: serde_json::to_value(&schema).unwrap(),
                        };
                        if let Ok(msg) = SubscriptionMessage::from_json(&response) {
                            let _ = sink.send(msg).await;
                        }
                    } else {
                        let error = PlexusStreamItem::Error {
                            provenance: Provenance::root("plexus"),
                            error: format!("Activation not found: {}", namespace),
                            recoverable: false,
                        };
                        if let Ok(msg) = SubscriptionMessage::from_json(&error) {
                            let _ = sink.send(msg).await;
                        }
                    }

                    let done = PlexusStreamItem::Done {
                        provenance: Provenance::root("plexus"),
                    };
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
    Box::pin(stream.map(move |item| item.into_plexus_item(provenance.clone())))
}

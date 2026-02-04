//! Plexus RPC builder - constructs a fully configured DynamicHub instance
//!
//! This module is used by both the main binary and examples.

use std::sync::{Arc, Weak};

use crate::activations::arbor::{Arbor, ArborConfig};
use crate::activations::bash::Bash;
use crate::activations::changelog::{Changelog, ChangelogStorageConfig};
use crate::activations::claudecode::{ClaudeCode, ClaudeCodeStorage, ClaudeCodeStorageConfig};
use crate::activations::claudecode_loopback::{ClaudeCodeLoopback, LoopbackStorageConfig};
use crate::activations::cone::{Cone, ConeStorageConfig};
use crate::activations::echo::Echo;
use crate::activations::health::Health;
use crate::activations::mustache::{Mustache, MustacheStorageConfig};
use crate::activations::solar::Solar;
use crate::plexus::DynamicHub;
use hyperforge::HyperforgeHub;
// use jsexec::{JsExec, JsExecConfig};  // temporarily disabled - needs API updates
use registry::Registry;

/// Build the Plexus RPC hub with registered activations
///
/// The hub implements the Plexus RPC protocol and provides introspection methods:
/// - substrate.call: Route calls to registered activations
/// - substrate.hash: Get configuration hash for cache invalidation
/// - substrate.list_activations: Enumerate registered activations
/// - substrate.schema: Get full Plexus RPC schema
///
/// Hub activations (with nested children) are registered with `register_hub`
/// to enable direct nested routing like `substrate.solar.mercury.info`.
///
/// This function uses `Arc::new_cyclic` to inject a weak reference to the hub
/// into Cone and ClaudeCode, enabling them to resolve foreign handles through
/// the hub without creating reference cycles.
///
/// This function is async because Arbor, Cone, and ClaudeCode require
/// async database initialization.
pub async fn build_plexus_rpc() -> Arc<DynamicHub> {
    // Initialize Arbor first (other activations depend on its storage)
    // Use explicit type annotation for Weak<DynamicHub> parent context
    let arbor: Arbor<Weak<DynamicHub>> = Arbor::with_context_type(ArborConfig::default())
        .await
        .expect("Failed to initialize Arbor");
    let arbor_storage = arbor.storage();

    // Initialize Cone with shared Arbor storage
    // Use explicit type annotation for Weak<DynamicHub> parent context
    let cone: Cone<Weak<DynamicHub>> = Cone::with_context_type(ConeStorageConfig::default(), arbor_storage.clone())
        .await
        .expect("Failed to initialize Cone");

    // Initialize ClaudeCode with shared Arbor storage
    // Use explicit type annotation for Weak<DynamicHub> parent context
    let claudecode_storage = ClaudeCodeStorage::new(
        ClaudeCodeStorageConfig::default(),
        arbor_storage,
    )
    .await
    .expect("Failed to initialize ClaudeCode storage");
    let claudecode: ClaudeCode<Weak<DynamicHub>> = ClaudeCode::with_context_type(Arc::new(claudecode_storage));

    // Initialize Mustache for template rendering
    let mustache = Mustache::new(MustacheStorageConfig::default())
        .await
        .expect("Failed to initialize Mustache");

    // Initialize Changelog for tracking Plexus RPC server changes
    let changelog = Changelog::new(ChangelogStorageConfig::default())
        .await
        .expect("Failed to initialize Changelog");

    // Initialize ClaudeCode Loopback for tool permission routing
    let loopback = ClaudeCodeLoopback::new(LoopbackStorageConfig::default())
        .await
        .expect("Failed to initialize ClaudeCodeLoopback");

    // Initialize JsExec for JavaScript execution in V8 isolates
    // let jsexec = JsExec::new(JsExecConfig::default());  // temporarily disabled

    // Initialize Registry for backend discovery
    let registry = Registry::with_defaults()
        .await
        .expect("Failed to initialize Registry");

    // Use Arc::new_cyclic to get a Weak<DynamicHub> during construction
    // This allows us to inject the parent context into Cone and ClaudeCode
    // before the hub is fully constructed, avoiding reference cycles
    let hub = Arc::new_cyclic(|weak_hub: &Weak<DynamicHub>| {
        // Inject parent context into activations that need it
        arbor.inject_parent(weak_hub.clone());
        cone.inject_parent(weak_hub.clone());
        claudecode.inject_parent(weak_hub.clone());

        // Build and return the DynamicHub with "substrate" namespace
        DynamicHub::new("substrate")
            .register(Health::new())
            .register(Echo::new())
            .register(Bash::new())
            .register(arbor)
            .register(cone)
            .register(claudecode)
            .register(mustache)
            .register(changelog.clone())
            .register(loopback)
            // .register(jsexec)  // temporarily disabled
            .register(registry)
            .register_hub(Solar::new())
            .register(HyperforgeHub::new())
    });

    // Run changelog startup check
    let plexus_hash = hub.compute_hash();
    match changelog.startup_check(&plexus_hash).await {
        Ok((hash_changed, is_documented, message)) => {
            if hash_changed && !is_documented {
                tracing::error!("{}", message);
            } else if hash_changed {
                tracing::info!("{}", message);
            } else {
                tracing::debug!("{}", message);
            }
        }
        Err(e) => {
            tracing::error!("Changelog startup check failed: {}", e);
        }
    }

    hub
}

//! Plexus builder - constructs a fully configured Plexus instance
//!
//! This module is used by both the main binary and examples.

use std::sync::Arc;

use crate::activations::arbor::{Arbor, ArborConfig};
use crate::activations::bash::Bash;
use crate::activations::changelog::{Changelog, ChangelogStorageConfig};
use crate::activations::claudecode::{ClaudeCode, ClaudeCodeStorage, ClaudeCodeStorageConfig};
use crate::activations::cone::{Cone, ConeStorageConfig};
use crate::activations::echo::Echo;
use crate::activations::health::Health;
use crate::activations::mustache::{Mustache, MustacheStorageConfig};
use crate::activations::solar::Solar;
use crate::plexus::Plexus;
use hyperforge::HyperforgeHub;

/// Build the plexus with registered activations
///
/// Plexus itself provides introspection methods:
/// - plexus.call: Route calls to registered activations
/// - plexus.hash: Get configuration hash for cache invalidation
/// - plexus.list_activations: Enumerate registered activations
/// - plexus.schema: Get full plexus schema
///
/// Hub activations (with nested children) are registered with `register_hub`
/// to enable direct nested routing like `plexus.solar.mercury.info`.
///
/// This function is async because Arbor, Cone, and ClaudeCode require
/// async database initialization.
pub async fn build_plexus() -> Arc<Plexus> {
    // Initialize Arbor first (other activations depend on its storage)
    let arbor = Arbor::new(ArborConfig::default())
        .await
        .expect("Failed to initialize Arbor");
    let arbor_storage = arbor.storage();

    // Initialize Cone with shared Arbor storage
    let cone = Cone::new(ConeStorageConfig::default(), arbor_storage.clone())
        .await
        .expect("Failed to initialize Cone");

    // Initialize ClaudeCode with shared Arbor storage
    let claudecode_storage = ClaudeCodeStorage::new(
        ClaudeCodeStorageConfig::default(),
        arbor_storage,
    )
    .await
    .expect("Failed to initialize ClaudeCode storage");
    let claudecode = ClaudeCode::new(Arc::new(claudecode_storage));

    // Initialize Mustache for template rendering
    let mustache = Mustache::new(MustacheStorageConfig::default())
        .await
        .expect("Failed to initialize Mustache");

    // Initialize Changelog for tracking plexus changes
    let changelog = Changelog::new(ChangelogStorageConfig::default())
        .await
        .expect("Failed to initialize Changelog");

    let plexus = Arc::new(
        Plexus::new()
            .register(Health::new())
            .register(Echo::new())
            .register(Bash::new())
            .register(arbor)
            .register(cone)
            .register(claudecode)
            .register(mustache)
            .register(changelog.clone())
            .register_hub(Solar::new())
            .register_hub(HyperforgeHub::new()),
    );

    // Run changelog startup check
    let plexus_hash = plexus.compute_hash();
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

    plexus
}

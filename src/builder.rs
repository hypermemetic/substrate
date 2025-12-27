//! Plexus builder - constructs a fully configured Plexus instance
//!
//! This module is used by both the main binary and examples.

use std::sync::Arc;

use crate::activations::echo::Echo;
use crate::activations::health::Health;
use crate::activations::solar::Solar;
use crate::plexus::Plexus;

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
pub fn build_plexus() -> Arc<Plexus> {
    Arc::new(
        Plexus::new()
            .register(Health::new())
            .register(Echo::new())
            .register_hub(Solar::new()),
    )
}

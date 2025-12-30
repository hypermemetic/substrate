//! Example Plexus builder
//!
//! This module provides an example of how to build a Plexus instance.
//! Real applications should create their own builder with their specific activations.

use std::sync::Arc;

use crate::activations::echo::Echo;
use crate::activations::health::Health;
use crate::plexus::Plexus;

/// Build an example plexus with minimal activations
///
/// This demonstrates how to construct a Plexus instance.
/// Real applications should define their own builder function
/// that registers their specific activations.
///
/// Plexus itself provides introspection methods:
/// - plexus.call: Route calls to registered activations
/// - plexus.hash: Get configuration hash for cache invalidation
pub fn build_example_plexus() -> Arc<Plexus> {
    Arc::new(
        Plexus::new()
            .register(Health::new())
            .register(Echo::new()),
    )
}

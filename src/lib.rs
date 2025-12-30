//! Hub Core - Core infrastructure for building hub-based systems
//!
//! This crate provides:
//! - `Plexus` - Central routing and method dispatch
//! - `Activation` - Trait for implementing plugins
//! - `PlexusMcpBridge` - MCP server integration
//! - `Handle` - Typed references to plugin method results
//!
//! # Example
//!
//! ```rust,no_run
//! use hub_core::plexus::Plexus;
//! use hub_core::activations::echo::Echo;
//! use hub_core::activations::health::Health;
//! use std::sync::Arc;
//!
//! let plexus = Arc::new(
//!     Plexus::new()
//!         .register(Health::new())
//!         .register(Echo::new())
//! );
//! ```

pub mod activations;
pub mod builder;
pub mod mcp_bridge;
pub mod plexus;
pub mod plugin_system;
pub mod types;

// Re-export commonly used items
pub use builder::build_example_plexus;
pub use mcp_bridge::PlexusMcpBridge;
pub use plexus::{Activation, Plexus, PlexusError};
pub use types::{Envelope, Handle, Origin};

pub mod activations;
pub mod builder;
pub mod mcp_bridge;
pub mod mcp_session;
pub mod plexus;
pub mod plugin_system;
pub mod types;

// Re-export serde helpers for macro-generated code
// This allows the hub_methods macro to reference serde helpers via crate::serde_helpers
pub use hub_core::serde_helpers;

// Re-export commonly used items
pub use builder::build_plexus;
pub use hub_core::plexus::DynamicHub as Plexus;
pub use mcp_bridge::PlexusMcpBridge;
pub use mcp_session::{SqliteSessionManager, SqliteSessionConfig};
pub use types::{Envelope, Handle, Origin};

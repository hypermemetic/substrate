pub mod activations;
pub mod builder;
pub mod mcp_bridge;
pub mod plexus;
pub mod plugin_system;
pub mod types;

// Re-export commonly used items
pub use builder::build_plexus;
pub use mcp_bridge::PlexusMcpBridge;
pub use types::{Envelope, Handle, Origin};

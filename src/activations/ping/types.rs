//! Ping activation event types

use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

/// Events emitted by the Ping activation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PingEvent {
    /// Pong response
    Pong {
        /// The original message that was sent
        message: String,
    },
    /// Echo response with a counter
    Echo {
        /// The message being echoed
        message: String,
        /// Which echo this is (1-indexed)
        index: u32,
        /// Total number of echoes
        total: u32,
    },
}

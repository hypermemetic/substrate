//! Echo activation event types
//!
//! This demonstrates plain domain types for the caller-wraps streaming pattern.
//! No trait implementations needed - just standard derives.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Events from echo operations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EchoEvent {
    /// Echo response
    Echo {
        /// The echoed message
        message: String,
        /// Number of times repeated
        count: u32,
    },
}

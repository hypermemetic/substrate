use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stream events from health check
///
/// This is a plain domain type - no trait implementations needed.
/// The caller (Plexus) wraps this with metadata when streaming.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HealthEvent {
    /// Current health status
    Status {
        status: String,
        uptime_seconds: u64,
        timestamp: i64,
    },
}

// Keep old name for backwards compatibility
pub type HealthStatus = HealthEvent;

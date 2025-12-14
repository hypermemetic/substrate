use crate::{
    plexus::{Provenance, types::PlexusStreamItem},
    plugin_system::types::ActivationStreamItem,
};
use serde::{Deserialize, Serialize};

/// Stream events from health check
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HealthEvent {
    /// Current health status
    #[serde(rename = "status")]
    Status {
        status: String,
        uptime_seconds: u64,
        timestamp: i64,
    },
}

impl ActivationStreamItem for HealthEvent {
    fn content_type() -> &'static str {
        "health.event"
    }

    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem {
        PlexusStreamItem::Data {
            provenance,
            content_type: Self::content_type().to_string(),
            data: serde_json::to_value(self).unwrap(),
        }
    }
}

// Keep old name for backwards compatibility
pub type HealthStatus = HealthEvent;

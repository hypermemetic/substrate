use crate::{
    plexus::{Provenance, types::PlexusStreamItem},
    plugin_system::types::ActivationStreamItem,
};
use serde::{Deserialize, Serialize};

/// Stream events from bash command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BashEvent {
    /// Standard output line
    #[serde(rename = "stdout")]
    Stdout { line: String },

    /// Standard error line
    #[serde(rename = "stderr")]
    Stderr { line: String },

    /// Exit code when process completes
    #[serde(rename = "exit")]
    Exit { code: i32 },
}

impl ActivationStreamItem for BashEvent {
    fn content_type() -> &'static str {
        "bash.event"
    }

    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem {
        PlexusStreamItem::Data {
            provenance,
            content_type: Self::content_type().to_string(),
            data: serde_json::to_value(self).unwrap(),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(self, BashEvent::Exit { .. })
    }
}

// Keep the old name as an alias for backwards compatibility
pub type BashOutput = BashEvent;

/// Error types for bash execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashError {
    pub message: String,
}

impl ActivationStreamItem for BashError {
    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem {
        PlexusStreamItem::Error {
            provenance,
            error: self.message,
            recoverable: false,
        }
    }
}

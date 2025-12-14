use super::path::Provenance;
use serde::{Deserialize, Serialize};

/// Body's unified stream item type
/// All behaviors convert their Stream<T> to Stream<PlexusStreamItem>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum PlexusStreamItem {
    /// Progress update
    #[serde(rename = "progress")]
    Progress {
        provenance: Provenance,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        percentage: Option<f32>,
    },

    /// Data chunk with type information
    #[serde(rename = "data")]
    Data {
        provenance: Provenance,
        content_type: String,
        data: serde_json::Value,
    },

    /// Error occurred
    #[serde(rename = "error")]
    Error {
        provenance: Provenance,
        error: String,
        recoverable: bool,
    },

    /// Stream completed successfully
    #[serde(rename = "done")]
    Done { provenance: Provenance },
}

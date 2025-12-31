//! Plexus streaming types
//!
//! These types define the wire format for all plexus streaming responses.
//! The key architectural principle is "caller wraps" - activations return
//! typed domain events, and the caller (Plexus) wraps them with metadata.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Metadata applied by the caller when wrapping activation responses
///
/// This metadata is added at each layer of the call stack, enabling
/// provenance tracking and cache invalidation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamMetadata {
    /// Call path through the system (e.g., ["plexus", "health"])
    pub provenance: Vec<String>,

    /// Hash of plexus configuration for cache invalidation
    /// Changes when activations are added/removed/updated
    pub plexus_hash: String,

    /// Unix timestamp (seconds) when the event was wrapped
    pub timestamp: i64,
}

impl StreamMetadata {
    /// Create new metadata with current timestamp
    pub fn new(provenance: Vec<String>, plexus_hash: String) -> Self {
        Self {
            provenance,
            plexus_hash,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

/// Universal stream item - all activations emit this type
///
/// The caller (Plexus routing layer) wraps activation responses with
/// metadata. This is the only type that crosses the wire.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlexusStreamItem {
    /// Data payload with caller-applied metadata
    Data {
        /// Metadata from calling layer
        metadata: StreamMetadata,
        /// Type identifier for deserialization (e.g., "health.status")
        content_type: String,
        /// The actual payload (serialized activation event)
        content: Value,
    },

    /// Progress update during long-running operations
    Progress {
        /// Metadata from calling layer
        metadata: StreamMetadata,
        /// Human-readable progress message
        message: String,
        /// Optional completion percentage (0.0 - 100.0)
        #[serde(skip_serializing_if = "Option::is_none")]
        percentage: Option<f32>,
    },

    /// Error occurred during processing
    Error {
        /// Metadata from calling layer
        metadata: StreamMetadata,
        /// Human-readable error message
        message: String,
        /// Optional error code for programmatic handling
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// Whether the operation can be retried
        recoverable: bool,
    },

    /// Stream completed successfully
    Done {
        /// Metadata from calling layer
        metadata: StreamMetadata,
    },
}

impl PlexusStreamItem {
    /// Create a Data item
    pub fn data(metadata: StreamMetadata, content_type: String, content: Value) -> Self {
        Self::Data {
            metadata,
            content_type,
            content,
        }
    }

    /// Create a Progress item
    pub fn progress(metadata: StreamMetadata, message: String, percentage: Option<f32>) -> Self {
        Self::Progress {
            metadata,
            message,
            percentage,
        }
    }

    /// Create an Error item
    pub fn error(
        metadata: StreamMetadata,
        message: String,
        code: Option<String>,
        recoverable: bool,
    ) -> Self {
        Self::Error {
            metadata,
            message,
            code,
            recoverable,
        }
    }

    /// Create a Done item
    pub fn done(metadata: StreamMetadata) -> Self {
        Self::Done { metadata }
    }

    /// Get the metadata from any stream item variant
    pub fn metadata(&self) -> &StreamMetadata {
        match self {
            Self::Data { metadata, .. } => metadata,
            Self::Progress { metadata, .. } => metadata,
            Self::Error { metadata, .. } => metadata,
            Self::Done { metadata } => metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_item_data_serialization() {
        let metadata = StreamMetadata {
            provenance: vec!["plexus".into(), "health".into()],
            plexus_hash: "abc123".into(),
            timestamp: 1735052400,
        };

        let item = PlexusStreamItem::data(
            metadata,
            "health.status".into(),
            serde_json::json!({ "status": "healthy", "uptime": 123 }),
        );

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"type\":\"data\""));
        assert!(json.contains("\"content_type\":\"health.status\""));
        assert!(json.contains("\"plexus_hash\":\"abc123\""));
        assert!(json.contains("\"provenance\":[\"plexus\",\"health\"]"));
    }

    #[test]
    fn test_stream_item_error_serialization() {
        let metadata = StreamMetadata::new(vec!["plexus".into()], "hash".into());

        let item = PlexusStreamItem::error(
            metadata,
            "Something went wrong".into(),
            Some("E001".into()),
            false,
        );

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"message\":\"Something went wrong\""));
        assert!(json.contains("\"code\":\"E001\""));
        assert!(json.contains("\"recoverable\":false"));
    }

    #[test]
    fn test_stream_item_progress_serialization() {
        let metadata = StreamMetadata::new(vec!["plexus".into()], "hash".into());

        let item = PlexusStreamItem::progress(metadata, "Processing...".into(), Some(50.0));

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"type\":\"progress\""));
        assert!(json.contains("\"message\":\"Processing...\""));
        assert!(json.contains("\"percentage\":50.0"));
    }

    #[test]
    fn test_stream_item_done_serialization() {
        let metadata = StreamMetadata::new(vec!["plexus".into()], "hash".into());

        let item = PlexusStreamItem::done(metadata);

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"type\":\"done\""));
    }
}

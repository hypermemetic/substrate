use super::path::Provenance;
use super::schema::Schema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Type of error that triggered guidance
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "error_kind", rename_all = "snake_case")]
pub enum GuidanceErrorType {
    /// Activation namespace not found
    ActivationNotFound { activation: String },
    /// Method not found within activation
    MethodNotFound { activation: String, method: String },
    /// Invalid parameters for method
    InvalidParams { method: String, reason: String },
}

/// Suggested action to resolve the error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GuidanceSuggestion {
    /// Call plexus_schema to see all activations
    CallPlexusSchema,
    /// Call plexus_activation_schema with namespace
    CallActivationSchema { namespace: String },
    /// Try calling a specific method
    TryMethod {
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        example_params: Option<Value>,
    },
    /// Custom suggestion from activation
    Custom { message: String },
}

/// Inner stream item type (the actual event data)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PlexusStreamEvent {
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

    /// Guidance for error resolution
    #[serde(rename = "guidance")]
    Guidance {
        provenance: Provenance,
        #[serde(flatten)]
        error_type: GuidanceErrorType,
        #[serde(skip_serializing_if = "Option::is_none")]
        available_methods: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        method_schema: Option<Schema>,
        #[serde(flatten)]
        suggestion: GuidanceSuggestion,
    },
}

/// Plexus stream item with hash for cache invalidation
///
/// Every response includes the plexus_hash, allowing clients to detect
/// when their cached schema is stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexusStreamItem {
    /// Hash of all activations for cache invalidation
    pub plexus_hash: String,

    /// The actual event data (flattened into same object)
    #[serde(flatten)]
    pub event: PlexusStreamEvent,
}

impl PlexusStreamItem {
    /// Create a new stream item with the given hash
    pub fn new(plexus_hash: String, event: PlexusStreamEvent) -> Self {
        Self { plexus_hash, event }
    }

    /// Create a Progress item
    pub fn progress(plexus_hash: String, provenance: Provenance, message: String, percentage: Option<f32>) -> Self {
        Self::new(plexus_hash, PlexusStreamEvent::Progress { provenance, message, percentage })
    }

    /// Create a Data item
    pub fn data(plexus_hash: String, provenance: Provenance, content_type: String, data: serde_json::Value) -> Self {
        Self::new(plexus_hash, PlexusStreamEvent::Data { provenance, content_type, data })
    }

    /// Create an Error item
    pub fn error(plexus_hash: String, provenance: Provenance, error: String, recoverable: bool) -> Self {
        Self::new(plexus_hash, PlexusStreamEvent::Error { provenance, error, recoverable })
    }

    /// Create a Done item
    pub fn done(plexus_hash: String, provenance: Provenance) -> Self {
        Self::new(plexus_hash, PlexusStreamEvent::Done { provenance })
    }

    /// Create a Guidance item
    pub fn guidance(
        plexus_hash: String,
        provenance: Provenance,
        error_type: GuidanceErrorType,
        available_methods: Option<Vec<String>>,
        method_schema: Option<Schema>,
        suggestion: GuidanceSuggestion,
    ) -> Self {
        Self::new(
            plexus_hash,
            PlexusStreamEvent::Guidance {
                provenance,
                error_type,
                available_methods,
                method_schema,
                suggestion,
            },
        )
    }
}

// Legacy constructors for backwards compatibility during migration
// TODO: Remove these once all code is updated to use new constructors
impl PlexusStreamItem {
    /// Legacy: Create Data without explicit hash (uses empty string)
    #[doc(hidden)]
    pub fn data_legacy(provenance: Provenance, content_type: String, data: serde_json::Value) -> Self {
        Self::data(String::new(), provenance, content_type, data)
    }

    /// Legacy: Create Done without explicit hash (uses empty string)
    #[doc(hidden)]
    pub fn done_legacy(provenance: Provenance) -> Self {
        Self::done(String::new(), provenance)
    }

    /// Legacy: Create Error without explicit hash (uses empty string)
    #[doc(hidden)]
    pub fn error_legacy(provenance: Provenance, error: String, recoverable: bool) -> Self {
        Self::error(String::new(), provenance, error, recoverable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guidance_types_contract() {
        // Verify types compile
        let error_type = GuidanceErrorType::ActivationNotFound {
            activation: "foo".to_string(),
        };
        let suggestion = GuidanceSuggestion::CallPlexusSchema;

        // Verify serialization
        let item = PlexusStreamItem::guidance(
            "hash123".to_string(),
            Provenance::root("test"),
            error_type,
            None,
            None,
            suggestion,
        );

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"type\":\"guidance\""));
        assert!(json.contains("\"action\":\"call_plexus_schema\""));
    }

    #[test]
    fn test_guidance_error_type_serialization() {
        // Test ActivationNotFound
        let error = GuidanceErrorType::ActivationNotFound {
            activation: "foo".to_string(),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"error_kind\":\"activation_not_found\""));
        assert!(json.contains("\"activation\":\"foo\""));

        // Test MethodNotFound
        let error = GuidanceErrorType::MethodNotFound {
            activation: "bash".to_string(),
            method: "execute".to_string(),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"error_kind\":\"method_not_found\""));
        assert!(json.contains("\"activation\":\"bash\""));
        assert!(json.contains("\"method\":\"execute\""));

        // Test InvalidParams
        let error = GuidanceErrorType::InvalidParams {
            method: "bash.execute".to_string(),
            reason: "missing command".to_string(),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"error_kind\":\"invalid_params\""));
        assert!(json.contains("\"method\":\"bash.execute\""));
        assert!(json.contains("\"reason\":\"missing command\""));
    }

    #[test]
    fn test_guidance_suggestion_serialization() {
        // Test CallPlexusSchema
        let suggestion = GuidanceSuggestion::CallPlexusSchema;
        let json = serde_json::to_string(&suggestion).unwrap();
        assert!(json.contains("\"action\":\"call_plexus_schema\""));

        // Test CallActivationSchema
        let suggestion = GuidanceSuggestion::CallActivationSchema {
            namespace: "arbor".to_string(),
        };
        let json = serde_json::to_string(&suggestion).unwrap();
        assert!(json.contains("\"action\":\"call_activation_schema\""));
        assert!(json.contains("\"namespace\":\"arbor\""));

        // Test TryMethod
        let suggestion = GuidanceSuggestion::TryMethod {
            method: "bash.execute".to_string(),
            example_params: Some(serde_json::json!("echo hello")),
        };
        let json = serde_json::to_string(&suggestion).unwrap();
        assert!(json.contains("\"action\":\"try_method\""));
        assert!(json.contains("\"method\":\"bash.execute\""));
        assert!(json.contains("\"example_params\""));

        // Test Custom
        let suggestion = GuidanceSuggestion::Custom {
            message: "Try checking the logs".to_string(),
        };
        let json = serde_json::to_string(&suggestion).unwrap();
        assert!(json.contains("\"action\":\"custom\""));
        assert!(json.contains("\"message\":\"Try checking the logs\""));
    }
}

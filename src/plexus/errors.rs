//! DEPRECATED: Stream-based guidance replaces error data structures
//!
//! This module is kept for historical reference only. Error guidance is now
//! provided via `GuidanceErrorType` and `GuidanceSuggestion` in stream events.
//!
//! **Migration:** Use `PlexusStreamEvent::Guidance` instead of parsing error data.
//! See: `docs/architecture/16680880693241553663_frontend-guidance-migration.md`
//!
//! ---
//!
//! ## Legacy Documentation

#![allow(dead_code)]

use jsonrpsee::types::ErrorObjectOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Standard JSON-RPC 2.0 error codes
pub mod codes {
    /// Invalid JSON was received (parse error)
    pub const PARSE_ERROR: i32 = -32700;
    /// The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;
    /// The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// A suggested JSON-RPC request to try next
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TryRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Value>,
}

impl TryRequest {
    /// Create a try request for plexus_schema (the discovery endpoint)
    pub fn schema() -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "plexus_schema".to_string(),
            params: vec![],
        }
    }

    /// Create a try request for a specific method with example params
    pub fn method(method: &str, params: Vec<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: method.to_string(),
            params,
        }
    }
}

/// Error data with `try` field for guided discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidedErrorData {
    /// Suggested request to try next
    #[serde(rename = "try")]
    pub try_request: TryRequest,

    /// Additional context-specific fields (flattened into the data object)
    #[serde(flatten)]
    pub context: Value,
}

impl GuidedErrorData {
    /// Create error data with just a try request
    pub fn new(try_request: TryRequest) -> Self {
        Self {
            try_request,
            context: json!({}),
        }
    }

    /// Create error data with additional context
    pub fn with_context(try_request: TryRequest, context: Value) -> Self {
        Self {
            try_request,
            context,
        }
    }
}

/// Builder for creating guided JSON-RPC errors
pub struct GuidedError;

impl GuidedError {
    /// Parse error - client sent non-JSON-RPC content
    pub fn parse_error(message: &str) -> ErrorObjectOwned {
        let data = GuidedErrorData::new(TryRequest::schema());
        ErrorObjectOwned::owned(
            codes::PARSE_ERROR,
            format!("Parse error: {}. This server speaks JSON-RPC 2.0 over WebSocket", message),
            Some(data),
        )
    }

    /// Invalid request - malformed JSON-RPC request
    pub fn invalid_request(message: &str) -> ErrorObjectOwned {
        let data = GuidedErrorData::new(TryRequest::schema());
        ErrorObjectOwned::owned(
            codes::INVALID_REQUEST,
            format!("Invalid request: {}", message),
            Some(data),
        )
    }

    /// Activation not found - the namespace doesn't exist
    pub fn activation_not_found(activation: &str, available: Vec<String>) -> ErrorObjectOwned {
        let data = GuidedErrorData::with_context(
            TryRequest::schema(),
            json!({
                "activation": activation,
                "available_activations": available,
            }),
        );
        ErrorObjectOwned::owned(
            codes::METHOD_NOT_FOUND,
            format!("Activation '{}' not found", activation),
            Some(data),
        )
    }

    /// Method not found - the activation exists but not the method
    pub fn method_not_found(
        activation: &str,
        method: &str,
        available_methods: Vec<String>,
        example_method: Option<(&str, Vec<Value>)>,
    ) -> ErrorObjectOwned {
        // If we have an example method, suggest trying it; otherwise suggest schema
        let try_request = match example_method {
            Some((method_name, params)) => TryRequest::method(method_name, params),
            None => TryRequest::schema(),
        };

        let data = GuidedErrorData::with_context(
            try_request,
            json!({
                "activation": activation,
                "method": method,
                "available_methods": available_methods,
            }),
        );
        ErrorObjectOwned::owned(
            codes::METHOD_NOT_FOUND,
            format!("Method '{}' not found in activation '{}'", method, activation),
            Some(data),
        )
    }

    /// Invalid params - the method exists but params are wrong
    pub fn invalid_params(
        method: &str,
        message: &str,
        usage: Option<&str>,
        example: Option<TryRequest>,
    ) -> ErrorObjectOwned {
        let try_request = example.unwrap_or_else(TryRequest::schema);

        let mut context = json!({
            "method": method,
        });

        if let Some(usage_str) = usage {
            context["usage"] = json!(usage_str);
        }

        let data = GuidedErrorData::with_context(try_request, context);
        ErrorObjectOwned::owned(
            codes::INVALID_PARAMS,
            format!("Invalid params for {}: {}", method, message),
            Some(data),
        )
    }

    /// Internal/execution error
    pub fn internal_error(message: &str) -> ErrorObjectOwned {
        let data = GuidedErrorData::new(TryRequest::schema());
        ErrorObjectOwned::owned(
            codes::INTERNAL_ERROR,
            format!("Internal error: {}", message),
            Some(data),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_includes_try() {
        let error = GuidedError::parse_error("invalid JSON");
        let data: GuidedErrorData = serde_json::from_str(
            error.data().unwrap().get()
        ).unwrap();

        assert_eq!(data.try_request.method, "plexus_schema");
    }

    #[test]
    fn test_activation_not_found_includes_available() {
        let error = GuidedError::activation_not_found(
            "foo",
            vec!["arbor".into(), "bash".into(), "health".into()],
        );
        let data: GuidedErrorData = serde_json::from_str(
            error.data().unwrap().get()
        ).unwrap();

        assert_eq!(data.try_request.method, "plexus_schema");
        assert_eq!(data.context["available_activations"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_method_not_found_with_example() {
        let error = GuidedError::method_not_found(
            "bash",
            "foo",
            vec!["execute".into()],
            Some(("bash_execute", vec![json!("echo hello")])),
        );
        let data: GuidedErrorData = serde_json::from_str(
            error.data().unwrap().get()
        ).unwrap();

        // Should suggest the example method, not schema
        assert_eq!(data.try_request.method, "bash_execute");
        assert_eq!(data.try_request.params[0], json!("echo hello"));
    }
}

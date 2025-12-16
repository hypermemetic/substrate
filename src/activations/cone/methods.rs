/// Method definitions for the Cone plugin using JSON Schema
///
/// This provides type-safe method definitions with automatic schema generation
/// for documentation and validation. By using `uuid::Uuid` directly and doc comments,
/// schemars automatically generates format: "uuid", descriptions, and required arrays.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// All available methods in the Cone plugin
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ConeMethod {
    /// Create a new cone (LLM agent with persistent context)
    Create {
        /// Human-readable name for the cone
        name: String,

        /// LLM model ID (e.g., "gpt-4o-mini", "claude-3-haiku-20240307")
        model_id: String,

        /// Optional system prompt / instructions
        #[serde(skip_serializing_if = "Option::is_none")]
        system_prompt: Option<String>,

        /// Optional configuration metadata
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// Get cone configuration by ID
    Get {
        /// UUID of the cone to retrieve
        cone_id: Uuid,
    },

    /// List all cones
    List,

    /// Delete a cone (associated tree is preserved)
    Delete {
        /// UUID of the cone to delete
        cone_id: Uuid,
    },

    /// Chat with a cone - appends prompt to context, calls LLM, advances head
    Chat {
        /// UUID of the cone to chat with
        cone_id: Uuid,

        /// User message / prompt
        prompt: String,
    },

    /// Move cone's canonical head to a different node in the tree
    SetHead {
        /// UUID of the cone
        cone_id: Uuid,

        /// UUID of the target node
        node_id: Uuid,
    },

    /// Get available LLM services and models
    Registry,
}

impl ConeMethod {
    /// Get the method name as a string
    pub fn name(&self) -> &'static str {
        match self {
            ConeMethod::Create { .. } => "create",
            ConeMethod::Get { .. } => "get",
            ConeMethod::List => "list",
            ConeMethod::Delete { .. } => "delete",
            ConeMethod::Chat { .. } => "chat",
            ConeMethod::SetHead { .. } => "set_head",
            ConeMethod::Registry => "registry",
        }
    }

    /// Get all available method names
    pub fn all_names() -> Vec<&'static str> {
        vec![
            "create",
            "get",
            "list",
            "delete",
            "chat",
            "set_head",
            "registry",
        ]
    }

    /// Get the JSON schema for all Cone methods
    pub fn schema() -> serde_json::Value {
        let schema = schemars::schema_for!(ConeMethod);
        serde_json::to_value(schema).unwrap()
    }

    /// Get a human-readable description of a method
    pub fn description(method_name: &str) -> Option<&'static str> {
        match method_name {
            "create" => Some("Create a new cone (LLM agent with persistent conversation context)"),
            "get" => Some("Get cone configuration by ID"),
            "list" => Some("List all cones"),
            "delete" => Some("Delete a cone (associated tree is preserved)"),
            "chat" => Some("Chat with a cone - streams LLM response and advances context"),
            "set_head" => Some("Move cone's context head to a different node in the tree"),
            "registry" => Some("Get available LLM services and models"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_names() {
        let method = ConeMethod::List;
        assert_eq!(method.name(), "list");
    }

    #[test]
    fn test_serialize() {
        let method = ConeMethod::List;
        let json = serde_json::to_string(&method).unwrap();
        assert!(json.contains("list"));
    }

    #[test]
    fn test_schema_has_uuid_format() {
        let schema = ConeMethod::schema();
        let schema_str = serde_json::to_string_pretty(&schema).unwrap();

        // Verify uuid format is present
        assert!(schema_str.contains("\"format\": \"uuid\""),
            "Schema should contain format: uuid");
    }

    #[test]
    fn test_schema_has_required_fields() {
        let schema = ConeMethod::schema();
        let schema_str = serde_json::to_string(&schema).unwrap();

        // Chat method should have cone_id and prompt as required
        // The schema structure uses oneOf, so we check the overall structure
        assert!(schema_str.contains("\"required\""),
            "Schema should contain required arrays");
    }

    #[test]
    fn test_schema_has_descriptions() {
        let schema = ConeMethod::schema();
        let schema_str = serde_json::to_string(&schema).unwrap();

        // Doc comments should become descriptions
        assert!(schema_str.contains("\"description\""),
            "Schema should contain descriptions from doc comments");
    }
}

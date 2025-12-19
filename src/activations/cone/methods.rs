/// Method definitions for the Cone plugin using JSON Schema
///
/// This provides type-safe method definitions with automatic schema generation
/// for documentation and validation. By using `uuid::Uuid` directly and doc comments,
/// schemars automatically generates format: "uuid", descriptions, and required arrays.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifier for a cone - either by name or UUID
///
/// Tagged enum with two variants:
/// - `{"by_name": {"name": "assistant"}}` - lookup by name (supports partial matching)
/// - `{"by_id": {"id": "550e8400-e29b-41d4-a716-446655440000"}}` - lookup by UUID
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConeIdentifier {
    /// Lookup cone by its human-readable name
    ByName {
        /// Cone name (supports partial matching, e.g., "assistant" or "assistant#550e")
        name: String
    },
    /// Lookup cone by its UUID
    ById {
        /// Cone UUID
        id: Uuid
    },
}

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

    /// Get cone configuration by name or ID
    Get {
        /// Cone identifier (provide either name or id)
        identifier: ConeIdentifier,
    },

    /// List all cones
    List,

    /// Delete a cone (associated tree is preserved)
    Delete {
        /// Cone identifier (provide either name or id)
        identifier: ConeIdentifier,
    },

    /// Chat with a cone - appends prompt to context, calls LLM, advances head
    Chat {
        /// Cone identifier (provide either name or id)
        identifier: ConeIdentifier,

        /// User message / prompt
        prompt: String,
    },

    /// Move cone's canonical head to a different node in the tree
    SetHead {
        /// Cone identifier (provide either name or id)
        identifier: ConeIdentifier,

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

impl crate::plexus::MethodEnumSchema for ConeMethod {
    fn method_names() -> &'static [&'static str] {
        &["create", "get", "list", "delete", "chat", "set_head", "registry"]
    }

    fn schema_with_consts() -> serde_json::Value {
        use schemars::JsonSchema;
        let schema = Self::json_schema(&mut schemars::SchemaGenerator::default());
        let mut value = serde_json::to_value(schema).expect("Schema should serialize");
        let method_names = Self::method_names();

        if let Some(obj) = value.as_object_mut() {
            if let Some(one_of) = obj.get_mut("oneOf") {
                if let Some(variants) = one_of.as_array_mut() {
                    for (i, variant) in variants.iter_mut().enumerate() {
                        if let Some(variant_obj) = variant.as_object_mut() {
                            if let Some(props) = variant_obj.get_mut("properties") {
                                if let Some(props_obj) = props.as_object_mut() {
                                    if let Some(method_prop) = props_obj.get_mut("method") {
                                        if let Some(method_obj) = method_prop.as_object_mut() {
                                            method_obj.remove("type");
                                            if let Some(name) = method_names.get(i) {
                                                method_obj.insert(
                                                    "const".to_string(),
                                                    serde_json::Value::String(name.to_string()),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        value
    }
}

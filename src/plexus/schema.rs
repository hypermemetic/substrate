/// JSON Schema types with strong typing
///
/// This module provides strongly-typed JSON Schema structures that plugins
/// use to describe their methods and parameters.
///
/// Schema generation is fully automatic via schemars. By using proper types
/// (uuid::Uuid instead of String) and doc comments, schemars generates complete
/// schemas with format annotations, descriptions, and required arrays.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A complete JSON Schema with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    /// The JSON Schema specification version
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none", default)]
    pub schema_version: Option<String>,

    /// Title of the schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description of what this schema represents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The schema type (typically "object" for root, can be string or array)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<serde_json::Value>,

    /// Properties for object types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, SchemaProperty>>,

    /// Required properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Enum variants (for discriminated unions)
    #[serde(rename = "oneOf", skip_serializing_if = "Option::is_none")]
    pub one_of: Option<Vec<Schema>>,

    /// Schema definitions (for $defs or definitions)
    #[serde(rename = "$defs", skip_serializing_if = "Option::is_none")]
    pub defs: Option<HashMap<String, serde_json::Value>>,

    /// Any additional schema properties
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// Schema type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SchemaType {
    Object,
    Array,
    String,
    Number,
    Integer,
    Boolean,
    Null,
}

/// A property definition in a schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaProperty {
    /// The type of this property (can be a single type or array of types for nullable)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub property_type: Option<serde_json::Value>,

    /// Description of this property
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Format hint (e.g., "uuid", "date-time", "email")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    /// For array types, the schema of items
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<SchemaProperty>>,

    /// For object types, nested properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, SchemaProperty>>,

    /// Required properties (for object types)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Default value for this property
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,

    /// Enum values if this is an enum
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<serde_json::Value>>,

    /// Reference to another schema definition
    #[serde(rename = "$ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// Any additional property metadata
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

impl Schema {
    /// Create a new schema with basic metadata
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            schema_version: Some("http://json-schema.org/draft-07/schema#".to_string()),
            title: Some(title.into()),
            description: Some(description.into()),
            schema_type: None,
            properties: None,
            required: None,
            one_of: None,
            defs: None,
            additional: HashMap::new(),
        }
    }

    /// Create an object schema
    pub fn object() -> Self {
        Self {
            schema_version: Some("http://json-schema.org/draft-07/schema#".to_string()),
            title: None,
            description: None,
            schema_type: Some(serde_json::json!("object")),
            properties: Some(HashMap::new()),
            required: None,
            one_of: None,
            defs: None,
            additional: HashMap::new(),
        }
    }

    /// Add a property to this schema
    pub fn with_property(mut self, name: impl Into<String>, property: SchemaProperty) -> Self {
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name.into(), property);
        self
    }

    /// Mark a property as required
    pub fn with_required(mut self, name: impl Into<String>) -> Self {
        self.required
            .get_or_insert_with(Vec::new)
            .push(name.into());
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl SchemaProperty {
    /// Create a string property
    pub fn string() -> Self {
        Self {
            property_type: Some(serde_json::json!("string")),
            description: None,
            format: None,
            items: None,
            properties: None,
            required: None,
            default: None,
            enum_values: None,
            reference: None,
            additional: HashMap::new(),
        }
    }

    /// Create a UUID property (string with format)
    pub fn uuid() -> Self {
        Self {
            property_type: Some(serde_json::json!("string")),
            description: None,
            format: Some("uuid".to_string()),
            items: None,
            properties: None,
            required: None,
            default: None,
            enum_values: None,
            reference: None,
            additional: HashMap::new(),
        }
    }

    /// Create an integer property
    pub fn integer() -> Self {
        Self {
            property_type: Some(serde_json::json!("integer")),
            description: None,
            format: None,
            items: None,
            properties: None,
            required: None,
            default: None,
            enum_values: None,
            reference: None,
            additional: HashMap::new(),
        }
    }

    /// Create an object property
    pub fn object() -> Self {
        Self {
            property_type: Some(serde_json::json!("object")),
            description: None,
            format: None,
            items: None,
            properties: Some(HashMap::new()),
            required: None,
            default: None,
            enum_values: None,
            reference: None,
            additional: HashMap::new(),
        }
    }

    /// Create an array property
    pub fn array(items: SchemaProperty) -> Self {
        Self {
            property_type: Some(serde_json::json!("array")),
            description: None,
            format: None,
            items: Some(Box::new(items)),
            properties: None,
            required: None,
            default: None,
            enum_values: None,
            reference: None,
            additional: HashMap::new(),
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add a default value
    pub fn with_default(mut self, default: serde_json::Value) -> Self {
        self.default = Some(default);
        self
    }

    /// Add nested properties (for object types)
    pub fn with_property(mut self, name: impl Into<String>, property: SchemaProperty) -> Self {
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name.into(), property);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creation() {
        let schema = Schema::object()
            .with_property("id", SchemaProperty::uuid().with_description("The unique identifier"))
            .with_property("name", SchemaProperty::string().with_description("The name"))
            .with_required("id");

        assert_eq!(schema.schema_type, Some(serde_json::json!("object")));
        assert!(schema.properties.is_some());
        assert_eq!(schema.required, Some(vec!["id".to_string()]));
    }

    #[test]
    fn test_serialization() {
        let schema = Schema::object()
            .with_property("id", SchemaProperty::uuid());

        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("uuid"));
    }
}

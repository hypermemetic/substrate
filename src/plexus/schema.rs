/// JSON Schema types with strong typing
///
/// This module provides strongly-typed JSON Schema structures that plugins
/// use to describe their methods and parameters.
///
/// Schema generation is fully automatic via schemars. By using proper types
/// (uuid::Uuid instead of String) and doc comments, schemars generates complete
/// schemas with format annotations, descriptions, and required arrays.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Plugin Schema (Recursive)
// ============================================================================

/// A plugin's complete schema, supporting recursive nesting for hubs.
///
/// This is the core type for the recursive plugin schema system:
/// - Leaf plugins have `children = None`
/// - Hub plugins have `children = Some([...])`
///
/// Category-theoretically: `Plugin ≅ μX. Methods × (1 + List(X))`
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PluginSchema {
    /// The plugin's namespace (e.g., "echo", "plexus")
    pub namespace: String,

    /// The plugin's version (e.g., "1.0.0")
    pub version: String,

    /// Human-readable description of the plugin
    pub description: String,

    /// Methods exposed by this plugin
    pub methods: Vec<MethodSchema>,

    /// Child plugins (None = leaf plugin, Some = hub plugin)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<PluginSchema>>,
}

/// Schema for a single method exposed by a plugin
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MethodSchema {
    /// Method name (e.g., "echo", "check")
    pub name: String,

    /// Human-readable description of what this method does
    pub description: String,

    /// JSON Schema for the method's parameters (None if no params)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<schemars::Schema>,

    /// JSON Schema for the method's return type (None if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returns: Option<schemars::Schema>,
}

impl PluginSchema {
    /// Create a new leaf plugin schema (no children)
    pub fn leaf(
        namespace: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        methods: Vec<MethodSchema>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            version: version.into(),
            description: description.into(),
            methods,
            children: None,
        }
    }

    /// Create a new hub plugin schema (with children)
    pub fn hub(
        namespace: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        methods: Vec<MethodSchema>,
        children: Vec<PluginSchema>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            version: version.into(),
            description: description.into(),
            methods,
            children: Some(children),
        }
    }

    /// Check if this is a hub (has children)
    pub fn is_hub(&self) -> bool {
        self.children.is_some()
    }

    /// Check if this is a leaf (no children)
    pub fn is_leaf(&self) -> bool {
        self.children.is_none()
    }
}

impl MethodSchema {
    /// Create a new method schema with just name and description
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            params: None,
            returns: None,
        }
    }

    /// Add parameter schema
    pub fn with_params(mut self, params: schemars::Schema) -> Self {
        self.params = Some(params);
        self
    }

    /// Add return type schema
    pub fn with_returns(mut self, returns: schemars::Schema) -> Self {
        self.returns = Some(returns);
        self
    }
}

// ============================================================================
// JSON Schema Types
// ============================================================================

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

    /// Extract a single method's schema from the oneOf array
    ///
    /// Searches the oneOf variants for a method matching the given name.
    /// Returns the variant schema if found, None otherwise.
    pub fn get_method_schema(&self, method_name: &str) -> Option<Schema> {
        let variants = self.one_of.as_ref()?;

        for variant in variants {
            // Check if this variant has a "method" property with const or enum
            if let Some(props) = &variant.properties {
                if let Some(method_prop) = props.get("method") {
                    // Try "const" first (schemars uses this for literal values)
                    if let Some(const_val) = method_prop.additional.get("const") {
                        if const_val.as_str() == Some(method_name) {
                            return Some(variant.clone());
                        }
                    }
                    // Fall back to enum_values
                    if let Some(enum_vals) = &method_prop.enum_values {
                        if enum_vals.first().and_then(|v| v.as_str()) == Some(method_name) {
                            return Some(variant.clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// List all method names from the oneOf array
    pub fn list_methods(&self) -> Vec<String> {
        let Some(variants) = &self.one_of else {
            return Vec::new();
        };

        variants
            .iter()
            .filter_map(|variant| {
                let props = variant.properties.as_ref()?;
                let method_prop = props.get("method")?;

                // Try "const" first
                if let Some(const_val) = method_prop.additional.get("const") {
                    return const_val.as_str().map(String::from);
                }
                // Fall back to enum_values
                method_prop
                    .enum_values
                    .as_ref()?
                    .first()?
                    .as_str()
                    .map(String::from)
            })
            .collect()
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

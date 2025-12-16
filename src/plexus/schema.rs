/// JSON Schema types with strong typing
///
/// This module provides strongly-typed JSON Schema structures that plugins
/// use to describe their methods and parameters.
///
/// ## Enrichment Architecture
///
/// The enrichment process works as follows:
/// 1. Auto-generate base JSON schema (may skip complex types like UUID)
/// 2. Parse schema into method enum variants
/// 3. Call `Describe::describe()` on each variant to get enrichment data
/// 4. Rebuild enriched schema with additional type information

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

/// Field enrichment data
///
/// Provides additional type information for a field that couldn't be
/// auto-derived from the base schema.
#[derive(Debug, Clone)]
pub struct FieldEnrichment {
    /// Field name
    pub name: String,

    /// Optional format annotation (e.g., "uuid", "date-time")
    pub format: Option<String>,

    /// Optional enhanced description
    pub description: Option<String>,

    /// Whether this field is required
    pub required: bool,
}

impl FieldEnrichment {
    /// Create a UUID field enrichment
    pub fn uuid(name: impl Into<String>, description: impl Into<String>, required: bool) -> Self {
        Self {
            name: name.into(),
            format: Some("uuid".to_string()),
            description: Some(description.into()),
            required,
        }
    }

    /// Create a generic field enrichment with format
    pub fn with_format(name: impl Into<String>, format: impl Into<String>, required: bool) -> Self {
        Self {
            name: name.into(),
            format: Some(format.into()),
            description: None,
            required,
        }
    }
}

/// Method enrichment data
///
/// Contains all enrichment information for a single method variant.
#[derive(Debug, Clone)]
pub struct MethodEnrichment {
    /// Method name (should match the enum variant name in snake_case)
    pub method_name: String,

    /// Field enrichments for this method's parameters
    pub fields: Vec<FieldEnrichment>,
}

/// Trait for providing schema enrichment data
///
/// Method enums implement this trait to provide additional type information
/// that couldn't be automatically derived during schema generation.
///
/// The default implementation returns `None`, meaning no enrichment is needed.
pub trait Describe {
    /// Provide enrichment data for this method variant
    ///
    /// Returns `None` if no enrichment is needed (all types were auto-derived).
    /// Returns `Some(MethodEnrichment)` to add format annotations and descriptions.
    fn describe(&self) -> Option<MethodEnrichment> {
        None
    }
}

/// Strongly-typed accessor for schema variant structure
///
/// Provides safe access to the nested structure of a discriminated union variant
/// in a JSON Schema, specifically for enum-based method definitions.
pub struct SchemaVariant<'a> {
    schema: &'a mut Schema,
}

impl<'a> SchemaVariant<'a> {
    /// Create a new variant accessor
    pub fn new(schema: &'a mut Schema) -> Self {
        Self { schema }
    }

    /// Get the method name from this variant
    pub fn method_name(&self) -> Option<&str> {
        let method_prop = self.schema
            .properties
            .as_ref()?
            .get("method")?;

        // Try "const" first (used by schemars for literal values)
        if let Some(const_val) = method_prop.additional.get("const") {
            return const_val.as_str();
        }

        // Fall back to enum_values
        method_prop.enum_values
            .as_ref()?
            .first()?
            .as_str()
    }

    /// Get mutable access to the params properties
    pub fn params_properties_mut(&mut self) -> Option<&mut HashMap<String, SchemaProperty>> {
        self.schema
            .properties
            .as_mut()?
            .get_mut("params")?
            .properties
            .as_mut()
    }

    /// Get mutable access to the params SchemaProperty itself
    pub fn params_mut(&mut self) -> Option<&mut SchemaProperty> {
        self.schema
            .properties
            .as_mut()?
            .get_mut("params")
    }

    /// Apply field enrichments to this variant's params
    pub fn apply_enrichments(&mut self, enrichment: &MethodEnrichment) {
        // Collect required field names
        let required_fields: Vec<String> = enrichment.fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name.clone())
            .collect();

        // Apply format and description enrichments to individual fields
        if let Some(param_props) = self.params_properties_mut() {
            for field in &enrichment.fields {
                if let Some(prop) = param_props.get_mut(&field.name) {
                    // Apply format annotation
                    if let Some(format) = &field.format {
                        prop.format = Some(format.clone());
                    }
                    // Apply description
                    if let Some(desc) = &field.description {
                        prop.description = Some(desc.clone());
                    }
                }
            }
        }

        // Set the required array on the params object itself
        if !required_fields.is_empty() {
            if let Some(params) = self.params_mut() {
                params.required = Some(required_fields);
            }
        }
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

    #[test]
    fn test_field_enrichment() {
        let enrichment = FieldEnrichment::uuid("tree_id", "UUID of the tree", true);
        assert_eq!(enrichment.name, "tree_id");
        assert_eq!(enrichment.format, Some("uuid".to_string()));
        assert!(enrichment.required);
    }

    #[test]
    fn test_apply_enrichments_sets_required_on_params() {
        // Create a schema mimicking what schemars generates for a method variant
        // with params containing tree_id and content fields
        let mut schema = Schema {
            schema_version: None,
            title: None,
            description: None,
            schema_type: Some(serde_json::json!("object")),
            properties: Some({
                let mut props = HashMap::new();
                props.insert("method".to_string(), SchemaProperty {
                    property_type: None,
                    description: None,
                    format: None,
                    items: None,
                    properties: None,
                    required: None,
                    default: None,
                    enum_values: None,
                    reference: None,
                    additional: {
                        let mut m = HashMap::new();
                        m.insert("const".to_string(), serde_json::json!("node_create_text"));
                        m
                    },
                });
                props.insert("params".to_string(), SchemaProperty {
                    property_type: Some(serde_json::json!("object")),
                    description: None,
                    format: None,
                    items: None,
                    properties: Some({
                        let mut p = HashMap::new();
                        p.insert("tree_id".to_string(), SchemaProperty::string());
                        p.insert("content".to_string(), SchemaProperty::string());
                        p.insert("parent".to_string(), SchemaProperty::string());
                        p
                    }),
                    required: None, // Initially no required field
                    default: None,
                    enum_values: None,
                    reference: None,
                    additional: HashMap::new(),
                });
                props
            }),
            required: Some(vec!["method".to_string(), "params".to_string()]),
            one_of: None,
            defs: None,
            additional: HashMap::new(),
        };

        // Create enrichment with required fields
        let enrichment = MethodEnrichment {
            method_name: "node_create_text".to_string(),
            fields: vec![
                FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                FieldEnrichment::uuid("parent", "UUID of the parent node", false), // optional
                FieldEnrichment {
                    name: "content".to_string(),
                    format: None,
                    description: Some("Text content".to_string()),
                    required: true,
                },
            ],
        };

        // Apply enrichments
        let mut variant = SchemaVariant::new(&mut schema);
        variant.apply_enrichments(&enrichment);

        // Verify required is set on params
        let params = schema.properties.as_ref().unwrap().get("params").unwrap();
        assert!(params.required.is_some(), "params.required should be set");

        let required = params.required.as_ref().unwrap();
        assert!(required.contains(&"tree_id".to_string()), "tree_id should be required");
        assert!(required.contains(&"content".to_string()), "content should be required");
        assert!(!required.contains(&"parent".to_string()), "parent should NOT be required");
        assert_eq!(required.len(), 2, "Should have exactly 2 required fields");

        // Verify format was applied to tree_id
        let tree_id_prop = params.properties.as_ref().unwrap().get("tree_id").unwrap();
        assert_eq!(tree_id_prop.format, Some("uuid".to_string()));
    }
}

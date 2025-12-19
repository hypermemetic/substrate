/// Method definitions for the Health activation using JSON Schema
///
/// This provides type-safe method definitions with automatic schema generation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// All available methods in the Health activation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum HealthMethod {
    /// Check the health status of the hub
    Check,
}

impl HealthMethod {
    /// Get the method name as a string
    pub fn name(&self) -> &'static str {
        match self {
            HealthMethod::Check => "check",
        }
    }

    /// Get all available method names
    pub fn all_names() -> Vec<&'static str> {
        vec!["check"]
    }

    /// Get the JSON schema for all Health methods
    pub fn schema() -> serde_json::Value {
        let schema = schemars::schema_for!(HealthMethod);
        serde_json::to_value(schema).unwrap()
    }

    /// Get a human-readable description of a method
    pub fn description(method_name: &str) -> Option<&'static str> {
        match method_name {
            "check" => Some("Check the health status of the hub and return uptime"),
            _ => None,
        }
    }
}

impl crate::plexus::MethodEnumSchema for HealthMethod {
    fn method_names() -> &'static [&'static str] {
        &["check"]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_names() {
        let method = HealthMethod::Check;
        assert_eq!(method.name(), "check");
    }

    #[test]
    fn test_schema_generation() {
        let schema = HealthMethod::schema();
        let schema_str = serde_json::to_string(&schema).unwrap();
        // Should have the method check
        assert!(schema_str.contains("check"));
    }
}

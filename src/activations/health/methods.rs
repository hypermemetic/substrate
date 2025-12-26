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
        // schemars 1.1+ generates const values for adjacently tagged enums
        serde_json::to_value(schemars::schema_for!(HealthMethod)).expect("Schema should serialize")
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

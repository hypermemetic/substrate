/// Method definitions for the Bash activation using JSON Schema
///
/// This provides type-safe method definitions with automatic schema generation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// All available methods in the Bash activation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum BashMethod {
    /// Execute a bash command and stream stdout, stderr, and exit code
    Execute {
        /// The bash command to execute
        command: String,
    },
}

impl BashMethod {
    /// Get the method name as a string
    pub fn name(&self) -> &'static str {
        match self {
            BashMethod::Execute { .. } => "execute",
        }
    }

    /// Get all available method names
    pub fn all_names() -> Vec<&'static str> {
        vec!["execute"]
    }

    /// Get the JSON schema for all Bash methods
    pub fn schema() -> serde_json::Value {
        let schema = schemars::schema_for!(BashMethod);
        serde_json::to_value(schema).unwrap()
    }

    /// Get a human-readable description of a method
    pub fn description(method_name: &str) -> Option<&'static str> {
        match method_name {
            "execute" => Some("Execute a bash command and stream stdout, stderr, and exit code"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_names() {
        let method = BashMethod::Execute {
            command: "echo test".to_string(),
        };
        assert_eq!(method.name(), "execute");
    }

    #[test]
    fn test_schema_has_required_fields() {
        let schema = BashMethod::schema();
        let schema_str = serde_json::to_string(&schema).unwrap();
        assert!(schema_str.contains("\"required\""));
    }

    #[test]
    fn test_schema_has_descriptions() {
        let schema = BashMethod::schema();
        let schema_str = serde_json::to_string(&schema).unwrap();
        assert!(schema_str.contains("\"description\""));
    }
}

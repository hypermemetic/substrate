//! MCP Schema Transformation
//!
//! Transforms Plexus activation schemas into MCP tool format.

use serde_json::{json, Value};

use super::types::McpTool;
use crate::plexus::{ActivationFullSchema, MethodSchemaInfo};

/// Transform Plexus activation schemas to MCP tools list
///
/// Each activation method becomes an MCP tool with name format "namespace.method"
/// Per MCP 2025-06-18 spec, description is required, so we provide a default if empty.
pub fn schemas_to_mcp_tools(schemas: &[ActivationFullSchema]) -> Vec<McpTool> {
    schemas
        .iter()
        .flat_map(|activation| {
            activation.methods.iter().map(move |method| {
                // Description is required per MCP 2025-06-18 spec
                let description = if method.description.is_empty() {
                    format!(
                        "{} method from {} activation",
                        method.name, activation.namespace
                    )
                } else {
                    method.description.clone()
                };

                McpTool {
                    name: format!("{}.{}", activation.namespace, method.name),
                    description,
                    input_schema: build_input_schema(method),
                }
            })
        })
        .collect()
}

/// Build JSON Schema for method input from Plexus schema info
fn build_input_schema(method: &MethodSchemaInfo) -> Value {
    // If we have a params schema from Plexus, use it
    if let Some(ref params) = method.params {
        // The params schema from Plexus is already a JSON Schema
        return serde_json::to_value(params).unwrap_or_else(|_| empty_object_schema());
    }

    // Otherwise return an empty object schema
    empty_object_schema()
}

/// Returns an empty object JSON Schema
fn empty_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schemas_to_mcp_tools_empty() {
        let schemas: Vec<ActivationFullSchema> = vec![];
        let tools = schemas_to_mcp_tools(&schemas);
        assert!(tools.is_empty());
    }

    #[test]
    fn test_schemas_to_mcp_tools_basic() {
        let schemas = vec![ActivationFullSchema {
            namespace: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test activation".to_string(),
            methods: vec![
                MethodSchemaInfo {
                    name: "method1".to_string(),
                    description: "First method".to_string(),
                    params: None,
                    returns: None,
                },
                MethodSchemaInfo {
                    name: "method2".to_string(),
                    description: "".to_string(),
                    params: None,
                    returns: None,
                },
            ],
        }];

        let tools = schemas_to_mcp_tools(&schemas);

        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "test.method1");
        assert_eq!(tools[0].description, "First method");
        assert_eq!(tools[1].name, "test.method2");
        // Empty description gets a default per MCP 2025-06-18 spec
        assert_eq!(tools[1].description, "method2 method from test activation");
    }

    #[test]
    fn test_schemas_to_mcp_tools_multiple_activations() {
        let schemas = vec![
            ActivationFullSchema {
                namespace: "alpha".to_string(),
                version: "1.0.0".to_string(),
                description: "".to_string(),
                methods: vec![MethodSchemaInfo {
                    name: "one".to_string(),
                    description: "".to_string(),
                    params: None,
                    returns: None,
                }],
            },
            ActivationFullSchema {
                namespace: "beta".to_string(),
                version: "1.0.0".to_string(),
                description: "".to_string(),
                methods: vec![MethodSchemaInfo {
                    name: "two".to_string(),
                    description: "".to_string(),
                    params: None,
                    returns: None,
                }],
            },
        ];

        let tools = schemas_to_mcp_tools(&schemas);

        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "alpha.one"));
        assert!(tools.iter().any(|t| t.name == "beta.two"));
    }

    #[test]
    fn test_input_schema_empty_when_no_params() {
        let method = MethodSchemaInfo {
            name: "test".to_string(),
            description: "".to_string(),
            params: None,
            returns: None,
        };

        let schema = build_input_schema(&method);

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
    }
}

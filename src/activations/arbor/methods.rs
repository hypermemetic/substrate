/// Method definitions for the Arbor plugin using JSON Schema
///
/// This provides type-safe method definitions with automatic schema generation
/// for documentation and validation. By using `uuid::Uuid` directly and doc comments,
/// schemars automatically generates format: "uuid", descriptions, and required arrays.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// All available methods in the Arbor plugin
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ArborMethod {
    /// Create a new conversation tree
    TreeCreate {
        /// Optional tree-level metadata (name, description, etc.)
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,

        /// Owner identifier (default: "system")
        #[serde(default = "default_owner")]
        owner_id: String,
    },

    /// Get a complete tree with all nodes
    TreeGet {
        /// UUID of the tree to retrieve
        tree_id: Uuid,
    },

    /// Get a lightweight tree structure (nodes without data)
    TreeGetSkeleton {
        /// UUID of the tree to retrieve
        tree_id: Uuid,
    },

    /// List all active trees
    TreeList,

    /// Update tree metadata
    TreeUpdateMetadata {
        /// UUID of the tree to update
        tree_id: Uuid,

        /// New metadata to set
        metadata: serde_json::Value,
    },

    /// Claim ownership of a tree (increment reference count)
    TreeClaim {
        /// UUID of the tree to claim
        tree_id: Uuid,

        /// Owner identifier
        owner_id: String,

        /// Number of references to add (default: 1)
        #[serde(default = "default_count")]
        count: i64,
    },

    /// Release ownership of a tree (decrement reference count)
    TreeRelease {
        /// UUID of the tree to release
        tree_id: Uuid,

        /// Owner identifier
        owner_id: String,

        /// Number of references to remove (default: 1)
        #[serde(default = "default_count")]
        count: i64,
    },

    /// Create a text node in a tree
    NodeCreateText {
        /// UUID of the tree
        tree_id: Uuid,

        /// Parent node ID (None for root-level)
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<Uuid>,

        /// Text content for the node
        content: String,

        /// Optional node metadata
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// Create an external node in a tree
    NodeCreateExternal {
        /// UUID of the tree
        tree_id: Uuid,

        /// Parent node ID (None for root-level)
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<Uuid>,

        /// Handle to external data
        handle: super::types::Handle,

        /// Optional node metadata
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// Get a node by ID
    NodeGet {
        /// UUID of the tree
        tree_id: Uuid,

        /// UUID of the node
        node_id: Uuid,
    },

    /// Get the children of a node
    NodeGetChildren {
        /// UUID of the tree
        tree_id: Uuid,

        /// UUID of the node
        node_id: Uuid,
    },

    /// Get the parent of a node
    NodeGetParent {
        /// UUID of the tree
        tree_id: Uuid,

        /// UUID of the node
        node_id: Uuid,
    },

    /// Get the path from root to a node
    NodeGetPath {
        /// UUID of the tree
        tree_id: Uuid,

        /// UUID of the node
        node_id: Uuid,
    },

    /// List all leaf nodes in a tree
    ContextListLeaves {
        /// UUID of the tree
        tree_id: Uuid,
    },

    /// Get the full path data from root to a node
    ContextGetPath {
        /// UUID of the tree
        tree_id: Uuid,

        /// UUID of the target node
        node_id: Uuid,
    },

    /// Get all external handles in the path to a node
    ContextGetHandles {
        /// UUID of the tree
        tree_id: Uuid,

        /// UUID of the target node
        node_id: Uuid,
    },

    /// List trees scheduled for deletion
    TreeListScheduled,

    /// List archived trees
    TreeListArchived,

    /// Render tree as text
    TreeRender {
        /// UUID of the tree to render
        tree_id: Uuid,
    },
}

fn default_owner() -> String {
    "system".to_string()
}

fn default_count() -> i64 {
    1
}

impl ArborMethod {
    /// Get the method name as a string
    pub fn name(&self) -> &'static str {
        match self {
            ArborMethod::TreeCreate { .. } => "tree_create",
            ArborMethod::TreeGet { .. } => "tree_get",
            ArborMethod::TreeGetSkeleton { .. } => "tree_get_skeleton",
            ArborMethod::TreeList => "tree_list",
            ArborMethod::TreeUpdateMetadata { .. } => "tree_update_metadata",
            ArborMethod::TreeClaim { .. } => "tree_claim",
            ArborMethod::TreeRelease { .. } => "tree_release",
            ArborMethod::NodeCreateText { .. } => "node_create_text",
            ArborMethod::NodeCreateExternal { .. } => "node_create_external",
            ArborMethod::NodeGet { .. } => "node_get",
            ArborMethod::NodeGetChildren { .. } => "node_get_children",
            ArborMethod::NodeGetParent { .. } => "node_get_parent",
            ArborMethod::NodeGetPath { .. } => "node_get_path",
            ArborMethod::ContextListLeaves { .. } => "context_list_leaves",
            ArborMethod::ContextGetPath { .. } => "context_get_path",
            ArborMethod::ContextGetHandles { .. } => "context_get_handles",
            ArborMethod::TreeListScheduled => "tree_list_scheduled",
            ArborMethod::TreeListArchived => "tree_list_archived",
            ArborMethod::TreeRender { .. } => "tree_render",
        }
    }

    /// Get all available method names
    pub fn all_names() -> Vec<&'static str> {
        vec![
            "tree_create",
            "tree_get",
            "tree_get_skeleton",
            "tree_list",
            "tree_update_metadata",
            "tree_claim",
            "tree_release",
            "node_create_text",
            "node_create_external",
            "node_get",
            "node_get_children",
            "node_get_parent",
            "node_get_path",
            "context_list_leaves",
            "context_get_path",
            "context_get_handles",
            "tree_list_scheduled",
            "tree_list_archived",
            "tree_render",
        ]
    }

    /// Get the JSON schema for all Arbor methods
    pub fn schema() -> serde_json::Value {
        let schema = schemars::schema_for!(ArborMethod);
        serde_json::to_value(schema).unwrap()
    }

    /// Get a human-readable description of a method
    pub fn description(method_name: &str) -> Option<&'static str> {
        match method_name {
            "tree_create" => Some("Create a new conversation tree with optional metadata"),
            "tree_get" => Some("Retrieve a complete tree with all nodes and their data"),
            "tree_get_skeleton" => Some("Get lightweight tree structure without node data"),
            "tree_list" => Some("List all active trees in the system"),
            "tree_update_metadata" => Some("Update the metadata of an existing tree"),
            "tree_claim" => Some("Claim ownership of a tree (increment reference count)"),
            "tree_release" => Some("Release ownership of a tree (decrement reference count)"),
            "node_create_text" => Some("Create a text node in a tree"),
            "node_create_external" => Some("Create a node with an external data handle"),
            "node_get" => Some("Get a specific node by ID"),
            "node_get_children" => Some("Get the children of a node"),
            "node_get_parent" => Some("Get the parent of a node"),
            "node_get_path" => Some("Get the path from root to a specific node"),
            "context_list_leaves" => Some("List all leaf nodes in a tree"),
            "context_get_path" => Some("Get the full path data from root to a node"),
            "context_get_handles" => Some("Get all external handles in the path to a node"),
            "tree_list_scheduled" => Some("List trees scheduled for deletion"),
            "tree_list_archived" => Some("List archived trees"),
            "tree_render" => Some("Render tree as text visualization"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_names() {
        let method = ArborMethod::TreeCreate {
            metadata: None,
            owner_id: "test".to_string(),
        };
        assert_eq!(method.name(), "tree_create");
    }

    #[test]
    fn test_serialize() {
        let method = ArborMethod::TreeList;
        let json = serde_json::to_string(&method).unwrap();
        assert!(json.contains("tree_list"));
    }

    #[test]
    fn test_schema_has_uuid_format() {
        let schema = ArborMethod::schema();
        let schema_str = serde_json::to_string_pretty(&schema).unwrap();

        // Verify uuid format is present
        assert!(schema_str.contains("\"format\": \"uuid\""),
            "Schema should contain format: uuid");
    }

    #[test]
    fn test_schema_has_required_fields() {
        let schema = ArborMethod::schema();
        let schema_str = serde_json::to_string(&schema).unwrap();

        // Methods should have required arrays
        assert!(schema_str.contains("\"required\""),
            "Schema should contain required arrays");
    }

    #[test]
    fn test_schema_has_descriptions() {
        let schema = ArborMethod::schema();
        let schema_str = serde_json::to_string(&schema).unwrap();

        // Doc comments should become descriptions
        assert!(schema_str.contains("\"description\""),
            "Schema should contain descriptions from doc comments");
    }
}

impl crate::plexus::MethodEnumSchema for ArborMethod {
    fn method_names() -> &'static [&'static str] {
        &[
            "tree_create",
            "tree_get",
            "tree_get_skeleton",
            "tree_list",
            "tree_update_metadata",
            "tree_claim",
            "tree_release",
            "node_create_text",
            "node_create_external",
            "node_get",
            "node_get_children",
            "node_get_parent",
            "node_get_path",
            "context_list_leaves",
            "context_get_path",
            "context_get_handles",
            "tree_list_scheduled",
            "tree_list_archived",
            "tree_render",
        ]
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

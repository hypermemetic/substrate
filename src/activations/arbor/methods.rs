/// Method definitions for the Arbor plugin using JSON Schema
///
/// This provides type-safe method definitions with automatic schema generation
/// for documentation and validation.

use crate::plexus::{Describe, FieldEnrichment, MethodEnrichment};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
        /// UUID of the tree to retrieve (as string)
        tree_id: String,
    },

    /// Get a lightweight tree structure (nodes without data)
    TreeGetSkeleton {
        /// UUID of the tree to retrieve (as string)
        tree_id: String,
    },

    /// List all active trees
    TreeList,

    /// Update tree metadata
    TreeUpdateMetadata {
        /// UUID of the tree to update (as string)
        tree_id: String,

        /// New metadata to set
        metadata: serde_json::Value,
    },

    /// Claim ownership of a tree (increment reference count)
    TreeClaim {
        /// UUID of the tree to claim (as string)
        tree_id: String,

        /// Owner identifier
        owner_id: String,

        /// Number of references to add (default: 1)
        #[serde(default = "default_count")]
        count: i64,
    },

    /// Release ownership of a tree (decrement reference count)
    TreeRelease {
        /// UUID of the tree to release (as string)
        tree_id: String,

        /// Owner identifier
        owner_id: String,

        /// Number of references to remove (default: 1)
        #[serde(default = "default_count")]
        count: i64,
    },

    /// Create a text node in a tree
    NodeCreateText {
        /// UUID of the tree (as string)
        tree_id: String,

        /// Parent node ID (None for root-level, as string)
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<String>,

        /// Text content for the node
        content: String,

        /// Optional node metadata
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// Create an external node in a tree
    NodeCreateExternal {
        /// UUID of the tree (as string)
        tree_id: String,

        /// Parent node ID (None for root-level, as string)
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<String>,

        /// Handle to external data
        handle: super::types::Handle,

        /// Optional node metadata
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// Get a node by ID
    NodeGet {
        /// UUID of the tree (as string)
        tree_id: String,

        /// UUID of the node (as string)
        node_id: String,
    },

    /// Get the children of a node
    NodeGetChildren {
        /// UUID of the tree (as string)
        tree_id: String,

        /// UUID of the node (as string)
        node_id: String,
    },

    /// Get the parent of a node
    NodeGetParent {
        /// UUID of the tree (as string)
        tree_id: String,

        /// UUID of the node (as string)
        node_id: String,
    },

    /// Get the path from root to a node
    NodeGetPath {
        /// UUID of the tree (as string)
        tree_id: String,

        /// UUID of the node (as string)
        node_id: String,
    },

    /// List all leaf nodes in a tree
    ContextListLeaves {
        /// UUID of the tree (as string)
        tree_id: String,
    },

    /// Get the full path data from root to a node
    ContextGetPath {
        /// UUID of the tree (as string)
        tree_id: String,

        /// UUID of the target node (as string)
        node_id: String,
    },

    /// Get all external handles in the path to a node
    ContextGetHandles {
        /// UUID of the tree (as string)
        tree_id: String,

        /// UUID of the target node (as string)
        node_id: String,
    },

    /// List trees scheduled for deletion
    TreeListScheduled,

    /// List archived trees
    TreeListArchived,

    /// Render tree as text
    TreeRender {
        /// UUID of the tree to render (as string)
        tree_id: String,
    },
}

fn default_owner() -> String {
    "system".to_string()
}

fn default_count() -> i64 {
    1
}

impl ArborMethod {
    /// Get enrichment data for a method by name (type-level, no instance needed)
    pub fn describe_by_name(method_name: &str) -> Option<MethodEnrichment> {
        let fields = match method_name {
            // Methods with no UUID fields
            "tree_create" | "tree_list" | "tree_list_scheduled" | "tree_list_archived" => {
                return None; // No enrichment needed
            }

            // Methods with tree_id only
            "tree_get" | "tree_get_skeleton" | "tree_render" => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree", true)]
            }

            "tree_update_metadata" => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree to update", true)]
            }

            "tree_claim" => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree to claim", true)]
            }

            "tree_release" => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree to release", true)]
            }

            "context_list_leaves" => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree", true)]
            }

            // Methods with tree_id and node_id
            "node_get" | "node_get_children" | "node_get_parent" | "node_get_path" => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the node", true),
                ]
            }

            "context_get_path" | "context_get_handles" => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the target node", true),
                ]
            }

            // Methods with tree_id and optional parent
            "node_create_text" | "node_create_external" => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("parent", "UUID of the parent node (optional)", false),
                ]
            }

            _ => return None,
        };

        Some(MethodEnrichment {
            method_name: method_name.to_string(),
            fields,
        })
    }

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
}

/// Implement Describe trait for schema enrichment
impl Describe for ArborMethod {
    fn describe(&self) -> Option<MethodEnrichment> {
        let method_name = self.name().to_string();
        let fields = match self {
            // Methods with no UUID fields
            ArborMethod::TreeCreate { .. } | ArborMethod::TreeList |
            ArborMethod::TreeListScheduled | ArborMethod::TreeListArchived => {
                return None; // No enrichment needed
            }

            // Methods with tree_id only
            ArborMethod::TreeGet { .. } | ArborMethod::TreeGetSkeleton { .. } | ArborMethod::TreeRender { .. } => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree", true)]
            }

            ArborMethod::TreeUpdateMetadata { .. } => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree to update", true)]
            }

            ArborMethod::TreeClaim { .. } => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree to claim", true)]
            }

            ArborMethod::TreeRelease { .. } => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree to release", true)]
            }

            ArborMethod::ContextListLeaves { .. } => {
                vec![FieldEnrichment::uuid("tree_id", "UUID of the tree", true)]
            }

            // Methods with tree_id and node_id
            ArborMethod::NodeGet { .. } => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the node", true),
                ]
            }

            ArborMethod::NodeGetChildren { .. } => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the node", true),
                ]
            }

            ArborMethod::NodeGetParent { .. } => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the node", true),
                ]
            }

            ArborMethod::NodeGetPath { .. } => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the node", true),
                ]
            }

            ArborMethod::ContextGetPath { .. } => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the target node", true),
                ]
            }

            ArborMethod::ContextGetHandles { .. } => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("node_id", "UUID of the target node", true),
                ]
            }

            // Methods with tree_id and optional parent
            ArborMethod::NodeCreateText { .. } | ArborMethod::NodeCreateExternal { .. } => {
                vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                    FieldEnrichment::uuid("parent", "UUID of the parent node (optional)", false),
                ]
            }
        };

        Some(MethodEnrichment { method_name, fields })
    }
}

//! Session-Typed Method Definitions for Arbor
//!
//! This module demonstrates how Arbor methods can be defined using session types
//! for compile-time protocol verification and automatic schema extraction.
//!
//! # Design Pattern
//!
//! Each method is defined as:
//! 1. Input/Output types with `JsonSchema` for introspection
//! 2. A session type protocol describing the communication pattern
//! 3. A `Method` impl connecting the protocol to the method name
//!
//! # Example Usage
//!
//! ```ignore
//! use substrate::activations::arbor::typed_methods::TreeGetMethod;
//! use substrate::plexus::Method;
//!
//! // Get the full method schema at runtime
//! let schema = TreeGetMethod::schema();
//! println!("{}", serde_json::to_string_pretty(&schema).unwrap());
//! ```

use crate::plexus::{Method, MethodCollection, MethodSchema};
use dialectic::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Note: We define our own output types here that implement schemars 1.0 JsonSchema.
// The actual Arbor types (Tree, Node, etc.) use schemars 0.8 through cllient.
// In a full migration, we'd update all types to schemars 1.0.

// ============================================================================
// TreeGet - Simple request-response
// ============================================================================

/// Input for tree_get method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeGetInput {
    /// UUID of the tree to retrieve
    pub tree_id: Uuid,
}

/// Output for tree_get method - returns the full tree
/// Note: In production, this would reference the actual Tree type.
/// For now, we use a simplified representation for schema demonstration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeGetOutput {
    /// The tree ID
    pub id: Uuid,

    /// Root node ID
    pub root: Uuid,

    /// Creation timestamp (Unix seconds)
    pub created_at: i64,

    /// Last modified timestamp (Unix seconds)
    pub updated_at: i64,

    /// Optional tree-level metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Protocol: Client sends tree_id, server responds with tree data
pub type TreeGetProtocol = Session! {
    send TreeGetInput;
    recv TreeGetOutput;
};

/// TreeGet method definition
pub struct TreeGetMethod;

impl Method for TreeGetMethod {
    type Protocol = TreeGetProtocol;
    type ServerProtocol = <TreeGetProtocol as Session>::Dual;
    const NAME: &'static str = "tree_get";
    const DESCRIPTION: &'static str = "Get a complete tree with all nodes";
}

// ============================================================================
// TreeList - Simple request-response
// ============================================================================

/// Output for tree_list method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeListOutput {
    /// List of tree IDs
    pub tree_ids: Vec<Uuid>,
}

/// Protocol: Client sends request (empty), server responds with tree list
pub type TreeListProtocol = Session! {
    recv TreeListOutput;
};

/// TreeList method definition
pub struct TreeListMethod;

impl Method for TreeListMethod {
    type Protocol = TreeListProtocol;
    type ServerProtocol = <TreeListProtocol as Session>::Dual;
    const NAME: &'static str = "tree_list";
    const DESCRIPTION: &'static str = "List all active trees";
}

// ============================================================================
// TreeCreate - Request with optional params, single response
// ============================================================================

/// Input for tree_create method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeCreateInput {
    /// Optional tree-level metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Owner identifier (default: "system")
    #[serde(default = "default_owner")]
    pub owner_id: String,
}

fn default_owner() -> String {
    "system".to_string()
}

/// Output for tree_create method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeCreateOutput {
    /// The ID of the newly created tree
    pub tree_id: Uuid,
}

/// Protocol: Client sends create params, server responds with tree ID
pub type TreeCreateProtocol = Session! {
    send TreeCreateInput;
    recv TreeCreateOutput;
};

/// TreeCreate method definition
pub struct TreeCreateMethod;

impl Method for TreeCreateMethod {
    type Protocol = TreeCreateProtocol;
    type ServerProtocol = <TreeCreateProtocol as Session>::Dual;
    const NAME: &'static str = "tree_create";
    const DESCRIPTION: &'static str = "Create a new conversation tree";
}

// ============================================================================
// NodeCreateText - Create a text node
// ============================================================================

/// Input for node_create_text method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeCreateTextInput {
    /// UUID of the tree
    pub tree_id: Uuid,

    /// Parent node ID (None for root-level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<Uuid>,

    /// Text content for the node
    pub content: String,

    /// Optional node metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Output for node_create_text method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeCreateOutput {
    /// The tree containing the new node
    pub tree_id: Uuid,

    /// The ID of the newly created node
    pub node_id: Uuid,

    /// Parent node ID (if any)
    pub parent: Option<Uuid>,
}

/// Protocol: Client sends node params, server responds with node info
pub type NodeCreateTextProtocol = Session! {
    send NodeCreateTextInput;
    recv NodeCreateOutput;
};

/// NodeCreateText method definition
pub struct NodeCreateTextMethod;

impl Method for NodeCreateTextMethod {
    type Protocol = NodeCreateTextProtocol;
    type ServerProtocol = <NodeCreateTextProtocol as Session>::Dual;
    const NAME: &'static str = "node_create_text";
    const DESCRIPTION: &'static str = "Create a text node in a tree";
}

// ============================================================================
// ArborTypedMethods - Collection of all typed methods
// ============================================================================

/// Collection of all session-typed Arbor methods
pub struct ArborTypedMethods;

impl MethodCollection for ArborTypedMethods {
    fn schemas() -> Vec<MethodSchema> {
        vec![
            TreeGetMethod::schema(),
            TreeListMethod::schema(),
            TreeCreateMethod::schema(),
            NodeCreateTextMethod::schema(),
        ]
    }

    fn method_names() -> Vec<&'static str> {
        vec![
            TreeGetMethod::NAME,
            TreeListMethod::NAME,
            TreeCreateMethod::NAME,
            NodeCreateTextMethod::NAME,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexus::ProtocolSchema;

    #[test]
    fn test_tree_get_schema() {
        let schema = TreeGetMethod::schema();

        assert_eq!(schema.name, "tree_get");
        assert_eq!(schema.description, "Get a complete tree with all nodes");

        // Client protocol: Send -> Recv -> Done
        match &schema.protocol {
            ProtocolSchema::Send { then, .. } => match &**then {
                ProtocolSchema::Recv { then, .. } => {
                    assert!(matches!(&**then, ProtocolSchema::Done));
                }
                _ => panic!("Expected Recv after Send"),
            },
            _ => panic!("Expected Send first"),
        }

        // Server protocol (dual): Recv -> Send -> Done
        match &schema.server_protocol {
            ProtocolSchema::Recv { then, .. } => match &**then {
                ProtocolSchema::Send { then, .. } => {
                    assert!(matches!(&**then, ProtocolSchema::Done));
                }
                _ => panic!("Expected Send after Recv"),
            },
            _ => panic!("Expected Recv first in server protocol"),
        }
    }

    #[test]
    fn test_tree_list_schema() {
        let schema = TreeListMethod::schema();

        assert_eq!(schema.name, "tree_list");

        // Client protocol: Recv -> Done (no input needed)
        match &schema.protocol {
            ProtocolSchema::Recv { then, .. } => {
                assert!(matches!(&**then, ProtocolSchema::Done));
            }
            _ => panic!("Expected Recv first"),
        }
    }

    #[test]
    fn test_method_collection() {
        let schemas = ArborTypedMethods::schemas();
        assert_eq!(schemas.len(), 4);

        let names = ArborTypedMethods::method_names();
        assert_eq!(names, vec!["tree_get", "tree_list", "tree_create", "node_create_text"]);
    }

    #[test]
    fn test_schema_serialization() {
        let schema = TreeGetMethod::schema();
        let json = serde_json::to_string_pretty(&schema).unwrap();

        // Verify it contains expected structure
        assert!(json.contains("tree_get"));
        assert!(json.contains("protocol"));
        assert!(json.contains("server_protocol"));
        assert!(json.contains("TreeGetInput"));
        assert!(json.contains("TreeGetOutput"));
    }

    #[test]
    fn test_payload_schemas() {
        let schema = TreeGetMethod::schema();

        // Verify client send payload has tree_id
        if let ProtocolSchema::Send { payload, .. } = &schema.protocol {
            let props = payload.get("properties").expect("Should have properties");
            assert!(props.get("tree_id").is_some(), "Should have tree_id property");
        }
    }

    #[test]
    fn test_full_schema_output() {
        // This test prints the full schema for visual verification
        let schema = TreeGetMethod::schema();
        let json = serde_json::to_string_pretty(&schema).unwrap();
        println!("TreeGet Schema:\n{}", json);

        // Also test streaming-style method would work (future)
        let all_schemas = ArborTypedMethods::schemas();
        println!("\nAll Arbor Methods:");
        for s in &all_schemas {
            println!("  - {} ({})", s.name, s.description);
        }
    }
}

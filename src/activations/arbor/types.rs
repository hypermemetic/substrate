use crate::{
    plexus::{Provenance, types::PlexusStreamItem},
    plugin_system::types::ActivationStreamItem,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

// ============================================================================
// Core Types
// ============================================================================

/// Wrapper around UUID with proper parsing and serialization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArborId(Uuid);

impl ArborId {
    /// Create a new random UUID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a nil UUID (all zeros)
    pub fn nil() -> Self {
        Self(Uuid::nil())
    }

    /// Parse from string
    pub fn parse_str(s: &str) -> Result<Self, String> {
        Uuid::from_str(s)
            .map(Self)
            .map_err(|e| format!("Invalid UUID: {}", e))
    }

    /// Get the inner UUID
    pub fn inner(&self) -> &Uuid {
        &self.0
    }

    /// Convert to string
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for ArborId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ArborId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ArborId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<ArborId> for Uuid {
    fn from(id: ArborId) -> Self {
        id.0
    }
}

impl Serialize for ArborId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for ArborId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse_str(&s).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ArborId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "UUID".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Just use the String schema, which will work for UUID strings
        gen.subschema_for::<String>()
    }
}

/// Unique identifier for a tree
pub type TreeId = ArborId;

/// Unique identifier for a node within a tree
pub type NodeId = ArborId;

/// Handle pointing to external data with versioning
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Handle {
    /// Source system identifier (e.g., "postgres", "s3", "redis", "bash")
    pub source: String,

    /// Source system version (semantic version: "MAJOR.MINOR.PATCH")
    pub source_version: String,

    /// Identifier within that source system
    pub identifier: String,

    /// Optional metadata for the handle (e.g., content type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Node type discriminator
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum NodeType {
    /// Built-in text node (data stored in Arbor)
    #[serde(rename = "text")]
    Text { content: String },

    /// External data reference
    #[serde(rename = "external")]
    External { handle: Handle },
}

/// Resource state in deletion lifecycle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    /// Fully accessible and mutable (ref_count >= 1)
    Active,
    /// Marked for deletion, can be claimed (ref_count = 0)
    ScheduledDelete,
    /// Archived, read-only access only
    Archived,
}

impl ResourceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceState::Active => "active",
            ResourceState::ScheduledDelete => "scheduled_delete",
            ResourceState::Archived => "archived",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(ResourceState::Active),
            "scheduled_delete" => Some(ResourceState::ScheduledDelete),
            "archived" => Some(ResourceState::Archived),
            _ => None,
        }
    }
}

/// Reference counting information for a resource
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceRefs {
    /// Total reference count
    pub ref_count: i64,

    /// Who owns references (owner_id -> count)
    pub owners: HashMap<String, i64>,
}

/// A node in the conversation tree
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    /// Unique identifier for this node
    pub id: NodeId,

    /// Parent node (None for root)
    pub parent: Option<NodeId>,

    /// Child nodes (in order)
    pub children: Vec<NodeId>,

    /// Node data (handle or built-in)
    pub data: NodeType,

    /// Reference counting state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ResourceState>,

    /// Reference count information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refs: Option<ResourceRefs>,

    /// Scheduled deletion timestamp (Unix seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_deletion_at: Option<i64>,

    /// Archived timestamp (Unix seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<i64>,

    /// Creation timestamp (Unix seconds)
    pub created_at: i64,

    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A conversation tree
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tree {
    /// Unique identifier for this tree
    pub id: TreeId,

    /// Root node ID
    pub root: NodeId,

    /// All nodes in the tree (NodeId -> Node)
    pub nodes: HashMap<NodeId, Node>,

    /// Reference counting state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ResourceState>,

    /// Reference count information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refs: Option<ResourceRefs>,

    /// Scheduled deletion timestamp (Unix seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_deletion_at: Option<i64>,

    /// Archived timestamp (Unix seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<i64>,

    /// Creation timestamp (Unix seconds)
    pub created_at: i64,

    /// Last modified timestamp (Unix seconds)
    pub updated_at: i64,

    /// Optional tree-level metadata (name, description, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Tree {
    /// Render the tree as a text representation
    pub fn render(&self) -> String {
        let mut output = String::new();
        self.render_node(&self.root, &mut output, "", true);
        output
    }

    fn render_node(&self, node_id: &NodeId, output: &mut String, prefix: &str, is_last: bool) {
        let Some(node) = self.nodes.get(node_id) else {
            return;
        };

        // Current line prefix
        let connector = if is_last { "└── " } else { "├── " };

        // Node content summary
        let content = match &node.data {
            NodeType::Text { content } => {
                let truncated = if content.len() > 60 {
                    format!("{}...", &content[..57])
                } else {
                    content.clone()
                };
                // Replace newlines with ↵
                truncated.replace('\n', "↵")
            }
            NodeType::External { handle } => {
                format!("[{}:{}]", handle.source, handle.identifier)
            }
        };

        output.push_str(&format!("{}{}{}\n", prefix, connector, content));

        // Child prefix
        let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

        // Render children
        let children = &node.children;
        for (i, child_id) in children.iter().enumerate() {
            let is_last_child = i == children.len() - 1;
            self.render_node(child_id, output, &child_prefix, is_last_child);
        }
    }
}

/// Lightweight node representation (just structure, no data)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeSkeleton {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Type indicator (but not the actual data)
    pub node_type: String, // "text" or "external"
}

impl From<&Node> for NodeSkeleton {
    fn from(node: &Node) -> Self {
        NodeSkeleton {
            id: node.id,
            parent: node.parent,
            children: node.children.clone(),
            node_type: match &node.data {
                NodeType::Text { .. } => "text".to_string(),
                NodeType::External { .. } => "external".to_string(),
            },
        }
    }
}

/// Lightweight tree structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreeSkeleton {
    pub id: TreeId,
    pub root: NodeId,
    pub nodes: HashMap<NodeId, NodeSkeleton>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ResourceState>,
}

impl From<&Tree> for TreeSkeleton {
    fn from(tree: &Tree) -> Self {
        TreeSkeleton {
            id: tree.id,
            root: tree.root,
            nodes: tree.nodes.iter().map(|(id, node)| (*id, node.into())).collect(),
            state: tree.state.clone(),
        }
    }
}

// ============================================================================
// Stream Events
// ============================================================================

/// Events emitted by Arbor operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ArborEvent {
    // Tree events
    #[serde(rename = "tree_created")]
    TreeCreated { tree_id: TreeId },

    #[serde(rename = "tree_deleted")]
    TreeDeleted { tree_id: TreeId },

    #[serde(rename = "tree_updated")]
    TreeUpdated { tree_id: TreeId },

    #[serde(rename = "tree_list")]
    TreeList { tree_ids: Vec<TreeId> },

    // Reference counting events
    #[serde(rename = "tree_claimed")]
    TreeClaimed {
        tree_id: TreeId,
        owner_id: String,
        new_count: i64,
    },

    #[serde(rename = "tree_released")]
    TreeReleased {
        tree_id: TreeId,
        owner_id: String,
        new_count: i64,
    },

    #[serde(rename = "tree_scheduled_deletion")]
    TreeScheduledDeletion { tree_id: TreeId, scheduled_at: i64 },

    #[serde(rename = "tree_archived")]
    TreeArchived { tree_id: TreeId, archived_at: i64 },

    #[serde(rename = "tree_refs")]
    TreeRefs { tree_id: TreeId, refs: ResourceRefs },

    // Node events
    #[serde(rename = "node_created")]
    NodeCreated {
        tree_id: TreeId,
        node_id: NodeId,
        parent: Option<NodeId>,
    },

    #[serde(rename = "node_updated")]
    NodeUpdated {
        tree_id: TreeId,
        old_id: NodeId,
        new_id: NodeId,
    },

    #[serde(rename = "node_deleted")]
    NodeDeleted { tree_id: TreeId, node_id: NodeId },

    #[serde(rename = "node_claimed")]
    NodeClaimed {
        tree_id: TreeId,
        node_id: NodeId,
        owner_id: String,
        new_count: i64,
    },

    #[serde(rename = "node_released")]
    NodeReleased {
        tree_id: TreeId,
        node_id: NodeId,
        owner_id: String,
        new_count: i64,
    },

    #[serde(rename = "node_scheduled_deletion")]
    NodeScheduledDeletion {
        tree_id: TreeId,
        node_id: NodeId,
        scheduled_at: i64,
    },

    #[serde(rename = "node_archived")]
    NodeArchived {
        tree_id: TreeId,
        node_id: NodeId,
        archived_at: i64,
    },

    #[serde(rename = "node_refs")]
    NodeRefs {
        tree_id: TreeId,
        node_id: NodeId,
        refs: ResourceRefs,
    },

    // Data events
    #[serde(rename = "tree_data")]
    TreeData { tree: Tree },

    #[serde(rename = "tree_skeleton")]
    TreeSkeleton { skeleton: TreeSkeleton },

    #[serde(rename = "node_data")]
    NodeData { tree_id: TreeId, node: Node },

    #[serde(rename = "node_children")]
    NodeChildren {
        tree_id: TreeId,
        node_id: NodeId,
        children: Vec<NodeId>,
    },

    #[serde(rename = "node_parent")]
    NodeParent {
        tree_id: TreeId,
        node_id: NodeId,
        parent: Option<NodeId>,
    },

    #[serde(rename = "context_path")]
    ContextPath { tree_id: TreeId, path: Vec<NodeId> },

    #[serde(rename = "context_path_data")]
    ContextPathData { tree_id: TreeId, nodes: Vec<Node> },

    #[serde(rename = "context_handles")]
    ContextHandles {
        tree_id: TreeId,
        handles: Vec<Handle>,
    },

    #[serde(rename = "context_leaves")]
    ContextLeaves {
        tree_id: TreeId,
        leaves: Vec<NodeId>,
    },

    // Scheduled deletion queries
    #[serde(rename = "trees_scheduled")]
    TreesScheduled { tree_ids: Vec<TreeId> },

    #[serde(rename = "nodes_scheduled")]
    NodesScheduled {
        tree_id: TreeId,
        node_ids: Vec<NodeId>,
    },

    // Archived queries
    #[serde(rename = "trees_archived")]
    TreesArchived { tree_ids: Vec<TreeId> },

    // Render
    #[serde(rename = "tree_render")]
    TreeRender { tree_id: TreeId, render: String },
}

impl ActivationStreamItem for ArborEvent {
    fn content_type() -> &'static str {
        "arbor.event"
    }

    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem {
        PlexusStreamItem::Data {
            provenance,
            content_type: Self::content_type().to_string(),
            data: serde_json::to_value(self).unwrap(),
        }
    }

    fn is_terminal(&self) -> bool {
        // All Arbor events are terminal (single response per operation)
        true
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Error types for Arbor operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArborError {
    pub message: String,
}

impl ActivationStreamItem for ArborError {
    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem {
        PlexusStreamItem::Error {
            provenance,
            error: self.message,
            recoverable: false,
        }
    }
}

impl std::fmt::Display for ArborError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ArborError {}

impl From<String> for ArborError {
    fn from(message: String) -> Self {
        ArborError { message }
    }
}

impl From<&str> for ArborError {
    fn from(message: &str) -> Self {
        ArborError {
            message: message.to_string(),
        }
    }
}

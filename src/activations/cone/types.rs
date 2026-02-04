use crate::activations::arbor::{NodeId, TreeId};
use plexus_macros::HandleEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::activation::Cone;

/// Unique identifier for an cone configuration
pub type ConeId = Uuid;

// ============================================================================
// Handle types for Cone activation
// ============================================================================

/// Type-safe handles for Cone activation data
///
/// Handles reference data stored in the Cone database and can be embedded
/// in Arbor tree nodes for external resolution.
#[derive(Debug, Clone, HandleEnum)]
#[handle(plugin_id = "Cone::PLUGIN_ID", version = "1.0.0")]
pub enum ConeHandle {
    /// Handle to a message in the cone database
    /// Format: `{plugin_id}@1.0.0::chat:msg-{uuid}:{role}:{name}`
    #[handle(
        method = "chat",
        table = "messages",
        key = "id",
        key_field = "message_id",
        strip_prefix = "msg-"
    )]
    Message {
        /// Message ID with "msg-" prefix (e.g., "msg-550e8400-...")
        message_id: String,
        /// Role: "user", "assistant", or "system"
        role: String,
        /// Display name (cone name or "user")
        name: String,
    },
}

/// Unique identifier for a message
pub type MessageId = Uuid;

/// Role of a message sender
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(MessageRole::User),
            "assistant" => Some(MessageRole::Assistant),
            "system" => Some(MessageRole::System),
            _ => None,
        }
    }
}

/// A message stored in the cone database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub cone_id: ConeId,
    pub role: MessageRole,
    pub content: String,
    pub created_at: i64,
    /// Model used (for assistant messages)
    pub model_id: Option<String>,
    /// Token usage (for assistant messages)
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

/// A position in the context tree - couples tree_id and node_id together.
/// This ensures we always have a valid reference into a specific tree.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct Position {
    /// The tree containing this position
    pub tree_id: TreeId,
    /// The specific node within the tree
    pub node_id: NodeId,
}

impl Position {
    /// Create a new position
    pub fn new(tree_id: TreeId, node_id: NodeId) -> Self {
        Self { tree_id, node_id }
    }

    /// Advance to a new node in the same tree
    pub fn advance(&self, new_node_id: NodeId) -> Self {
        Self {
            tree_id: self.tree_id,
            node_id: new_node_id,
        }
    }
}

/// Cone configuration - defines an cone's identity and behavior
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConeConfig {
    /// Unique identifier for this cone
    pub id: ConeId,
    /// Human-readable name
    pub name: String,
    /// Model ID to use (e.g., "gpt-4o-mini", "claude-3-haiku-20240307")
    pub model_id: String,
    /// System prompt / instructions for the cone
    pub system_prompt: Option<String>,
    /// The canonical head - current position in conversation tree
    /// This couples tree_id and node_id together
    pub head: Position,
    /// Additional configuration metadata
    pub metadata: Option<Value>,
    /// Created timestamp
    pub created_at: i64,
    /// Last updated timestamp
    pub updated_at: i64,
}

impl ConeConfig {
    /// Get the tree ID (convenience accessor)
    pub fn tree_id(&self) -> TreeId {
        self.head.tree_id
    }

    /// Get the current node ID (convenience accessor)
    pub fn node_id(&self) -> NodeId {
        self.head.node_id
    }
}

/// Lightweight cone info (for listing)
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConeInfo {
    pub id: ConeId,
    pub name: String,
    pub model_id: String,
    pub head: Position,
    pub created_at: i64,
}

impl From<&ConeConfig> for ConeInfo {
    fn from(config: &ConeConfig) -> Self {
        Self {
            id: config.id,
            name: config.name.clone(),
            model_id: config.model_id.clone(),
            head: config.head,
            created_at: config.created_at,
        }
    }
}

// ============================================================================
// Method-specific return types
// Each method returns only its valid variants, making the API clearer
// ============================================================================

/// Result of cone.create
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum CreateResult {
    #[serde(rename = "cone_created")]
    Created {
        cone_id: ConeId,
        /// Initial position (tree + root node)
        head: Position,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Result of cone.get
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum GetResult {
    #[serde(rename = "cone_data")]
    Data { cone: ConeConfig },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Result of cone.list
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ListResult {
    #[serde(rename = "cone_list")]
    List { cones: Vec<ConeInfo> },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Result of cone.delete
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum DeleteResult {
    #[serde(rename = "cone_deleted")]
    Deleted { cone_id: ConeId },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Events emitted during cone.chat (streaming)
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ChatEvent {
    /// Chat response started
    #[serde(rename = "chat_start")]
    Start {
        cone_id: ConeId,
        /// Position of the user message node
        user_position: Position,
    },
    /// Chat content chunk (streaming)
    #[serde(rename = "chat_content")]
    Content {
        cone_id: ConeId,
        content: String,
    },
    /// Chat response complete
    #[serde(rename = "chat_complete")]
    Complete {
        cone_id: ConeId,
        /// The new head position (tree + response node)
        new_head: Position,
        /// Total tokens used (if available)
        usage: Option<ChatUsage>,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Result of cone.set_head
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum SetHeadResult {
    #[serde(rename = "head_updated")]
    Updated {
        cone_id: ConeId,
        old_head: Position,
        new_head: Position,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Result of cone.registry
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum RegistryResult {
    #[serde(rename = "registry")]
    Registry(cllient::RegistryExport),
}

/// Resolved message from handle resolution
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ResolveResult {
    #[serde(rename = "resolved_message")]
    Message {
        id: String,
        role: String,
        content: String,
        model: Option<String>,
        name: String,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

// ============================================================================
// Legacy ConeEvent - kept for backwards compatibility but deprecated
// ============================================================================

/// Events emitted by cone operations (deprecated - use method-specific types)
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
#[deprecated(note = "Use method-specific result types instead")]
#[allow(dead_code)]
pub enum ConeEvent {
    /// Cone created
    #[serde(rename = "cone_created")]
    ConeCreated {
        cone_id: ConeId,
        /// Initial position (tree + root node)
        head: Position,
    },

    /// Cone deleted
    #[serde(rename = "cone_deleted")]
    ConeDeleted { cone_id: ConeId },

    /// Cone configuration updated
    #[serde(rename = "cone_updated")]
    ConeUpdated { cone_id: ConeId },

    /// Cone data returned
    #[serde(rename = "cone_data")]
    ConeData { cone: ConeConfig },

    /// List of cones
    #[serde(rename = "cone_list")]
    ConeList { cones: Vec<ConeInfo> },

    /// Chat response started (streaming)
    #[serde(rename = "chat_start")]
    ChatStart {
        cone_id: ConeId,
        /// Position of the user message node
        user_position: Position,
    },

    /// Chat content chunk (streaming)
    #[serde(rename = "chat_content")]
    ChatContent {
        cone_id: ConeId,
        content: String,
    },

    /// Chat response complete
    #[serde(rename = "chat_complete")]
    ChatComplete {
        cone_id: ConeId,
        /// The new head position (tree + response node)
        new_head: Position,
        /// Total tokens used (if available)
        usage: Option<ChatUsage>,
    },

    /// Head position updated (without chat)
    #[serde(rename = "head_updated")]
    HeadUpdated {
        cone_id: ConeId,
        old_head: Position,
        new_head: Position,
    },

    /// Resolved message from a handle
    #[serde(rename = "resolved_message")]
    ResolvedMessage {
        id: String,
        role: String,
        content: String,
        model: Option<String>,
        name: String,
    },

    /// Error during operation
    #[serde(rename = "error")]
    Error { message: String },

    /// Registry information (available models and services)
    #[serde(rename = "registry")]
    Registry(cllient::RegistryExport),
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

/// Error type for cone operations
#[derive(Debug, Clone)]
pub struct ConeError {
    pub message: String,
}

impl std::fmt::Display for ConeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ConeError {}

impl From<String> for ConeError {
    fn from(s: String) -> Self {
        Self { message: s }
    }
}

impl From<&str> for ConeError {
    fn from(s: &str) -> Self {
        Self { message: s.to_string() }
    }
}

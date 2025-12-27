use crate::activations::arbor::{NodeId, TreeId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Unique identifier for a ClaudeCode session
pub type ClaudeCodeId = Uuid;

/// Unique identifier for a message
pub type MessageId = Uuid;

/// Role of a message sender
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Model selection for Claude Code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    Opus,
    Sonnet,
    Haiku,
}

impl Model {
    pub fn as_str(&self) -> &'static str {
        match self {
            Model::Opus => "opus",
            Model::Sonnet => "sonnet",
            Model::Haiku => "haiku",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "opus" => Some(Model::Opus),
            "sonnet" => Some(Model::Sonnet),
            "haiku" => Some(Model::Haiku),
            _ => None,
        }
    }
}

/// A position in the context tree - couples tree_id and node_id together.
/// Same structure as Cone's Position for consistency.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
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

/// A message stored in the claudecode database
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub id: MessageId,
    pub session_id: ClaudeCodeId,
    pub role: MessageRole,
    pub content: String,
    pub created_at: i64,
    /// Model used (for assistant messages)
    pub model_id: Option<String>,
    /// Token usage (for assistant messages)
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    /// Cost in USD (from Claude Code)
    pub cost_usd: Option<f64>,
}

/// ClaudeCode session configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClaudeCodeConfig {
    /// Unique identifier for this session
    pub id: ClaudeCodeId,
    /// Human-readable name
    pub name: String,
    /// Claude Code's internal session ID (for --resume)
    pub claude_session_id: Option<String>,
    /// The canonical head - current position in conversation tree
    pub head: Position,
    /// Working directory for Claude Code
    pub working_dir: String,
    /// Model to use
    pub model: Model,
    /// System prompt / instructions
    pub system_prompt: Option<String>,
    /// MCP server configuration (JSON)
    pub mcp_config: Option<Value>,
    /// Additional metadata
    pub metadata: Option<Value>,
    /// Created timestamp
    pub created_at: i64,
    /// Last updated timestamp
    pub updated_at: i64,
}

impl ClaudeCodeConfig {
    /// Get the tree ID (convenience accessor)
    pub fn tree_id(&self) -> TreeId {
        self.head.tree_id
    }

    /// Get the current node ID (convenience accessor)
    pub fn node_id(&self) -> NodeId {
        self.head.node_id
    }
}

/// Lightweight session info (for listing)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClaudeCodeInfo {
    pub id: ClaudeCodeId,
    pub name: String,
    pub model: Model,
    pub head: Position,
    pub claude_session_id: Option<String>,
    pub working_dir: String,
    pub created_at: i64,
}

impl From<&ClaudeCodeConfig> for ClaudeCodeInfo {
    fn from(config: &ClaudeCodeConfig) -> Self {
        Self {
            id: config.id,
            name: config.name.clone(),
            model: config.model,
            head: config.head,
            claude_session_id: config.claude_session_id.clone(),
            working_dir: config.working_dir.clone(),
            created_at: config.created_at,
        }
    }
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub num_turns: Option<i32>,
}

// ═══════════════════════════════════════════════════════════════════════════
// METHOD-SPECIFIC RETURN TYPES
// Each method returns exactly what it needs - no shared enums
// ═══════════════════════════════════════════════════════════════════════════

/// Result of creating a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreateResult {
    #[serde(rename = "created")]
    Ok {
        id: ClaudeCodeId,
        head: Position,
    },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of getting a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GetResult {
    #[serde(rename = "ok")]
    Ok { config: ClaudeCodeConfig },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of listing sessions
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListResult {
    #[serde(rename = "ok")]
    Ok { sessions: Vec<ClaudeCodeInfo> },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of deleting a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeleteResult {
    #[serde(rename = "deleted")]
    Ok { id: ClaudeCodeId },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of forking a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ForkResult {
    #[serde(rename = "forked")]
    Ok {
        id: ClaudeCodeId,
        head: Position,
    },
    #[serde(rename = "error")]
    Err { message: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// CHAT EVENTS - Streaming conversation (needs enum for multiple event types)
// ═══════════════════════════════════════════════════════════════════════════

/// Events emitted during chat streaming
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// Chat started - user message stored, streaming begins
    #[serde(rename = "start")]
    Start {
        id: ClaudeCodeId,
        user_position: Position,
    },

    /// Content chunk (streaming tokens)
    #[serde(rename = "content")]
    Content { text: String },

    /// Thinking block - Claude's internal reasoning
    #[serde(rename = "thinking")]
    Thinking { thinking: String },

    /// Tool use detected
    #[serde(rename = "tool_use")]
    ToolUse {
        tool_name: String,
        tool_use_id: String,
        input: Value,
    },

    /// Tool result received
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
    },

    /// Chat complete - response stored, head updated
    #[serde(rename = "complete")]
    Complete {
        new_head: Position,
        claude_session_id: String,
        usage: Option<ChatUsage>,
    },

    /// Unknown event from Claude Code - forwards unrecognized events
    /// Data is stored separately (referenced by handle) and also forwarded inline
    #[serde(rename = "unknown")]
    Unknown {
        event_type: String,
        handle: String,
        data: Value,
    },

    /// Error during chat
    #[serde(rename = "error")]
    Err { message: String },
}

/// Error type for ClaudeCode operations
#[derive(Debug, Clone)]
pub struct ClaudeCodeError {
    pub message: String,
}

impl std::fmt::Display for ClaudeCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ClaudeCodeError {}

impl From<String> for ClaudeCodeError {
    fn from(s: String) -> Self {
        Self { message: s }
    }
}

impl From<&str> for ClaudeCodeError {
    fn from(s: &str) -> Self {
        Self { message: s.to_string() }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Raw events from Claude Code CLI (for parsing stream-json output)
// ═══════════════════════════════════════════════════════════════════════════

/// Raw events from Claude Code's stream-json output
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum RawClaudeEvent {
    /// System initialization event
    #[serde(rename = "system")]
    System {
        subtype: Option<String>,
        #[serde(rename = "session_id")]
        session_id: Option<String>,
        model: Option<String>,
        cwd: Option<String>,
        tools: Option<Vec<String>>,
    },

    /// Assistant message event
    #[serde(rename = "assistant")]
    Assistant {
        message: Option<RawMessage>,
    },

    /// User message event
    #[serde(rename = "user")]
    User {
        message: Option<RawMessage>,
    },

    /// Result event (session complete)
    #[serde(rename = "result")]
    Result {
        subtype: Option<String>,
        session_id: Option<String>,
        cost_usd: Option<f64>,
        is_error: Option<bool>,
        duration_ms: Option<i64>,
        num_turns: Option<i32>,
        result: Option<String>,
        error: Option<String>,
    },

    /// Stream event (partial message chunks from --include-partial-messages)
    #[serde(rename = "stream_event")]
    StreamEvent {
        event: StreamEventInner,
        session_id: Option<String>,
    },

    /// Unknown event type - captures events we don't recognize
    /// This is constructed manually in executor.rs, not via serde
    #[serde(skip)]
    Unknown {
        event_type: String,
        data: Value,
    },
}

/// Inner event types for stream_event
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEventInner {
    #[serde(rename = "message_start")]
    MessageStart {
        message: Option<StreamMessage>,
    },

    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: Option<StreamContentBlock>,
    },

    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: usize,
        delta: StreamDelta,
    },

    #[serde(rename = "content_block_stop")]
    ContentBlockStop {
        index: usize,
    },

    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaInfo,
    },

    #[serde(rename = "message_stop")]
    MessageStop,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamMessage {
    pub model: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum StreamContentBlock {
    #[serde(rename = "text")]
    Text { text: Option<String> },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Option<Value>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum StreamDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageDeltaInfo {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawMessage {
    pub id: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub content: Option<Vec<RawContentBlock>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum RawContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<String>,
        is_error: Option<bool>,
    },
}

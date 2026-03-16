use crate::activations::arbor::{NodeId, TreeId};
use plexus_macros::HandleEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use super::activation::ClaudeCode;

/// Unique identifier for a ClaudeCode session
pub type ClaudeCodeId = Uuid;

// ============================================================================
// Handle types for ClaudeCode activation
// ============================================================================

/// Type-safe handles for ClaudeCode activation data
///
/// Handles reference data stored in the ClaudeCode database and can be embedded
/// in Arbor tree nodes for external resolution.
#[derive(Debug, Clone, HandleEnum)]
#[handle(plugin_id = "ClaudeCode::PLUGIN_ID", version = "1.0.0")]
pub enum ClaudeCodeHandle {
    /// Handle to a message in the claudecode database
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
        /// Display name
        name: String,
    },

    /// Handle to an unknown/passthrough event
    /// Format: `{plugin_id}@1.0.0::passthrough:{event_id}:{event_type}`
    /// Note: No resolution - passthrough events are inline only
    #[handle(method = "passthrough")]
    Passthrough {
        /// Event ID
        event_id: String,
        /// Event type string
        event_type: String,
    },
}

// ============================================================================
// Handle resolution result types
// ============================================================================

/// Result of resolving a ClaudeCode handle
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum ResolveResult {
    /// Successfully resolved message
    #[serde(rename = "resolved_message")]
    Message {
        id: String,
        role: String,
        content: String,
        model: Option<String>,
        name: String,
    },
    /// Resolution error
    #[serde(rename = "error")]
    Error { message: String },
}

/// Unique identifier for an active stream
pub type StreamId = Uuid;

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
    /// Claude Code's internal session ID (for --resume, populated after first chat)
    pub claude_session_id: Option<String>,
    /// Session ID for loopback MCP URL correlation (e.g., orcha-xxx-claude-yyy)
    pub loopback_session_id: Option<String>,
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
    /// Enable loopback mode - routes tool permissions through parent for approval
    pub loopback_enabled: bool,
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
    pub loopback_enabled: bool,
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
            loopback_enabled: config.loopback_enabled,
            created_at: config.created_at,
        }
    }
}

/// Token usage information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChatUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub num_turns: Option<i32>,
}

// ═══════════════════════════════════════════════════════════════════════════
// STREAM MANAGEMENT TYPES (for non-blocking chat with loopback)
// ═══════════════════════════════════════════════════════════════════════════

/// Status of an active stream
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamStatus {
    /// Stream is actively receiving events
    Running,
    /// Stream is waiting for tool permission approval
    AwaitingPermission,
    /// Stream completed successfully
    Complete,
    /// Stream failed with an error
    Failed,
}

/// Information about an active stream
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamInfo {
    /// Unique stream identifier
    pub stream_id: StreamId,
    /// Session this stream belongs to
    pub session_id: ClaudeCodeId,
    /// Current status
    pub status: StreamStatus,
    /// Position of the user message node (set at start)
    pub user_position: Option<Position>,
    /// Number of events buffered
    pub event_count: u64,
    /// Read position (how many events have been consumed)
    pub read_position: u64,
    /// When the stream started
    pub started_at: i64,
    /// When the stream ended (if complete/failed)
    pub ended_at: Option<i64>,
    /// Error message if failed
    pub error: Option<String>,
}

/// A buffered event in the stream
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BufferedEvent {
    /// Sequence number within the stream
    pub seq: u64,
    /// The chat event
    pub event: ChatEvent,
    /// Timestamp when event was received
    pub timestamp: i64,
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

/// Result of starting an async chat (non-blocking)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStartResult {
    #[serde(rename = "started")]
    Ok {
        stream_id: StreamId,
        session_id: ClaudeCodeId,
    },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of polling a stream for events
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PollResult {
    #[serde(rename = "ok")]
    Ok {
        /// Current stream status
        status: StreamStatus,
        /// Events since last poll (or from specified offset)
        events: Vec<BufferedEvent>,
        /// Current read position after this poll
        read_position: u64,
        /// Total events in buffer
        total_events: u64,
        /// True if there are more events available
        has_more: bool,
    },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of listing active streams
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamListResult {
    #[serde(rename = "ok")]
    Ok { streams: Vec<StreamInfo> },
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

    /// Passthrough for unrecognized Claude Code events
    /// Data is stored separately (referenced by handle) and also forwarded inline
    #[serde(rename = "passthrough")]
    Passthrough {
        event_type: String,
        handle: String,
        data: Value,
    },

    /// Error during chat
    #[serde(rename = "error")]
    Err { message: String },
}

/// Typed errors for ClaudeCode operations
#[derive(Debug, Error)]
pub enum ClaudeCodeError {
    #[error("failed to resolve working directory '{path}': {source}")]
    PathResolution { path: String, source: std::io::Error },

    #[error("session not found: {identifier}")]
    SessionNotFound { identifier: String },

    #[error("ambiguous session name '{name}' matches multiple sessions: {matches}")]
    AmbiguousSession { name: String, matches: String },

    #[error("database error: {operation}: {source}")]
    Database { operation: &'static str, source: sqlx::Error },

    #[error("parse error: {context}: {detail}")]
    Parse { context: &'static str, detail: String },

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("arbor error: {0}")]
    Arbor(String),
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

    /// The exact shell command launched (emitted before spawn, constructed manually)
    #[serde(skip)]
    LaunchCommand { command: String },

    /// A line from Claude's stderr (emitted after stdout closes, constructed manually)
    #[serde(skip)]
    Stderr { text: String },
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

// ═══════════════════════════════════════════════════════════════════════════
// ARBOR SOURCE OF TRUTH TYPES (Milestone 1)
// These types enable storing conversation events as arbor nodes and rendering
// them back into Claude API message format for time travel, forking, etc.
// ═══════════════════════════════════════════════════════════════════════════

/// Events stored as arbor text nodes - each event is a self-describing JSON blob
/// that maps 1:1 to Claude API structures
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeEvent {
    /// User message node
    #[serde(rename = "user_message")]
    UserMessage { content: String },

    /// Assistant turn start marker
    #[serde(rename = "assistant_start")]
    AssistantStart,

    /// Text content block (child of assistant_start)
    #[serde(rename = "content_text")]
    ContentText { text: String },

    /// Tool use block (child of assistant_start)
    #[serde(rename = "content_tool_use")]
    ContentToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Thinking block (child of assistant_start)
    #[serde(rename = "content_thinking")]
    ContentThinking { thinking: String },

    /// Tool result message (becomes a user message in Claude API)
    #[serde(rename = "user_tool_result")]
    UserToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },

    /// Assistant turn complete marker
    #[serde(rename = "assistant_complete")]
    AssistantComplete { usage: Option<ChatUsage> },

    /// The exact shell command used to launch Claude (for debugging)
    #[serde(rename = "launch_command")]
    LaunchCommand { command: String },

    /// Stderr output captured from the Claude process (errors, warnings)
    #[serde(rename = "claude_stderr")]
    ClaudeStderr { text: String },
}

/// Claude API message format - what we render arbor nodes into
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClaudeMessage {
    /// Role: "user" or "assistant"
    pub role: String,
    /// Message content blocks
    pub content: Vec<ContentBlock>,
}

/// Content blocks within a Claude message
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content
    #[serde(rename = "text")]
    Text { text: String },

    /// Tool use
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Tool result
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },

    /// Thinking block
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

/// Result of render_context method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RenderResult {
    #[serde(rename = "ok")]
    Ok { messages: Vec<ClaudeMessage> },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of get_tree method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GetTreeResult {
    #[serde(rename = "ok")]
    Ok { tree_id: TreeId, head: NodeId },
    #[serde(rename = "error")]
    Err { message: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// SESSION FILE CRUD RESULTS
// ═══════════════════════════════════════════════════════════════════════════

/// Result of sessions_list method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionsListResult {
    #[serde(rename = "ok")]
    Ok { sessions: Vec<String> },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of sessions_get method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionsGetResult {
    #[serde(rename = "ok")]
    Ok {
        session_id: String,
        event_count: usize,
        events: Vec<serde_json::Value>,
    },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of sessions_import method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionsImportResult {
    #[serde(rename = "ok")]
    Ok { tree_id: TreeId, session_id: String },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of sessions_export method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionsExportResult {
    #[serde(rename = "ok")]
    Ok { tree_id: TreeId, session_id: String },
    #[serde(rename = "error")]
    Err { message: String },
}

/// Result of sessions_delete method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionsDeleteResult {
    #[serde(rename = "ok")]
    Ok { session_id: String, deleted: bool },
    #[serde(rename = "error")]
    Err { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_event_serialization() {
        let event = NodeEvent::ContentText {
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: NodeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn test_claude_message_structure() {
        let msg = ClaudeMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "test".to_string(),
            }],
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"][0]["type"], "text");
    }

    #[test]
    fn test_json_schema_generation() {
        use schemars::schema_for;

        // Test that all new types generate schemas without panicking
        let _schema = schema_for!(NodeEvent);
        let _schema = schema_for!(ClaudeMessage);
        let _schema = schema_for!(ContentBlock);
        let _schema = schema_for!(RenderResult);
        let _schema = schema_for!(GetTreeResult);
    }

    #[test]
    fn test_all_node_event_variants() {
        // Test serialization of all NodeEvent variants
        let events = vec![
            NodeEvent::UserMessage {
                content: "Hello".to_string(),
            },
            NodeEvent::AssistantStart,
            NodeEvent::ContentText {
                text: "Response".to_string(),
            },
            NodeEvent::ContentToolUse {
                id: "tool_123".to_string(),
                name: "Write".to_string(),
                input: serde_json::json!({"file": "test.txt"}),
            },
            NodeEvent::ContentThinking {
                thinking: "Let me think...".to_string(),
            },
            NodeEvent::UserToolResult {
                tool_use_id: "tool_123".to_string(),
                content: "Success".to_string(),
                is_error: false,
            },
            NodeEvent::AssistantComplete {
                usage: Some(ChatUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(200),
                    cost_usd: Some(0.01),
                    num_turns: Some(1),
                }),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let parsed: NodeEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, parsed);
        }
    }

    #[test]
    fn test_all_content_block_variants() {
        // Test serialization of all ContentBlock variants
        let blocks = vec![
            ContentBlock::Text {
                text: "Hello".to_string(),
            },
            ContentBlock::ToolUse {
                id: "tool_456".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tool_456".to_string(),
                content: "file1.txt\nfile2.txt".to_string(),
                is_error: false,
            },
            ContentBlock::Thinking {
                thinking: "Analyzing...".to_string(),
            },
        ];

        for block in blocks {
            let json = serde_json::to_string(&block).unwrap();
            let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
            assert_eq!(block, parsed);
        }
    }

    #[test]
    fn test_node_event_json_format() {
        // Verify that NodeEvent produces the expected JSON structure
        let event = NodeEvent::ContentToolUse {
            id: "toolu_123".to_string(),
            name: "Write".to_string(),
            input: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "content_tool_use");
        assert_eq!(json["id"], "toolu_123");
        assert_eq!(json["name"], "Write");
        assert_eq!(json["input"]["path"], "/tmp/test.txt");
    }

    #[test]
    fn test_render_result_variants() {
        // Test RenderResult::Ok
        let result = RenderResult::Ok {
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "test".to_string(),
                }],
            }],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["type"], "ok");
        assert!(json["messages"].is_array());

        // Test RenderResult::Err
        let result = RenderResult::Err {
            message: "test error".to_string(),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["message"], "test error");
    }

    #[test]
    fn test_get_tree_result_variants() {
        use crate::activations::arbor::{NodeId, TreeId};

        // Test GetTreeResult::Ok
        let tree_id = TreeId::new();
        let node_id = NodeId::new();
        let result = GetTreeResult::Ok {
            tree_id,
            head: node_id,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["type"], "ok");

        // Test GetTreeResult::Err
        let result = GetTreeResult::Err {
            message: "not found".to_string(),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["message"], "not found");
    }
}

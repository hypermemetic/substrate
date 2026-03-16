pub use crate::activations::lattice::GatherStrategy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ═══════════════════════════════════════════════════════════════════════════
// Error Types
// ═══════════════════════════════════════════════════════════════════════════

/// Structured error type for Orcha operations
#[derive(Debug, Error)]
pub enum OrchaError {
    #[error("session not found: {session_id}")]
    SessionNotFound { session_id: String },

    #[error("orchestration error: {detail}")]
    OrchestrationError { detail: String },

    #[error("storage error during {operation}: {detail}")]
    StorageError { operation: String, detail: String },

    #[error("validation error: {detail}")]
    ValidationError { detail: String },
}

impl From<String> for OrchaError {
    fn from(detail: String) -> Self {
        OrchaError::OrchestrationError { detail }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Orcha Node Kind — typed dispatch
// ═══════════════════════════════════════════════════════════════════════════

/// Typed Orcha node payload — serialized into NodeSpec::Task { data }.
/// graph_runner deserializes this to dispatch to the correct executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "orcha_type", rename_all = "snake_case")]
pub enum OrchaNodeKind {
    Task { task: String, #[serde(default)] max_retries: Option<u8> },
    Synthesize { task: String, #[serde(default)] max_retries: Option<u8> },
    Validate { command: String, cwd: Option<String>, #[serde(default)] max_retries: Option<u8> },
    Review { prompt: String },
    Plan { task: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// Session Management Types
// ═══════════════════════════════════════════════════════════════════════════

/// Unique identifier for an orcha session
pub type SessionId = String;

/// Current state of an orcha session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    /// Session is idle, ready to accept tasks
    Idle,
    /// Session is executing a task
    Running {
        /// Current Claude Code stream ID
        stream_id: String,
        /// Current sequence number for polling
        sequence: u64,
        /// Number of active agents (multi-agent mode)
        #[serde(default)]
        active_agents: u32,
        /// Number of completed agents (multi-agent mode)
        #[serde(default)]
        completed_agents: u32,
        /// Number of failed agents (multi-agent mode)
        #[serde(default)]
        failed_agents: u32,
    },
    /// Session is waiting for approval response
    WaitingApproval {
        /// Approval request ID
        approval_id: String,
    },
    /// Session is waiting for validation
    Validating {
        /// Test command being executed
        test_command: String,
    },
    /// Session has completed successfully
    Complete,
    /// Session has failed
    Failed {
        /// Error message
        error: String,
    },
}

/// Session metadata
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub model: String,
    pub created_at: i64, // Unix timestamp
    pub last_activity: i64,
    pub state: SessionState,
    pub retry_count: u32,
    pub max_retries: u32,
    /// Agent mode (single or multi)
    #[serde(default)]
    pub agent_mode: AgentMode,
    /// Primary agent ID (if in multi mode)
    pub primary_agent_id: Option<AgentId>,
    /// Arbor tree ID for tracking orchestration events
    pub tree_id: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Request/Response Types
// ═══════════════════════════════════════════════════════════════════════════

/// Request to create a new orcha session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateSessionRequest {
    /// Model to use (sonnet, opus, haiku)
    pub model: String,
    /// Working directory for the session
    #[serde(default = "default_cwd")]
    pub working_directory: String,
    /// Approval rules for the session
    pub rules: Option<String>,
    /// Maximum retry attempts for validation failures
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Enable multi-agent mode (default: false for backward compatibility)
    #[serde(default)]
    pub multi_agent: bool,
}

/// Request to run a task with full orchestration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunTaskRequest {
    /// Model to use (sonnet, opus, haiku)
    pub model: String,
    /// Task description/prompt
    pub task: String,
    /// Working directory for the session
    #[serde(default = "default_cwd")]
    pub working_directory: String,
    /// Approval rules (for Claude-as-judge)
    pub rules: Option<String>,
    /// Maximum retry attempts for validation failures
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Show Claude's output and tool use (default: false)
    #[serde(default)]
    pub verbose: bool,
    /// Enable automatic approval via Haiku decision agent (default: true)
    ///
    /// When true, spawns ephemeral Haiku session to judge each approval.
    /// When false, approvals must be handled manually via orcha.approve_request.
    #[serde(default = "default_auto_approve")]
    pub auto_approve: bool,
}

fn default_auto_approve() -> bool {
    false
}

fn default_cwd() -> String {
    "/workspace".to_string()
}

fn default_max_retries() -> u32 {
    3
}

/// Result of creating a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreateSessionResult {
    Ok {
        session_id: SessionId,
        created_at: i64,
    },
    Err {
        message: String,
    },
}

/// Request to submit a task to a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubmitTaskRequest {
    pub session_id: SessionId,
    pub task: String,
}

/// Result of submitting a task
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SubmitTaskResult {
    Ok {
        stream_id: String,
    },
    Err {
        message: String,
    },
}

/// Request to get session status
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetSessionRequest {
    pub session_id: SessionId,
}

/// Result of getting session status
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GetSessionResult {
    Ok {
        session: SessionInfo,
    },
    Err {
        message: String,
    },
}

/// Request to respond to an approval
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RespondApprovalRequest {
    pub approval_id: String,
    pub approve: bool,
    pub message: Option<String>,
}

/// Result of responding to approval
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RespondApprovalResult {
    Ok,
    Err {
        message: String,
    },
}

/// Request to list pending approvals for a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListApprovalsRequest {
    pub session_id: SessionId,
}

/// Information about a pending approval request
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalInfo {
    pub approval_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub tool_input: serde_json::Value,
    pub created_at: String, // ISO 8601 timestamp
}

/// Result of listing pending approvals
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListApprovalsResult {
    Ok {
        approvals: Vec<ApprovalInfo>,
    },
    Err {
        message: String,
    },
}

/// Request to approve a pending request
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApproveRequest {
    pub approval_id: String,
    /// Optional message explaining approval decision
    pub message: Option<String>,
}

/// Request to deny a pending request
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DenyRequest {
    pub approval_id: String,
    /// Reason for denial (shown to agent)
    pub reason: Option<String>,
}

/// Result of approving or denying a request
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApprovalActionResult {
    Ok {
        approval_id: String,
        message: Option<String>,
    },
    Err {
        message: String,
    },
}

/// Result of updating session state
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateSessionStateResult {
    Ok,
    Err {
        message: String,
    },
}

/// Result of extracting validation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtractValidationResult {
    Ok {
        artifact: ValidationArtifact,
    },
    NotFound,
}

/// Result of running validation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunValidationResult {
    Ok {
        result: ValidationResult,
    },
}

/// Result of incrementing retry counter
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IncrementRetryResult {
    Ok {
        retry_count: u32,
        max_retries: u32,
        exceeded: bool,
    },
    Err {
        message: String,
    },
}

/// Result of listing sessions
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListSessionsResult {
    Ok {
        sessions: Vec<SessionInfo>,
    },
}

/// Result of deleting session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeleteSessionResult {
    Ok,
    Err {
        message: String,
    },
}

/// Request to check status of a running session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckStatusRequest {
    pub session_id: SessionId,
}

/// Result of checking session status
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CheckStatusResult {
    Ok {
        summary: String,
        /// Per-agent summaries (empty for single-agent mode)
        #[serde(default)]
        agent_summaries: Vec<AgentSummary>,
    },
    Err {
        message: String,
    },
}

/// Result of starting async task
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunTaskAsyncResult {
    Ok {
        session_id: SessionId,
    },
    Err {
        message: String,
    },
}

/// Result of listing monitor trees
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListMonitorTreesResult {
    Ok {
        trees: Vec<MonitorTreeInfo>,
    },
}

/// Information about a monitor tree
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MonitorTreeInfo {
    pub tree_id: String,
    pub session_id: String,
    pub tree_path: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Event Streaming Types
// ═══════════════════════════════════════════════════════════════════════════

/// Events streamed from an orcha session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchaEvent {
    /// Session state changed
    StateChange {
        session_id: SessionId,
        state: SessionState,
    },

    /// Output from the agent (Claude content)
    Output {
        text: String,
    },

    /// Tool use detected
    ToolUse {
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
    },

    /// Tool result
    ToolResult {
        tool_id: String,
        content: String,
        is_error: bool,
    },

    /// Approval request detected
    ApprovalRequest {
        approval_id: String,
        tool_name: String,
        input: serde_json::Value,
        timeout_seconds: u64,
    },

    /// Approval response received
    ApprovalResponse {
        approval_id: String,
        approved: bool,
        message: Option<String>,
    },

    /// Validation artifact detected
    ValidationArtifact {
        test_command: String,
        cwd: String,
    },

    /// Validation started
    ValidationStarted {
        test_command: String,
    },

    /// Validation result
    ValidationResult {
        success: bool,
        output: String,
    },

    /// Retry attempt
    RetryAttempt {
        attempt: u32,
        max_retries: u32,
        reason: String,
    },

    /// Task completed successfully
    Complete {
        session_id: SessionId,
    },

    /// Task failed
    Failed {
        session_id: SessionId,
        error: String,
    },

    /// Progress update
    Progress {
        message: String,
        percentage: Option<f32>,
    },

    /// Agent spawned (multi-agent mode)
    AgentSpawned {
        session_id: SessionId,
        agent_id: AgentId,
        subtask: String,
        parent_agent_id: Option<AgentId>,
    },

    /// Agent state changed (multi-agent mode)
    AgentStateChange {
        agent_id: AgentId,
        state: AgentState,
    },

    /// Agent completed (multi-agent mode)
    AgentComplete {
        agent_id: AgentId,
        subtask: String,
    },

    /// Agent failed (multi-agent mode)
    AgentFailed {
        agent_id: AgentId,
        subtask: String,
        error: String,
    },

    /// Graph execution has started
    GraphStarted {
        graph_id: String,
    },

    /// A node is ready and has been dispatched for execution
    NodeStarted {
        node_id: String,
        label: Option<String>,
        /// Ticket ID (e.g. "CALC-1") if this node was built from a ticket definition
        ticket_id: Option<String>,
        /// Completion percentage before this node started (complete_nodes / total_nodes * 100)
        percentage: Option<u32>,
    },

    /// A node completed successfully
    NodeComplete {
        node_id: String,
        label: Option<String>,
        /// Ticket ID (e.g. "CALC-1") if this node was built from a ticket definition
        ticket_id: Option<String>,
        output_summary: Option<String>,
        /// Completion percentage after this node finished (complete_nodes / total_nodes * 100)
        percentage: Option<u32>,
    },

    /// A node failed
    NodeFailed {
        node_id: String,
        label: Option<String>,
        /// Ticket ID (e.g. "CALC-1") if this node was built from a ticket definition
        ticket_id: Option<String>,
        error: String,
        /// Completion percentage after this node failed (complete_nodes / total_nodes * 100)
        percentage: Option<u32>,
    },

    /// A validate node is retrying after a failed validation attempt
    Retrying {
        node_id: String,
        ticket_id: Option<String>,
        attempt: usize,
        max_attempts: usize,
        error: String,
    },

    /// Live output chunk from a node during execution
    NodeOutput {
        node_id: String,
        ticket_id: Option<String>,
        chunk: String,
    },

    /// Graph was cancelled via cancel_graph
    Cancelled {
        graph_id: String,
    },

    /// A pending approval request is waiting for a human decision
    ApprovalPending {
        approval_id: String,
        graph_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        created_at: String,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// Validation Types
// ═══════════════════════════════════════════════════════════════════════════

/// Validation artifact extracted from agent output
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValidationArtifact {
    pub test_command: String,
    pub cwd: String,
}

/// Result of running a validation test
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValidationResult {
    pub success: bool,
    pub output: String,
    pub exit_code: Option<i32>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Multi-Agent Orchestration Types
// ═══════════════════════════════════════════════════════════════════════════

/// Unique identifier for an agent
pub type AgentId = String;

/// Agent mode for sessions
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    /// Single-agent mode (backward compatible)
    Single,
    /// Multi-agent orchestration mode
    Multi,
}

impl Default for AgentMode {
    fn default() -> Self {
        AgentMode::Single
    }
}

/// Agent-specific state (mirrors SessionState but for individual agents)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentState {
    /// Agent is idle, ready to start
    Idle,
    /// Agent is executing a task
    Running {
        /// Current sequence number for polling
        sequence: u64,
    },
    /// Agent is waiting for approval response
    WaitingApproval {
        /// Approval request ID
        approval_id: String,
    },
    /// Agent is running validation
    Validating {
        /// Test command being executed
        test_command: String,
    },
    /// Agent has completed successfully
    Complete,
    /// Agent has failed
    Failed {
        /// Error message
        error: String,
    },
}

/// Agent metadata and state
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentInfo {
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub claudecode_session_id: String,
    pub subtask: String,
    pub state: AgentState,
    pub is_primary: bool,
    pub parent_agent_id: Option<AgentId>,
    pub created_at: i64,
    pub last_activity: i64,
    pub completed_at: Option<i64>,
    pub error_message: Option<String>,
}

/// Request to spawn a new agent
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpawnAgentRequest {
    pub session_id: SessionId,
    pub subtask: String,
    /// Optional parent agent (if spawned by another agent)
    pub parent_agent_id: Option<AgentId>,
}

/// Result of spawning an agent
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SpawnAgentResult {
    Ok {
        agent_id: AgentId,
        claudecode_session_id: String,
    },
    Err {
        message: String,
    },
}

/// Request to list agents in a session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListAgentsRequest {
    pub session_id: SessionId,
}

/// Result of listing agents
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListAgentsResult {
    Ok {
        agents: Vec<AgentInfo>,
    },
    Err {
        message: String,
    },
}

/// Request to get specific agent info
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetAgentRequest {
    pub agent_id: AgentId,
}

/// Result of getting agent info
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GetAgentResult {
    Ok {
        agent: AgentInfo,
    },
    Err {
        message: String,
    },
}

/// Summary of an individual agent's work
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentSummary {
    pub agent_id: AgentId,
    pub subtask: String,
    pub state: AgentState,
    pub summary: String,  // AI-generated summary of this agent's work
}

// ═══════════════════════════════════════════════════════════════════════════
// Graph Builder Result Types
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchaCreateGraphResult {
    Ok { graph_id: String },
    Err { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchaAddNodeResult {
    Ok { node_id: String },
    Err { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchaAddDependencyResult {
    Ok,
    Err { message: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// Inline Graph Definition Types (for run_graph_definition)
// ═══════════════════════════════════════════════════════════════════════════

/// Typed node spec in Orcha vocabulary — no raw NodeSpec JSON needed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchaNodeSpec {
    Task { task: String, #[serde(default)] max_retries: Option<u8> },
    Synthesize { task: String, #[serde(default)] max_retries: Option<u8> },
    Validate { command: String, cwd: Option<String>, #[serde(default)] max_retries: Option<u8> },
    Gather { strategy: GatherStrategy },
    Review { prompt: String },
    Plan { task: String },
}

/// One node in an inline graph definition.
/// `id` is a caller-supplied stable label used in OrchaEdgeDef.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchaNodeDef {
    pub id: String,
    pub spec: OrchaNodeSpec,
}

/// One edge in an inline graph definition.
/// `from`/`to` reference OrchaNodeDef.id values.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchaEdgeDef {
    pub from: String,
    pub to: String,
}

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Unique identifier for an approval request
pub type ApprovalId = Uuid;

/// Status of an approval request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
    TimedOut,
}

/// A pending approval request
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalRequest {
    pub id: ApprovalId,
    pub session_id: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub input: Value,
    pub status: ApprovalStatus,
    pub response_message: Option<String>,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

/// Request to the permit MCP tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermitRequest {
    pub tool_name: String,
    pub tool_use_id: String,
    pub input: Value,
}

/// Response from the permit MCP tool (Claude Code expected format)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "behavior", rename_all = "lowercase")]
pub enum PermitResponse {
    Allow {
        #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
        updated_input: Option<Value>,
    },
    Deny {
        message: String,
    },
}

/// Configuration for loopback mode
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LoopbackConfig {
    /// Session ID for correlation
    pub session_id: String,
    /// Plexus MCP URL
    pub mcp_url: String,
    /// Tools to auto-approve (no human needed)
    #[serde(default)]
    pub auto_approve_tools: Vec<String>,
    /// Tools to always deny
    #[serde(default)]
    pub deny_tools: Vec<String>,
    /// Timeout in seconds for approval (default: 300)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 { 300 }

// Result types for hub methods
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RespondResult {
    #[serde(rename = "ok")]
    Ok { approval_id: ApprovalId },
    #[serde(rename = "error")]
    Err { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PendingResult {
    #[serde(rename = "ok")]
    Ok { approvals: Vec<ApprovalRequest> },
    #[serde(rename = "error")]
    Err { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigureResult {
    #[serde(rename = "ok")]
    Ok { mcp_config: Value },
    #[serde(rename = "error")]
    Err { message: String },
}

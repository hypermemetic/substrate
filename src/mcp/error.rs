//! MCP Error Types
//!
//! JSON-RPC 2.0 compatible error codes and types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::state::McpStateError;
use crate::plexus::PlexusError;

/// Standard JSON-RPC 2.0 error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Invalid JSON was received
    ParseError = -32700,
    /// The JSON sent is not a valid Request object
    InvalidRequest = -32600,
    /// The method does not exist / is not available
    MethodNotFound = -32601,
    /// Invalid method parameter(s)
    InvalidParams = -32602,
    /// Internal JSON-RPC error
    InternalError = -32603,
    /// Server not initialized
    ServerNotInitialized = -32002,
    /// Request cancelled
    RequestCancelled = -32800,
}

/// MCP-specific errors
#[derive(Debug, Clone)]
pub enum McpError {
    /// Method not found
    MethodNotFound(String),
    /// Invalid parameters
    InvalidParams(String),
    /// State machine error
    State(McpStateError),
    /// Unsupported protocol version
    UnsupportedVersion(String),
    /// Tool not found
    ToolNotFound(String),
    /// Resource not found
    ResourceNotFound(String),
    /// Prompt not found
    PromptNotFound(String),
    /// Internal error
    Internal(String),
    /// Not implemented yet
    NotImplemented(String),
    /// Serialization error
    Serialization(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::MethodNotFound(method) => write!(f, "Method not found: {}", method),
            McpError::InvalidParams(msg) => write!(f, "Invalid params: {}", msg),
            McpError::State(e) => write!(f, "State error: {}", e),
            McpError::UnsupportedVersion(v) => write!(f, "Unsupported protocol version: {}", v),
            McpError::ToolNotFound(name) => write!(f, "Tool not found: {}", name),
            McpError::ResourceNotFound(uri) => write!(f, "Resource not found: {}", uri),
            McpError::PromptNotFound(name) => write!(f, "Prompt not found: {}", name),
            McpError::Internal(msg) => write!(f, "Internal error: {}", msg),
            McpError::NotImplemented(method) => write!(f, "Not implemented: {}", method),
            McpError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for McpError {}

impl From<McpStateError> for McpError {
    fn from(e: McpStateError) -> Self {
        McpError::State(e)
    }
}

impl From<serde_json::Error> for McpError {
    fn from(e: serde_json::Error) -> Self {
        McpError::Serialization(e.to_string())
    }
}

impl From<PlexusError> for McpError {
    fn from(e: PlexusError) -> Self {
        match e {
            PlexusError::ActivationNotFound(name) => McpError::ToolNotFound(name),
            PlexusError::MethodNotFound { activation, method } => {
                McpError::ToolNotFound(format!("{}.{}", activation, method))
            }
            PlexusError::InvalidParams(msg) => McpError::InvalidParams(msg),
            PlexusError::ExecutionError(msg) => McpError::Internal(msg),
        }
    }
}

impl McpError {
    /// Get the JSON-RPC error code for this error
    pub fn code(&self) -> i32 {
        match self {
            McpError::MethodNotFound(_) => ErrorCode::MethodNotFound as i32,
            McpError::InvalidParams(_) => ErrorCode::InvalidParams as i32,
            McpError::State(e) => match e {
                McpStateError::NotReady { .. } => ErrorCode::ServerNotInitialized as i32,
                _ => ErrorCode::InvalidRequest as i32,
            },
            McpError::UnsupportedVersion(_) => ErrorCode::InvalidParams as i32,
            McpError::ToolNotFound(_) => ErrorCode::InvalidParams as i32,
            McpError::ResourceNotFound(_) => ErrorCode::InvalidParams as i32,
            McpError::PromptNotFound(_) => ErrorCode::InvalidParams as i32,
            McpError::Internal(_) => ErrorCode::InternalError as i32,
            McpError::NotImplemented(_) => ErrorCode::MethodNotFound as i32,
            McpError::Serialization(_) => ErrorCode::ParseError as i32,
        }
    }

    /// Convert to JSON-RPC error object
    pub fn to_json_rpc_error(&self) -> Value {
        serde_json::json!({
            "code": self.code(),
            "message": self.to_string()
        })
    }
}

/// JSON-RPC 2.0 Error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<McpError> for JsonRpcError {
    fn from(e: McpError) -> Self {
        JsonRpcError {
            code: e.code(),
            message: e.to_string(),
            data: None,
        }
    }
}

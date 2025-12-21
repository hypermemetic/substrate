//! MCP (Model Context Protocol) Interface
//!
//! This module implements the MCP 2024-11-05 and 2025-03-26 specifications,
//! exposing Plexus activations as MCP tools over Streamable HTTP with SSE.
//!
//! ## Architecture
//!
//! The MCP layer is a thin view over Plexus:
//! - Same activations, different delivery mechanism
//! - JSON-RPC 2.0 protocol with SSE streaming for tools/call
//! - State machine guards method access (must initialize first)
//!
//! ## Modules
//!
//! - [`state`] - Protocol state machine (Uninitialized → Initializing → Ready → ShuttingDown)
//! - [`error`] - MCP-specific error types with JSON-RPC error codes
//! - [`types`] - MCP protocol types (requests, responses, capabilities)
//! - [`interface`] - Main McpInterface that routes methods to handlers

pub mod error;
pub mod interface;
pub mod schema;
pub mod state;
pub mod transport;
pub mod types;

pub use error::{ErrorCode, JsonRpcError, McpError};
pub use interface::McpInterface;
pub use state::{McpState, McpStateError, McpStateMachine};
pub use types::*;

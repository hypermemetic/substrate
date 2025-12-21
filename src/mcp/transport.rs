//! MCP HTTP Transport
//!
//! Implements the MCP Streamable HTTP transport (2025-03-26/2025-06-18 spec).
//! Exposes the MCP interface at `/mcp` endpoint.
//!
//! Key features:
//! - Per-session state management via session_id query parameter
//! - Session-aware state machine (each session has its own lifecycle)
//! - Proper error status codes (400 for invalid JSON, 401 for invalid session)

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::{
    state::McpStateMachine,
    types::{JsonRpcRequest, JsonRpcResponse},
};
use crate::plexus::Plexus;

/// Session ID type alias
pub type SessionId = String;

/// Per-session MCP state
pub struct McpSession {
    pub state: McpStateMachine,
    pub created_at: std::time::Instant,
}

impl McpSession {
    pub fn new() -> Self {
        Self {
            state: McpStateMachine::new(),
            created_at: std::time::Instant::now(),
        }
    }
}

/// Shared state for MCP HTTP handlers with per-session tracking
#[derive(Clone)]
pub struct McpHttpState {
    pub plexus: Arc<Plexus>,
    pub sessions: Arc<RwLock<HashMap<SessionId, McpSession>>>,
}

impl McpHttpState {
    /// Create new MCP HTTP state with the given Plexus
    pub fn new(plexus: Arc<Plexus>) -> Self {
        Self {
            plexus,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create a session
    pub fn get_session(&self, session_id: &str) -> Option<()> {
        self.sessions.read().unwrap().get(session_id).map(|_| ())
    }

    /// Create a new session and return its ID
    pub fn create_session(&self) -> SessionId {
        let session_id = Uuid::new_v4().to_string();
        self.sessions.write().unwrap().insert(session_id.clone(), McpSession::new());
        session_id
    }

    /// Get session state for reading
    pub fn with_session<F, R>(&self, session_id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&McpSession) -> R,
    {
        self.sessions.read().unwrap().get(session_id).map(f)
    }
}

/// Query parameters for MCP endpoint
#[derive(Debug, Deserialize)]
pub struct McpQuery {
    pub session_id: Option<String>,
}

/// Create an Axum router for MCP endpoints
pub fn mcp_router(plexus: Arc<Plexus>) -> Router {
    let state = McpHttpState::new(plexus);

    Router::new()
        .route("/mcp", post(handle_mcp_post_raw))
        .with_state(state)
}

/// Handle POST /mcp with raw body to detect batch requests
///
/// We parse the body ourselves to distinguish between:
/// - Batch requests (JSON array) - return 400 with "batch not supported"
/// - Single requests (JSON object) - process normally
/// - Invalid JSON - return 400 with parse error
async fn handle_mcp_post_raw(
    State(state): State<McpHttpState>,
    Query(query): Query<McpQuery>,
    body: axum::body::Bytes,
) -> Response {
    // Try to parse as JSON value first
    let json_value: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            // Invalid JSON - return 400
            return (
                StatusCode::BAD_REQUEST,
                Json(JsonRpcResponse::error(
                    Value::Null,
                    super::error::JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    },
                )),
            )
                .into_response();
        }
    };

    // Check if it's a batch request (array)
    if json_value.is_array() {
        // Batch requests are not supported in 2025-06-18
        return (
            StatusCode::BAD_REQUEST,
            Json(JsonRpcResponse::error(
                Value::Null,
                super::error::JsonRpcError {
                    code: -32600,
                    message: "Batch requests are not supported".to_string(),
                    data: None,
                },
            )),
        )
            .into_response();
    }

    // Try to parse as a single JSON-RPC request
    let request: JsonRpcRequest = match serde_json::from_value(json_value) {
        Ok(r) => r,
        Err(e) => {
            // Invalid request structure
            return (
                StatusCode::BAD_REQUEST,
                Json(JsonRpcResponse::error(
                    Value::Null,
                    super::error::JsonRpcError {
                        code: -32600,
                        message: format!("Invalid request: {}", e),
                        data: None,
                    },
                )),
            )
                .into_response();
        }
    };

    // Now handle the valid request
    handle_mcp_post(State(state), Query(query), request).await
}

/// Handle POST /mcp
///
/// Accepts JSON-RPC 2.0 requests and returns JSON-RPC responses.
///
/// Session handling:
/// - `initialize` requests create a new session and return the session_id
/// - All other requests require a valid session_id in query params
/// - Invalid session_id returns 401 Unauthorized
async fn handle_mcp_post(
    State(state): State<McpHttpState>,
    Query(query): Query<McpQuery>,
    request: JsonRpcRequest,
) -> Response {
    let method = &request.method;
    let is_notification = request.is_notification();
    let request_id = request.id.clone().unwrap_or(Value::Null);

    tracing::debug!(
        method = %method,
        id = ?request.id,
        session_id = ?query.session_id,
        "MCP request received"
    );

    // Handle initialize specially - it creates a new session
    if method == "initialize" {
        return handle_initialize(&state, request).await;
    }

    // All other requests require a valid session
    let session_id = match &query.session_id {
        Some(id) => id.clone(),
        None => {
            // No session ID provided - for backwards compatibility, allow if no sessions exist
            // This handles the case where the first request isn't initialize
            return (
                StatusCode::BAD_REQUEST,
                Json(JsonRpcResponse::error(
                    request_id,
                    super::error::JsonRpcError {
                        code: -32600,
                        message: "Session ID required. Call initialize first.".to_string(),
                        data: None,
                    },
                )),
            )
                .into_response();
        }
    };

    // Validate session exists
    if state.get_session(&session_id).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(JsonRpcResponse::error(
                request_id,
                super::error::JsonRpcError {
                    code: -32001,
                    message: "Invalid or expired session".to_string(),
                    data: None,
                },
            )),
        )
            .into_response();
    }

    // Handle notifications/initialized specially - transitions session to Ready
    if method == "notifications/initialized" {
        return handle_initialized(&state, &session_id, request).await;
    }

    // For all other requests, require Ready state
    let result = handle_request(&state, &session_id, request).await;

    // For notifications, return 202 Accepted with no body
    if is_notification {
        return (StatusCode::ACCEPTED, "").into_response();
    }

    // Build response with session header
    let response = match result {
        Ok(result) => JsonRpcResponse::success(request_id, result),
        Err(e) => JsonRpcResponse::error(request_id, e.into()),
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE.as_str(), "application/json"),
            ("mcp-session-id", session_id.as_str()),
        ],
        Json(response),
    )
        .into_response()
}

/// Handle initialize request - creates new session
///
/// For HTTP transport, we auto-transition to Ready state after successful initialize.
/// This is because HTTP is stateless and requiring notifications/initialized doesn't
/// make as much sense as it does for STDIO transport with persistent connections.
async fn handle_initialize(
    state: &McpHttpState,
    request: JsonRpcRequest,
) -> Response {
    use super::interface::handle_initialize_request;
    use super::state::McpState;

    let request_id = request.id.clone().unwrap_or(Value::Null);

    // Create new session
    let session_id = state.create_session();

    // Process initialize first
    let result = handle_initialize_request(&state.plexus, request.params).await;

    // If initialize succeeded, transition directly to Ready
    // For HTTP transport, we skip Initializing state since notifications/initialized
    // doesn't make as much sense in a stateless HTTP context
    if result.is_ok() {
        let transition_result = state.sessions.write().unwrap().get_mut(&session_id).map(|session| {
            // Transition Uninitialized -> Initializing -> Ready in one step
            session.state.transition(McpState::Initializing)
                .and_then(|_| session.state.transition(McpState::Ready))
        });

        if let Some(Err(e)) = transition_result {
            tracing::warn!(error = %e, "State transition failed after initialize");
        } else {
            tracing::info!(session_id = %session_id, "MCP session Ready (HTTP auto-transition)");
        }
    }

    let response = match result {
        Ok(result) => JsonRpcResponse::success(request_id, result),
        Err(e) => JsonRpcResponse::error(request_id, e.into()),
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE.as_str(), "application/json"),
            ("mcp-session-id", session_id.as_str()),
        ],
        Json(response),
    )
        .into_response()
}

/// Handle initialized notification - transitions session to Ready
///
/// For HTTP transport, this is optional since we auto-transition to Ready after initialize.
/// However, we still accept it for compatibility with clients that send it.
async fn handle_initialized(
    state: &McpHttpState,
    session_id: &str,
    _request: JsonRpcRequest,
) -> Response {
    use super::state::McpState;

    // Check current state
    let current_state = state.with_session(session_id, |s| s.state.current());

    match current_state {
        Some(McpState::Ready) => {
            // Already ready (HTTP auto-transition), just accept
            tracing::debug!(session_id = %session_id, "Received initialized notification, already Ready");
            (StatusCode::ACCEPTED, "").into_response()
        }
        Some(McpState::Initializing) => {
            // STDIO-style flow, transition to Ready
            let transition_result = state.sessions.write().unwrap().get_mut(session_id).map(|session| {
                session.state.transition(McpState::Ready)
            });

            if let Some(Err(e)) = transition_result {
                tracing::warn!(error = %e, "Failed to transition to Ready");
            } else {
                tracing::info!(session_id = %session_id, "MCP session now Ready");
            }
            (StatusCode::ACCEPTED, "").into_response()
        }
        Some(_) | None => {
            // Invalid state or session
            (StatusCode::ACCEPTED, "").into_response()
        }
    }
}

/// Handle a regular MCP request (requires Ready state)
async fn handle_request(
    state: &McpHttpState,
    session_id: &str,
    request: JsonRpcRequest,
) -> Result<Value, super::error::McpError> {
    use super::interface::handle_mcp_request;
    use super::state::McpState;

    // Check session is Ready
    let is_ready = state.with_session(session_id, |session| {
        session.state.current() == McpState::Ready
    });

    match is_ready {
        Some(true) => {
            // Session is ready, handle the request
            handle_mcp_request(&state.plexus, &request.method, request.params).await
        }
        Some(false) => {
            // Session exists but not ready
            let current = state.with_session(session_id, |s| s.state.current());
            Err(super::error::McpError::State(
                super::state::McpStateError::NotReady {
                    actual: current.unwrap_or(McpState::Uninitialized),
                },
            ))
        }
        None => {
            // Session doesn't exist (shouldn't happen, checked earlier)
            Err(super::error::McpError::State(
                super::state::McpStateError::NotReady {
                    actual: McpState::Uninitialized,
                },
            ))
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app() -> Router {
        let plexus = Arc::new(Plexus::new());
        mcp_router(plexus)
    }

    #[tokio::test]
    async fn test_mcp_endpoint_initialize() {
        let app = test_app();

        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {"name": "test", "version": "1.0"}
                    }
                }"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), 10000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert!(json["result"]["protocolVersion"].is_string());
        assert!(json["result"]["serverInfo"]["name"].is_string());
    }

    #[tokio::test]
    async fn test_mcp_no_session_returns_bad_request() {
        let app = test_app();

        // Request without session_id for non-initialize method should fail
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), 10000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert!(json["error"].is_object());
        assert_eq!(json["error"]["code"], -32600); // Invalid request
    }

    #[tokio::test]
    async fn test_mcp_invalid_session_returns_unauthorized() {
        let app = test_app();

        // Request with invalid session_id should return 401
        let request = Request::builder()
            .method("POST")
            .uri("/mcp?session_id=invalid-session")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_mcp_session_id_header() {
        let app = test_app();

        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {"name": "test", "version": "1.0"}
                    }
                }"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Check for session ID header
        assert!(response.headers().get("mcp-session-id").is_some());
    }
}

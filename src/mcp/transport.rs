//! MCP HTTP Transport
//!
//! Implements the MCP Streamable HTTP transport (2025-03-26 spec).
//! Exposes the MCP interface at `/mcp` endpoint.

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::Value;
use uuid::Uuid;

use super::{
    interface::McpInterface,
    types::{JsonRpcRequest, JsonRpcResponse},
};

/// Shared state for MCP HTTP handlers
#[derive(Clone)]
pub struct McpHttpState {
    pub mcp: Arc<McpInterface>,
}

/// Create an Axum router for MCP endpoints
pub fn mcp_router(mcp: Arc<McpInterface>) -> Router {
    let state = McpHttpState { mcp };

    Router::new()
        .route("/mcp", post(handle_mcp_post))
        .with_state(state)
}

/// Handle POST /mcp
///
/// Accepts JSON-RPC 2.0 requests and returns JSON-RPC responses.
/// For streaming methods (tools/call), returns SSE stream.
async fn handle_mcp_post(
    State(state): State<McpHttpState>,
    Json(request): Json<JsonRpcRequest>,
) -> Response {
    // Generate session ID for tracking
    let session_id = Uuid::new_v4().to_string();

    tracing::debug!(
        method = %request.method,
        id = ?request.id,
        session_id = %session_id,
        "MCP request received"
    );

    // Check if this is a notification (no id)
    let is_notification = request.is_notification();

    // Route to MCP interface
    let result = state.mcp.handle(&request.method, request.params).await;

    // For notifications, don't send a response
    if is_notification {
        match result {
            Ok(_) => {
                return (StatusCode::ACCEPTED, "").into_response();
            }
            Err(e) => {
                tracing::warn!(error = %e, "Notification failed");
                return (StatusCode::ACCEPTED, "").into_response();
            }
        }
    }

    // For requests, send JSON-RPC response
    let request_id = request.id.unwrap_or(Value::Null);

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


#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use crate::plexus::Plexus;
    use tower::ServiceExt;

    async fn test_app() -> Router {
        let plexus = Arc::new(Plexus::new());
        let mcp = Arc::new(McpInterface::new(plexus));
        mcp_router(mcp)
    }

    #[tokio::test]
    async fn test_mcp_endpoint_initialize() {
        let app = test_app().await;

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
    async fn test_mcp_endpoint_unknown_method() {
        let app = test_app().await;

        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"jsonrpc": "2.0", "id": 1, "method": "unknown/method", "params": {}}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), 10000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert!(json["error"].is_object());
        assert_eq!(json["error"]["code"], -32601); // Method not found
    }

    #[tokio::test]
    async fn test_mcp_session_id_header() {
        let app = test_app().await;

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

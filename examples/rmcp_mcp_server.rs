//! Demo MCP server using rmcp - stub-based, ready for Plexus integration
//!
//! Run with: cargo run --example rmcp_mcp_server
//!
//! Test with curl:
//! ```bash
//! # Initialize
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'
//!
//! # List tools
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
//!
//! # Call tool (with SSE for streaming)
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -H "Accept: text/event-stream" \
//!   -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"stub.echo","arguments":{"message":"hello"}}}'
//! ```

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
};
use tokio::time::{Duration, sleep};

/// Convert a serde_json::Value to JsonObject (Map<String, Value>)
fn value_to_schema(v: serde_json::Value) -> Arc<JsonObject> {
    match v {
        serde_json::Value::Object(map) => Arc::new(map),
        _ => Arc::new(serde_json::Map::new()),
    }
}

/// Stub handler that will later be replaced with Plexus bridge
#[derive(Clone)]
pub struct StubMcpHandler {
    /// Simulated tools - in real impl, this comes from Plexus.list_full_schemas()
    tools: Vec<Tool>,
}

impl StubMcpHandler {
    pub fn new() -> Self {
        // Define stub tools that mirror what Plexus activations would provide
        let tools = vec![
            Tool::new(
                "stub.echo",
                "Echo back the input message",
                value_to_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Message to echo"
                        }
                    },
                    "required": ["message"]
                })),
            ),
            Tool::new(
                "stub.stream_count",
                "Count from 1 to N, streaming each number",
                value_to_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "count": {
                            "type": "integer",
                            "description": "Number to count to"
                        },
                        "delay_ms": {
                            "type": "integer",
                            "description": "Delay between numbers in milliseconds"
                        }
                    },
                    "required": ["count"]
                })),
            ),
            Tool::new(
                "stub.error",
                "Simulate an error",
                value_to_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Error message"
                        }
                    },
                    "required": ["message"]
                })),
            ),
        ];

        Self { tools }
    }

    /// Simulate streaming tool execution
    /// In real impl, this calls plexus.call() and streams PlexusStreamEvents
    async fn execute_tool(
        &self,
        name: &str,
        arguments: Option<JsonObject>,
        ctx: &RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let args = arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::json!({}));
        let progress_token = ctx.meta.get_progress_token();

        match name {
            "stub.echo" => {
                let message = args.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(empty)");

                Ok(CallToolResult::success(vec![
                    Content::text(format!("Echo: {}", message))
                ]))
            }

            "stub.stream_count" => {
                let count = args.get("count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as u32;
                let delay_ms = args.get("delay_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100);

                // Stream each number via notifications - NO BUFFERING
                for i in 1..=count {
                    // Check cancellation
                    if ctx.ct.is_cancelled() {
                        return Err(McpError::internal_error("Cancelled", None));
                    }

                    // Send progress notification
                    if let Some(ref token) = progress_token {
                        let _ = ctx.peer.notify_progress(ProgressNotificationParam {
                            progress_token: token.clone(),
                            progress: i as f64,
                            total: Some(count as f64),
                            message: Some(format!("Counting: {}/{}", i, count)),
                        }).await;
                    }

                    // Send data via logging notification (structured)
                    let _ = ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                        level: LoggingLevel::Info,
                        logger: Some("stub.stream".into()),
                        data: serde_json::json!({
                            "type": "data",
                            "content_type": "application/json",
                            "data": { "number": i },
                        }),
                    }).await;

                    sleep(Duration::from_millis(delay_ms)).await;
                }

                // Return completion marker only - data already streamed
                Ok(CallToolResult::success(vec![
                    Content::text(format!("Streamed {} numbers via notifications", count))
                ]))
            }

            "stub.error" => {
                let message = args.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Something went wrong");

                // Stream error via notification first
                let _ = ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                    level: LoggingLevel::Error,
                    logger: Some("stub.stream".into()),
                    data: serde_json::json!({
                        "type": "error",
                        "error": message,
                        "recoverable": false,
                    }),
                }).await;

                Ok(CallToolResult::error(vec![
                    Content::text(message.to_string())
                ]))
            }

            _ => {
                // Unknown tool - send guidance (like Plexus would)
                let _ = ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                    level: LoggingLevel::Warning,
                    logger: Some("stub.guidance".into()),
                    data: serde_json::json!({
                        "type": "guidance",
                        "error_kind": "method_not_found",
                        "method": name,
                        "available_methods": self.tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(),
                        "suggestion": {
                            "action": "try_method",
                            "method": "stub.echo",
                        }
                    }),
                }).await;

                Err(McpError::invalid_params(format!("Unknown tool: {}", name), None))
            }
        }
    }
}

impl ServerHandler for StubMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_logging()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Stub MCP server demonstrating streaming pattern. \
                 Will be replaced with Plexus bridge.".into()
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tools.clone(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.execute_tool(&request.name, request.arguments, &ctx).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rmcp=debug".parse()?)
                .add_directive("rmcp_mcp_server=debug".parse()?)
        )
        .init();

    let addr = "127.0.0.1:3000";
    tracing::info!("Starting stub MCP server on http://{}/mcp", addr);

    // Create the handler
    let handler = Arc::new(StubMcpHandler::new());

    // Create StreamableHttpService
    use rmcp::transport::streamable_http_server::{
        StreamableHttpService,
        session::local::LocalSessionManager,
        StreamableHttpServerConfig,
    };

    let config = StreamableHttpServerConfig::default();
    let session_manager = LocalSessionManager::default().into();

    let handler_clone = handler.clone();
    let service = StreamableHttpService::new(
        move || Ok((*handler_clone).clone()),
        session_manager,
        config,
    );

    // Build axum router
    let app = axum::Router::new()
        .nest_service("/mcp", service);

    // Run server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Server listening on http://{}", addr);
    tracing::info!("Test with:");
    tracing::info!("  curl -X POST http://{}/mcp -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{{}},\"clientInfo\":{{\"name\":\"test\",\"version\":\"1.0\"}}}}}}'", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

//! MCP server using rmcp with Plexus backend
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
//! # Call tool (health check)
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -H "Accept: text/event-stream" \
//!   -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"health.check","arguments":{}}}'
//! ```

use std::sync::Arc;

use futures::StreamExt;
use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
};
use serde_json::json;
use substrate::{
    build_plexus,
    plexus::{Plexus, PlexusError, PlexusStreamEvent, ActivationFullSchema},
};

// =============================================================================
// Schema Transformation (PINNED FOR REVIEW)
// =============================================================================

/// Convert Plexus activation schemas to rmcp Tool format
///
/// MCP requires all tool inputSchema to have "type": "object" at root.
/// schemars may produce schemas without this (e.g., for unit types).
fn schemas_to_rmcp_tools(schemas: Vec<ActivationFullSchema>) -> Vec<Tool> {
    schemas
        .into_iter()
        .flat_map(|activation| {
            let namespace = activation.namespace.clone();
            activation.methods.into_iter().map(move |method| {
                let name = format!("{}.{}", namespace, method.name);
                let description = method.description.clone();

                // Convert schemars::Schema to JSON, ensure "type": "object" exists
                let input_schema = method
                    .params
                    .and_then(|s| serde_json::to_value(s).ok())
                    .and_then(|v| v.as_object().cloned())
                    .map(|mut obj| {
                        // MCP requires "type": "object" at schema root
                        if !obj.contains_key("type") {
                            obj.insert("type".to_string(), json!("object"));
                        }
                        Arc::new(obj)
                    })
                    .unwrap_or_else(|| {
                        // Empty params = empty object schema
                        Arc::new(serde_json::Map::from_iter([
                            ("type".to_string(), json!("object")),
                        ]))
                    });

                Tool::new(name, description, input_schema)
            })
        })
        .collect()
}

// =============================================================================
// Error Mapping
// =============================================================================

/// Convert PlexusError to McpError
fn plexus_to_mcp_error(e: PlexusError) -> McpError {
    match e {
        PlexusError::ActivationNotFound(name) => {
            McpError::invalid_params(format!("Unknown activation: {}", name), None)
        }
        PlexusError::MethodNotFound { activation, method } => {
            McpError::invalid_params(format!("Unknown method: {}.{}", activation, method), None)
        }
        PlexusError::InvalidParams(reason) => McpError::invalid_params(reason, None),
        PlexusError::ExecutionError(error) => McpError::internal_error(error, None),
        PlexusError::HandleNotSupported(activation) => {
            McpError::invalid_params(format!("Handle resolution not supported: {}", activation), None)
        }
    }
}

// =============================================================================
// Plexus MCP Bridge
// =============================================================================

/// MCP handler that bridges to Plexus
#[derive(Clone)]
pub struct PlexusMcpBridge {
    plexus: Arc<Plexus>,
}

impl PlexusMcpBridge {
    pub fn new(plexus: Arc<Plexus>) -> Self {
        Self { plexus }
    }
}

impl ServerHandler for PlexusMcpBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_logging()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Plexus MCP server - provides access to all registered activations.".into(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let schemas = self.plexus.list_full_schemas();
        let tools = schemas_to_rmcp_tools(schemas);

        tracing::debug!("Listing {} tools", tools.len());

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let method_name = &request.name;
        let arguments = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or(json!({}));

        tracing::debug!("Calling tool: {} with args: {:?}", method_name, arguments);

        // Get progress token if provided
        let progress_token = ctx.meta.get_progress_token();

        // Logger name: plexus.namespace.method (e.g., plexus.bash.execute)
        let logger = format!("plexus.{}", method_name);

        // Call Plexus and get stream
        let stream = self
            .plexus
            .call(method_name, arguments)
            .await
            .map_err(plexus_to_mcp_error)?;

        // Stream events via notifications AND buffer for final result
        let mut had_error = false;
        let mut buffered_data: Vec<serde_json::Value> = Vec::new();
        let mut error_messages: Vec<String> = Vec::new();

        tokio::pin!(stream);
        while let Some(item) = stream.next().await {
            // Check cancellation on each iteration
            if ctx.ct.is_cancelled() {
                return Err(McpError::internal_error("Cancelled", None));
            }

            match &item.event {
                PlexusStreamEvent::Progress {
                    message,
                    percentage,
                    ..
                } => {
                    // Only send progress if client provided token
                    if let Some(ref token) = progress_token {
                        let _ = ctx
                            .peer
                            .notify_progress(ProgressNotificationParam {
                                progress_token: token.clone(),
                                progress: percentage.unwrap_or(0.0) as f64,
                                total: None,
                                message: Some(message.clone()),
                            })
                            .await;
                    }
                }

                PlexusStreamEvent::Data {
                    data, content_type, ..
                } => {
                    // Buffer data for final result
                    buffered_data.push(data.clone());

                    // Also stream via notifications for real-time consumers
                    let _ = ctx
                        .peer
                        .notify_logging_message(LoggingMessageNotificationParam {
                            level: LoggingLevel::Info,
                            logger: Some(logger.clone()),
                            data: json!({
                                "type": "data",
                                "content_type": content_type,
                                "data": data,
                            }),
                        })
                        .await;
                }

                PlexusStreamEvent::Error {
                    error, recoverable, ..
                } => {
                    // Buffer errors for final result
                    error_messages.push(error.clone());

                    let _ = ctx
                        .peer
                        .notify_logging_message(LoggingMessageNotificationParam {
                            level: LoggingLevel::Error,
                            logger: Some(logger.clone()),
                            data: json!({
                                "type": "error",
                                "error": error,
                                "recoverable": recoverable,
                            }),
                        })
                        .await;

                    if !recoverable {
                        had_error = true;
                    }
                }

                PlexusStreamEvent::Done { .. } => {
                    break;
                }

                PlexusStreamEvent::Guidance { .. } => {
                    // Always stream guidance
                    let _ = ctx
                        .peer
                        .notify_logging_message(LoggingMessageNotificationParam {
                            level: LoggingLevel::Warning,
                            logger: Some(logger.clone()),
                            data: serde_json::to_value(&item.event).unwrap_or_default(),
                        })
                        .await;
                }
            }
        }

        // Return buffered data in the final result
        if had_error {
            let error_content = if error_messages.is_empty() {
                "Stream completed with errors".to_string()
            } else {
                error_messages.join("\n")
            };
            Ok(CallToolResult::error(vec![Content::text(error_content)]))
        } else {
            // Convert buffered data to content
            let content = if buffered_data.is_empty() {
                vec![Content::text("(no output)")]
            } else if buffered_data.len() == 1 {
                // Single value - return as text if string, otherwise JSON
                match &buffered_data[0] {
                    serde_json::Value::String(s) => vec![Content::text(s.clone())],
                    other => vec![Content::text(serde_json::to_string_pretty(other).unwrap_or_default())],
                }
            } else {
                // Multiple values - join strings or return as JSON array
                let all_strings = buffered_data.iter().all(|v| v.is_string());
                if all_strings {
                    let joined: String = buffered_data
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join("");
                    vec![Content::text(joined)]
                } else {
                    vec![Content::text(serde_json::to_string_pretty(&buffered_data).unwrap_or_default())]
                }
            };
            Ok(CallToolResult::success(content))
        }
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rmcp=debug".parse()?)
                .add_directive("rmcp_mcp_server=debug".parse()?)
                .add_directive("substrate=info".parse()?),
        )
        .init();

    let addr = "127.0.0.1:3000";
    tracing::info!("Building Plexus with all activations...");

    // Build Plexus with all activations (already returns Arc<Plexus>)
    let plexus = build_plexus().await;
    let methods = plexus.list_methods();
    tracing::info!("Plexus ready with {} methods", methods.len());
    for method in &methods {
        tracing::debug!("  - {}", method);
    }

    // Create the handler
    let handler = PlexusMcpBridge::new(plexus);

    // Create StreamableHttpService
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let config = StreamableHttpServerConfig::default();
    let session_manager = LocalSessionManager::default().into();

    let handler_clone = handler.clone();
    let service = StreamableHttpService::new(
        move || Ok(handler_clone.clone()),
        session_manager,
        config,
    );

    // Build axum router
    let app = axum::Router::new().nest_service("/mcp", service);

    // Run server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("MCP server listening on http://{}/mcp", addr);
    tracing::info!("Test with:");
    tracing::info!("  curl -X POST http://{}/mcp -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{{}},\"clientInfo\":{{\"name\":\"test\",\"version\":\"1.0\"}}}}}}'", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

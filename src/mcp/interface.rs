//! MCP Interface
//!
//! The main MCP interface that wraps Plexus and routes MCP protocol methods.
//!
//! This module provides two modes of operation:
//! 1. `McpInterface` - A self-contained interface with its own state machine (for testing)
//! 2. Standalone functions for transport layer to use with per-session state

use std::sync::Arc;

use futures::StreamExt;
use serde_json::Value;

use super::{
    error::McpError,
    schema::schemas_to_mcp_tools,
    state::{McpState, McpStateMachine},
    types::{
        InitializeParams, InitializeResult, LoggingCapability, ResourcesCapability,
        ServerCapabilities, ServerInfo, ToolContent, ToolsCallParams, ToolsCallResult,
        ToolsCapability, ToolsListParams, ToolsListResult, SUPPORTED_VERSIONS,
    },
};
use crate::plexus::{types::PlexusStreamEvent, Plexus};

// ============================================================================
// Standalone request handlers (used by transport layer with per-session state)
// ============================================================================

/// Handle an initialize request (creates new session)
pub async fn handle_initialize_request(
    plexus: &Arc<Plexus>,
    params: Value,
) -> Result<Value, McpError> {
    // Parse params
    let params: InitializeParams = serde_json::from_value(params)?;

    // Validate protocol version
    if !SUPPORTED_VERSIONS.contains(&params.protocol_version.as_str()) {
        return Err(McpError::UnsupportedVersion(params.protocol_version));
    }

    tracing::info!(
        client = %params.client_info.name,
        client_version = %params.client_info.version,
        protocol_version = %params.protocol_version,
        "MCP initialize request"
    );

    // Build capabilities based on registered activations
    let capabilities = build_capabilities(plexus);

    let result = InitializeResult {
        protocol_version: params.protocol_version,
        capabilities,
        server_info: ServerInfo {
            name: "substrate".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    };

    Ok(serde_json::to_value(result)?)
}

/// Handle a general MCP request (requires Ready state - transport must verify)
pub async fn handle_mcp_request(
    plexus: &Arc<Plexus>,
    method: &str,
    params: Value,
) -> Result<Value, McpError> {
    tracing::debug!(method = %method, "Handling MCP request");

    match method {
        // Utility - ping returns timestamp per mcp-validator expectations
        "ping" => Ok(serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339()
        })),

        // Tools
        "tools/list" => handle_tools_list_impl(plexus, params).await,
        "tools/call" => handle_tools_call_impl(plexus, params).await,

        // Resources
        "resources/list" => Err(McpError::NotImplemented("resources/list".to_string())),
        "resources/read" => Err(McpError::NotImplemented("resources/read".to_string())),

        // Prompts
        "prompts/list" => Err(McpError::NotImplemented("prompts/list".to_string())),
        "prompts/get" => Err(McpError::NotImplemented("prompts/get".to_string())),

        // Notifications (shouldn't come through here)
        "notifications/cancelled" => Err(McpError::NotImplemented(
            "notifications/cancelled".to_string(),
        )),

        // Unknown method
        _ => Err(McpError::MethodNotFound(method.to_string())),
    }
}

/// Build server capabilities based on registered activations
fn build_capabilities(plexus: &Arc<Plexus>) -> ServerCapabilities {
    // Check if we have specific activations registered
    let has_arbor = plexus
        .list_activations()
        .iter()
        .any(|a| a.namespace == "arbor");

    ServerCapabilities {
        // Tools are always available (from Plexus activations)
        tools: Some(ToolsCapability { list_changed: true }),
        // Resources only if Arbor is available
        resources: if has_arbor {
            Some(ResourcesCapability {
                subscribe: true,
                list_changed: true,
            })
        } else {
            None
        },
        // Prompts not yet implemented
        prompts: None,
        // Logging always available
        logging: Some(LoggingCapability {}),
    }
}

/// Handle tools/list request
async fn handle_tools_list_impl(plexus: &Arc<Plexus>, params: Value) -> Result<Value, McpError> {
    // Parse params (cursor is optional)
    let params: ToolsListParams = serde_json::from_value(params).unwrap_or_default();

    // Get all activation schemas and transform to MCP tools
    let schemas = plexus.list_full_schemas();
    let all_tools = schemas_to_mcp_tools(&schemas);

    // Handle pagination (50 tools per page)
    let (tools, next_cursor) = paginate(all_tools, params.cursor.as_deref(), 50);

    let result = ToolsListResult { tools, next_cursor };

    Ok(serde_json::to_value(result)?)
}

/// Handle tools/call request
async fn handle_tools_call_impl(plexus: &Arc<Plexus>, params: Value) -> Result<Value, McpError> {
    // Parse params
    let params: ToolsCallParams = serde_json::from_value(params)?;

    tracing::debug!(
        tool = %params.name,
        "Executing tool call"
    );

    // Call the Plexus method
    let mut stream = plexus.call(&params.name, params.arguments).await?;

    // Collect stream results
    let mut content: Vec<ToolContent> = Vec::new();
    let mut is_error = false;
    let mut error_messages: Vec<String> = Vec::new();

    while let Some(item) = stream.next().await {
        match item.event {
            PlexusStreamEvent::Data {
                data, content_type, ..
            } => {
                let text = if content_type == "text/plain" {
                    data.as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| serde_json::to_string_pretty(&data).unwrap_or_default())
                } else {
                    serde_json::to_string_pretty(&data).unwrap_or_default()
                };
                content.push(ToolContent::Text { text });
            }
            PlexusStreamEvent::Error { error, .. } => {
                is_error = true;
                error_messages.push(error);
            }
            PlexusStreamEvent::Guidance {
                error_type,
                suggestion,
                ..
            } => {
                is_error = true;
                let msg = format!("Guidance: {:?} - {:?}", error_type, suggestion);
                error_messages.push(msg);
            }
            PlexusStreamEvent::Progress {
                message,
                percentage,
                ..
            } => {
                tracing::trace!(
                    tool = %params.name,
                    message = %message,
                    percentage = ?percentage,
                    "Tool progress"
                );
            }
            PlexusStreamEvent::Done { .. } => {
                tracing::debug!(tool = %params.name, "Tool call complete");
            }
        }
    }

    // Add error messages as text content
    if !error_messages.is_empty() {
        for msg in error_messages {
            content.push(ToolContent::Text { text: msg });
        }
    }

    // Ensure at least one content item
    if content.is_empty() {
        content.push(ToolContent::Text {
            text: "Tool executed successfully (no output)".to_string(),
        });
    }

    let result = ToolsCallResult { content, is_error };
    Ok(serde_json::to_value(result)?)
}

/// Paginate a list of items
fn paginate<T>(items: Vec<T>, cursor: Option<&str>, page_size: usize) -> (Vec<T>, Option<String>) {
    let start = cursor
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(0);

    let page: Vec<T> = items.into_iter().skip(start).take(page_size).collect();
    let count = page.len();
    let next = if count == page_size {
        Some((start + page_size).to_string())
    } else {
        None
    };

    (page, next)
}

// ============================================================================
// McpInterface - Self-contained interface (for testing and simple use cases)
// ============================================================================

/// The MCP Interface - routes MCP protocol methods to handlers
pub struct McpInterface {
    /// Reference to the Plexus for accessing activations
    plexus: Arc<Plexus>,
    /// Protocol state machine
    state: McpStateMachine,
    /// Server information
    server_info: ServerInfo,
}

impl McpInterface {
    /// Create a new MCP interface wrapping a Plexus instance
    pub fn new(plexus: Arc<Plexus>) -> Self {
        Self {
            plexus,
            state: McpStateMachine::new(),
            server_info: ServerInfo {
                name: "substrate".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        }
    }

    /// Get the Plexus instance
    pub fn plexus(&self) -> &Arc<Plexus> {
        &self.plexus
    }

    /// Get the state machine
    pub fn state(&self) -> &McpStateMachine {
        &self.state
    }

    /// Get server info
    pub fn server_info(&self) -> &ServerInfo {
        &self.server_info
    }

    /// Route an MCP request to the appropriate handler
    ///
    /// This is the main entry point for MCP protocol methods.
    /// Methods are routed based on the method name.
    pub async fn handle(&self, method: &str, params: Value) -> Result<Value, McpError> {
        tracing::debug!(method = %method, "Handling MCP request");

        match method {
            // Lifecycle
            "initialize" => self.handle_initialize(params).await,
            "notifications/initialized" => self.handle_initialized(params).await,

            // Utility
            "ping" => self.handle_ping(params).await,

            // Tools
            "tools/list" => self.handle_tools_list(params).await,
            "tools/call" => self.handle_tools_call(params).await,

            // Resources
            "resources/list" => self.handle_resources_list(params).await,
            "resources/read" => self.handle_resources_read(params).await,

            // Prompts
            "prompts/list" => self.handle_prompts_list(params).await,
            "prompts/get" => self.handle_prompts_get(params).await,

            // Notifications
            "notifications/cancelled" => self.handle_cancelled(params).await,

            // Unknown method
            _ => Err(McpError::MethodNotFound(method.to_string())),
        }
    }

    // === Lifecycle Handlers ===

    /// Handle the `initialize` request (MCP-4)
    ///
    /// This must be called first before any other methods.
    /// Validates protocol version and returns server capabilities.
    async fn handle_initialize(&self, params: Value) -> Result<Value, McpError> {
        // Must be in Uninitialized state
        self.state.require(McpState::Uninitialized)?;

        // Parse params
        let params: InitializeParams = serde_json::from_value(params)?;

        // Validate protocol version
        if !SUPPORTED_VERSIONS.contains(&params.protocol_version.as_str()) {
            return Err(McpError::UnsupportedVersion(params.protocol_version));
        }

        tracing::info!(
            client = %params.client_info.name,
            client_version = %params.client_info.version,
            protocol_version = %params.protocol_version,
            "MCP initialize request"
        );

        // Transition to Initializing
        self.state.transition(McpState::Initializing)?;

        // Build capabilities based on registered activations
        let capabilities = self.build_capabilities();

        let result = InitializeResult {
            protocol_version: params.protocol_version,
            capabilities,
            server_info: self.server_info.clone(),
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Build server capabilities based on registered activations
    fn build_capabilities(&self) -> ServerCapabilities {
        // Check if we have specific activations registered
        let has_arbor = self.plexus.list_activations().iter().any(|a| a.namespace == "arbor");

        ServerCapabilities {
            // Tools are always available (from Plexus activations)
            tools: Some(ToolsCapability { list_changed: true }),
            // Resources only if Arbor is available
            resources: if has_arbor {
                Some(ResourcesCapability {
                    subscribe: true,
                    list_changed: true,
                })
            } else {
                None
            },
            // Prompts not yet implemented
            prompts: None,
            // Logging always available
            logging: Some(LoggingCapability {}),
        }
    }

    /// Handle the `notifications/initialized` notification (MCP-6)
    ///
    /// This completes the initialization handshake. After this,
    /// the server is fully operational and accepts all methods.
    async fn handle_initialized(&self, _params: Value) -> Result<Value, McpError> {
        // Must be in Initializing state
        self.state.require(McpState::Initializing)?;

        // Transition to Ready
        self.state.transition(McpState::Ready)?;

        tracing::info!("MCP session initialized, now accepting requests");

        // Notifications don't return a result
        // The JSON-RPC layer should not send a response for notifications
        Ok(Value::Null)
    }

    // === Utility Handlers ===

    /// Handle the `ping` request (MCP-7)
    ///
    /// Simple health check that returns an empty object.
    /// Can be used for keepalive in long-running connections.
    async fn handle_ping(&self, _params: Value) -> Result<Value, McpError> {
        // Ping works in Ready state only
        self.state.require_ready()?;

        Ok(serde_json::json!({}))
    }

    // === Tool Handlers ===

    /// Handle the `tools/list` request (MCP-5 + MCP-8)
    ///
    /// Returns a list of all available tools (Plexus activation methods).
    /// Supports pagination via cursor.
    async fn handle_tools_list(&self, params: Value) -> Result<Value, McpError> {
        // Require Ready state
        self.state.require_ready()?;

        // Parse params (cursor is optional)
        let params: ToolsListParams = serde_json::from_value(params).unwrap_or_default();

        // Get all activation schemas and transform to MCP tools
        let schemas = self.plexus.list_full_schemas();
        let all_tools = schemas_to_mcp_tools(&schemas);

        // Handle pagination (50 tools per page)
        let (tools, next_cursor) = self.paginate(all_tools, params.cursor.as_deref(), 50);

        let result = ToolsListResult { tools, next_cursor };

        Ok(serde_json::to_value(result)?)
    }

    /// Paginate a list of items
    fn paginate<T>(
        &self,
        items: Vec<T>,
        cursor: Option<&str>,
        page_size: usize,
    ) -> (Vec<T>, Option<String>) {
        let start = cursor
            .and_then(|c| c.parse::<usize>().ok())
            .unwrap_or(0);

        let page: Vec<T> = items.into_iter().skip(start).take(page_size).collect();
        let count = page.len();
        let next = if count == page_size {
            Some((start + page_size).to_string())
        } else {
            None
        };

        (page, next)
    }

    /// Handle the `tools/call` request (MCP-9)
    ///
    /// Invokes a tool (Plexus activation method) and returns the result.
    /// The response contains content items (text, image, or resource) and
    /// an isError flag if the tool execution failed.
    async fn handle_tools_call(&self, params: Value) -> Result<Value, McpError> {
        // Require Ready state
        self.state.require_ready()?;

        // Parse params
        let params: ToolsCallParams = serde_json::from_value(params)?;

        tracing::debug!(
            tool = %params.name,
            "Executing tool call"
        );

        // Call the Plexus method
        // Tool names are in format "namespace.method" (e.g., "bash.execute")
        let mut stream = self.plexus.call(&params.name, params.arguments).await?;

        // Collect stream results
        let mut content: Vec<ToolContent> = Vec::new();
        let mut is_error = false;
        let mut error_messages: Vec<String> = Vec::new();

        while let Some(item) = stream.next().await {
            match item.event {
                PlexusStreamEvent::Data { data, content_type, .. } => {
                    // Convert data to text content
                    let text = if content_type == "text/plain" {
                        // If it's plain text, try to extract string directly
                        data.as_str().map(|s| s.to_string()).unwrap_or_else(|| {
                            serde_json::to_string_pretty(&data).unwrap_or_default()
                        })
                    } else {
                        // For other types, serialize to JSON
                        serde_json::to_string_pretty(&data).unwrap_or_default()
                    };
                    content.push(ToolContent::Text { text });
                }
                PlexusStreamEvent::Error { error, .. } => {
                    is_error = true;
                    error_messages.push(error);
                }
                PlexusStreamEvent::Guidance { error_type, suggestion, .. } => {
                    // Convert guidance to error message
                    is_error = true;
                    let msg = format!(
                        "Guidance: {:?} - {:?}",
                        error_type, suggestion
                    );
                    error_messages.push(msg);
                }
                PlexusStreamEvent::Progress { message, percentage, .. } => {
                    // Log progress but don't add to content
                    tracing::trace!(
                        tool = %params.name,
                        message = %message,
                        percentage = ?percentage,
                        "Tool progress"
                    );
                }
                PlexusStreamEvent::Done { .. } => {
                    // Stream complete
                    tracing::debug!(tool = %params.name, "Tool call complete");
                }
            }
        }

        // If we had errors, add them as text content
        if !error_messages.is_empty() {
            for msg in error_messages {
                content.push(ToolContent::Text { text: msg });
            }
        }

        // Ensure we have at least one content item
        if content.is_empty() {
            content.push(ToolContent::Text {
                text: "Tool executed successfully (no output)".to_string(),
            });
        }

        let result = ToolsCallResult { content, is_error };
        Ok(serde_json::to_value(result)?)
    }

    // === Resource Handlers (stubs - implemented in MCP-11) ===

    async fn handle_resources_list(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("resources/list".to_string()))
    }

    async fn handle_resources_read(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("resources/read".to_string()))
    }

    // === Prompt Handlers (stubs - implemented in MCP-12) ===

    async fn handle_prompts_list(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("prompts/list".to_string()))
    }

    async fn handle_prompts_get(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("prompts/get".to_string()))
    }

    // === Notification Handlers (stubs - implemented in MCP-10) ===

    async fn handle_cancelled(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("notifications/cancelled".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexus::Plexus;
    use serde_json::json;

    #[tokio::test]
    async fn test_new_interface() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        assert_eq!(mcp.server_info().name, "substrate");
        assert!(!mcp.server_info().version.is_empty());
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        let result = mcp.handle("unknown/method", Value::Null).await;
        assert!(matches!(result, Err(McpError::MethodNotFound(_))));
    }

    #[tokio::test]
    async fn test_stubs_return_not_implemented() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        // Stub methods should return NotImplemented until implemented
        // Implemented: initialize, initialized, tools/list, tools/call, ping
        let stub_methods = [
            "resources/list",
            "resources/read",
            "prompts/list",
            "prompts/get",
            "notifications/cancelled",
        ];

        for method in stub_methods {
            let result = mcp.handle(method, Value::Null).await;
            assert!(
                matches!(result, Err(McpError::NotImplemented(_))),
                "Method {} should return NotImplemented",
                method
            );
        }
    }

    // === Initialize Tests (MCP-4) ===

    #[tokio::test]
    async fn test_initialize_success() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });

        let result = mcp.handle("initialize", params).await.unwrap();

        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "substrate");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn test_initialize_unsupported_version() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        let params = json!({
            "protocolVersion": "1999-01-01",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });

        let result = mcp.handle("initialize", params).await;
        assert!(matches!(result, Err(McpError::UnsupportedVersion(_))));
    }

    #[tokio::test]
    async fn test_initialize_wrong_state() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });

        // First initialize should succeed
        mcp.handle("initialize", params.clone()).await.unwrap();

        // Second initialize should fail (already initializing)
        let result = mcp.handle("initialize", params).await;
        assert!(matches!(result, Err(McpError::State(_))));
    }

    #[tokio::test]
    async fn test_initialize_transitions_state() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        assert_eq!(mcp.state().current(), McpState::Uninitialized);

        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });

        mcp.handle("initialize", params).await.unwrap();

        assert_eq!(mcp.state().current(), McpState::Initializing);
    }

    // === Initialized Tests (MCP-6) ===

    #[tokio::test]
    async fn test_initialized_success() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        // First initialize
        let init_params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });
        mcp.handle("initialize", init_params).await.unwrap();

        // Then send initialized notification
        let result = mcp.handle("notifications/initialized", Value::Null).await.unwrap();

        // Notifications return null
        assert_eq!(result, Value::Null);
        // State should now be Ready
        assert_eq!(mcp.state().current(), McpState::Ready);
    }

    #[tokio::test]
    async fn test_initialized_wrong_state_uninitialized() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        // Try to call initialized without initialize first
        let result = mcp.handle("notifications/initialized", Value::Null).await;
        assert!(matches!(result, Err(McpError::State(_))));
    }

    #[tokio::test]
    async fn test_initialized_wrong_state_already_ready() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        // Complete handshake
        let init_params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });
        mcp.handle("initialize", init_params).await.unwrap();
        mcp.handle("notifications/initialized", Value::Null).await.unwrap();

        // Try to call initialized again
        let result = mcp.handle("notifications/initialized", Value::Null).await;
        assert!(matches!(result, Err(McpError::State(_))));
    }

    /// Helper to complete the full MCP handshake
    async fn complete_handshake(mcp: &McpInterface) {
        let init_params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });
        mcp.handle("initialize", init_params).await.unwrap();
        mcp.handle("notifications/initialized", Value::Null).await.unwrap();
    }

    // === Ping Tests (MCP-7) ===

    #[tokio::test]
    async fn test_ping_requires_ready() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        // Without handshake, should fail
        let result = mcp.handle("ping", Value::Null).await;
        assert!(matches!(result, Err(McpError::State(_))));
    }

    #[tokio::test]
    async fn test_ping_success() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);
        complete_handshake(&mcp).await;

        let result = mcp.handle("ping", Value::Null).await.unwrap();

        // Ping returns empty object
        assert_eq!(result, json!({}));
    }

    // === Tools List Tests (MCP-5 + MCP-8) ===

    #[tokio::test]
    async fn test_tools_list_requires_ready() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        // Without handshake, should fail
        let result = mcp.handle("tools/list", Value::Null).await;
        assert!(matches!(result, Err(McpError::State(_))));
    }

    #[tokio::test]
    async fn test_tools_list_empty() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);
        complete_handshake(&mcp).await;

        let result = mcp.handle("tools/list", Value::Null).await.unwrap();

        assert!(result["tools"].is_array());
        assert_eq!(result["tools"].as_array().unwrap().len(), 0);
        assert!(result["nextCursor"].is_null());
    }

    #[tokio::test]
    async fn test_tools_list_with_cursor_param() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);
        complete_handshake(&mcp).await;

        // With cursor param
        let result = mcp
            .handle("tools/list", json!({ "cursor": "0" }))
            .await
            .unwrap();

        assert!(result["tools"].is_array());
    }

    // === Tools Call Tests (MCP-9) ===

    #[tokio::test]
    async fn test_tools_call_requires_ready() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);

        // Without handshake, should fail
        let result = mcp
            .handle("tools/call", json!({ "name": "health.check", "arguments": {} }))
            .await;
        assert!(matches!(result, Err(McpError::State(_))));
    }

    #[tokio::test]
    async fn test_tools_call_unknown_tool() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);
        complete_handshake(&mcp).await;

        // Call a non-existent tool
        let result = mcp
            .handle(
                "tools/call",
                json!({ "name": "nonexistent.method", "arguments": {} }),
            )
            .await
            .unwrap();

        // Should return result with isError: true and guidance
        assert!(result["isError"].as_bool().unwrap_or(false));
        assert!(result["content"].is_array());
    }

    #[tokio::test]
    async fn test_tools_call_invalid_params() {
        let plexus = Arc::new(Plexus::new());
        let mcp = McpInterface::new(plexus);
        complete_handshake(&mcp).await;

        // Missing required 'name' field - serde deserialization fails
        let result = mcp.handle("tools/call", json!({})).await;
        // Serde errors become Serialization errors
        assert!(matches!(result, Err(McpError::Serialization(_))));
    }
}

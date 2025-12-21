//! MCP Interface
//!
//! The main MCP interface that wraps Plexus and routes MCP protocol methods.

use std::sync::Arc;

use serde_json::Value;

use super::{
    error::McpError,
    state::{McpState, McpStateMachine},
    types::{
        InitializeParams, InitializeResult, LoggingCapability, ResourcesCapability,
        ServerCapabilities, ServerInfo, ToolsCapability, SUPPORTED_VERSIONS,
    },
};
use crate::plexus::Plexus;

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

    async fn handle_initialized(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("notifications/initialized".to_string()))
    }

    // === Utility Handlers (stubs - implemented in MCP-7) ===

    async fn handle_ping(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("ping".to_string()))
    }

    // === Tool Handlers (stubs - implemented in MCP-5, MCP-9) ===

    async fn handle_tools_list(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("tools/list".to_string()))
    }

    async fn handle_tools_call(&self, _params: Value) -> Result<Value, McpError> {
        Err(McpError::NotImplemented("tools/call".to_string()))
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
        // Note: initialize is implemented (MCP-4)
        let stub_methods = [
            "notifications/initialized",
            "ping",
            "tools/list",
            "tools/call",
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
}

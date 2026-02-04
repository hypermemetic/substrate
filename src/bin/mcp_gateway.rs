//! MCP Gateway - Stable MCP server that proxies to Plexus
//!
//! This binary runs an MCP HTTP server that routes tool calls to Plexus
//! over JSON-RPC WebSocket. It maintains client connections even when
//! Plexus restarts.
//!
//! Architecture:
//! ```
//! Claude Code <--MCP HTTP--> MCP Gateway <--JSON-RPC WS--> Substrate/Plexus
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::any,
};
use clap::Parser;
use jsonrpsee::{
    core::{client::SubscriptionClientT, params::ObjectParams},
    ws_client::{WsClient, WsClientBuilder},
};
use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
    transport::streamable_http_server::{
        session::local::LocalSessionManager,
        StreamableHttpServerConfig, StreamableHttpService,
    },
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

/// CLI arguments for MCP gateway
#[derive(Parser, Debug)]
#[command(name = "mcp-gateway")]
#[command(about = "MCP Gateway - stable MCP server that proxies to Plexus")]
struct Args {
    /// Port for MCP HTTP server
    #[arg(short, long, default_value = "4445")]
    port: u16,

    /// Plexus WebSocket URL
    #[arg(long, default_value = "ws://127.0.0.1:4444")]
    plexus_url: String,

    /// Reconnect interval in seconds when Plexus is down
    #[arg(long, default_value = "2")]
    reconnect_interval: u64,

    /// Test mode: connect to plexus, fetch schemas, and exit
    #[arg(long)]
    test: bool,
}

/// Plexus method schema (from plexus.schema response)
#[derive(Debug, Clone, Deserialize)]
struct MethodSchema {
    name: String,
    #[serde(default)]
    description: String,
    params: Option<Value>,
}

/// Child summary (for hub plugins)
#[derive(Debug, Clone, Deserialize)]
struct ChildSummary {
    namespace: String,
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    hash: String,
}

/// Plexus plugin schema (from plexus.schema response)
#[derive(Debug, Clone, Deserialize)]
struct PluginSchema {
    namespace: String,
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    methods: Vec<MethodSchema>,
    /// Children (for hub plugins like plexus)
    children: Option<Vec<ChildSummary>>,
}

/// Stream item from plexus subscriptions (call, schema, hash, etc.)
/// All plexus streams use this unified format
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PlexusStreamItem {
    Progress {
        message: String,
        percentage: Option<f32>,
    },
    Data {
        content: Value,
        #[allow(dead_code)]
        content_type: Option<String>,
    },
    Error {
        message: String,
        recoverable: bool,
    },
    Done,
}

/// Plexus hub client with automatic reconnection
struct PlexusClient {
    url: String,
    reconnect_interval: Duration,
    client: RwLock<Option<WsClient>>,
    cached_schemas: RwLock<Vec<PluginSchema>>,
}

impl PlexusClient {
    fn new(url: String, reconnect_interval: Duration) -> Self {
        Self {
            url,
            reconnect_interval,
            client: RwLock::new(None),
            cached_schemas: RwLock::new(Vec::new()),
        }
    }

    /// Connect to Plexus, returns true if successful
    async fn connect(&self) -> bool {
        let client = WsClientBuilder::default()
            .connection_timeout(Duration::from_secs(5))
            .build(&self.url)
            .await;

        match client {
            Ok(c) => {
                tracing::info!("Connected to Plexus at {}", self.url);
                *self.client.write().await = Some(c);

                // Refresh schemas on connect
                if let Err(e) = self.refresh_schemas().await {
                    tracing::warn!("Failed to refresh schemas: {}", e);
                }

                true
            }
            Err(e) => {
                tracing::warn!("Failed to connect to Plexus: {}", e);
                false
            }
        }
    }

    /// Ensure we have a connection, reconnecting if needed
    async fn ensure_connected(&self) -> Result<(), String> {
        // Check if current connection is alive
        {
            let client = self.client.read().await;
            if let Some(c) = client.as_ref() {
                if c.is_connected() {
                    return Ok(());
                }
            }
        }

        // Need to reconnect
        tracing::info!("Reconnecting to Plexus...");
        *self.client.write().await = None;

        // Try to connect with retries
        for attempt in 1..=3 {
            if self.connect().await {
                return Ok(());
            }
            if attempt < 3 {
                tokio::time::sleep(self.reconnect_interval).await;
            }
        }

        Err("Failed to connect to Plexus after 3 attempts".to_string())
    }

    /// Fetch a single plugin schema by namespace (routes through plexus.call)
    async fn fetch_plugin_schema(&self, namespace: &str) -> Result<Option<PluginSchema>, String> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or("Not connected")?;

        // Route through plexus.call to get child schemas
        let method = format!("{}.schema", namespace);

        // Use ObjectParams for named params (object), not positional (array)
        let mut params = ObjectParams::new();
        params.insert("method", &method).map_err(|e| format!("Failed to build params: {}", e))?;

        let mut sub = client
            .subscribe::<PlexusStreamItem, _>(
                "plexus.call",
                params,
                "plexus.call_unsub",
            )
            .await
            .map_err(|e| format!("Failed to call {}: {}", method, e))?;

        while let Some(result) = sub.next().await {
            match result {
                Ok(PlexusStreamItem::Data { content, .. }) => {
                    if let Ok(schema) = serde_json::from_value::<PluginSchema>(content) {
                        return Ok(Some(schema));
                    }
                }
                Ok(PlexusStreamItem::Done) => break,
                Ok(PlexusStreamItem::Error { message, .. }) => {
                    tracing::warn!("{} schema error: {}", namespace, message);
                    break;
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// Fetch the root plexus schema directly (it's registered as plexus.schema subscription)
    async fn fetch_plexus_schema(&self) -> Result<Option<PluginSchema>, String> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or("Not connected")?;

        let mut sub = client
            .subscribe::<PlexusStreamItem, _>(
                "plexus.schema",
                jsonrpsee::rpc_params![],
                "plexus.schema_unsub",
            )
            .await
            .map_err(|e| format!("Failed to subscribe to plexus.schema: {}", e))?;

        while let Some(result) = sub.next().await {
            match result {
                Ok(PlexusStreamItem::Data { content, .. }) => {
                    if let Ok(schema) = serde_json::from_value::<PluginSchema>(content) {
                        return Ok(Some(schema));
                    }
                }
                Ok(PlexusStreamItem::Done) => break,
                Ok(PlexusStreamItem::Error { message, .. }) => {
                    tracing::warn!("plexus.schema error: {}", message);
                    break;
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// Refresh schemas from Plexus (fetches plexus schema + all child schemas)
    async fn refresh_schemas(&self) -> Result<(), String> {
        let mut schemas = Vec::new();

        // First get plexus schema directly (it's registered as plexus.schema subscription)
        let plexus_schema = self.fetch_plexus_schema().await?;

        if let Some(schema) = plexus_schema {
            // Collect child namespaces before adding plexus schema
            let children: Vec<String> = schema.children
                .as_ref()
                .map(|c| c.iter().map(|cs| cs.namespace.clone()).collect())
                .unwrap_or_default();

            schemas.push(schema);

            // Fetch full schema for each child plugin
            for child_ns in children {
                match self.fetch_plugin_schema(&child_ns).await {
                    Ok(Some(child_schema)) => schemas.push(child_schema),
                    Ok(None) => tracing::warn!("No schema returned for {}", child_ns),
                    Err(e) => tracing::warn!("Failed to fetch {} schema: {}", child_ns, e),
                }
            }
        }

        tracing::info!("Loaded {} plugin schemas from Plexus", schemas.len());
        *self.cached_schemas.write().await = schemas;

        Ok(())
    }

    /// Get cached schemas
    async fn get_schemas(&self) -> Vec<PluginSchema> {
        self.cached_schemas.read().await.clone()
    }

    /// Call a method on Plexus
    async fn call(&self, method: &str, params: Value) -> Result<Vec<PlexusStreamItem>, String> {
        self.ensure_connected().await?;

        let client = self.client.read().await;
        let client = client.as_ref().ok_or("Not connected")?;

        // Use ObjectParams for named parameters
        let mut rpc_params = ObjectParams::new();
        rpc_params.insert("method", method).map_err(|e| format!("Failed to build params: {}", e))?;
        rpc_params.insert("params", &params).map_err(|e| format!("Failed to build params: {}", e))?;

        let mut sub = client
            .subscribe::<PlexusStreamItem, _>(
                "plexus.call",
                rpc_params,
                "plexus.call_unsub",
            )
            .await
            .map_err(|e| format!("Failed to call {}: {}", method, e))?;

        let mut results = Vec::new();

        while let Some(result) = sub.next().await {
            match result {
                Ok(item) => {
                    let is_done = matches!(item, PlexusStreamItem::Done);
                    results.push(item);
                    if is_done {
                        break;
                    }
                }
                Err(e) => {
                    return Err(format!("Error during call: {}", e));
                }
            }
        }

        Ok(results)
    }
}

/// MCP handler that bridges to Plexus hub via JSON-RPC
#[derive(Clone)]
struct PlexusGatewayBridge {
    hub: Arc<PlexusClient>,
}

impl PlexusGatewayBridge {
    fn new(hub: Arc<PlexusClient>) -> Self {
        Self { hub }
    }
}

/// Convert Plexus schemas to MCP tools
fn schemas_to_tools(schemas: &[PluginSchema]) -> Vec<Tool> {
    schemas
        .iter()
        .flat_map(|plugin| {
            let namespace = &plugin.namespace;
            plugin.methods.iter().map(move |method| {
                let name = format!("{}.{}", namespace, method.name);
                let description = method.description.clone();

                let input_schema = method
                    .params
                    .as_ref()
                    .and_then(|s| s.as_object().cloned())
                    .map(|mut obj| {
                        if !obj.contains_key("type") {
                            obj.insert("type".to_string(), json!("object"));
                        }
                        Arc::new(obj)
                    })
                    .unwrap_or_else(|| {
                        Arc::new(serde_json::Map::from_iter([(
                            "type".to_string(),
                            json!("object"),
                        )]))
                    });

                Tool::new(name, description, input_schema)
            })
        })
        .collect()
}

impl ServerHandler for PlexusGatewayBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_logging()
                .build(),
            server_info: Implementation {
                name: "mcp-gateway".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(
                "MCP Gateway - proxies to Plexus. Survives Plexus restarts.".into(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // Try to refresh schemas (best effort)
        if let Err(e) = self.hub.ensure_connected().await {
            tracing::warn!("Could not connect to Plexus for schema refresh: {}", e);
        } else if let Err(e) = self.hub.refresh_schemas().await {
            tracing::warn!("Could not refresh schemas: {}", e);
        }

        let schemas = self.hub.get_schemas().await;
        let tools = schemas_to_tools(&schemas);

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
            .map(Value::Object)
            .unwrap_or(json!({}));

        tracing::debug!("Gateway calling tool: {} with args: {:?}", method_name, arguments);

        // Call Plexus hub
        let results = self
            .hub
            .call(method_name, arguments)
            .await
            .map_err(|e| McpError::internal_error(e, None))?;

        // Process results
        let mut had_error = false;
        let mut buffered_data: Vec<Value> = Vec::new();
        let mut error_messages: Vec<String> = Vec::new();
        let progress_token = ctx.meta.get_progress_token();
        let logger = format!("gateway.{}", method_name);

        for item in results {
            if ctx.ct.is_cancelled() {
                return Err(McpError::internal_error("Cancelled", None));
            }

            match item {
                PlexusStreamItem::Progress { message, percentage } => {
                    if let Some(ref token) = progress_token {
                        let _ = ctx
                            .peer
                            .notify_progress(ProgressNotificationParam {
                                progress_token: token.clone(),
                                progress: percentage.unwrap_or(0.0) as f64,
                                total: None,
                                message: Some(message),
                            })
                            .await;
                    }
                }
                PlexusStreamItem::Data { content, .. } => {
                    buffered_data.push(content.clone());

                    let _ = ctx
                        .peer
                        .notify_logging_message(LoggingMessageNotificationParam {
                            level: LoggingLevel::Info,
                            logger: Some(logger.clone()),
                            data: json!({
                                "type": "data",
                                "data": content,
                            }),
                        })
                        .await;
                }
                PlexusStreamItem::Error { message, recoverable } => {
                    error_messages.push(message.clone());

                    let _ = ctx
                        .peer
                        .notify_logging_message(LoggingMessageNotificationParam {
                            level: LoggingLevel::Error,
                            logger: Some(logger.clone()),
                            data: json!({
                                "type": "error",
                                "error": message,
                                "recoverable": recoverable,
                            }),
                        })
                        .await;

                    if !recoverable {
                        had_error = true;
                    }
                }
                PlexusStreamItem::Done => break,
            }
        }

        // Build response
        if had_error {
            let error_content = if error_messages.is_empty() {
                "Stream completed with errors".to_string()
            } else {
                error_messages.join("\n")
            };
            Ok(CallToolResult::error(vec![Content::text(error_content)]))
        } else {
            let text_content = if buffered_data.is_empty() {
                "(no output)".to_string()
            } else if buffered_data.len() == 1 {
                match &buffered_data[0] {
                    Value::String(s) => s.clone(),
                    other => serde_json::to_string_pretty(other).unwrap_or_default(),
                }
            } else {
                let all_strings = buffered_data.iter().all(|v| v.is_string());
                if all_strings {
                    buffered_data
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join("")
                } else {
                    serde_json::to_string_pretty(&buffered_data).unwrap_or_default()
                }
            };

            Ok(CallToolResult::success(vec![Content::text(text_content)]))
        }
    }
}

/// Middleware to log HTTP requests
async fn log_request_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();

    tracing::debug!("MCP Gateway: {} {}", method, uri);

    next.run(request).await
}

/// Debug endpoint
async fn debug_handler() -> impl IntoResponse {
    let info = r#"{
  "server": "mcp-gateway",
  "description": "MCP Gateway - proxies to Plexus",
  "mcp_endpoint": "/mcp",
  "features": [
    "Maintains MCP connections through Plexus restarts",
    "Automatic reconnection to Plexus",
    "Cached tool schemas"
  ]
}"#;

    (StatusCode::OK, [("content-type", "application/json")], info)
}

/// Fallback handler
async fn fallback_handler(request: Request) -> impl IntoResponse {
    let uri = request.uri().clone();
    tracing::warn!("MCP Gateway: Unmatched route: {}", uri);

    (
        StatusCode::NOT_FOUND,
        [("content-type", "application/json")],
        r#"{"error": "Not found", "hint": "MCP endpoint is at /mcp"}"#,
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,mcp_gateway=debug"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    if args.test {
        tracing::info!("Test mode: connecting to Plexus at {}", args.plexus_url);
    } else {
        tracing::info!("Starting MCP Gateway");
        tracing::info!("  MCP HTTP port: {}", args.port);
        tracing::info!("  Plexus URL: {}", args.plexus_url);
    }

    // Create Plexus hub client
    let hub_client = Arc::new(PlexusClient::new(
        args.plexus_url.clone(),
        Duration::from_secs(args.reconnect_interval),
    ));

    // Initial connection
    if !hub_client.connect().await {
        if args.test {
            tracing::error!("Failed to connect to Plexus");
            std::process::exit(1);
        }
        tracing::warn!("Could not connect to Plexus on startup - will retry on requests");
    }

    // Test mode: fetch schemas and exit
    if args.test {
        tracing::info!("Connected! Fetching schemas...");

        let schemas = hub_client.get_schemas().await;
        let total_methods: usize = schemas.iter().map(|s| s.methods.len()).sum();
        tracing::info!("Plugins: {}, Total methods: {}", schemas.len(), total_methods);

        for schema in &schemas {
            tracing::info!("  {} - {} methods", schema.namespace, schema.methods.len());
            for method in &schema.methods {
                tracing::info!("    - {}", method.name);
            }
        }

        // Convert to MCP tools
        let tools = schemas_to_tools(&schemas);
        tracing::info!("MCP tools: {}", tools.len());
        for tool in &tools {
            tracing::info!("  - {}", tool.name);
        }

        tracing::info!("Test complete!");
        return Ok(());
    }

    // Create MCP bridge
    let bridge = PlexusGatewayBridge::new(hub_client.clone());

    // Log available tools
    let schemas = hub_client.get_schemas().await;
    let total_methods: usize = schemas.iter().map(|s| s.methods.len()).sum();
    tracing::info!("Plugins: {}, Methods: {}", schemas.len(), total_methods);
    for schema in &schemas {
        tracing::info!("  {} ({} methods)", schema.namespace, schema.methods.len());
    }

    // Create MCP service
    let config = StreamableHttpServerConfig::default();
    let session_manager = LocalSessionManager::default().into();
    let bridge_clone = bridge.clone();
    let mcp_service = StreamableHttpService::new(
        move || Ok(bridge_clone.clone()),
        session_manager,
        config,
    );

    // Build axum router
    let app = axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .route("/debug", any(debug_handler))
        .fallback(fallback_handler)
        .layer(middleware::from_fn(log_request_middleware));

    // Start server
    let addr: SocketAddr = format!("127.0.0.1:{}", args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("MCP Gateway listening on http://{}/mcp", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

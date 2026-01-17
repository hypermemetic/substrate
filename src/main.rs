use substrate::{build_plexus, PlexusMcpBridge, SqliteSessionManager, SqliteSessionConfig};
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::RpcModule;
use clap::Parser;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::any,
};

/// CLI arguments for substrate
#[derive(Parser, Debug)]
#[command(name = "substrate")]
#[command(about = "Substrate plexus server - JSON-RPC over WebSocket or stdio")]
struct Args {
    /// Run in stdio mode for MCP compatibility (line-delimited JSON-RPC over stdin/stdout)
    #[arg(long)]
    stdio: bool,

    /// Port for WebSocket server (ignored in stdio mode)
    #[arg(short, long, default_value = "4444")]
    port: u16,

    /// Disable built-in MCP HTTP server (use mcp-gateway instead)
    #[arg(long)]
    no_mcp: bool,
}

/// Middleware to log all incoming HTTP requests
async fn log_request_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();

    tracing::info!("▶▶▶ MCP HTTP REQUEST ▶▶▶");
    tracing::info!("  Method: {}", method);
    tracing::info!("  URI: {}", uri);
    tracing::info!("  Headers:");
    for (name, value) in headers.iter() {
        tracing::info!("    {}: {:?}", name, value);
    }

    let response = next.run(request).await;

    let status = response.status();
    tracing::info!("◀◀◀ MCP HTTP RESPONSE ◀◀◀");
    tracing::info!("  Status: {}", status);

    response
}

/// Fallback handler for unmatched routes - logs and returns debug info
async fn fallback_handler(request: Request) -> impl IntoResponse {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();

    tracing::error!("╔══════════════════════════════════════════════════════════╗");
    tracing::error!("║  UNMATCHED REQUEST - NO ROUTE FOUND                      ║");
    tracing::error!("╚══════════════════════════════════════════════════════════╝");
    tracing::error!("  Method: {}", method);
    tracing::error!("  URI: {}", uri);
    tracing::error!("  Path: {}", uri.path());
    tracing::error!("  Query: {:?}", uri.query());
    tracing::error!("  Headers:");
    for (name, value) in headers.iter() {
        tracing::error!("    {}: {:?}", name, value);
    }

    // Try to read body for POST requests
    let body_hint = if method == axum::http::Method::POST {
        "(body not captured in fallback - check /mcp endpoint)"
    } else {
        "(no body expected)"
    };
    tracing::error!("  Body: {}", body_hint);
    tracing::error!("");
    tracing::error!("  HINT: MCP endpoint is at /mcp");
    tracing::error!("  HINT: Make sure to send 'initialize' request first!");

    let debug_response = format!(
        r#"{{
  "error": "Route not found",
  "received": {{
    "method": "{}",
    "uri": "{}",
    "path": "{}"
  }},
  "hint": "MCP endpoint is at /mcp. Send 'initialize' request first.",
  "available_endpoints": ["/mcp", "/debug"]
}}"#,
        method, uri, uri.path()
    );

    (StatusCode::NOT_FOUND, [("content-type", "application/json")], debug_response)
}

/// Debug endpoint that returns server info
async fn debug_handler() -> impl IntoResponse {
    tracing::info!("Debug endpoint hit");

    let info = r#"{
  "server": "substrate-plexus",
  "mcp_endpoint": "/mcp",
  "mcp_protocol": "MCP Streamable HTTP (2025-03-26)",
  "notes": [
    "MCP requires 'initialize' request before 'tools/list'",
    "Accept header must include 'application/json, text/event-stream'",
    "Tool names use format: namespace.method (e.g., 'loopback.permit')",
    "Claude Code references tools as: mcp__<server>__<tool_name>"
  ]
}"#;

    (StatusCode::OK, [("content-type", "application/json")], info)
}

/// Serve RPC module over stdio (MCP-compatible transport)
async fn serve_stdio(module: RpcModule<()>) -> anyhow::Result<()> {
    tracing::info!("Substrate plexus started in stdio mode (MCP-compatible)");

    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        tracing::debug!("Received request: {}", trimmed);

        // Call the same RpcModule used by WebSocket server
        // Buffer size of 1024 messages for subscription notifications
        let (response, mut sub_receiver) = module
            .raw_json_request(trimmed, 1024)
            .await
            .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

        // Write initial response to stdout
        let response_str = response.get();
        stdout.write_all(response_str.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;

        tracing::debug!("Sent response: {}", response_str);

        // Spawn task to forward subscription notifications (if any)
        // The receiver will be empty for non-subscription responses
        tokio::spawn(async move {
            while let Some(notification) = sub_receiver.recv().await {
                let notification_str = notification.get();
                tracing::debug!("Forwarding notification: {}", notification_str);

                // Get a new stdout handle for each notification
                let mut out = tokio::io::stdout();
                if out.write_all(notification_str.as_bytes()).await.is_err() {
                    break;
                }
                if out.write_all(b"\n").await.is_err() {
                    break;
                }
                if out.flush().await.is_err() {
                    break;
                }
            }
        });
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Load .env file if present (silently ignore if not found)
    dotenvy::dotenv().ok();

    // Initialize tracing with filtering
    // In debug builds, enable debug logging for substrate by default
    // In stdio mode, reduce verbosity to avoid polluting stdout
    let filter = if args.stdio {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "substrate=warn,jsonrpsee=warn"
                )
            })
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| {
                // Set base level to warn, then enable specific modules
                // This hides sqlx and other noisy deps by default
                #[cfg(debug_assertions)]
                let default_filter = "warn,substrate=trace,hub_macro=trace";
                #[cfg(not(debug_assertions))]
                let default_filter = "warn,substrate=debug,hub_macro=debug";
                tracing_subscriber::EnvFilter::new(default_filter)
            })
    };

    // In stdio mode, send logs to stderr to keep stdout clean for JSON-RPC
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // Log start time first
    tracing::info!("Starting substrate at {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"));

    // Log level calibration sequence
    tracing::error!("▓▓▓ SUBSTRATE BOOT SEQUENCE ▓▓▓");
    tracing::warn!("  ├─ warn  :: caution signals armed");
    tracing::info!("  ├─ info  :: telemetry online");
    tracing::debug!("  ├─ debug :: introspection enabled");
    tracing::trace!("  └─ trace :: full observability unlocked");

    // Build plexus (returns Arc<Plexus>)
    let plexus = build_plexus().await;
    let activations = plexus.list_activations_info();
    let methods = plexus.list_methods();
    let plexus_hash = plexus.compute_hash();

    // Convert plexus to RPC module for JSON-RPC server (consumes plexus)
    // Arc::try_unwrap extracts the inner Plexus if this is the only reference
    let module = match Arc::try_unwrap(plexus) {
        Ok(p) => p.into_rpc_module()?,
        Err(_) => panic!("plexus has multiple references - cannot convert to RPC module"),
    };

    // Choose transport based on CLI flag
    if args.stdio {
        // Stdio transport (MCP-compatible)
        serve_stdio(module).await
    } else {
        // WebSocket transport (default) + optional MCP HTTP endpoint
        let ws_addr: SocketAddr = format!("127.0.0.1:{}", args.port).parse()?;

        // Start WebSocket server for Plexus RPC
        let ws_server = Server::builder()
            .build(ws_addr)
            .await?;
        let ws_handle: ServerHandle = ws_server.start(module);

        tracing::info!("Substrate plexus started");
        tracing::info!("  WebSocket: ws://{}", ws_addr);

        // Optionally start MCP HTTP server (disabled with --no-mcp)
        let mcp_addr: SocketAddr = format!("127.0.0.1:{}", args.port + 1).parse()?;

        let mcp_handle = if !args.no_mcp {

            // Build MCP interface with a fresh Plexus (since module consumed the first one)
            let mcp_plexus = build_plexus().await;
            let mcp_bridge = PlexusMcpBridge::new(mcp_plexus);

            // Create StreamableHttpService for MCP with persistent SQLite sessions
            let config = StreamableHttpServerConfig::default();
            let session_manager = SqliteSessionManager::new(SqliteSessionConfig::default())
                .await
                .expect("Failed to initialize SQLite session manager");
            let bridge_clone = mcp_bridge.clone();
            let mcp_service = StreamableHttpService::new(
                move || Ok(bridge_clone.clone()),
                session_manager.into(),
                config,
            );

            // Build axum router with MCP at /mcp, debug endpoint, and request logging
            let mcp_app = axum::Router::new()
                .nest_service("/mcp", mcp_service)
                .route("/debug", any(debug_handler))
                .fallback(fallback_handler)
                .layer(middleware::from_fn(log_request_middleware));

            // Start MCP HTTP server
            let mcp_listener = tokio::net::TcpListener::bind(mcp_addr).await?;
            let handle = tokio::spawn(async move {
                axum::serve(mcp_listener, mcp_app).await
            });

            tracing::info!("  MCP HTTP:  http://{}/mcp", mcp_addr);
            Some(handle)
        } else {
            tracing::info!("  MCP HTTP:  disabled (use mcp-gateway instead)");
            None
        };

        tracing::info!("Plexus hash: {}", plexus_hash);
        tracing::info!("");
        tracing::info!("Activations ({}):", activations.len());
        for activation in &activations {
            tracing::info!("  {} v{} - {}",
                activation.namespace,
                activation.version,
                activation.description
            );
            for method in &activation.methods {
                tracing::info!("    - {}_{}", activation.namespace, method);
            }
        }
        tracing::info!("");
        tracing::info!("Total methods: {}", methods.len());

        // Wait for servers to stop
        match mcp_handle {
            Some(mcp) => {
                tokio::select! {
                    _ = ws_handle.stopped() => {
                        tracing::info!("WebSocket server stopped");
                    }
                    result = mcp => {
                        match result {
                            Ok(Ok(())) => tracing::info!("MCP server stopped"),
                            Ok(Err(e)) => tracing::error!("MCP server error: {}", e),
                            Err(e) => tracing::error!("MCP server task failed: {}", e),
                        }
                    }
                }
            }
            None => {
                // Only WebSocket server running
                ws_handle.stopped().await;
                tracing::info!("WebSocket server stopped");
            }
        }

        Ok(())
    }
}

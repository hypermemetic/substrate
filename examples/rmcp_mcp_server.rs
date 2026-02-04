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

use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use substrate::{build_plexus_rpc, PlexusMcpBridge};

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
    tracing::info!("Building Plexus hub with all activations...");

    // Build Plexus hub with all activations (returns Arc<DynamicHub>)
    let hub = build_plexus_rpc().await;
    let methods = hub.list_methods();
    tracing::info!("Plexus ready with {} methods", methods.len());
    for method in &methods {
        tracing::debug!("  - {}", method);
    }

    // Create the handler using the library's PlexusMcpBridge
    let handler = PlexusMcpBridge::new(hub);

    // Create StreamableHttpService
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

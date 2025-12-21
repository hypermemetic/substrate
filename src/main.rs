use substrate::{
    plexus::Plexus,
    activations::{
        bash::Bash,
        health::Health,
        arbor::{ArborConfig, ArborStorage, Arbor},
        cone::{ConeStorageConfig, Cone},
        claudecode::{ClaudeCode, ClaudeCodeStorage, ClaudeCodeStorageConfig},
    },
    mcp::{McpInterface, transport::mcp_router},
};
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::RpcModule;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Get the substrate data directory in the current working directory
fn substrate_data_dir() -> PathBuf {
    let cwd = std::env::current_dir().expect("Failed to get current working directory");
    cwd.join(".substrate")
}

/// Ensure the substrate data directory exists and return paths for databases
fn init_data_dir() -> std::io::Result<(PathBuf, PathBuf, PathBuf)> {
    let data_dir = substrate_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    let arbor_db = data_dir.join("arbor.db");
    let cone_db = data_dir.join("cone.db");
    let claudecode_db = data_dir.join("claudecode.db");

    Ok((arbor_db, cone_db, claudecode_db))
}

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
}

/// Serve RPC module over stdio (MCP-compatible transport)
async fn serve_stdio(module: RpcModule<()>) -> anyhow::Result<()> {
    tracing::info!("Substrate plexus started in stdio mode (MCP-compatible)");
    tracing::info!("Data directory: {}", substrate_data_dir().display());

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

/// Build the plexus with all activations registered
async fn build_plexus() -> Plexus {
    let (arbor_db, cone_db, claudecode_db) = init_data_dir()
        .expect("Failed to initialize substrate data directory");

    // Create shared arbor storage
    let arbor_config = ArborConfig {
        db_path: arbor_db,
        ..ArborConfig::default()
    };
    let arbor_storage = Arc::new(
        ArborStorage::new(arbor_config)
            .await
            .expect("Failed to initialize Arbor storage")
    );

    // Cone shares the same arbor storage
    let cone_config = ConeStorageConfig {
        db_path: cone_db,
    };

    // ClaudeCode shares the same arbor storage
    let claudecode_config = ClaudeCodeStorageConfig {
        db_path: claudecode_db,
    };
    let claudecode_storage = Arc::new(
        ClaudeCodeStorage::new(claudecode_config, arbor_storage.clone())
            .await
            .expect("Failed to initialize ClaudeCode storage")
    );

    Plexus::new()
        .register(Health::new())
        .register(Bash::new())
        .register(Arbor::with_storage(arbor_storage.clone()))
        .register(Cone::new(cone_config, arbor_storage).await.unwrap())
        .register(ClaudeCode::new(claudecode_storage))
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

    // Log level calibration sequence
    tracing::error!("▓▓▓ SUBSTRATE BOOT SEQUENCE ▓▓▓");
    tracing::warn!("  ├─ warn  :: caution signals armed");
    tracing::info!("  ├─ info  :: telemetry online");
    tracing::debug!("  ├─ debug :: introspection enabled");
    tracing::trace!("  └─ trace :: full observability unlocked");

    // Build plexus with all activations
    let plexus = build_plexus().await;
    let activations = plexus.list_activations();
    let methods = plexus.list_methods();
    let plexus_methods = plexus.list_plexus_methods();
    let plexus_hash = plexus.compute_hash();

    // Convert plexus to RPC module for JSON-RPC server (consumes plexus)
    let module = plexus.into_rpc_module()?;

    // Choose transport based on CLI flag
    if args.stdio {
        // Stdio transport (MCP-compatible)
        serve_stdio(module).await
    } else {
        // WebSocket transport (default) + MCP HTTP endpoint
        let ws_addr: SocketAddr = format!("127.0.0.1:{}", args.port).parse()?;
        let mcp_addr: SocketAddr = format!("127.0.0.1:{}", args.port + 1).parse()?;

        // Start WebSocket server for Plexus RPC
        let ws_server = Server::builder()
            .build(ws_addr)
            .await?;
        let ws_handle: ServerHandle = ws_server.start(module);

        // Build MCP interface with a fresh Plexus (since module consumed the first one)
        let mcp_plexus = Arc::new(build_plexus().await);
        let mcp_interface = Arc::new(McpInterface::new(mcp_plexus));
        let mcp_app = mcp_router(mcp_interface);

        // Start MCP HTTP server
        let mcp_listener = tokio::net::TcpListener::bind(mcp_addr).await?;
        let mcp_handle = tokio::spawn(async move {
            axum::serve(mcp_listener, mcp_app).await
        });

        tracing::info!("Substrate plexus started");
        tracing::info!("  WebSocket: ws://{}", ws_addr);
        tracing::info!("  MCP HTTP:  http://{}/mcp", mcp_addr);
        tracing::info!("Data directory: {}", substrate_data_dir().display());
        tracing::info!("Plexus hash: {}", plexus_hash);
        tracing::info!("");
        tracing::info!("Plexus methods ({}):", plexus_methods.len());
        for method in &plexus_methods {
            tracing::info!("  - {}", method);
        }
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
        tracing::info!("Total methods: {} (+{} plexus)", methods.len(), plexus_methods.len());

        // Wait for either server to stop
        tokio::select! {
            _ = ws_handle.stopped() => {
                tracing::info!("WebSocket server stopped");
            }
            result = mcp_handle => {
                match result {
                    Ok(Ok(())) => tracing::info!("MCP server stopped"),
                    Ok(Err(e)) => tracing::error!("MCP server error: {}", e),
                    Err(e) => tracing::error!("MCP server task failed: {}", e),
                }
            }
        }

        Ok(())
    }
}

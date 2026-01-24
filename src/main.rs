use substrate::build_plexus;
use hub_transport::TransportServer;
use clap::Parser;
use std::sync::Arc;

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

    // Log activation info
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

    // Configure transport server using hub-transport library
    let rpc_converter = |arc: Arc<substrate::Plexus>| {
        substrate::Plexus::arc_into_rpc_module(arc)
            .map_err(|e| anyhow::anyhow!("Failed to create RPC module: {}", e))
    };

    let mut builder = TransportServer::builder(plexus, rpc_converter);

    // Add requested transports
    if args.stdio {
        builder = builder.with_stdio();
    } else {
        builder = builder.with_websocket(args.port);

        if !args.no_mcp {
            builder = builder.with_mcp_http(args.port + 1);
        }
    }

    // Log what we're starting
    if args.stdio {
        tracing::info!("Starting stdio transport (MCP-compatible)");
    } else {
        tracing::info!("Substrate plexus started");
        tracing::info!("  WebSocket: ws://127.0.0.1:{}", args.port);
        if !args.no_mcp {
            tracing::info!("  MCP HTTP:  http://127.0.0.1:{}/mcp", args.port + 1);
        } else {
            tracing::info!("  MCP HTTP:  disabled (use mcp-gateway instead)");
        }
    }

    // Start the transport server
    builder.build().await?.serve().await
}

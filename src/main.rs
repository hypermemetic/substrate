use plexus_substrate::build_plexus_rpc;
use plexus_transport::TransportServer;
#[cfg(feature = "mcp-gateway")]
use plexus_transport::{serve_combined, RouteFn};
use clap::Parser;
#[cfg(feature = "mcp-gateway")]
use std::sync::Arc;
use daemonize::Daemonize;
use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// CLI arguments for substrate
#[derive(Parser, Debug)]
#[command(name = "substrate")]
#[command(about = "Substrate Plexus RPC server - JSON-RPC over WebSocket or stdio")]
struct Args {
    /// Run in stdio mode for MCP compatibility (line-delimited JSON-RPC over stdin/stdout)
    #[arg(long)]
    stdio: bool,

    /// Run in foreground mode (don't daemonize)
    #[arg(long)]
    fg: bool,

    /// Port for WebSocket + MCP HTTP server (ignored in stdio mode)
    #[arg(short, long, default_value = "4444")]
    port: u16,

    /// Disable built-in MCP HTTP server (WebSocket only)
    #[arg(long)]
    no_mcp: bool,

    /// Bearer token required on all WebSocket and MCP HTTP connections.
    /// Also read from the PLEXUS_API_KEY environment variable.
    /// When neither is provided, no authentication is required.
    #[arg(long, env = "PLEXUS_API_KEY")]
    api_key: Option<String>,

    /// Log directory (defaults to ./logs)
    #[arg(long)]
    log_dir: Option<PathBuf>,
}

fn setup_logging(args: &Args) -> anyhow::Result<(PathBuf, tracing_appender::non_blocking::WorkerGuard)> {
    // Determine log directory
    let log_dir = args.log_dir.clone().unwrap_or_else(|| PathBuf::from("logs"));

    // Create log directory if it doesn't exist
    std::fs::create_dir_all(&log_dir)?;

    // Clean up old log files (keep last 7 days, max 100MB total)
    cleanup_old_logs(&log_dir, 7, 100 * 1024 * 1024)?;

    // Set up rotating file appender (daily rotation)
    let file_appender = tracing_appender::rolling::daily(&log_dir, "substrate.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Determine filter level
    let filter = if args.stdio {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("substrate=warn,jsonrpsee=warn")
            })
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| {
                #[cfg(debug_assertions)]
                let default_filter = "warn,substrate=trace,plexus_macros=trace";
                #[cfg(not(debug_assertions))]
                let default_filter = "warn,substrate=debug,plexus_macros=debug";
                tracing_subscriber::EnvFilter::new(default_filter)
            })
    };

    // Create layers: one for stderr, one for file
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    // Initialize subscriber with both layers
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    // Return the current log file path and guard
    let log_file = log_dir.join(format!(
        "substrate.log.{}",
        chrono::Local::now().format("%Y-%m-%d")
    ));
    Ok((log_file, guard))
}

fn cleanup_old_logs(log_dir: &PathBuf, max_days: u64, max_total_bytes: u64) -> anyhow::Result<()> {
    use std::time::SystemTime;

    let now = SystemTime::now();
    let max_age = std::time::Duration::from_secs(max_days * 24 * 60 * 60);

    let mut total_size: u64 = 0;
    let mut log_files: Vec<_> = std::fs::read_dir(log_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().map_or(false, |ext| ext == "log") ||
            entry.file_name().to_string_lossy().starts_with("substrate.log")
        })
        .collect();

    // Sort by modification time (newest first)
    log_files.sort_by_key(|entry| {
        entry.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH)
    });
    log_files.reverse();

    for entry in log_files {
        let metadata = entry.metadata()?;
        let modified = metadata.modified()?;
        let age = now.duration_since(modified).unwrap_or_default();
        let size = metadata.len();

        // Delete if too old or if total size exceeds limit
        if age > max_age || total_size > max_total_bytes {
            std::fs::remove_file(entry.path())?;
        } else {
            total_size += size;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Load .env file if present (silently ignore if not found)
    dotenvy::dotenv().ok();

    // Set up logging and get log file path
    // Keep the guard alive for the entire program duration
    let (log_file, _guard) = setup_logging(&args)?;

    // Log start time first
    tracing::info!("Starting substrate at {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"));

    // Log level calibration sequence
    tracing::error!("▓▓▓ SUBSTRATE BOOT SEQUENCE ▓▓▓");
    tracing::warn!("  ├─ warn  :: caution signals armed");
    tracing::info!("  ├─ info  :: telemetry online");
    tracing::debug!("  ├─ debug :: introspection enabled");
    tracing::trace!("  └─ trace :: full observability unlocked");

    if args.api_key.is_some() {
        tracing::info!("Authentication: bearer token configured (--api-key / PLEXUS_API_KEY)");
    } else {
        tracing::warn!("Authentication: DISABLED — set --api-key or PLEXUS_API_KEY to require bearer tokens");
    }

    // Log file location
    tracing::info!("");
    tracing::info!("📝 Log file: {}", log_file.display());
    tracing::info!("   (Daily rotation, keeps last 7 days, max 100MB total)");

    // Build Plexus RPC hub (returns Arc<DynamicHub>)
    let hub = build_plexus_rpc().await;
    let activations = hub.list_activations_info();
    let methods = hub.list_methods();
    let plexus_hash = hub.compute_hash();

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

    // Daemonize if not in foreground mode and not in stdio mode
    if !args.fg && !args.stdio {
        tracing::info!("");
        tracing::info!("▓▓▓ DAEMONIZING ▓▓▓");
        tracing::info!("Substrate will now run in the background.");
        tracing::info!("Use --fg flag to run in foreground mode.");

        // Give logs time to flush
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let daemonize = Daemonize::new()
            .working_directory(std::env::current_dir()?)
            .umask(0o027);

        if let Err(e) = daemonize.start() {
            tracing::error!("Failed to daemonize: {}", e);
            return Err(anyhow::anyhow!("Failed to daemonize: {}", e));
        }
    }

    let rpc_converter = |arc| {
        use plexus_core::plexus::DynamicHub;
        DynamicHub::arc_into_rpc_module(arc)
            .map_err(|e| anyhow::anyhow!("Failed to create RPC module: {}", e))
    };

    if args.stdio {
        // Stdio mode: line-delimited JSON-RPC over stdin/stdout
        tracing::info!("Starting stdio transport (MCP-compatible)");
        TransportServer::builder(hub, rpc_converter)
            .with_stdio()
            .build().await?.serve().await
    } else if args.no_mcp {
        // WebSocket only
        tracing::info!("Substrate Plexus RPC server started");
        tracing::info!("  WebSocket: ws://127.0.0.1:{}", args.port);
        tracing::info!("  MCP HTTP:  disabled");
        TransportServer::builder(hub, rpc_converter)
            .with_websocket(args.port)
            .with_api_key(args.api_key)
            .build().await?.serve().await
    } else {
        #[cfg(feature = "mcp-gateway")]
        {
            // Combined WebSocket + MCP HTTP on same port
            use plexus_core::plexus::DynamicHub;
            eprintln!("[TRACE] main: calling hub.list_plugin_schemas()");
            let flat_schemas = hub.list_plugin_schemas();
            eprintln!("[TRACE] main: list_plugin_schemas() returned {} schemas", flat_schemas.len());
            eprintln!("[TRACE] main: calling DynamicHub::arc_into_rpc_module()");
            let module = DynamicHub::arc_into_rpc_module(hub.clone())
                .map_err(|e| anyhow::anyhow!("Failed to create RPC module: {}", e))?;
            eprintln!("[TRACE] main: arc_into_rpc_module() completed successfully");
            let hub_route = hub.clone();
            eprintln!("[TRACE] main: creating route_fn closure");
            let route_fn: RouteFn = Arc::new(move |method, params| {
                let hub = hub_route.clone();
                Box::pin(async move { hub.route(&method, params).await })
            });
            eprintln!("[TRACE] main: parsing socket address");
            let addr: std::net::SocketAddr = format!("127.0.0.1:{}", args.port).parse()?;
            tracing::info!("Substrate Plexus RPC server started");
            tracing::info!("  WebSocket: ws://127.0.0.1:{}", args.port);
            tracing::info!("  MCP HTTP:  http://127.0.0.1:{}/mcp", args.port);
            eprintln!("[TRACE] main: calling serve_combined()");
            let handle = serve_combined(module, hub, Some(flat_schemas), Some(route_fn), addr, args.api_key, false).await?;
            eprintln!("[TRACE] main: serve_combined() started, waiting for stopped()");
            handle.stopped().await;
            Ok(())
        }
        #[cfg(not(feature = "mcp-gateway"))]
        {
            // Fallback: WebSocket + separate MCP HTTP on next port
            tracing::info!("Substrate Plexus RPC server started");
            tracing::info!("  WebSocket: ws://127.0.0.1:{}", args.port);
            tracing::info!("  MCP HTTP:  http://127.0.0.1:{}/mcp", args.port + 1);
            TransportServer::builder(hub, rpc_converter)
                .with_websocket(args.port)
                .with_mcp_http(args.port + 1)
                .with_api_key(args.api_key)
                .build().await?.serve().await
        }
    }
}

use substrate::{
    plexus::Plexus,
    activations::{
        bash::Bash,
        health::Health,
        arbor::{ArborConfig, ArborStorage, Arbor},
        cone::{ConeStorageConfig, Cone},
    },
};
use jsonrpsee::server::{Server, ServerHandle};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

/// Get the substrate data directory in the current working directory
fn substrate_data_dir() -> PathBuf {
    let cwd = std::env::current_dir().expect("Failed to get current working directory");
    cwd.join(".substrate")
}

/// Ensure the substrate data directory exists and return paths for databases
fn init_data_dir() -> std::io::Result<(PathBuf, PathBuf)> {
    let data_dir = substrate_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    let arbor_db = data_dir.join("arbor.db");
    let cone_db = data_dir.join("cone.db");

    Ok((arbor_db, cone_db))
}

/// Build the plexus with all activations registered
async fn build_plexus() -> Plexus {
    let (arbor_db, cone_db) = init_data_dir()
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

    Plexus::new()
        .register(Health::new())
        .register(Bash::new())
        .register(Arbor::with_storage(arbor_storage.clone()))
        .register(Cone::new(cone_config, arbor_storage).await.unwrap())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (silently ignore if not found)
    dotenvy::dotenv().ok();

    // Initialize tracing with filtering
    // Show substrate and jsonrpsee, hide noisy lower-level crates entirely
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(
                "substrate=info,jsonrpsee=info,hyper=off,tokio=off,tower=off,sqlx=warn,h2=off,rustls=off"
            )
        });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    // Build plexus with all activations
    let plexus = build_plexus().await;
    let activations = plexus.list_activations();
    let methods = plexus.list_methods();
    let plexus_methods = plexus.list_plexus_methods();
    let plexus_hash = plexus.compute_hash();

    // Convert plexus to RPC module for JSON-RPC server (consumes plexus)
    let module = plexus.into_rpc_module()?;

    // Start server (guidance provided via stream events)
    let port = std::env::var("SUBSTRATE_PORT").unwrap_or_else(|_| "4444".to_string());
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    let server = Server::builder()
        .build(addr)
        .await?;
    let handle: ServerHandle = server.start(module);

    tracing::info!("Substrate plexus started at ws://{}", addr);
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

    // Keep server running
    handle.stopped().await;

    Ok(())
}

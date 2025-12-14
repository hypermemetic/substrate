use substrate::{
    plexus::Plexus,
    activations::{
        bash::Bash,
        health::Health,
        arbor::{ArborConfig, Arbor},
        cone::{ConeStorageConfig, Cone},
    },
};
use jsonrpsee::server::{Server, ServerHandle};
use std::net::SocketAddr;

/// Build the plexus with all activations registered
async fn build_plexus() -> Plexus {
    Plexus::new()
        .register(Health::new())
        .register(Bash::new())
        .register(Arbor::new(ArborConfig::default()).await.unwrap())
        .register(Cone::new(ConeStorageConfig::default()).await.unwrap())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Build plexus with all activations
    let plexus = build_plexus().await;
    let activations = plexus.list_activations();
    let methods = plexus.list_methods();

    // Convert plexus to RPC module for JSON-RPC server (consumes plexus)
    let module = plexus.into_rpc_module()?;

    // Start server
    let addr: SocketAddr = "127.0.0.1:4444".parse()?;
    let server = Server::builder().build(addr).await?;
    let handle: ServerHandle = server.start(module);

    tracing::info!("Substrate plexus started at ws://{}", addr);
    tracing::info!("");
    tracing::info!("Plexus methods:");
    tracing::info!("  - plexus_schema");
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
    tracing::info!("Total methods: {} (+1 plexus)", methods.len());

    // Keep server running
    handle.stopped().await;

    Ok(())
}

//! Simple CLI for testing plexus activations
//!
//! Simplified for caller-wraps streaming architecture refactor.
//! See docs/architecture/16680179837700061695_caller-wraps-streaming.md

use substrate::{
    plexus::DynamicHub,
    activations::health::Health,
};
use futures::StreamExt;
use serde_json::json;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build the hub with Health activation only
    let plexus = DynamicHub::new("plexus")
        .register(Health::new());

    // Get args
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: substrate-cli <command> [args...]");
        println!();
        println!("Commands:");
        println!("  list                     - List all available methods");
        println!("  activations              - List all activations with descriptions");
        println!("  help <method>            - Get help for a specific method");
        println!("  schema <namespace>       - Get enriched schema for an activation");
        println!("  <method> [params...]     - Call a method");
        println!();
        println!("Examples:");
        println!("  substrate-cli list");
        println!("  substrate-cli activations");
        println!("  substrate-cli health.check");
        return Ok(());
    }

    let command = &args[1];

    // Handle special commands
    match command.as_str() {
        "list" => {
            println!("Available methods:");
            for method in plexus.list_methods() {
                if let Some(help) = plexus.get_method_help(&method) {
                    println!("  {} - {}", method, help.lines().next().unwrap_or(""));
                } else {
                    println!("  {}", method);
                }
            }
            return Ok(());
        }
        "activations" => {
            println!("Registered activations:");
            for activation_info in plexus.list_activations_info() {
                println!("\n  {} (v{})", activation_info.namespace, activation_info.version);
                println!("    {}", activation_info.description);
                println!("    Methods: {}", activation_info.methods.join(", "));
            }
            return Ok(());
        }
        "help" => {
            if args.len() < 3 {
                println!("Usage: substrate-cli help <method>");
                return Ok(());
            }
            let method = &args[2];
            if let Some(help) = plexus.get_method_help(method) {
                println!("Help for {}:\n", method);
                println!("{}", help);
            } else {
                println!("No help available for method: {}", method);
            }
            return Ok(());
        }
        "schema" => {
            if args.len() < 3 {
                println!("Usage: substrate-cli schema <namespace>");
                println!("\nAvailable namespaces:");
                for activation_info in plexus.list_activations_info() {
                    println!("  {}", activation_info.namespace);
                }
                return Ok(());
            }
            let namespace = &args[2];
            if let Some(schema) = plexus.get_activation_schema(namespace) {
                println!("Schema for {}:\n", namespace);
                println!("{}", serde_json::to_string_pretty(&schema)?);
            } else {
                println!("Activation not found: {}", namespace);
            }
            return Ok(());
        }
        _ => {
            // Fall through to method call
        }
    }

    let method = command;

    // Build params from remaining args
    let params = if args.len() > 2 {
        serde_json::from_str(&args[2]).unwrap_or_else(|_| json!(args[2]))
    } else {
        json!(null)
    };

    println!("Calling: {} with params: {}", method, params);
    println!("---");

    // Route to the target activation and stream results
    let mut stream = plexus.route(method, params).await?;

    while let Some(item) = stream.next().await {
        println!("{}", serde_json::to_string_pretty(&item)?);
    }

    println!("---");
    println!("Stream complete");

    Ok(())
}

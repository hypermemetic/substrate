use substrate::{
    plexus::Plexus,
    activations::{bash::Bash, health::Health, arbor::{ArborConfig, Arbor}},
};
use futures::StreamExt;
use serde_json::json;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build the plexus with activations
    let arbor_config = ArborConfig::default();
    let arbor = Arbor::new(arbor_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Arbor activation: {}", e))?;

    let plexus = Plexus::new()
        .register(Health::new())
        .register(Bash::new())
        .register(arbor);

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
        println!("Method call formats:");
        println!("  1. JSON string:          substrate-cli <method> '{{\"key\": \"value\"}}'");
        println!("  2. Flag-style params:    substrate-cli <method> --key value --key2 value2");
        println!("  3. Simple string:        substrate-cli <method> 'string value'");
        println!();
        println!("Examples:");
        println!("  substrate-cli list");
        println!("  substrate-cli activations");
        println!("  substrate-cli help bash.execute");
        println!("  substrate-cli schema arbor");
        println!("  substrate-cli health.check");
        println!("  substrate-cli bash.execute 'echo hello'");
        println!("  substrate-cli arbor.tree_create --owner_id claude");
        println!("  substrate-cli arbor.tree_get --tree_id 'abc-123-def'");
        println!("  substrate-cli arbor.node_create_text --tree_id 'abc' --content 'hello' --metadata '{{\"role\":\"user\"}}'");
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
            for activation_info in plexus.list_activations() {
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
                for activation_info in plexus.list_activations() {
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
        // Check if using --key value syntax
        if args[2].starts_with("--") || args.get(3).map(|s| s.starts_with("--")).unwrap_or(false) {
            // Parse --key value pairs into JSON object
            let mut map = serde_json::Map::new();
            let mut i = 2;
            while i < args.len() {
                if args[i].starts_with("--") {
                    let key = args[i].trim_start_matches("--");
                    if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                        let value = &args[i + 1];
                        // Try to parse as JSON first, otherwise treat as string
                        let json_value = serde_json::from_str(value)
                            .unwrap_or_else(|_| json!(value));
                        map.insert(key.to_string(), json_value);
                        i += 2;
                    } else {
                        // Flag without value, treat as true
                        map.insert(key.to_string(), json!(true));
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            json!(map)
        } else {
            // Parse the JSON string
            serde_json::from_str(&args[2]).unwrap_or_else(|_| json!(args[2]))
        }
    } else {
        json!(null)
    };

    println!("Calling: {} with params: {}", method, params);
    println!("---");

    // Call the plexus and stream results
    let mut stream = plexus.call(method, params).await?;

    while let Some(item) = stream.next().await {
        // Pretty print each item
        println!("{}", serde_json::to_string_pretty(&item)?);
    }

    println!("---");
    println!("Stream complete");

    Ok(())
}

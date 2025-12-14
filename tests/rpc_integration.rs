//! Integration tests for JSON-RPC endpoints
//!
//! These tests assume a substrate server is running on 127.0.0.1:4444
//! Start the server with: cargo run
//!
//! Run tests with: cargo test --test rpc_integration -- --test-threads=1

use jsonrpsee::{
    core::client::SubscriptionClientT,
    rpc_params,
    ws_client::WsClientBuilder,
};
use serde_json::{json, Value};
use std::time::Duration;

const SERVER_URL: &str = "ws://127.0.0.1:4444";

async fn create_client() -> Result<jsonrpsee::ws_client::WsClient, Box<dyn std::error::Error>> {
    let client = WsClientBuilder::default()
        .connection_timeout(Duration::from_secs(5))
        .build(SERVER_URL)
        .await?;
    Ok(client)
}

#[tokio::test]
async fn test_health_check() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>("health_check", rpc_params![], "unsubscribe_check")
        .await
        .expect("Failed to subscribe to health_check");

    // Get at least one event
    let event = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("Timeout waiting for health event")
        .expect("Stream ended unexpectedly")
        .expect("Error receiving event");

    // Verify structure
    assert!(event.get("event").is_some(), "Event should have 'event' field");
    let data = event.get("data").expect("Event should have 'data' field");
    let inner_data = data.get("data").expect("Data should have inner 'data' field");
    assert_eq!(inner_data.get("status").and_then(|v| v.as_str()), Some("healthy"));
}

#[tokio::test]
async fn test_bash_execute() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>("bash_execute", rpc_params!["echo hello"], "unsubscribe_execute")
        .await
        .expect("Failed to subscribe to bash_execute");

    let mut events = Vec::new();

    // Collect events until timeout
    while let Ok(Some(result)) = tokio::time::timeout(Duration::from_secs(3), subscription.next()).await {
        let event = result.expect("Error receiving event");
        events.push(event);
    }

    assert!(!events.is_empty(), "Should have received at least one event, got: {:?}", events);

    // Check for stdout containing hello
    let found_output = events.iter().any(|e| {
        let data = e.get("data").and_then(|d| d.get("data"));
        data.and_then(|d| d.get("line")).and_then(|v| v.as_str())
            .map(|s| s.contains("hello"))
            .unwrap_or(false)
    });

    assert!(found_output, "Should have received 'hello' in stdout. Events: {:?}", events);
}

#[tokio::test]
async fn test_arbor_tree_create() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let metadata = json!({"test": true});
    let mut subscription = client
        .subscribe::<Value, _>(
            "arbor_tree_create",
            rpc_params![metadata, "test-owner"],
            "unsubscribe_tree_create",
        )
        .await
        .expect("Failed to subscribe to arbor_tree_create");

    let event = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("Timeout waiting for tree_create event")
        .expect("Stream ended unexpectedly")
        .expect("Error receiving event");

    // Verify we got a tree created event with a tree_id
    let data = event.get("data").expect("Event should have 'data' field");
    let inner = data.get("data").expect("Data should have inner 'data' field");
    assert!(inner.get("tree_id").is_some(), "Should have tree_id in response");
}

#[tokio::test]
async fn test_arbor_tree_list() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>("arbor_tree_list", rpc_params![], "unsubscribe_tree_list")
        .await
        .expect("Failed to subscribe to arbor_tree_list");

    let event = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("Timeout waiting for tree_list event")
        .expect("Stream ended unexpectedly")
        .expect("Error receiving event");

    // Debug: print event
    eprintln!("tree_list event: {:?}", event);

    // Check for trees array
    let data = event.get("data").expect("Event should have 'data' field");
    let inner = data.get("data").expect("Data should have inner 'data' field");
    assert!(
        inner.get("trees").is_some() || inner.get("type").is_some(),
        "Should have trees array in response: {:?}", inner
    );
}

#[tokio::test]
async fn test_arbor_full_workflow() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    // 1. Create a tree
    let mut sub = client
        .subscribe::<Value, _>(
            "arbor_tree_create",
            rpc_params![json!({"workflow_test": true}), "workflow-owner"],
            "unsubscribe_tree_create",
        )
        .await
        .expect("Failed to create tree");

    let event = tokio::time::timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout")
        .expect("No event")
        .expect("Error");

    let data = &event["data"]["data"];
    let tree_id = data["tree_id"]
        .as_str()
        .expect(&format!("No tree_id in: {:?}", data))
        .to_string();

    // 2. Get tree to find root
    let mut sub = client
        .subscribe::<Value, _>(
            "arbor_tree_get",
            rpc_params![tree_id.clone()],
            "unsubscribe_tree_get",
        )
        .await
        .expect("Failed to get tree");

    let event = tokio::time::timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout")
        .expect("No event")
        .expect("Error");

    let tree_data = &event["data"]["data"];
    let root_id = tree_data["tree"]["root"]
        .as_str()
        .expect(&format!("No root in tree: {:?}", tree_data))
        .to_string();

    // 3. Create a text node
    let mut sub = client
        .subscribe::<Value, _>(
            "arbor_node_create_text",
            rpc_params![tree_id.clone(), root_id.clone(), "Hello, world!", json!({"role": "user"})],
            "unsubscribe_node_create_text",
        )
        .await
        .expect("Failed to create node");

    let event = tokio::time::timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout")
        .expect("No event")
        .expect("Error");

    let node_data = &event["data"]["data"];
    let node_id = node_data["node_id"]
        .as_str()
        .expect(&format!("No node_id in: {:?}", node_data))
        .to_string();

    // 4. Get the node
    let mut sub = client
        .subscribe::<Value, _>(
            "arbor_node_get",
            rpc_params![tree_id.clone(), node_id.clone()],
            "unsubscribe_node_get",
        )
        .await
        .expect("Failed to get node");

    let event = tokio::time::timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout")
        .expect("No event")
        .expect("Error");

    let node_resp = &event["data"]["data"];
    let content = node_resp["node"]["data"]["content"]
        .as_str()
        .expect(&format!("No content in: {:?}", node_resp));
    assert_eq!(content, "Hello, world!");

    // 5. Get context path
    let mut sub = client
        .subscribe::<Value, _>(
            "arbor_context_get_path",
            rpc_params![tree_id.clone(), node_id.clone()],
            "unsubscribe_context_get_path",
        )
        .await
        .expect("Failed to get context path");

    let event = tokio::time::timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout")
        .expect("No event")
        .expect("Error");

    let path_data = &event["data"]["data"];
    let nodes = path_data["nodes"]
        .as_array()
        .expect(&format!("No nodes array in: {:?}", path_data));
    assert!(nodes.len() >= 2, "Path should have at least root and our node");
}

#[tokio::test]
async fn test_cone_create() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>(
            "cone_create",
            rpc_params!["test-cone", "gpt-4o-mini", "You are a test assistant.", json!(null)],
            "unsubscribe_create",
        )
        .await
        .expect("Failed to subscribe to cone_create");

    let event = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("Timeout waiting for cone_create event")
        .expect("Stream ended unexpectedly")
        .expect("Error receiving event");

    // Verify we got a cone created event
    let data = event.get("data").expect("Event should have 'data' field");
    let inner = data.get("data").expect("Data should have inner 'data' field");
    assert!(inner.get("cone_id").is_some() || inner.get("agent_id").is_some(),
            "Should have cone_id in response: {:?}", inner);
}

#[tokio::test]
async fn test_cone_list() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>("cone_list", rpc_params![], "unsubscribe_list")
        .await
        .expect("Failed to subscribe to cone_list");

    let event = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("Timeout waiting for cone_list event")
        .expect("Stream ended unexpectedly")
        .expect("Error receiving event");

    // Should have cones/agents array
    let data = event.get("data").expect("Event should have 'data' field");
    let inner = data.get("data").expect("Data should have inner 'data' field");
    assert!(
        inner.get("cones").is_some() || inner.get("agents").is_some(),
        "Should have cones array in response: {:?}", inner
    );
}

#[tokio::test]
async fn test_cone_registry() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>("cone_registry", rpc_params![], "unsubscribe_registry")
        .await
        .expect("Failed to subscribe to cone_registry");

    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("Timeout waiting for cone_registry event")
        .expect("Stream ended unexpectedly")
        .expect("Error receiving event");

    // Debug: print structure
    eprintln!("registry event keys: {:?}", event.as_object().map(|o| o.keys().collect::<Vec<_>>()));

    // Verify registry structure - the Registry variant wraps RegistryExport directly
    let data = event.get("data").expect("Event should have 'data' field");
    let inner = data.get("data").expect("Data should have inner 'data' field");

    // Should have services array
    let services = inner.get("services").expect("Should have services array");
    assert!(services.is_array(), "services should be an array");

    // Should have families array
    let families = inner.get("families").expect("Should have families array");
    assert!(families.is_array(), "families should be an array");
    let families_arr = families.as_array().unwrap();
    assert!(!families_arr.is_empty(), "Should have at least one family");

    // Should have models array
    let models = inner.get("models").expect("Should have models array");
    assert!(models.is_array(), "models should be an array");
    let models_arr = models.as_array().unwrap();
    assert!(!models_arr.is_empty(), "Should have at least one model");

    // Should have stats
    let stats = inner.get("stats").expect("Should have stats");
    assert!(stats.get("model_count").is_some(), "Stats should have model_count");

    // Verify first model has expected structure (from cllient::ModelExport)
    let first_model = &models_arr[0];
    assert!(first_model.get("id").is_some(), "Model should have id");
    assert!(first_model.get("family").is_some(), "Model should have family");
    assert!(first_model.get("service").is_some(), "Model should have service");
    assert!(first_model.get("capabilities").is_some(), "Model should have capabilities");
    assert!(first_model.get("pricing").is_some(), "Model should have pricing");
    assert!(first_model.get("status").is_some(), "Model should have verification status");

    eprintln!("Found {} services, {} families, {} models (stats: {:?})",
        services.as_array().map(|a| a.len()).unwrap_or(0),
        families_arr.len(),
        models_arr.len(),
        stats);
}

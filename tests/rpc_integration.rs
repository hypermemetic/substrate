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

/// Test that plexus_activation_schema includes required fields in params
#[tokio::test]
async fn test_schema_includes_required_in_params() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>("plexus_activation_schema", rpc_params!["arbor"], "unsubscribe_schema")
        .await
        .expect("Failed to subscribe to plexus_activation_schema");

    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("Timeout waiting for schema")
        .expect("Stream ended")
        .expect("Error receiving event");

    // Check that plexus_hash is present in the response
    let plexus_hash = event.get("plexus_hash")
        .expect("Response should include plexus_hash at top level");
    assert!(plexus_hash.is_string(), "plexus_hash should be a string");
    let hash_str = plexus_hash.as_str().unwrap();
    assert!(!hash_str.is_empty(), "plexus_hash should not be empty");
    eprintln!("✓ Found plexus_hash in response: {}", hash_str);

    // Navigate to the schema data
    let schema = event.get("data").expect("Should have data field");
    let one_of = schema.get("oneOf").expect("Schema should have oneOf");

    // Find node_create_text variant
    let variants = one_of.as_array().expect("oneOf should be array");
    let node_create_text = variants.iter().find(|v| {
        v.get("properties")
            .and_then(|p| p.get("method"))
            .and_then(|m| m.get("const").or_else(|| m.get("enum").and_then(|e| e.get(0))))
            .and_then(|v| v.as_str()) == Some("node_create_text")
    }).expect("Should find node_create_text variant");

    // Get params and check for required field
    let params = node_create_text
        .get("properties")
        .and_then(|p| p.get("params"))
        .expect("Should have params property");

    let required = params.get("required")
        .expect("params should have 'required' field - this is the fix we're testing!");

    let required_arr = required.as_array().expect("required should be array");

    // Verify tree_id and content are required
    let required_names: Vec<&str> = required_arr.iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert!(required_names.contains(&"tree_id"),
        "tree_id should be required, got: {:?}", required_names);
    assert!(required_names.contains(&"content"),
        "content should be required, got: {:?}", required_names);
    assert!(!required_names.contains(&"parent"),
        "parent should NOT be required");

    eprintln!("✓ Schema correctly includes required fields in params: {:?}", required_names);
}

/// Test method-level schema query (plexus_activation_schema with method param)
#[tokio::test]
async fn test_method_level_schema_query() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    // Query for a specific method's schema
    let mut subscription = client
        .subscribe::<Value, _>(
            "plexus_activation_schema",
            rpc_params!["arbor", "node_create_text"],
            "unsubscribe_schema"
        )
        .await
        .expect("Failed to subscribe to plexus_activation_schema");

    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("Timeout waiting for schema")
        .expect("Stream ended")
        .expect("Error receiving event");

    // Should return method schema with content_type "plexus.method_schema"
    let content_type = event.get("content_type")
        .and_then(|v| v.as_str())
        .expect("Should have content_type");
    assert_eq!(content_type, "plexus.method_schema",
        "Method-specific query should return plexus.method_schema");

    // The data should be the method variant schema directly, not a oneOf
    let schema = event.get("data").expect("Should have data");

    // Should have properties with method const
    let method_const = schema
        .get("properties")
        .and_then(|p| p.get("method"))
        .and_then(|m| m.get("const"))
        .and_then(|c| c.as_str());
    assert_eq!(method_const, Some("node_create_text"),
        "Should have method const = node_create_text");

    // Should have params with required fields
    let params = schema
        .get("properties")
        .and_then(|p| p.get("params"))
        .expect("Should have params");
    let required = params.get("required")
        .and_then(|r| r.as_array())
        .expect("Should have required array");

    let required_names: Vec<&str> = required.iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(required_names.contains(&"tree_id"));
    assert!(required_names.contains(&"content"));

    eprintln!("✓ Method-level schema query works: node_create_text has required {:?}", required_names);
}

/// Test that querying unknown method returns error with available methods
#[tokio::test]
async fn test_method_schema_unknown_method() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    let mut subscription = client
        .subscribe::<Value, _>(
            "plexus_activation_schema",
            rpc_params!["arbor", "nonexistent_method"],
            "unsubscribe_schema"
        )
        .await
        .expect("Failed to subscribe");

    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("Error receiving event");

    // Should return an error event
    let event_type = event.get("type")
        .and_then(|v| v.as_str())
        .expect("Should have type");
    assert_eq!(event_type, "error", "Should return error for unknown method");

    // Error should mention available methods
    let error_msg = event.get("error")
        .and_then(|e| e.as_str())
        .expect("Should have error message");
    assert!(error_msg.contains("nonexistent_method"), "Error should mention the requested method");
    assert!(error_msg.contains("tree_create") || error_msg.contains("Available"),
        "Error should list available methods");

    eprintln!("✓ Unknown method query returns helpful error: {}", error_msg);
}

/// Test that all responses include plexus_hash
#[tokio::test]
async fn test_responses_include_plexus_hash() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    // Test health_check
    let mut subscription = client
        .subscribe::<Value, _>("health_check", rpc_params![], "unsubscribe_check")
        .await
        .expect("Failed to subscribe to health_check");

    let event = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("Error receiving event");

    let plexus_hash = event.get("plexus_hash")
        .expect("health_check response should include plexus_hash");
    assert!(plexus_hash.is_string() && !plexus_hash.as_str().unwrap().is_empty(),
        "plexus_hash should be a non-empty string");

    eprintln!("✓ health_check includes plexus_hash: {}", plexus_hash);

    // Test plexus_hash endpoint directly
    let mut subscription = client
        .subscribe::<Value, _>("plexus_hash", rpc_params![], "unsubscribe_hash")
        .await
        .expect("Failed to subscribe to plexus_hash");

    let event = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("Error receiving event");

    let plexus_hash_top = event.get("plexus_hash").expect("Should have plexus_hash");
    let data = event.get("data").expect("Should have data");
    let hash_in_data = data.get("hash").expect("data should contain hash");

    // The hash in the response should match the hash in data
    assert_eq!(plexus_hash_top, hash_in_data,
        "plexus_hash at top level should match hash in data");

    eprintln!("✓ plexus_hash endpoint returns consistent hash: {}", plexus_hash_top);
}

/// Test that unknown activations return guided errors with `try` field
#[tokio::test]
async fn test_guided_error_unknown_activation() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    // Try to call an unknown activation - this should fail with a guided error
    let result = client
        .subscribe::<Value, _>("unknown_method", rpc_params![], "unsubscribe_unknown")
        .await;

    // The subscription should fail with an error
    let err = result.expect_err("Should have failed for unknown activation");
    let err_str = format!("{:?}", err);

    eprintln!("Error for unknown_method: {}", err_str);

    // The error should mention the method not found
    assert!(
        err_str.contains("unknown") || err_str.contains("not found") || err_str.contains("Method"),
        "Error should indicate method/activation not found: {}", err_str
    );
}

/// Test that unknown activation errors include the `try` field with guidance
#[tokio::test]
async fn test_guided_error_includes_try_field() {
    let client = create_client().await.expect("Failed to connect to server - is it running?");

    // Try foo_bar - 'foo' is not a valid activation
    let result = client
        .subscribe::<Value, _>("foo_bar", rpc_params![], "unsubscribe_foo")
        .await;

    let err = result.expect_err("Should have failed for unknown activation 'foo'");
    let err_str = format!("{:?}", err);

    eprintln!("Error for foo_bar: {}", err_str);

    // Check that the error includes our guided error data
    // The middleware should have enriched it with available_activations and try field
    assert!(
        err_str.contains("Activation 'foo' not found"),
        "Error message should say activation not found: {}", err_str
    );
    assert!(
        err_str.contains("plexus_schema"),
        "Error should include 'try' field with plexus_schema: {}", err_str
    );
    assert!(
        err_str.contains("available_activations"),
        "Error should include available_activations: {}", err_str
    );
    assert!(
        err_str.contains("arbor") && err_str.contains("bash") && err_str.contains("health"),
        "Error should list available activations: {}", err_str
    );
}

fn server_url() -> String {
    let port = std::env::var("SUBSTRATE_PORT").unwrap_or_else(|_| "4444".to_string());
    format!("ws://127.0.0.1:{}", port)
}

async fn create_client() -> Result<jsonrpsee::ws_client::WsClient, Box<dyn std::error::Error>> {
    let client = WsClientBuilder::default()
        .connection_timeout(Duration::from_secs(5))
        .build(&server_url())
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

    // Verify structure - data contains the payload directly
    let data = event.get("data").expect("Event should have 'data' field");
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("healthy"));
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

    // Check for stdout containing hello - data is directly in event["data"]
    let found_output = events.iter().any(|e| {
        e.get("data")
            .and_then(|d| d.get("line"))
            .and_then(|v| v.as_str())
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

    // Verify we got a tree created event with a tree_id - data is directly in event["data"]
    let data = event.get("data").expect("Event should have 'data' field");
    assert!(data.get("tree_id").is_some(), "Should have tree_id in response: {:?}", data);
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

    // Check for trees array - data is directly in event["data"]
    let data = event.get("data").expect("Event should have 'data' field");
    assert!(
        data.get("tree_ids").is_some() || data.get("trees").is_some() || data.get("type").is_some(),
        "Should have tree data in response: {:?}", data
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

    let data = event.get("data").expect("Should have data");
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

    let tree_data = event.get("data").expect("Should have data");
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

    let node_data = event.get("data").expect("Should have data");
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

    let node_resp = event.get("data").expect("Should have data");
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

    let path_data = event.get("data").expect("Should have data");
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

    // Verify we got a cone created event - data is directly in event["data"]
    let data = event.get("data").expect("Event should have 'data' field");
    assert!(data.get("cone_id").is_some() || data.get("agent_id").is_some(),
            "Should have cone_id in response: {:?}", data);
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

    // Should have cones/agents array - data is directly in event["data"]
    let data = event.get("data").expect("Event should have 'data' field");
    assert!(
        data.get("cones").is_some() || data.get("agents").is_some() || data.get("type").is_some(),
        "Should have cones data in response: {:?}", data
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

    // Verify registry structure - data is directly in event["data"]
    let data = event.get("data").expect("Event should have 'data' field");

    // Should have services array
    let services = data.get("services").expect("Should have services array");
    assert!(services.is_array(), "services should be an array");

    // Should have families array
    let families = data.get("families").expect("Should have families array");
    assert!(families.is_array(), "families should be an array");
    let families_arr = families.as_array().unwrap();
    assert!(!families_arr.is_empty(), "Should have at least one family");

    // Should have models array
    let models = data.get("models").expect("Should have models array");
    assert!(models.is_array(), "models should be an array");
    let models_arr = models.as_array().unwrap();
    assert!(!models_arr.is_empty(), "Should have at least one model");

    // Should have stats
    let stats = data.get("stats").expect("Should have stats");
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

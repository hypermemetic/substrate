# BIDIR-9: Test Coverage

**Status:** Planning
**Blocked By:** BIDIR-8
**Unlocks:** -

## Scope

Comprehensive test coverage for bidirectional functionality across all layers: core types, channel communication, transport adapters, and end-to-end integration.

## Acceptance Criteria

- [ ] Unit tests for core types (BIDIR-2)
- [ ] Unit tests for BidirChannel (BIDIR-3)
- [ ] Integration tests for hub-macro codegen (BIDIR-4)
- [ ] Unit tests for helper functions (BIDIR-5)
- [ ] Transport-level tests for MCP (BIDIR-6)
- [ ] Transport-level tests for WebSocket (BIDIR-7)
- [ ] End-to-end tests using Interactive activation (BIDIR-8)
- [ ] Timeout and cancellation tests
- [ ] Edge case coverage: concurrent requests, rapid request/response, error recovery

## Implementation Notes

### Core Types Tests (BIDIR-2)

```rust
// hub-core/src/plexus/tests/bidir_types.rs

use super::*;
use serde_json::json;

mod request_type_tests {
    use super::*;

    #[test]
    fn test_confirm_serialization() {
        let req = RequestType::Confirm {
            message: "Continue?".into(),
            default: Some(true),
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["message"], "Continue?");
        assert_eq!(json["default"], true);

        let parsed: RequestType = serde_json::from_value(json).unwrap();
        match parsed {
            RequestType::Confirm { message, default } => {
                assert_eq!(message, "Continue?");
                assert_eq!(default, Some(true));
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_prompt_serialization() {
        let req = RequestType::Prompt {
            message: "Enter name:".into(),
            default: Some("anonymous".into()),
            placeholder: Some("Your name here".into()),
        };

        let json = serde_json::to_value(&req).unwrap();
        let parsed: RequestType = serde_json::from_value(json).unwrap();

        match parsed {
            RequestType::Prompt { message, default, placeholder } => {
                assert_eq!(message, "Enter name:");
                assert_eq!(default, Some("anonymous".into()));
                assert_eq!(placeholder, Some("Your name here".into()));
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_select_serialization() {
        let req = RequestType::Select {
            message: "Pick one:".into(),
            options: vec![
                SelectOption {
                    value: "a".into(),
                    label: "Option A".into(),
                    description: Some("First option".into()),
                },
                SelectOption {
                    value: "b".into(),
                    label: "Option B".into(),
                    description: None,
                },
            ],
            multi_select: true,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["options"].as_array().unwrap().len(), 2);
        assert_eq!(json["multi_select"], true);

        let parsed: RequestType = serde_json::from_value(json).unwrap();
        match parsed {
            RequestType::Select { options, multi_select, .. } => {
                assert_eq!(options.len(), 2);
                assert!(multi_select);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_custom_request_type() {
        let req = RequestType::Custom {
            type_name: "file_picker".into(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "filter": { "type": "string" }
                }
            })),
        };

        let json = serde_json::to_value(&req).unwrap();
        let parsed: RequestType = serde_json::from_value(json).unwrap();

        match parsed {
            RequestType::Custom { type_name, schema } => {
                assert_eq!(type_name, "file_picker");
                assert!(schema.is_some());
            }
            _ => panic!("Wrong variant"),
        }
    }
}

mod response_payload_tests {
    use super::*;

    #[test]
    fn test_all_variants_serialize() {
        let variants = vec![
            (ResponsePayload::Confirmed(true), "Confirmed"),
            (ResponsePayload::Text("hello".into()), "Text"),
            (ResponsePayload::Selected(vec!["a".into(), "b".into()]), "Selected"),
            (ResponsePayload::Custom(json!({"key": "value"})), "Custom"),
            (ResponsePayload::Cancelled, "Cancelled"),
            (ResponsePayload::Timeout, "Timeout"),
        ];

        for (payload, expected_tag) in variants {
            let json = serde_json::to_value(&payload).unwrap();
            // Verify it serializes and can be parsed back
            let _: ResponsePayload = serde_json::from_value(json.clone()).unwrap();
        }
    }

    #[test]
    fn test_confirmed_values() {
        for value in [true, false] {
            let payload = ResponsePayload::Confirmed(value);
            let json = serde_json::to_value(&payload).unwrap();
            let parsed: ResponsePayload = serde_json::from_value(json).unwrap();
            assert!(matches!(parsed, ResponsePayload::Confirmed(v) if v == value));
        }
    }

    #[test]
    fn test_empty_text() {
        let payload = ResponsePayload::Text(String::new());
        let json = serde_json::to_value(&payload).unwrap();
        let parsed: ResponsePayload = serde_json::from_value(json).unwrap();
        assert!(matches!(parsed, ResponsePayload::Text(s) if s.is_empty()));
    }

    #[test]
    fn test_empty_selection() {
        let payload = ResponsePayload::Selected(vec![]);
        let json = serde_json::to_value(&payload).unwrap();
        let parsed: ResponsePayload = serde_json::from_value(json).unwrap();
        assert!(matches!(parsed, ResponsePayload::Selected(v) if v.is_empty()));
    }
}

mod client_response_tests {
    use super::*;

    #[test]
    fn test_client_response_roundtrip() {
        let response = ClientResponse {
            request_id: "abc-123".into(),
            payload: ResponsePayload::Confirmed(true),
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: ClientResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.request_id, "abc-123");
        assert!(matches!(parsed.payload, ResponsePayload::Confirmed(true)));
    }
}

mod bidir_error_tests {
    use super::*;

    #[test]
    fn test_error_display_messages() {
        let errors = vec![
            (BidirError::Cancelled, "cancelled"),
            (BidirError::Timeout, "timeout"),
            (BidirError::NotSupported, "not supported"),
            (
                BidirError::TypeMismatch {
                    expected: "Confirmed".into(),
                    got: "Text".into(),
                },
                "mismatch",
            ),
            (BidirError::Transport("connection lost".into()), "connection"),
        ];

        for (error, should_contain) in errors {
            let msg = bidir_error_message(&error);
            assert!(
                msg.to_lowercase().contains(should_contain),
                "Expected '{}' to contain '{}'",
                msg,
                should_contain
            );
        }
    }
}
```

### Channel Tests (BIDIR-3)

```rust
// hub-core/src/plexus/tests/bidir_channel.rs

use super::*;
use std::time::Duration;
use tokio::sync::mpsc;

mod bidir_channel_tests {
    use super::*;

    #[tokio::test]
    async fn test_successful_request_response() {
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let channel = Arc::new(BidirChannel::new(stream_tx, true));
        let channel_clone = channel.clone();

        // Spawn request in background
        let request_task = tokio::spawn(async move {
            channel_clone.confirm("Test?").await
        });

        // Receive the request
        let item = stream_rx.recv().await.unwrap();
        let request_id = match item {
            PlexusStreamItem::Request { request_id, .. } => request_id,
            _ => panic!("Expected Request item"),
        };

        // Send response
        channel.handle_response(ClientResponse {
            request_id,
            payload: ResponsePayload::Confirmed(true),
        }).unwrap();

        // Verify result
        let result = request_task.await.unwrap();
        assert_eq!(result.unwrap(), true);
    }

    #[tokio::test]
    async fn test_timeout() {
        let (stream_tx, _stream_rx) = mpsc::channel(32);
        let channel = BidirChannel::new(stream_tx, true);

        let result = channel.request_with_timeout(
            RequestType::Confirm { message: "Test?".into(), default: None },
            Duration::from_millis(50),
        ).await;

        assert!(matches!(result, Err(BidirError::Timeout)));
    }

    #[tokio::test]
    async fn test_not_supported() {
        let (stream_tx, _stream_rx) = mpsc::channel(32);
        let channel = BidirChannel::unidirectional(stream_tx);

        assert!(!channel.is_bidirectional());

        let result = channel.confirm("Test?").await;
        assert!(matches!(result, Err(BidirError::NotSupported)));
    }

    #[tokio::test]
    async fn test_response_to_unknown_request() {
        let (stream_tx, _stream_rx) = mpsc::channel(32);
        let channel = BidirChannel::new(stream_tx, true);

        let result = channel.handle_response(ClientResponse {
            request_id: "nonexistent".into(),
            payload: ResponsePayload::Confirmed(true),
        });

        assert!(matches!(result, Err(BidirError::Transport(_))));
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let channel = Arc::new(BidirChannel::new(stream_tx, true));

        // Start 3 concurrent requests
        let mut tasks = Vec::new();
        for i in 0..3 {
            let ch = channel.clone();
            tasks.push(tokio::spawn(async move {
                ch.prompt(&format!("Question {}", i)).await
            }));
        }

        // Collect all request IDs
        let mut request_ids = Vec::new();
        for _ in 0..3 {
            let item = stream_rx.recv().await.unwrap();
            if let PlexusStreamItem::Request { request_id, .. } = item {
                request_ids.push(request_id);
            }
        }

        assert_eq!(request_ids.len(), 3);

        // Respond to all
        for (i, id) in request_ids.into_iter().enumerate() {
            channel.handle_response(ClientResponse {
                request_id: id,
                payload: ResponsePayload::Text(format!("Answer {}", i)),
            }).unwrap();
        }

        // All tasks should complete
        for task in tasks {
            let result = task.await.unwrap();
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn test_rapid_request_response() {
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let channel = Arc::new(BidirChannel::new(stream_tx, true));
        let channel_clone = channel.clone();

        // Spawn rapid request sender
        let sender = tokio::spawn(async move {
            for i in 0..100 {
                let result = channel_clone.confirm(&format!("Q{}", i)).await;
                assert!(result.is_ok());
            }
        });

        // Rapid responder
        for _ in 0..100 {
            let item = stream_rx.recv().await.unwrap();
            if let PlexusStreamItem::Request { request_id, .. } = item {
                channel.handle_response(ClientResponse {
                    request_id,
                    payload: ResponsePayload::Confirmed(true),
                }).unwrap();
            }
        }

        sender.await.unwrap();
    }
}

mod bidir_ext_tests {
    use super::*;

    #[tokio::test]
    async fn test_confirm_helper() {
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let channel = Arc::new(BidirChannel::new(stream_tx, true));
        let channel_clone = channel.clone();

        let task = tokio::spawn(async move {
            channel_clone.confirm("Continue?").await
        });

        let item = stream_rx.recv().await.unwrap();
        let request_id = match &item {
            PlexusStreamItem::Request { request_id, request_type, .. } => {
                assert!(matches!(request_type, RequestType::Confirm { .. }));
                request_id.clone()
            }
            _ => panic!("Expected Confirm request"),
        };

        channel.handle_response(ClientResponse {
            request_id,
            payload: ResponsePayload::Confirmed(true),
        }).unwrap();

        assert_eq!(task.await.unwrap().unwrap(), true);
    }

    #[tokio::test]
    async fn test_prompt_helper() {
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let channel = Arc::new(BidirChannel::new(stream_tx, true));
        let channel_clone = channel.clone();

        let task = tokio::spawn(async move {
            channel_clone.prompt("Name?").await
        });

        let item = stream_rx.recv().await.unwrap();
        let request_id = match &item {
            PlexusStreamItem::Request { request_id, request_type, .. } => {
                assert!(matches!(request_type, RequestType::Prompt { .. }));
                request_id.clone()
            }
            _ => panic!("Expected Prompt request"),
        };

        channel.handle_response(ClientResponse {
            request_id,
            payload: ResponsePayload::Text("Alice".into()),
        }).unwrap();

        assert_eq!(task.await.unwrap().unwrap(), "Alice");
    }

    #[tokio::test]
    async fn test_select_helper() {
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let channel = Arc::new(BidirChannel::new(stream_tx, true));
        let channel_clone = channel.clone();

        let task = tokio::spawn(async move {
            channel_clone.select(
                "Pick:",
                vec![
                    SelectOption { value: "a".into(), label: "A".into(), description: None },
                    SelectOption { value: "b".into(), label: "B".into(), description: None },
                ],
            ).await
        });

        let item = stream_rx.recv().await.unwrap();
        let request_id = match &item {
            PlexusStreamItem::Request { request_id, request_type, .. } => {
                assert!(matches!(request_type, RequestType::Select { multi_select: false, .. }));
                request_id.clone()
            }
            _ => panic!("Expected Select request"),
        };

        channel.handle_response(ClientResponse {
            request_id,
            payload: ResponsePayload::Selected(vec!["b".into()]),
        }).unwrap();

        assert_eq!(task.await.unwrap().unwrap(), "b");
    }

    #[tokio::test]
    async fn test_type_mismatch() {
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let channel = Arc::new(BidirChannel::new(stream_tx, true));
        let channel_clone = channel.clone();

        let task = tokio::spawn(async move {
            channel_clone.confirm("Continue?").await
        });

        let item = stream_rx.recv().await.unwrap();
        let request_id = match item {
            PlexusStreamItem::Request { request_id, .. } => request_id,
            _ => panic!("Expected Request"),
        };

        // Send wrong type response
        channel.handle_response(ClientResponse {
            request_id,
            payload: ResponsePayload::Text("wrong type".into()),
        }).unwrap();

        let result = task.await.unwrap();
        assert!(matches!(result, Err(BidirError::TypeMismatch { .. })));
    }
}
```

### Helper Function Tests (BIDIR-5)

```rust
// hub-core/src/plexus/tests/bidir_helpers.rs

use super::*;

mod fallback_tests {
    use super::*;

    #[tokio::test]
    async fn test_auto_confirm_when_not_supported() {
        let (tx, _rx) = mpsc::channel(32);
        let ctx = BidirChannel::unidirectional(tx);
        let fallback = BidirWithFallback::new(&ctx).auto_confirm();

        assert!(fallback.confirm("Test?").await);
    }

    #[tokio::test]
    async fn test_no_auto_confirm_default() {
        let (tx, _rx) = mpsc::channel(32);
        let ctx = BidirChannel::unidirectional(tx);
        let fallback = BidirWithFallback::new(&ctx);

        assert!(!fallback.confirm("Test?").await);
    }

    #[tokio::test]
    async fn test_default_text_when_not_supported() {
        let (tx, _rx) = mpsc::channel(32);
        let ctx = BidirChannel::unidirectional(tx);
        let fallback = BidirWithFallback::new(&ctx).with_default("fallback-value");

        assert_eq!(fallback.prompt("Name?").await, Some("fallback-value".into()));
    }

    #[tokio::test]
    async fn test_no_default_text() {
        let (tx, _rx) = mpsc::channel(32);
        let ctx = BidirChannel::unidirectional(tx);
        let fallback = BidirWithFallback::new(&ctx);

        assert_eq!(fallback.prompt("Name?").await, None);
    }
}

mod timeout_config_tests {
    use super::*;

    #[test]
    fn test_default_timeouts() {
        let config = TimeoutConfig::default();

        assert_eq!(config.confirm, Duration::from_secs(30));
        assert_eq!(config.prompt, Duration::from_secs(60));
        assert_eq!(config.select, Duration::from_secs(45));
        assert_eq!(config.custom, Duration::from_secs(120));
    }

    #[test]
    fn test_quick_timeouts() {
        let config = TimeoutConfig::quick();

        assert!(config.confirm < Duration::from_secs(30));
        assert!(config.prompt < Duration::from_secs(60));
    }

    #[test]
    fn test_patient_timeouts() {
        let config = TimeoutConfig::patient();

        assert!(config.confirm > Duration::from_secs(30));
        assert!(config.prompt > Duration::from_secs(60));
    }

    #[test]
    fn test_for_request_type() {
        let config = TimeoutConfig::default();

        let confirm_timeout = config.for_request_type(&RequestType::Confirm {
            message: "".into(),
            default: None,
        });
        assert_eq!(confirm_timeout, config.confirm);

        let prompt_timeout = config.for_request_type(&RequestType::Prompt {
            message: "".into(),
            default: None,
            placeholder: None,
        });
        assert_eq!(prompt_timeout, config.prompt);
    }
}
```

### Transport Tests (BIDIR-6, BIDIR-7)

```rust
// substrate/tests/bidir_mcp.rs

use substrate::mcp_bridge::PlexusMcpBridge;

mod mcp_bidir_tests {
    use super::*;

    #[tokio::test]
    async fn test_response_tool_available() {
        let bridge = setup_test_bridge().await;
        let tools = bridge.list_tools().await.unwrap();

        assert!(tools.iter().any(|t| t.name == "_plexus_respond"));
    }

    #[tokio::test]
    async fn test_request_notification_sent() {
        let (bridge, mut notification_rx) = setup_bridge_with_notifications().await;

        // Call interactive method
        tokio::spawn(async move {
            bridge.call_tool(CallToolParams {
                name: "interactive.confirm_demo".into(),
                arguments: Some(json!({"message": "Test?"})),
            }, ctx).await
        });

        // Should receive request notification
        let notification = notification_rx.recv().await.unwrap();
        assert_eq!(notification.data["type"], "request");
        assert!(notification.data["request_id"].is_string());
    }

    #[tokio::test]
    async fn test_response_routing() {
        let (bridge, mut notification_rx) = setup_bridge_with_notifications().await;

        let call_task = tokio::spawn({
            let bridge = bridge.clone();
            async move {
                bridge.call_tool(CallToolParams {
                    name: "interactive.confirm_demo".into(),
                    arguments: Some(json!({})),
                }, ctx).await
            }
        });

        // Get request notification
        let notification = notification_rx.recv().await.unwrap();
        let request_id = notification.data["request_id"].as_str().unwrap();

        // Send response
        bridge.call_tool(CallToolParams {
            name: "_plexus_respond".into(),
            arguments: Some(json!({
                "request_id": request_id,
                "payload": { "Confirmed": true }
            })),
        }, ctx).await.unwrap();

        // Original call should complete
        let result = call_task.await.unwrap().unwrap();
        assert!(!result.is_error.unwrap_or(false));
    }

    #[tokio::test]
    async fn test_invalid_request_id_response() {
        let bridge = setup_test_bridge().await;

        let result = bridge.call_tool(CallToolParams {
            name: "_plexus_respond".into(),
            arguments: Some(json!({
                "request_id": "nonexistent-id",
                "payload": { "Confirmed": true }
            })),
        }, ctx).await;

        assert!(result.is_err());
    }
}

// substrate/tests/bidir_ws.rs

mod ws_bidir_tests {
    use super::*;

    #[tokio::test]
    async fn test_subscription_with_bidir() {
        let (server, addr) = start_test_server().await;
        let client = connect_ws_client(&addr).await;

        // Subscribe
        let mut sub = client
            .subscribe::<SubscriptionMessage, _>(
                "plexus_subscribe",
                rpc_params!["interactive.confirm_demo", {}],
            )
            .await
            .unwrap();

        // Should receive request
        loop {
            match sub.next().await.unwrap().unwrap() {
                SubscriptionMessage::Request { request_id, .. } => {
                    // Respond
                    client
                        .request::<bool, _>(
                            "plexus_respond",
                            rpc_params![sub.subscription_id().to_string(), request_id, {"Confirmed": true}],
                        )
                        .await
                        .unwrap();
                    break;
                }
                SubscriptionMessage::Progress { .. } => continue,
                _ => panic!("Unexpected message"),
            }
        }

        // Should complete
        loop {
            match sub.next().await.unwrap().unwrap() {
                SubscriptionMessage::Done => break,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn test_subscription_cancel() {
        let (server, addr) = start_test_server().await;
        let client = connect_ws_client(&addr).await;

        let sub = client
            .subscribe::<SubscriptionMessage, _>(
                "plexus_subscribe",
                rpc_params!["interactive.wizard", {}],
            )
            .await
            .unwrap();

        // Drop subscription to cancel
        drop(sub);

        // Server should clean up (verify via metrics or logs)
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
```

### End-to-End Integration Tests

```rust
// hyperforge/tests/bidir_e2e.rs

mod e2e_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_wizard_flow_mcp() {
        let server = start_mcp_server().await;

        // Simulate Claude Code behavior
        let client = McpClient::connect(&server.address()).await;

        // Call wizard
        let call_handle = client.call_tool_async("interactive.wizard", json!({})).await;

        // Handle all interactions
        while let Some(notification) = client.next_notification().await {
            if notification.data["type"] == "request" {
                let request_id = notification.data["request_id"].as_str().unwrap();
                let request_type = &notification.data["request_type"];

                let response = auto_respond(request_type);
                client.call_tool("_plexus_respond", json!({
                    "request_id": request_id,
                    "payload": response
                })).await.unwrap();
            }
        }

        let result = call_handle.await.unwrap();
        assert!(result.content.iter().any(|c| c.text.contains("success")));
    }

    #[tokio::test]
    async fn test_full_wizard_flow_websocket() {
        let server = start_ws_server().await;
        let client = WsClient::connect(&server.address()).await;

        let mut sub = client.subscribe("interactive.wizard", json!({})).await;

        let mut completed = false;
        while !completed {
            match sub.next().await.unwrap() {
                SubscriptionMessage::Request { request_id, request_type, .. } => {
                    let response = auto_respond(&request_type);
                    client.respond(&sub.id(), &request_id, response).await.unwrap();
                }
                SubscriptionMessage::Done => {
                    completed = true;
                }
                _ => continue,
            }
        }

        assert!(completed);
    }

    fn auto_respond(request_type: &RequestType) -> ResponsePayload {
        match request_type {
            RequestType::Confirm { .. } => ResponsePayload::Confirmed(true),
            RequestType::Prompt { .. } => ResponsePayload::Text("test-value".into()),
            RequestType::Select { options, .. } => {
                ResponsePayload::Selected(vec![options[0].value.clone()])
            }
            RequestType::Custom { .. } => ResponsePayload::Custom(json!({})),
        }
    }
}
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `hub-core/src/plexus/tests/mod.rs` | Add test modules |
| `hub-core/src/plexus/tests/bidir_types.rs` | NEW: Core type tests |
| `hub-core/src/plexus/tests/bidir_channel.rs` | NEW: Channel tests |
| `hub-core/src/plexus/tests/bidir_helpers.rs` | NEW: Helper tests |
| `substrate/tests/bidir_mcp.rs` | NEW: MCP transport tests |
| `substrate/tests/bidir_ws.rs` | NEW: WebSocket transport tests |
| `hyperforge/tests/bidir_e2e.rs` | NEW: End-to-end tests |

## Testing

Run the full test suite:

```bash
# Unit tests
cargo test -p hub-core bidir

# Transport tests
cargo test -p substrate bidir

# E2E tests
cargo test -p hyperforge bidir_e2e

# All bidir tests
cargo test bidir
```

## Notes

- Tests use `tokio::test` for async support
- Transport tests require test harness for server setup
- E2E tests validate the full flow works as documented
- Concurrent and rapid tests help find race conditions
- Timeout tests use short durations to keep test suite fast
- Consider property-based testing for serialization roundtrips

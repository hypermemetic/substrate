//! WebSocket bidirectional streaming support.
//!
//! This module implements the WebSocket transport mapping for bidirectional
//! communication as specified in BIDIR-7. It provides:
//!
//! - `SubscriptionRegistry`: Tracks active subscriptions for routing responses
//! - `PlexusBidirRpc`: RPC trait for the `plexus_respond` method
//! - Helper functions for handling bidirectional subscriptions
//!
//! # Architecture
//!
//! WebSocket connections are inherently bidirectional, making this implementation
//! cleaner than MCP. The flow is:
//!
//! ```text
//! 1. Client subscribes via standard jsonrpsee subscription
//! 2. Server streams PlexusStreamItem → SubscriptionMessage
//! 3. When server needs input, it sends SubscriptionMessage::Request
//! 4. Client responds via separate plexus_respond RPC call
//! 5. Response is routed to the appropriate BidirChannel
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::mpsc;

use hub_core::plexus::{
    BidirChannel, ClientMessage, ClientResponse, PlexusStreamItem,
    ResponsePayload, SubscriptionMessage,
};

// =============================================================================
// Subscription Registry
// =============================================================================

/// Unique identifier for a subscription.
///
/// This wraps the jsonrpsee subscription ID for type safety.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubscriptionId(String);

impl From<String> for SubscriptionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SubscriptionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::fmt::Display for SubscriptionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Registry of active subscriptions for routing client messages.
///
/// When a bidirectional subscription is started, it registers its sender here.
/// When a `plexus_respond` call arrives, this registry routes the response
/// to the appropriate subscription's BidirChannel.
///
/// Thread-safe with read-optimized locking (many reads, few writes).
#[derive(Default)]
pub struct SubscriptionRegistry {
    /// Map of subscription ID to message sender.
    senders: RwLock<HashMap<SubscriptionId, mpsc::Sender<ClientMessage>>>,
}

impl SubscriptionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a subscription's message sender.
    ///
    /// Called when a new bidirectional subscription is started.
    pub fn register(&self, id: SubscriptionId, tx: mpsc::Sender<ClientMessage>) {
        let mut senders = self.senders.write();
        senders.insert(id, tx);
    }

    /// Unregister a subscription.
    ///
    /// Called when a subscription is closed or dropped.
    pub fn unregister(&self, id: &SubscriptionId) {
        let mut senders = self.senders.write();
        senders.remove(id);
    }

    /// Get a sender for a subscription, if it exists.
    ///
    /// Returns a clone of the sender for thread-safe use.
    pub fn get(&self, id: &SubscriptionId) -> Option<mpsc::Sender<ClientMessage>> {
        let senders = self.senders.read();
        senders.get(id).cloned()
    }

    /// Get the number of active subscriptions.
    pub fn len(&self) -> usize {
        self.senders.read().len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.senders.read().is_empty()
    }
}

// =============================================================================
// Bidirectional Subscription Handler
// =============================================================================

/// Context for a bidirectional subscription.
///
/// Contains all the state needed to handle a single bidirectional subscription,
/// including the channel for sending responses back to the activation.
pub struct BidirSubscriptionContext {
    /// The bidirectional channel for this subscription.
    pub bidir_channel: Arc<BidirChannel>,
    /// Receiver for client messages routed from the registry.
    pub client_rx: mpsc::Receiver<ClientMessage>,
}

impl BidirSubscriptionContext {
    /// Create a new bidirectional subscription context.
    ///
    /// # Arguments
    /// * `bidirectional_supported` - Whether the transport supports bidirectional communication
    /// * `provenance` - The call path (e.g., ["plexus", "cone", "chat"])
    /// * `plexus_hash` - Current plexus configuration hash
    ///
    /// # Returns
    /// A tuple of (context, sender_to_stream, sender_for_registry)
    pub fn new(
        bidirectional_supported: bool,
        provenance: Vec<String>,
        plexus_hash: String,
    ) -> (Self, mpsc::Receiver<PlexusStreamItem>, mpsc::Sender<ClientMessage>) {
        // Channel for stream items from activation to transport
        let (stream_tx, stream_rx) = mpsc::channel::<PlexusStreamItem>(32);

        // Channel for client messages from registry to subscription handler
        let (client_tx, client_rx) = mpsc::channel::<ClientMessage>(32);

        let bidir_channel = Arc::new(BidirChannel::new(
            stream_tx,
            bidirectional_supported,
            provenance,
            plexus_hash,
        ));

        let ctx = Self {
            bidir_channel,
            client_rx,
        };

        (ctx, stream_rx, client_tx)
    }
}

/// Handle incoming client messages and route them to the BidirChannel.
///
/// This task runs for the lifetime of the subscription, routing responses
/// from the `plexus_respond` RPC call to the appropriate pending request.
pub async fn handle_client_messages(
    bidir_channel: Arc<BidirChannel>,
    mut client_rx: mpsc::Receiver<ClientMessage>,
) {
    while let Some(msg) = client_rx.recv().await {
        match msg {
            ClientMessage::Response {
                request_id,
                payload,
            } => {
                let response = ClientResponse {
                    request_id,
                    payload,
                };
                if let Err(e) = bidir_channel.handle_response(response) {
                    tracing::warn!("Failed to route response: {:?}", e);
                }
            }
            ClientMessage::Cancel => {
                tracing::debug!("Received cancel message");
                break;
            }
        }
    }
}

/// Convert a PlexusStreamItem to a SubscriptionMessage for sending to the client.
///
/// This strips the internal metadata from the stream item, keeping only
/// the payload information relevant to the client.
pub fn to_subscription_message(item: PlexusStreamItem) -> SubscriptionMessage {
    item.into()
}

// =============================================================================
// Response Error Types
// =============================================================================

/// Error type for responding to requests.
#[derive(Debug)]
pub enum RespondError {
    /// Subscription not found in registry.
    SubscriptionNotFound(String),
    /// Failed to send message to subscription.
    SendFailed(String),
}

impl std::fmt::Display for RespondError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SubscriptionNotFound(id) => {
                write!(f, "Subscription '{}' not found or closed", id)
            }
            Self::SendFailed(reason) => {
                write!(f, "Failed to send response: {}", reason)
            }
        }
    }
}

impl std::error::Error for RespondError {}

// =============================================================================
// Helper Functions
// =============================================================================

/// Route a response to a subscription via the registry.
///
/// This is called by the `plexus_respond` RPC handler to route a client's
/// response to the appropriate subscription.
///
/// # Arguments
/// * `registry` - The subscription registry
/// * `subscription_id` - ID of the subscription to route to
/// * `request_id` - ID of the request being responded to
/// * `payload` - The response payload
///
/// # Returns
/// `Ok(())` if the response was routed successfully, or an error if the
/// subscription was not found or the send failed.
pub async fn route_response(
    registry: &SubscriptionRegistry,
    subscription_id: &str,
    request_id: String,
    payload: ResponsePayload,
) -> Result<(), RespondError> {
    let sub_id = SubscriptionId::from(subscription_id);

    let tx = registry
        .get(&sub_id)
        .ok_or_else(|| RespondError::SubscriptionNotFound(subscription_id.to_string()))?;

    let message = ClientMessage::Response {
        request_id,
        payload,
    };

    tx.send(message)
        .await
        .map_err(|_| RespondError::SendFailed("Channel closed".into()))
}

/// Cancel a subscription via the registry.
///
/// Sends a Cancel message to the subscription, causing it to gracefully
/// shut down.
pub async fn cancel_subscription(
    registry: &SubscriptionRegistry,
    subscription_id: &str,
) -> Result<(), RespondError> {
    let sub_id = SubscriptionId::from(subscription_id);

    let tx = registry
        .get(&sub_id)
        .ok_or_else(|| RespondError::SubscriptionNotFound(subscription_id.to_string()))?;

    tx.send(ClientMessage::Cancel)
        .await
        .map_err(|_| RespondError::SendFailed("Channel closed".into()))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use hub_core::plexus::BidirectionalContext;
    use std::time::Duration;

    // =========================================================================
    // Registry Functional Tests
    // =========================================================================

    #[test]
    fn test_registry_tracks_and_retrieves_subscriptions() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::channel(1);

        // Initially empty
        assert!(registry.is_empty());
        assert!(registry.get(&SubscriptionId::from("sub-1")).is_none());

        // Register a subscription
        registry.register(SubscriptionId::from("sub-1"), tx);

        // Verify get() returns the channel
        assert_eq!(registry.len(), 1);
        assert!(registry.get(&SubscriptionId::from("sub-1")).is_some());
        assert!(registry.get(&SubscriptionId::from("other")).is_none());

        // Unregister
        registry.unregister(&SubscriptionId::from("sub-1"));

        // Verify get() returns None after unregister
        assert!(registry.get(&SubscriptionId::from("sub-1")).is_none());
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn test_route_response_delivers_to_correct_channel() {
        // Create BidirSubscriptionContext
        let (ctx, _stream_rx, client_tx) = BidirSubscriptionContext::new(
            true,
            vec!["plexus".into(), "test".into()],
            "hash123".into(),
        );

        // Register it in a registry
        let registry = SubscriptionRegistry::new();
        registry.register(SubscriptionId::from("sub-1"), client_tx);

        // Start a request on the BidirChannel (this will wait for response)
        let bidir_channel = ctx.bidir_channel.clone();
        let request_task = tokio::spawn(async move {
            bidir_channel
                .request_with_timeout(
                    hub_core::plexus::RequestType::Confirm {
                        message: "Do you confirm?".into(),
                        default: None,
                    },
                    Duration::from_secs(5),
                )
                .await
        });

        // Give the request time to be sent and register its pending handler
        tokio::time::sleep(Duration::from_millis(10)).await;

        // The BidirChannel sends a PlexusStreamItem::Request to stream_rx
        // In a real scenario, the transport would extract the request_id
        // For this test, we need to get the request_id from the pending map
        // Since we can't directly access it, we'll use route_response which
        // sends ClientMessage::Response to the registered subscription

        // Get the request_id by receiving from stream_rx
        let mut stream_rx = _stream_rx;
        let stream_item = stream_rx.recv().await.expect("Should receive request");

        let request_id = match stream_item {
            PlexusStreamItem::Request { request_id, .. } => request_id,
            other => panic!("Expected Request, got {:?}", other),
        };

        // Call route_response with a ClientResponse
        let result = route_response(
            &registry,
            "sub-1",
            request_id,
            ResponsePayload::Confirmed(true),
        )
        .await;

        assert!(result.is_ok(), "route_response should succeed");

        // Now handle_client_messages would normally route this, but we need
        // to manually process since we're testing. Let's spawn the handler.
        let bidir_channel_for_handler = ctx.bidir_channel.clone();
        let mut client_rx = ctx.client_rx;

        // Receive the message that was routed
        let msg = client_rx.recv().await.expect("Should receive message");
        match msg {
            ClientMessage::Response {
                request_id,
                payload,
            } => {
                // Route to BidirChannel
                bidir_channel_for_handler
                    .handle_response(ClientResponse {
                        request_id,
                        payload,
                    })
                    .expect("handle_response should succeed");
            }
            _ => panic!("Expected Response message"),
        }

        // Verify the BidirChannel receives the response
        let result = request_task.await.expect("Task should complete");
        assert!(
            matches!(result, Ok(ResponsePayload::Confirmed(true))),
            "BidirChannel should receive Confirmed(true), got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_cancel_subscription_sends_cancel_message() {
        // Create subscription context
        let (ctx, _stream_rx, client_tx) = BidirSubscriptionContext::new(
            true,
            vec!["plexus".into(), "test".into()],
            "hash123".into(),
        );

        let registry = SubscriptionRegistry::new();
        registry.register(SubscriptionId::from("sub-1"), client_tx);

        // Call cancel_subscription
        let result = cancel_subscription(&registry, "sub-1").await;
        assert!(result.is_ok());

        // Verify the client_rx receives ClientMessage::Cancel
        let mut client_rx = ctx.client_rx;
        let msg = client_rx.recv().await.expect("Should receive cancel message");
        assert!(
            matches!(msg, ClientMessage::Cancel),
            "Expected Cancel message, got {:?}",
            msg
        );
    }

    #[tokio::test]
    async fn test_concurrent_subscriptions_isolated() {
        let registry = SubscriptionRegistry::new();

        // Register two subscriptions with different IDs
        let (tx1, mut rx1) = mpsc::channel(10);
        let (tx2, mut rx2) = mpsc::channel(10);

        registry.register(SubscriptionId::from("sub-1"), tx1);
        registry.register(SubscriptionId::from("sub-2"), tx2);

        assert_eq!(registry.len(), 2);

        // Send response to subscription 1
        let result = route_response(
            &registry,
            "sub-1",
            "req-1".into(),
            ResponsePayload::Confirmed(true),
        )
        .await;
        assert!(result.is_ok());

        // Verify only subscription 1 receives it
        let msg1 = rx1.recv().await.expect("Sub-1 should receive message");
        match msg1 {
            ClientMessage::Response {
                request_id,
                payload,
            } => {
                assert_eq!(request_id, "req-1");
                assert!(matches!(payload, ResponsePayload::Confirmed(true)));
            }
            _ => panic!("Expected Response message"),
        }

        // Subscription 2 should NOT have received anything
        // Use try_recv to check without blocking
        let msg2 = rx2.try_recv();
        assert!(
            msg2.is_err(),
            "Sub-2 should not receive message meant for sub-1"
        );

        // Now send a different response to subscription 2
        let result = route_response(
            &registry,
            "sub-2",
            "req-2".into(),
            ResponsePayload::Text("hello".into()),
        )
        .await;
        assert!(result.is_ok());

        // Verify subscription 2 receives its message
        let msg2 = rx2.recv().await.expect("Sub-2 should receive message");
        match msg2 {
            ClientMessage::Response {
                request_id,
                payload,
            } => {
                assert_eq!(request_id, "req-2");
                assert!(matches!(payload, ResponsePayload::Text(s) if s == "hello"));
            }
            _ => panic!("Expected Response message"),
        }

        // And subscription 1 should not have received sub-2's message
        let msg1_extra = rx1.try_recv();
        assert!(
            msg1_extra.is_err(),
            "Sub-1 should not receive message meant for sub-2"
        );
    }

    // =========================================================================
    // handle_client_messages Tests
    // =========================================================================

    #[tokio::test]
    async fn test_handle_client_messages_routes_response_to_bidir_channel() {
        let (stream_tx, mut stream_rx) = mpsc::channel(10);
        let bidir_channel = Arc::new(BidirChannel::new(
            stream_tx,
            true,
            vec!["test".into()],
            "hash".into(),
        ));

        let (client_tx, client_rx) = mpsc::channel(10);

        // Start the handler task
        let channel_clone = bidir_channel.clone();
        let handler = tokio::spawn(async move {
            handle_client_messages(channel_clone, client_rx).await;
        });

        // Start a request on the BidirChannel
        let channel_for_request = bidir_channel.clone();
        let request_task = tokio::spawn(async move {
            channel_for_request
                .request_with_timeout(
                    hub_core::plexus::RequestType::Prompt {
                        message: "Enter name".into(),
                        default: None,
                        placeholder: None,
                    },
                    Duration::from_secs(5),
                )
                .await
        });

        // Receive the request from stream
        let stream_item = stream_rx.recv().await.expect("Should receive request");
        let request_id = match stream_item {
            PlexusStreamItem::Request { request_id, .. } => request_id,
            other => panic!("Expected Request, got {:?}", other),
        };

        // Send the response via client_tx (simulating what route_response does)
        client_tx
            .send(ClientMessage::Response {
                request_id,
                payload: ResponsePayload::Text("Alice".into()),
            })
            .await
            .expect("Send should succeed");

        // The handler should route it to the BidirChannel
        let result = request_task.await.expect("Task should complete");
        assert!(
            matches!(result, Ok(ResponsePayload::Text(ref s)) if s == "Alice"),
            "Expected Text('Alice'), got {:?}",
            result
        );

        // Clean up: send cancel to stop the handler
        client_tx
            .send(ClientMessage::Cancel)
            .await
            .expect("Cancel send should succeed");
        handler.await.expect("Handler should complete");
    }

    #[tokio::test]
    async fn test_handle_client_messages_stops_on_cancel() {
        let (stream_tx, _stream_rx) = mpsc::channel(1);
        let bidir_channel = Arc::new(BidirChannel::new(
            stream_tx,
            true,
            vec!["test".into()],
            "hash".into(),
        ));

        let (client_tx, client_rx) = mpsc::channel(10);

        // Start the handler task
        let channel_clone = bidir_channel.clone();
        let handler = tokio::spawn(async move {
            handle_client_messages(channel_clone, client_rx).await;
        });

        // Send a cancel message
        client_tx.send(ClientMessage::Cancel).await.unwrap();

        // Handler should exit gracefully
        let result = tokio::time::timeout(Duration::from_millis(100), handler).await;
        assert!(
            result.is_ok(),
            "Handler should complete promptly after Cancel"
        );
    }

    // =========================================================================
    // Error Handling Tests
    // =========================================================================

    #[tokio::test]
    async fn test_route_response_subscription_not_found() {
        let registry = SubscriptionRegistry::new();

        let result = route_response(
            &registry,
            "nonexistent",
            "req-1".into(),
            ResponsePayload::Confirmed(true),
        )
        .await;

        match result {
            Err(RespondError::SubscriptionNotFound(id)) => {
                assert_eq!(id, "nonexistent");
            }
            other => panic!("Expected SubscriptionNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_route_response_channel_closed() {
        let registry = SubscriptionRegistry::new();
        let (tx, rx) = mpsc::channel(1);

        registry.register(SubscriptionId::from("sub-1"), tx);

        // Close the receiver
        drop(rx);

        // Try to send - should fail with SendFailed
        let result = route_response(
            &registry,
            "sub-1",
            "req-1".into(),
            ResponsePayload::Confirmed(true),
        )
        .await;

        assert!(
            matches!(result, Err(RespondError::SendFailed(_))),
            "Expected SendFailed, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_cancel_subscription_not_found() {
        let registry = SubscriptionRegistry::new();

        let result = cancel_subscription(&registry, "nonexistent").await;

        assert!(
            matches!(result, Err(RespondError::SubscriptionNotFound(_))),
            "Expected SubscriptionNotFound, got {:?}",
            result
        );
    }

    // =========================================================================
    // BidirSubscriptionContext Tests
    // =========================================================================

    #[tokio::test]
    async fn test_bidir_subscription_context_channels_connected() {
        let (ctx, mut stream_rx, _client_tx) = BidirSubscriptionContext::new(
            true,
            vec!["plexus".into(), "test".into()],
            "hash123".into(),
        );

        assert!(ctx.bidir_channel.is_bidirectional());

        // Test that bidir_channel can send to stream_rx
        let channel = ctx.bidir_channel.clone();
        let request_task = tokio::spawn(async move {
            channel
                .request_with_timeout(
                    hub_core::plexus::RequestType::Confirm {
                        message: "test".into(),
                        default: None,
                    },
                    Duration::from_millis(100),
                )
                .await
        });

        // stream_rx should receive the request
        let item = stream_rx.recv().await;
        assert!(item.is_some(), "stream_rx should receive request");

        // Let request timeout (we're not responding)
        let result = request_task.await.expect("Task should complete");
        assert!(
            matches!(result, Err(hub_core::plexus::BidirError::Timeout)),
            "Expected Timeout, got {:?}",
            result
        );
    }

    // =========================================================================
    // to_subscription_message Conversion Tests
    // =========================================================================

    #[test]
    fn test_to_subscription_message_progress() {
        use hub_core::plexus::types::StreamMetadata;

        let metadata = StreamMetadata::new(vec!["test".into()], "hash".into());
        let item = PlexusStreamItem::progress(metadata, "Working...".into(), Some(50.0));

        let msg = to_subscription_message(item);

        match msg {
            SubscriptionMessage::Progress {
                message,
                percentage,
            } => {
                assert_eq!(message, "Working...");
                assert_eq!(percentage, Some(50.0));
            }
            _ => panic!("Expected Progress message"),
        }
    }

    #[test]
    fn test_to_subscription_message_data() {
        use hub_core::plexus::types::StreamMetadata;
        use serde_json::json;

        let metadata = StreamMetadata::new(vec!["test".into()], "hash".into());
        let item = PlexusStreamItem::data(
            metadata,
            "application/json".into(),
            json!({"result": "success"}),
        );

        let msg = to_subscription_message(item);

        match msg {
            SubscriptionMessage::Data {
                content_type,
                content,
            } => {
                assert_eq!(content_type, "application/json");
                assert_eq!(content, json!({"result": "success"}));
            }
            _ => panic!("Expected Data message"),
        }
    }

    #[test]
    fn test_to_subscription_message_error() {
        use hub_core::plexus::types::StreamMetadata;

        let metadata = StreamMetadata::new(vec!["test".into()], "hash".into());
        // PlexusStreamItem::error signature: (metadata, message, error_code, recoverable)
        let item = PlexusStreamItem::error(metadata, "Something went wrong".into(), None, true);

        let msg = to_subscription_message(item);

        match msg {
            SubscriptionMessage::Error {
                message,
                recoverable,
                ..
            } => {
                assert_eq!(message, "Something went wrong");
                assert!(recoverable);
            }
            _ => panic!("Expected Error message"),
        }
    }

    #[test]
    fn test_to_subscription_message_done() {
        use hub_core::plexus::types::StreamMetadata;

        let metadata = StreamMetadata::new(vec!["test".into()], "hash".into());
        let item = PlexusStreamItem::done(metadata);

        let msg = to_subscription_message(item);

        assert!(
            matches!(msg, SubscriptionMessage::Done),
            "Expected Done message"
        );
    }

    // =========================================================================
    // Edge Case Tests (BIDIR-9)
    // =========================================================================

    #[tokio::test]
    async fn test_rapid_sequential_responses_through_registry() {
        let registry = SubscriptionRegistry::new();
        let (tx, mut rx) = mpsc::channel(200);

        registry.register(SubscriptionId::from("sub-1"), tx);

        // Send 100 rapid responses
        for i in 0..100 {
            let result = route_response(
                &registry,
                "sub-1",
                format!("req-{}", i),
                ResponsePayload::Text(format!("response-{}", i)),
            )
            .await;
            assert!(result.is_ok(), "Response {} should succeed", i);
        }

        // Verify all 100 were received
        let mut count = 0;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ClientMessage::Response { request_id, payload } => {
                    let expected_idx: usize = request_id.strip_prefix("req-").unwrap().parse().unwrap();
                    match payload {
                        ResponsePayload::Text(s) => {
                            assert_eq!(s, format!("response-{}", expected_idx));
                        }
                        _ => panic!("Expected Text payload"),
                    }
                    count += 1;
                }
                _ => panic!("Expected Response message"),
            }
        }
        assert_eq!(count, 100, "Should have received all 100 responses");
    }

    #[tokio::test]
    async fn test_unicode_in_route_response() {
        let registry = SubscriptionRegistry::new();
        let (tx, mut rx) = mpsc::channel(10);

        registry.register(SubscriptionId::from("sub-unicode"), tx);

        // Unicode test string: emoji, CJK, Arabic, Hebrew
        let unicode_text = "\u{1F600}\u{1F389} \u{4E2D}\u{6587} \u{0639}\u{0631}\u{0628}\u{064A} \u{05E2}\u{05D1}\u{05E8}\u{05D9}\u{05EA}";

        let result = route_response(
            &registry,
            "sub-unicode",
            "req-unicode".into(),
            ResponsePayload::Text(unicode_text.to_string()),
        )
        .await;

        assert!(result.is_ok(), "Unicode response should succeed");

        let msg = rx.recv().await.expect("Should receive message");
        match msg {
            ClientMessage::Response { payload, .. } => match payload {
                ResponsePayload::Text(s) => {
                    assert_eq!(s, unicode_text, "Unicode should be preserved");
                }
                _ => panic!("Expected Text payload"),
            },
            _ => panic!("Expected Response message"),
        }
    }

    #[tokio::test]
    async fn test_very_long_text_through_registry() {
        let registry = SubscriptionRegistry::new();
        let (tx, mut rx) = mpsc::channel(10);

        registry.register(SubscriptionId::from("sub-long"), tx);

        // 1MB text
        let long_text: String = "x".repeat(1_000_000);

        let result = route_response(
            &registry,
            "sub-long",
            "req-long".into(),
            ResponsePayload::Text(long_text.clone()),
        )
        .await;

        assert!(result.is_ok(), "Long text response should succeed");

        let msg = rx.recv().await.expect("Should receive message");
        match msg {
            ClientMessage::Response { payload, .. } => match payload {
                ResponsePayload::Text(s) => {
                    assert_eq!(s.len(), 1_000_000, "Should preserve 1MB text");
                    assert_eq!(s, long_text);
                }
                _ => panic!("Expected Text payload"),
            },
            _ => panic!("Expected Response message"),
        }
    }

    #[tokio::test]
    async fn test_many_concurrent_subscriptions() {
        let registry = SubscriptionRegistry::new();
        let mut receivers = Vec::new();

        // Register 50 concurrent subscriptions
        for i in 0..50 {
            let (tx, rx) = mpsc::channel(10);
            registry.register(SubscriptionId::from(format!("sub-{}", i)), tx);
            receivers.push(rx);
        }

        assert_eq!(registry.len(), 50);

        // Send to each subscription
        for i in 0..50 {
            let result = route_response(
                &registry,
                &format!("sub-{}", i),
                format!("req-{}", i),
                ResponsePayload::Confirmed(true),
            )
            .await;
            assert!(result.is_ok());
        }

        // Verify each received exactly one message
        for (i, mut rx) in receivers.into_iter().enumerate() {
            let msg = rx.recv().await.expect(&format!("Sub-{} should receive message", i));
            match msg {
                ClientMessage::Response { request_id, .. } => {
                    assert_eq!(request_id, format!("req-{}", i));
                }
                _ => panic!("Expected Response"),
            }
            // Should be no more messages
            assert!(rx.try_recv().is_err(), "Sub-{} should have only one message", i);
        }
    }

    #[tokio::test]
    async fn test_subscription_unregister_then_route_fails() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::channel(10);

        registry.register(SubscriptionId::from("sub-temp"), tx);
        assert_eq!(registry.len(), 1);

        // Unregister
        registry.unregister(&SubscriptionId::from("sub-temp"));
        assert!(registry.is_empty());

        // Routing should now fail
        let result = route_response(
            &registry,
            "sub-temp",
            "req-1".into(),
            ResponsePayload::Confirmed(true),
        )
        .await;

        assert!(
            matches!(result, Err(RespondError::SubscriptionNotFound(_))),
            "Should fail after unregister"
        );
    }

    #[tokio::test]
    async fn test_handle_client_messages_ignores_bad_request_ids() {
        let (stream_tx, _stream_rx) = mpsc::channel(10);
        let bidir_channel = Arc::new(BidirChannel::new(
            stream_tx,
            true,
            vec!["test".into()],
            "hash".into(),
        ));

        let (client_tx, client_rx) = mpsc::channel(10);

        // Start handler
        let channel_clone = bidir_channel.clone();
        let handler = tokio::spawn(async move {
            handle_client_messages(channel_clone, client_rx).await;
        });

        // Send response with non-existent request ID (should log warning but not crash)
        client_tx
            .send(ClientMessage::Response {
                request_id: "nonexistent-request-id".into(),
                payload: ResponsePayload::Confirmed(true),
            })
            .await
            .expect("Send should succeed");

        // Send cancel to stop handler
        client_tx
            .send(ClientMessage::Cancel)
            .await
            .expect("Cancel send should succeed");

        // Handler should complete gracefully without panicking
        let result = tokio::time::timeout(Duration::from_millis(100), handler).await;
        assert!(
            result.is_ok(),
            "Handler should complete gracefully even with bad request IDs"
        );
    }

    #[test]
    fn test_registry_concurrent_access() {
        use std::thread;

        let registry = Arc::new(SubscriptionRegistry::new());

        // Spawn multiple threads doing concurrent operations
        let mut handles = vec![];

        for i in 0..10 {
            let reg = registry.clone();
            handles.push(thread::spawn(move || {
                let (tx, _rx) = mpsc::channel(1);
                let id = SubscriptionId::from(format!("concurrent-{}", i));

                // Register
                reg.register(id.clone(), tx);

                // Read
                assert!(reg.get(&id).is_some());

                // Check length (might vary due to concurrent access)
                let _ = reg.len();

                // Unregister
                reg.unregister(&id);

                assert!(reg.get(&id).is_none());
            }));
        }

        for h in handles {
            h.join().expect("Thread should complete without panic");
        }

        // Registry should be empty after all threads complete
        assert!(registry.is_empty(), "Registry should be empty after all unregisters");
    }
}

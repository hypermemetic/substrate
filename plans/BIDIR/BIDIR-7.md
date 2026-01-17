# BIDIR-7: WebSocket Transport Mapping

**Status:** Planning
**Blocked By:** BIDIR-3, BIDIR-4, BIDIR-5
**Unlocks:** BIDIR-8

## Scope

Map bidirectional protocol to jsonrpsee WebSocket subscriptions. Unlike MCP which requires a workaround pattern, WebSocket already supports true bidirectional communication, making this implementation more straightforward.

## Acceptance Criteria

- [ ] Handle `PlexusStreamItem::Request` in WebSocket subscription stream
- [ ] Accept client responses via subscription messaging
- [ ] Integrate with existing jsonrpsee subscription infrastructure
- [ ] Support multiple concurrent bidirectional requests per subscription
- [ ] Timeout handling at transport level

## Design Considerations

WebSocket connections are inherently bidirectional, so we can use a cleaner approach than MCP:

```
┌───────────────────────────────────────────────────────────────────────┐
│ 1. Client subscribes: plexus_subscribe { method: "sync", params: {} } │
│                                                                       │
│ 2. Server streams via subscription:                                   │
│    - { type: "progress", message: "Processing..." }                   │
│    - { type: "request", request_id: "abc", request_type: "confirm" }  │
│                                                                       │
│ 3. Client sends on same subscription:                                 │
│    - { type: "response", request_id: "abc", payload: true }           │
│                                                                       │
│ 4. Server continues streaming:                                        │
│    - { type: "data", content: {...} }                                 │
│    - { type: "done" }                                                 │
└───────────────────────────────────────────────────────────────────────┘
```

## Implementation Notes

### Subscription Message Types

```rust
// hub-core/src/plexus/transport/websocket.rs

use serde::{Deserialize, Serialize};

/// Messages sent from server to client during subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SubscriptionMessage {
    /// Progress update
    Progress {
        message: String,
        percentage: Option<f32>,
    },

    /// Data payload
    Data {
        content_type: String,
        content: Value,
    },

    /// Server requests client input
    Request {
        request_id: String,
        request_type: RequestType,
        timeout_ms: Option<u64>,
    },

    /// Error during processing
    Error {
        message: String,
        code: Option<String>,
        recoverable: bool,
    },

    /// Stream complete
    Done,
}

/// Messages sent from client to server during subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Response to a server request
    Response {
        request_id: String,
        payload: ResponsePayload,
    },

    /// Client-initiated cancellation
    Cancel,
}

impl From<PlexusStreamItem> for SubscriptionMessage {
    fn from(item: PlexusStreamItem) -> Self {
        match item {
            PlexusStreamItem::Progress { message, percentage, .. } => {
                SubscriptionMessage::Progress { message, percentage }
            }
            PlexusStreamItem::Data { content_type, content, .. } => {
                SubscriptionMessage::Data { content_type, content }
            }
            PlexusStreamItem::Request { request_id, request_type, timeout_ms, .. } => {
                SubscriptionMessage::Request { request_id, request_type, timeout_ms }
            }
            PlexusStreamItem::Error { message, code, recoverable, .. } => {
                SubscriptionMessage::Error { message, code, recoverable }
            }
            PlexusStreamItem::Done { .. } => SubscriptionMessage::Done,
        }
    }
}
```

### Bidirectional Subscription Handler

```rust
// substrate/src/ws_server.rs

use jsonrpsee::core::SubscriptionSink;
use tokio::sync::mpsc;

/// Handle a bidirectional plexus subscription
pub async fn handle_bidir_subscription(
    plexus: Arc<Plexus>,
    method: String,
    params: Value,
    mut sink: SubscriptionSink,
    mut client_rx: mpsc::Receiver<ClientMessage>,
) -> Result<(), PlexusError> {
    // Create bidirectional channel
    let (stream_tx, mut stream_rx) = mpsc::channel::<PlexusStreamItem>(32);
    let bidir_channel = Arc::new(BidirChannel::new(stream_tx, true));

    // Spawn response handler - routes client messages to channel
    let channel_clone = bidir_channel.clone();
    let response_handler = tokio::spawn(async move {
        while let Some(msg) = client_rx.recv().await {
            match msg {
                ClientMessage::Response { request_id, payload } => {
                    let response = ClientResponse { request_id, payload };
                    if let Err(e) = channel_clone.handle_response(response) {
                        tracing::warn!("Failed to route response: {:?}", e);
                    }
                }
                ClientMessage::Cancel => {
                    // Trigger cancellation - implementation depends on CancellationToken
                    break;
                }
            }
        }
    });

    // Start the plexus stream with context
    let stream = plexus.route_with_context(&method, params, Some(bidir_channel)).await?;
    tokio::pin!(stream);

    // Merge stream items into output
    loop {
        tokio::select! {
            Some(item) = stream.next() => {
                let message = SubscriptionMessage::from(item.clone());

                // Send to client
                if sink.send(&message).await.is_err() {
                    // Client disconnected
                    break;
                }

                // Check for done
                if matches!(item, PlexusStreamItem::Done { .. }) {
                    break;
                }
            }
            else => break,
        }
    }

    // Cleanup
    response_handler.abort();
    Ok(())
}
```

### RPC Module Integration

```rust
// substrate/src/ws_server.rs

use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::{RpcResult, SubscriptionResult};
use jsonrpsee::PendingSubscriptionSink;

#[rpc(server)]
pub trait PlexusRpc {
    /// Call a plexus method with bidirectional support
    #[subscription(
        name = "plexus_subscribe" => "plexus_subscription",
        unsubscribe = "plexus_unsubscribe",
        item = SubscriptionMessage
    )]
    async fn subscribe(
        &self,
        method: String,
        params: Option<Value>,
    ) -> SubscriptionResult;
}

pub struct PlexusRpcHandler {
    plexus: Arc<Plexus>,
}

impl PlexusRpcServer for PlexusRpcHandler {
    async fn subscribe(
        &self,
        pending: PendingSubscriptionSink,
        method: String,
        params: Option<Value>,
    ) -> SubscriptionResult {
        let sink = pending.accept().await?;
        let params = params.unwrap_or(Value::Null);

        // Create channel for client messages
        let (client_tx, client_rx) = mpsc::channel(32);

        // Store sender for this subscription (keyed by subscription ID)
        self.register_subscription(sink.subscription_id(), client_tx);

        // Handle subscription
        let plexus = self.plexus.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_bidir_subscription(plexus, method, params, sink, client_rx).await {
                tracing::error!("Subscription error: {:?}", e);
            }
        });

        Ok(())
    }
}
```

### Client Message Ingestion

```rust
// substrate/src/ws_server.rs

/// Handle incoming client messages on the WebSocket
pub async fn handle_client_message(
    handler: &PlexusRpcHandler,
    subscription_id: SubscriptionId,
    message: ClientMessage,
) {
    if let Some(tx) = handler.get_subscription_sender(&subscription_id) {
        if tx.send(message).await.is_err() {
            tracing::warn!("Subscription {} closed, dropping message", subscription_id);
        }
    }
}

/// Custom RPC method for clients to send responses
#[rpc(server)]
pub trait PlexusBidirRpc {
    /// Send a response to a pending request within a subscription
    #[method(name = "plexus_respond")]
    async fn respond(
        &self,
        subscription_id: String,
        request_id: String,
        payload: ResponsePayload,
    ) -> RpcResult<bool>;
}

impl PlexusBidirRpcServer for PlexusRpcHandler {
    async fn respond(
        &self,
        subscription_id: String,
        request_id: String,
        payload: ResponsePayload,
    ) -> RpcResult<bool> {
        let message = ClientMessage::Response { request_id, payload };
        let sub_id = SubscriptionId::from(subscription_id);

        if let Some(tx) = self.get_subscription_sender(&sub_id) {
            tx.send(message).await
                .map(|_| true)
                .map_err(|_| ErrorObjectOwned::owned(
                    -32000,
                    "Subscription not found or closed",
                    None::<()>
                ))
        } else {
            Err(ErrorObjectOwned::owned(
                -32000,
                "Subscription not found",
                None::<()>
            ))
        }
    }
}
```

### Subscription Registry

```rust
// substrate/src/ws_server.rs

use std::collections::HashMap;
use parking_lot::RwLock;

/// Registry of active subscriptions for routing client messages
pub struct SubscriptionRegistry {
    senders: RwLock<HashMap<SubscriptionId, mpsc::Sender<ClientMessage>>>,
}

impl SubscriptionRegistry {
    pub fn new() -> Self {
        Self {
            senders: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, id: SubscriptionId, tx: mpsc::Sender<ClientMessage>) {
        self.senders.write().insert(id, tx);
    }

    pub fn unregister(&self, id: &SubscriptionId) {
        self.senders.write().remove(id);
    }

    pub fn get(&self, id: &SubscriptionId) -> Option<mpsc::Sender<ClientMessage>> {
        self.senders.read().get(id).cloned()
    }
}

impl PlexusRpcHandler {
    fn register_subscription(&self, id: SubscriptionId, tx: mpsc::Sender<ClientMessage>) {
        self.registry.register(id, tx);
    }

    fn get_subscription_sender(&self, id: &SubscriptionId) -> Option<mpsc::Sender<ClientMessage>> {
        self.registry.get(id)
    }
}
```

### Client-Side Example (TypeScript)

```typescript
// Example client implementation using jsonrpsee client
import { WebSocket } from 'ws';

class PlexusBidirClient {
  private ws: WebSocket;
  private pendingResponses: Map<string, (payload: any) => void> = new Map();

  async subscribeWithBidir(method: string, params: any) {
    // Subscribe to the stream
    const subscriptionId = await this.rpc('plexus_subscribe', [method, params]);

    // Handle incoming messages
    this.ws.on('message', (data) => {
      const msg = JSON.parse(data.toString());

      if (msg.params?.subscription === subscriptionId) {
        const item = msg.params.result;

        switch (item.type) {
          case 'request':
            // Present UI and collect response
            this.handleRequest(subscriptionId, item);
            break;
          case 'progress':
            console.log(`Progress: ${item.message}`);
            break;
          case 'data':
            console.log('Data:', item.content);
            break;
          case 'done':
            console.log('Stream complete');
            break;
        }
      }
    });
  }

  private async handleRequest(subscriptionId: string, request: any) {
    // Example: Auto-confirm for demo
    const response = await this.promptUser(request.request_type);

    // Send response via RPC
    await this.rpc('plexus_respond', [
      subscriptionId,
      request.request_id,
      response
    ]);
  }
}
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `hub-core/src/plexus/transport/websocket.rs` | NEW: SubscriptionMessage, ClientMessage types |
| `hub-core/src/plexus/transport/mod.rs` | NEW: transport module |
| `substrate/src/ws_server.rs` | Add bidir subscription handling |

## Testing

```rust
#[tokio::test]
async fn test_ws_bidir_request_response() {
    // Start test server
    let (server, addr) = start_test_server().await;

    // Connect client
    let client = WsClientBuilder::default()
        .build(&format!("ws://{}", addr))
        .await
        .unwrap();

    // Subscribe to interactive method
    let mut sub = client
        .subscribe::<SubscriptionMessage, _>("plexus_subscribe", rpc_params!["interactive_demo", {}])
        .await
        .unwrap();

    // Receive progress
    let msg = sub.next().await.unwrap().unwrap();
    assert!(matches!(msg, SubscriptionMessage::Progress { .. }));

    // Receive request
    let msg = sub.next().await.unwrap().unwrap();
    let request_id = match msg {
        SubscriptionMessage::Request { request_id, .. } => request_id,
        _ => panic!("Expected request"),
    };

    // Send response
    let sub_id = sub.subscription_id().to_string();
    client
        .request::<bool, _>("plexus_respond", rpc_params![sub_id, request_id, {"Confirmed": true}])
        .await
        .unwrap();

    // Receive data
    let msg = sub.next().await.unwrap().unwrap();
    assert!(matches!(msg, SubscriptionMessage::Data { .. }));

    // Receive done
    let msg = sub.next().await.unwrap().unwrap();
    assert!(matches!(msg, SubscriptionMessage::Done));
}

#[tokio::test]
async fn test_ws_bidir_timeout() {
    let (server, addr) = start_test_server().await;
    let client = WsClientBuilder::default()
        .build(&format!("ws://{}", addr))
        .await
        .unwrap();

    let mut sub = client
        .subscribe::<SubscriptionMessage, _>("plexus_subscribe", rpc_params!["timeout_demo", {}])
        .await
        .unwrap();

    // Receive request
    let msg = sub.next().await.unwrap().unwrap();
    assert!(matches!(msg, SubscriptionMessage::Request { .. }));

    // Don't respond - wait for timeout
    tokio::time::sleep(Duration::from_secs(35)).await;

    // Should receive error or done
    let msg = sub.next().await.unwrap().unwrap();
    assert!(matches!(
        msg,
        SubscriptionMessage::Error { .. } | SubscriptionMessage::Done
    ));
}

#[tokio::test]
async fn test_ws_bidir_cancel() {
    let (server, addr) = start_test_server().await;
    let client = WsClientBuilder::default()
        .build(&format!("ws://{}", addr))
        .await
        .unwrap();

    let mut sub = client
        .subscribe::<SubscriptionMessage, _>("plexus_subscribe", rpc_params!["long_running", {}])
        .await
        .unwrap();

    // Receive request
    let msg = sub.next().await.unwrap().unwrap();
    assert!(matches!(msg, SubscriptionMessage::Request { .. }));

    // Send cancel
    let sub_id = sub.subscription_id().to_string();
    // Unsubscribe to cancel
    drop(sub);

    // Server should clean up gracefully
}

#[tokio::test]
async fn test_ws_concurrent_requests() {
    let (server, addr) = start_test_server().await;
    let client = WsClientBuilder::default()
        .build(&format!("ws://{}", addr))
        .await
        .unwrap();

    // Start two subscriptions
    let mut sub1 = client
        .subscribe::<SubscriptionMessage, _>("plexus_subscribe", rpc_params!["interactive_demo", {}])
        .await
        .unwrap();

    let mut sub2 = client
        .subscribe::<SubscriptionMessage, _>("plexus_subscribe", rpc_params!["interactive_demo", {}])
        .await
        .unwrap();

    // Both should work independently
    // ... handle requests for both subscriptions
}
```

## Notes

- WebSocket is cleaner than MCP because it's naturally bidirectional
- jsonrpsee subscriptions provide the infrastructure we need
- `plexus_respond` is a separate RPC call rather than inline in subscription for simplicity
- Subscription registry uses `RwLock` for concurrent read access
- Client disconnection triggers cleanup via subscription drop
- Multiple concurrent subscriptions from same client are supported via unique subscription IDs

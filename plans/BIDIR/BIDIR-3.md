# BIDIR-3: Channel-Based Activation Context

**Status:** Planning
**Blocked By:** BIDIR-2
**Unlocks:** BIDIR-6, BIDIR-7

## Scope

Create the channel-based infrastructure that enables activations to send requests and receive responses during method execution.

## Acceptance Criteria

- [ ] `BidirChannel` struct for request/response communication
- [ ] Integration with `Activation::call()` signature (backward compatible)
- [ ] Request ID generation and correlation
- [ ] Timeout handling with configurable defaults

## Implementation Notes

### BidirChannel Implementation

```rust
// hub-core/src/plexus/bidirectional.rs

use tokio::sync::{mpsc, oneshot};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

/// Channel for bidirectional communication during method execution
pub struct BidirChannel {
    /// Sender for PlexusStreamItem (including Request variants)
    stream_tx: mpsc::Sender<PlexusStreamItem>,

    /// Pending requests awaiting responses
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ResponsePayload>>>>,

    /// Default timeout for requests
    default_timeout: Duration,

    /// Whether bidirectional is supported by transport
    bidirectional_supported: bool,
}

impl BidirChannel {
    /// Create a new bidirectional channel
    pub fn new(
        stream_tx: mpsc::Sender<PlexusStreamItem>,
        bidirectional_supported: bool,
    ) -> Self {
        Self {
            stream_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            default_timeout: Duration::from_secs(30),
            bidirectional_supported,
        }
    }

    /// Create a unidirectional-only channel (for backward compatibility)
    pub fn unidirectional(stream_tx: mpsc::Sender<PlexusStreamItem>) -> Self {
        Self::new(stream_tx, false)
    }

    /// Handle incoming response from client
    pub fn handle_response(&self, response: ClientResponse) -> Result<(), BidirError> {
        let mut pending = self.pending.lock();
        if let Some(tx) = pending.remove(&response.request_id) {
            tx.send(response.payload)
                .map_err(|_| BidirError::Transport("Response channel closed".into()))?;
            Ok(())
        } else {
            Err(BidirError::Transport(format!(
                "No pending request with ID: {}",
                response.request_id
            )))
        }
    }

    /// Generate unique request ID
    fn generate_request_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[async_trait]
impl BidirectionalContext for BidirChannel {
    async fn request(&self, request_type: RequestType) -> Result<ResponsePayload, BidirError> {
        self.request_with_timeout(request_type, self.default_timeout).await
    }

    async fn request_with_timeout(
        &self,
        request_type: RequestType,
        timeout: Duration,
    ) -> Result<ResponsePayload, BidirError> {
        if !self.bidirectional_supported {
            return Err(BidirError::NotSupported);
        }

        let request_id = Self::generate_request_id();
        let (tx, rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending.lock();
            pending.insert(request_id.clone(), tx);
        }

        // Build and send request item
        let request_item = PlexusStreamItem::Request {
            metadata: StreamMetadata::now(),
            request_id: request_id.clone(),
            request_type,
            payload: Value::Null,
            timeout_ms: Some(timeout.as_millis() as u64),
        };

        self.stream_tx.send(request_item).await
            .map_err(|_| BidirError::Transport("Stream channel closed".into()))?;

        // Wait for response with timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(_)) => {
                // Channel was dropped
                self.pending.lock().remove(&request_id);
                Err(BidirError::Transport("Response channel dropped".into()))
            }
            Err(_) => {
                // Timeout
                self.pending.lock().remove(&request_id);
                Err(BidirError::Timeout)
            }
        }
    }

    fn is_bidirectional(&self) -> bool {
        self.bidirectional_supported
    }
}
```

### CallContext Wrapper

```rust
// hub-core/src/plexus/bidirectional.rs

/// Context passed to activation methods
pub struct CallContext {
    /// Channel for bidirectional communication
    pub bidir: Arc<BidirChannel>,

    /// Cancellation token
    pub cancellation: tokio_util::sync::CancellationToken,

    /// Method being called (for provenance)
    pub method: String,
}

impl CallContext {
    pub fn new(bidir: Arc<BidirChannel>, method: String) -> Self {
        Self {
            bidir,
            cancellation: tokio_util::sync::CancellationToken::new(),
            method,
        }
    }

    /// Create context for unidirectional methods (backward compatible)
    pub fn unidirectional(stream_tx: mpsc::Sender<PlexusStreamItem>, method: String) -> Self {
        Self::new(
            Arc::new(BidirChannel::unidirectional(stream_tx)),
            method,
        )
    }
}
```

### Stream Builder with Bidirectional Support

```rust
// hub-core/src/plexus/streaming.rs (additions)

/// Build a bidirectional stream from an activation method
pub fn bidirectional_stream<T, F, Fut>(
    method_name: &str,
    provenance: Vec<String>,
    context: Arc<BidirChannel>,
    f: F,
) -> PlexusStream
where
    T: Serialize + Send + 'static,
    F: FnOnce(Arc<BidirChannel>) -> Fut + Send + 'static,
    Fut: Future<Output = impl Stream<Item = T> + Send + 'static> + Send + 'static,
{
    let method = method_name.to_string();
    let (tx, rx) = mpsc::channel(32);

    // Spawn task that runs the method with context
    tokio::spawn(async move {
        let stream = f(context).await;
        tokio::pin!(stream);
        while let Some(item) = stream.next().await {
            let wrapped = wrap_item(item, &method, &provenance);
            if tx.send(wrapped).await.is_err() {
                break;
            }
        }
        // Send Done
        let _ = tx.send(PlexusStreamItem::Done {
            metadata: StreamMetadata::now_with_provenance(&method, &provenance)
        }).await;
    });

    Box::pin(ReceiverStream::new(rx))
}
```

## Integration with Existing Architecture

### Option A: New Activation Method Signature (Recommended)

```rust
// For bidirectional methods, activation implements:
pub trait ActivationBidir: Activation {
    async fn call_bidir(
        &self,
        method: &str,
        params: Value,
        context: CallContext,
    ) -> Result<PlexusStream, PlexusError>;
}

// Default implementation falls back to unidirectional:
impl<T: Activation> ActivationBidir for T {
    async fn call_bidir(
        &self,
        method: &str,
        params: Value,
        _context: CallContext,
    ) -> Result<PlexusStream, PlexusError> {
        self.call(method, params).await
    }
}
```

### Option B: Context in Route

```rust
// Plexus::route gains optional context parameter
impl Plexus {
    pub async fn route_bidir(
        &self,
        method: &str,
        params: Value,
        bidir_tx: Option<mpsc::Sender<PlexusStreamItem>>,
        bidir_rx: Option<mpsc::Receiver<ClientResponse>>,
    ) -> Result<PlexusStream, PlexusError> {
        // ... dispatch with context
    }
}
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `hub-core/src/plexus/bidirectional.rs` | Add BidirChannel, CallContext |
| `hub-core/src/plexus/streaming.rs` | Add bidirectional_stream helper |
| `hub-core/src/plexus/plexus.rs` | Add route_bidir method |

## Testing

```rust
#[tokio::test]
async fn test_bidir_channel_request_response() {
    let (stream_tx, mut stream_rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(stream_tx, true));

    // Spawn request task
    let channel_clone = channel.clone();
    let request_task = tokio::spawn(async move {
        channel_clone.confirm("Continue?").await
    });

    // Receive request from stream
    let item = stream_rx.recv().await.unwrap();
    let request_id = match item {
        PlexusStreamItem::Request { request_id, .. } => request_id,
        _ => panic!("Expected Request"),
    };

    // Send response
    channel.handle_response(ClientResponse {
        request_id,
        payload: ResponsePayload::Confirmed(true),
    }).unwrap();

    // Check result
    let result = request_task.await.unwrap();
    assert_eq!(result.unwrap(), true);
}

#[tokio::test]
async fn test_bidir_channel_timeout() {
    let (stream_tx, _stream_rx) = mpsc::channel(32);
    let channel = BidirChannel::new(stream_tx, true);

    let result = channel.request_with_timeout(
        RequestType::Confirm { message: "Test".into(), default: None },
        Duration::from_millis(100),
    ).await;

    assert!(matches!(result, Err(BidirError::Timeout)));
}

#[tokio::test]
async fn test_bidir_not_supported() {
    let (stream_tx, _) = mpsc::channel(32);
    let channel = BidirChannel::unidirectional(stream_tx);

    let result = channel.confirm("Test").await;
    assert!(matches!(result, Err(BidirError::NotSupported)));
}
```

## Notes

- Channel buffer size (32) is configurable
- Pending requests are cleaned up on timeout
- Thread-safe via `parking_lot::Mutex` for low contention
- `CancellationToken` enables cooperative cancellation

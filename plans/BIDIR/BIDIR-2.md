# BIDIR-2: Core Types and Traits

**Status:** Planning
**Blocked By:** -
**Unlocks:** BIDIR-3, BIDIR-4, BIDIR-5

## Scope

Define the core types needed for bidirectional streaming in hub-core.

## Acceptance Criteria

- [ ] New `PlexusStreamItem` variant for server requests
- [ ] New type for client responses
- [ ] Request/response correlation via IDs
- [ ] Backward compatible - existing variants unchanged

## Implementation Notes

### New PlexusStreamItem Variant

```rust
// hub-core/src/plexus/types.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlexusStreamItem {
    // Existing variants (unchanged)
    Data { metadata: StreamMetadata, content_type: String, content: Value },
    Progress { metadata: StreamMetadata, message: String, percentage: Option<f32> },
    Error { metadata: StreamMetadata, message: String, code: Option<String>, recoverable: bool },
    Done { metadata: StreamMetadata },

    // NEW: Server requests input from client
    Request {
        metadata: StreamMetadata,
        request_id: String,           // Unique ID for correlation
        request_type: RequestType,    // Type of request (prompt, confirm, etc.)
        payload: Value,               // Request-specific data
        timeout_ms: Option<u64>,      // Optional timeout
    },
}
```

### RequestType Enum

```rust
// hub-core/src/plexus/types.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestType {
    /// Yes/No confirmation
    Confirm {
        message: String,
        default: Option<bool>,
    },

    /// Text input prompt
    Prompt {
        message: String,
        default: Option<String>,
        placeholder: Option<String>,
    },

    /// Select from options
    Select {
        message: String,
        options: Vec<SelectOption>,
        multi_select: bool,
    },

    /// Custom request (escape hatch)
    Custom {
        type_name: String,
        schema: Option<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}
```

### ClientResponse Type

```rust
// hub-core/src/plexus/types.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientResponse {
    /// Matches PlexusStreamItem::Request.request_id
    pub request_id: String,

    /// Response payload matching request type
    pub payload: ResponsePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponsePayload {
    /// Response to Confirm request
    Confirmed(bool),

    /// Response to Prompt request
    Text(String),

    /// Response to Select request
    Selected(Vec<String>),

    /// Custom response
    Custom(Value),

    /// Client cancelled/declined
    Cancelled,

    /// Client timed out (sent by transport layer)
    Timeout,
}
```

### BidirectionalContext Trait

```rust
// hub-core/src/plexus/bidirectional.rs (NEW FILE)

use async_trait::async_trait;

/// Context for methods that support bidirectional communication
#[async_trait]
pub trait BidirectionalContext: Send + Sync {
    /// Send a request to the client and wait for response
    async fn request(&self, request_type: RequestType) -> Result<ResponsePayload, BidirError>;

    /// Send a request with custom timeout
    async fn request_with_timeout(
        &self,
        request_type: RequestType,
        timeout: Duration,
    ) -> Result<ResponsePayload, BidirError>;

    /// Check if bidirectional communication is supported by current transport
    fn is_bidirectional(&self) -> bool;
}

#[derive(Debug, Clone)]
pub enum BidirError {
    /// Client cancelled the request
    Cancelled,
    /// Request timed out waiting for response
    Timeout,
    /// Transport doesn't support bidirectional
    NotSupported,
    /// Response type doesn't match request type
    TypeMismatch { expected: String, got: String },
    /// Transport error
    Transport(String),
}
```

### Helper Traits for Common Patterns

```rust
// hub-core/src/plexus/bidirectional.rs

/// Extension trait for common request patterns
#[async_trait]
pub trait BidirExt: BidirectionalContext {
    /// Simple yes/no confirmation
    async fn confirm(&self, message: impl Into<String> + Send) -> Result<bool, BidirError> {
        match self.request(RequestType::Confirm {
            message: message.into(),
            default: None
        }).await? {
            ResponsePayload::Confirmed(v) => Ok(v),
            ResponsePayload::Cancelled => Err(BidirError::Cancelled),
            ResponsePayload::Timeout => Err(BidirError::Timeout),
            other => Err(BidirError::TypeMismatch {
                expected: "Confirmed".into(),
                got: format!("{:?}", other)
            }),
        }
    }

    /// Text input prompt
    async fn prompt(&self, message: impl Into<String> + Send) -> Result<String, BidirError> {
        match self.request(RequestType::Prompt {
            message: message.into(),
            default: None,
            placeholder: None,
        }).await? {
            ResponsePayload::Text(s) => Ok(s),
            ResponsePayload::Cancelled => Err(BidirError::Cancelled),
            ResponsePayload::Timeout => Err(BidirError::Timeout),
            other => Err(BidirError::TypeMismatch {
                expected: "Text".into(),
                got: format!("{:?}", other)
            }),
        }
    }

    /// Select from options (returns first selection or error)
    async fn select<T: Into<SelectOption> + Send>(
        &self,
        message: impl Into<String> + Send,
        options: Vec<T>,
    ) -> Result<String, BidirError> {
        let options: Vec<SelectOption> = options.into_iter().map(|o| o.into()).collect();
        match self.request(RequestType::Select {
            message: message.into(),
            options,
            multi_select: false,
        }).await? {
            ResponsePayload::Selected(mut v) => {
                v.pop().ok_or(BidirError::Cancelled)
            },
            ResponsePayload::Cancelled => Err(BidirError::Cancelled),
            ResponsePayload::Timeout => Err(BidirError::Timeout),
            other => Err(BidirError::TypeMismatch {
                expected: "Selected".into(),
                got: format!("{:?}", other)
            }),
        }
    }
}

// Blanket implementation
impl<T: BidirectionalContext> BidirExt for T {}
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `hub-core/src/plexus/types.rs` | Add Request variant, RequestType, ResponsePayload |
| `hub-core/src/plexus/bidirectional.rs` | NEW: BidirectionalContext trait and helpers |
| `hub-core/src/plexus/mod.rs` | Export new module |

## Testing

```rust
#[test]
fn test_request_type_serialization() {
    let req = RequestType::Confirm {
        message: "Continue?".into(),
        default: Some(true)
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: RequestType = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, RequestType::Confirm { .. }));
}

#[test]
fn test_response_payload_variants() {
    let responses = vec![
        ResponsePayload::Confirmed(true),
        ResponsePayload::Text("hello".into()),
        ResponsePayload::Selected(vec!["a".into(), "b".into()]),
        ResponsePayload::Custom(json!({"key": "value"})),
        ResponsePayload::Cancelled,
        ResponsePayload::Timeout,
    ];

    for resp in responses {
        let json = serde_json::to_string(&resp).unwrap();
        let _: ResponsePayload = serde_json::from_str(&json).unwrap();
    }
}
```

## Notes

- Request IDs should be UUID v4 for uniqueness
- Timeout defaults to 30 seconds if not specified
- `Custom` variants provide escape hatch for domain-specific interactions
- All types must be `Send + Sync` for async compatibility

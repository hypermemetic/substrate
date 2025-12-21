# MCP-13: Cancellation Support

## Metadata
- **blocked_by:** [MCP-10]
- **unlocks:** []
- **priority:** Medium

## Scope

Implement `notifications/cancelled` to abort in-progress tool calls.

## Protocol

**Notification (no response):**
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/cancelled",
  "params": {
    "requestId": "3",
    "reason": "User cancelled"
  }
}
```

## Implementation

```rust
// src/mcp/handlers/cancelled.rs

#[derive(Debug, Deserialize)]
pub struct CancelledParams {
    #[serde(rename = "requestId")]
    pub request_id: Value,  // Can be string or number
    pub reason: Option<String>,
}

impl McpInterface {
    /// Track pending requests that can be cancelled
    pending_requests: Arc<RwLock<HashMap<String, CancellationToken>>>,

    pub async fn handle_cancelled(&self, params: Value) -> Result<Value, McpError> {
        let params: CancelledParams = serde_json::from_value(params)?;

        let request_id = match params.request_id {
            Value::String(s) => s,
            Value::Number(n) => n.to_string(),
            _ => return Err(McpError::InvalidParams("requestId must be string or number")),
        };

        // Find and cancel the pending request
        if let Some(token) = self.pending_requests.write().unwrap().remove(&request_id) {
            token.cancel();
            tracing::info!(
                request_id = %request_id,
                reason = ?params.reason,
                "Cancelled pending request"
            );
        }

        // Notification - no response
        Ok(Value::Null)
    }

    /// Register a cancellable request
    fn register_cancellable(&self, request_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.pending_requests
            .write()
            .unwrap()
            .insert(request_id.to_string(), token.clone());
        token
    }

    /// Modified tools/call to support cancellation
    pub async fn handle_tools_call_cancellable(
        &self,
        request_id: &str,
        params: Value,
    ) -> Result<Value, McpError> {
        let cancel_token = self.register_cancellable(request_id);

        // ... existing tools/call logic ...

        // Pass token to stream buffering
        let result = buffer_plexus_stream_cancellable(stream, cancel_token).await;

        // Cleanup
        self.pending_requests.write().unwrap().remove(request_id);

        Ok(serde_json::to_value(result)?)
    }
}

/// Buffering with cancellation support
async fn buffer_plexus_stream_cancellable<S>(
    stream: S,
    cancel: CancellationToken,
) -> ToolCallResult {
    pin_mut!(stream);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return ToolCallResult {
                    content: vec![McpContent {
                        content_type: "text".into(),
                        text: "Operation cancelled".into(),
                    }],
                    is_error: true,
                };
            }
            event = stream.next() => {
                match event {
                    Some(e) => { /* process as normal */ }
                    None => break,
                }
            }
        }
    }

    // ... build result ...
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/cancelled.rs`
- Modify `src/mcp/interface.rs` for pending request tracking
- Modify `src/mcp/buffer.rs` for cancellation support
- Update `src/mcp/handlers/mod.rs`

## Acceptance Criteria

- [ ] Tracks pending request IDs with cancellation tokens
- [ ] `notifications/cancelled` aborts matching request
- [ ] Cancelled operations return `isError: true`
- [ ] Cleanup of pending requests on completion
- [ ] Works with both stdio and HTTP transports
- [ ] Logs cancellation with reason

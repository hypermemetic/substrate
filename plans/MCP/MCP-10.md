# MCP-10: Cancellation Support

## Metadata
- **blocked_by:** [MCP-9]
- **unlocks:** []
- **priority:** Medium

## Scope

Implement `notifications/cancelled` to abort in-progress SSE streams.

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

impl McpInterface {
    /// Track active SSE streams by request ID
    active_streams: Arc<RwLock<HashMap<String, CancellationToken>>>,

    pub async fn handle_cancelled(&self, params: Value) -> Result<Value, McpError> {
        let params: CancelledParams = serde_json::from_value(params)?;
        let request_id = params.request_id.to_string();

        if let Some(token) = self.active_streams.write().unwrap().remove(&request_id) {
            token.cancel();
            tracing::info!(request_id = %request_id, reason = ?params.reason, "Cancelled SSE stream");
        }

        Ok(Value::Null)  // Notification - no response
    }

    /// Register stream for cancellation
    fn register_stream(&self, request_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.active_streams.write().unwrap()
            .insert(request_id.to_string(), token.clone());
        token
    }
}
```

## Modified tools/call with cancellation

```rust
pub fn handle_tools_call_sse(&self, request_id: Value, params: ToolsCallParams) -> impl Stream<Item = SseEvent> {
    let cancel = self.register_stream(&request_id.to_string());
    let active_streams = self.active_streams.clone();

    async_stream::stream! {
        // ... setup ...

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    yield result_sse(request_id.clone(), "Operation cancelled", true);
                    break;
                }
                event = stream.next() => {
                    match event {
                        Some(e) => { /* process */ }
                        None => break,
                    }
                }
            }
        }

        // Cleanup
        active_streams.write().unwrap().remove(&request_id.to_string());
    }
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/cancelled.rs`
- Modify `src/mcp/interface.rs` for stream tracking
- Modify `src/mcp/handlers/tools_call.rs` for cancellation

## Acceptance Criteria

- [ ] Tracks active streams by request ID
- [ ] `notifications/cancelled` aborts matching stream
- [ ] Cancelled streams emit final error event
- [ ] Cleanup on stream completion
- [ ] Logs cancellation with reason

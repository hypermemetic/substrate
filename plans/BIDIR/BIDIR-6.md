# BIDIR-6: MCP Transport Mapping

**Status:** Planning
**Blocked By:** BIDIR-3, BIDIR-4, BIDIR-5
**Unlocks:** BIDIR-8

## Scope

Map bidirectional protocol to MCP transport, handling Request/Response flow over SSE notifications.

## Acceptance Criteria

- [ ] Server sends Request via MCP notification
- [ ] Client can respond via follow-up mechanism
- [ ] Timeout handling at transport level
- [ ] Backward compatible with non-interactive clients

## Design Considerations

MCP is fundamentally **request → streaming response**, not truly bidirectional. We have several options:

### Option A: Notification + Polling (Recommended)

Server sends Request as notification, client responds via separate `tools/call`:

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. Client calls: tools/call { name: "sync", arguments: {...} }  │
│                                                                 │
│ 2. Server streams via SSE:                                      │
│    - notifications/progress { "Processing..." }                 │
│    - notifications/logging { type: "request", id: "abc",        │
│                              request_type: "confirm", ... }     │
│                                                                 │
│ 3. Client sends: tools/call { name: "_respond",                 │
│                               arguments: { request_id: "abc",   │
│                                           payload: true } }     │
│                                                                 │
│ 4. Server continues streaming:                                  │
│    - notifications/progress { "Syncing..." }                    │
│    - Final result                                               │
└─────────────────────────────────────────────────────────────────┘
```

### Option B: Custom Notification Type

Define new MCP notification for bidirectional:

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/plexus_request",
  "params": {
    "request_id": "abc-123",
    "request_type": "confirm",
    "message": "Continue with sync?",
    "timeout_ms": 30000
  }
}
```

### Option C: Embed in Progress Notification

Use existing progress notification with extended data:

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "tool-1",
    "progress": 50,
    "message": "Waiting for confirmation...",
    "_plexus_request": {
      "request_id": "abc-123",
      "request_type": "confirm",
      "data": { "message": "Continue?" }
    }
  }
}
```

## Implementation Notes (Option A)

### MCP Bridge Changes

```rust
// substrate/src/mcp_bridge.rs

impl PlexusMcpBridge {
    // NEW: Internal tool for responding to requests
    fn response_tool_schema() -> Tool {
        Tool {
            name: "_plexus_respond".into(),
            description: Some("Respond to a Plexus interactive request".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string" },
                    "payload": {}
                },
                "required": ["request_id", "payload"]
            }),
        }
    }
}

#[async_trait]
impl ServerHandler for PlexusMcpBridge {
    async fn list_tools(&self) -> Result<Vec<Tool>, McpError> {
        let mut tools = self.generate_tools().await;
        // Add internal response tool
        tools.push(Self::response_tool_schema());
        Ok(tools)
    }

    async fn call_tool(&self, params: CallToolParams, ctx: RequestContext) -> Result<CallToolResult, McpError> {
        if params.name == "_plexus_respond" {
            return self.handle_response(params, ctx).await;
        }
        // ... existing tool handling with bidir support
    }
}
```

### Request Notification Handling

```rust
// substrate/src/mcp_bridge.rs

impl PlexusMcpBridge {
    async fn call_tool_with_bidir(
        &self,
        params: CallToolParams,
        ctx: RequestContext,
    ) -> Result<CallToolResult, McpError> {
        let method = self.resolve_method(&params.name);
        let arguments = params.arguments.unwrap_or_default();

        // Create bidirectional channel
        let (stream_tx, mut stream_rx) = mpsc::channel(32);
        let (response_tx, response_rx) = mpsc::channel(32);

        let bidir_channel = Arc::new(BidirChannel::new(
            stream_tx.clone(),
            true, // MCP supports bidir via response tool
        ));

        // Spawn response handler
        let channel_clone = bidir_channel.clone();
        let response_handler = tokio::spawn(async move {
            while let Some(response) = response_rx.recv().await {
                let _ = channel_clone.handle_response(response);
            }
        });

        // Register channel for response routing
        self.register_pending_call(&params.name, bidir_channel.clone());

        // Call plexus with context
        let stream = self.plexus.route_with_context(
            &method,
            arguments,
            Some(bidir_channel),
        ).await?;

        // Process stream
        let mut buffered_data = Vec::new();
        tokio::pin!(stream);

        while let Some(item) = stream.next().await {
            if ctx.ct.is_cancelled() {
                break;
            }

            match &item {
                PlexusStreamItem::Request { request_id, request_type, timeout_ms, .. } => {
                    // Send request notification to client
                    self.send_request_notification(
                        &ctx,
                        request_id,
                        request_type,
                        *timeout_ms,
                    ).await?;
                }
                PlexusStreamItem::Progress { message, percentage, .. } => {
                    ctx.peer.notify_progress(/* ... */).await?;
                }
                PlexusStreamItem::Data { content, .. } => {
                    buffered_data.push(content.clone());
                }
                PlexusStreamItem::Error { message, .. } => {
                    buffered_data.push(json!({ "error": message }));
                }
                PlexusStreamItem::Done { .. } => break,
            }
        }

        // Cleanup
        self.unregister_pending_call(&params.name);
        response_handler.abort();

        Ok(CallToolResult::success(/* ... */))
    }

    async fn send_request_notification(
        &self,
        ctx: &RequestContext,
        request_id: &str,
        request_type: &RequestType,
        timeout_ms: Option<u64>,
    ) -> Result<(), McpError> {
        // Use logging notification to send request
        ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
            level: LoggingLevel::Info,
            logger: Some("plexus.bidir".into()),
            data: json!({
                "type": "request",
                "request_id": request_id,
                "request_type": request_type,
                "timeout_ms": timeout_ms,
            }),
        }).await?;

        Ok(())
    }

    async fn handle_response(
        &self,
        params: CallToolParams,
        _ctx: RequestContext,
    ) -> Result<CallToolResult, McpError> {
        let args = params.arguments.unwrap_or_default();
        let request_id: String = serde_json::from_value(args["request_id"].clone())
            .map_err(|_| McpError::invalid_params("Missing request_id", None))?;
        let payload: Value = args["payload"].clone();

        // Route response to pending channel
        if let Some(channel) = self.get_pending_channel(&request_id) {
            let response = ClientResponse {
                request_id,
                payload: parse_response_payload(payload)?,
            };
            channel.handle_response(response)
                .map_err(|e| McpError::internal_error(&e.to_string(), None))?;

            Ok(CallToolResult::success(vec![Content::text("Response received")]))
        } else {
            Err(McpError::invalid_params(
                &format!("No pending request with ID: {}", request_id),
                None
            ))
        }
    }
}
```

### Pending Call Registry

```rust
// substrate/src/mcp_bridge.rs

struct PendingCalls {
    channels: HashMap<String, Arc<BidirChannel>>,
    request_to_call: HashMap<String, String>,  // request_id -> call_name
}

impl PlexusMcpBridge {
    fn register_pending_call(&self, call_name: &str, channel: Arc<BidirChannel>) {
        let mut pending = self.pending.lock();
        pending.channels.insert(call_name.to_string(), channel);
    }

    fn get_pending_channel(&self, request_id: &str) -> Option<Arc<BidirChannel>> {
        let pending = self.pending.lock();
        pending.request_to_call.get(request_id)
            .and_then(|name| pending.channels.get(name).cloned())
    }
}
```

## Client Integration (Claude Code)

For Claude Code to support bidirectional:

1. **Watch for request notifications** in logging messages
2. **Present UI** based on request_type (confirm dialog, text input, etc.)
3. **Call `_plexus_respond`** tool with user's response

```typescript
// Pseudocode for Claude Code integration
function handleLoggingNotification(params: LoggingNotificationParams) {
  const data = params.data;
  if (data.type === 'request') {
    const response = await presentInteractiveUI(data.request_type);
    await callTool('_plexus_respond', {
      request_id: data.request_id,
      payload: response
    });
  }
}
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `substrate/src/mcp_bridge.rs` | Add bidir handling, response tool |
| `hub-core/src/plexus/plexus.rs` | Add route_with_context method |

## Testing

```rust
#[tokio::test]
async fn test_mcp_bidir_request_response() {
    let bridge = PlexusMcpBridge::new(plexus);

    // Simulate call that triggers request
    let call_params = CallToolParams {
        name: "interactive_method".into(),
        arguments: Some(json!({})),
    };

    // Start call in background
    let call_task = tokio::spawn(async move {
        bridge.call_tool(call_params, ctx).await
    });

    // Wait for request notification
    let notification = rx.recv().await.unwrap();
    assert!(notification.data["type"] == "request");

    // Send response
    let response_params = CallToolParams {
        name: "_plexus_respond".into(),
        arguments: Some(json!({
            "request_id": notification.data["request_id"],
            "payload": { "Confirmed": true }
        })),
    };
    bridge.call_tool(response_params, ctx).await.unwrap();

    // Call should complete
    let result = call_task.await.unwrap().unwrap();
    assert!(!result.is_error.unwrap_or(false));
}
```

## Notes

- `_plexus_respond` tool is internal, clients discover it via list_tools
- Request timeout enforced server-side, client just responds when ready
- Multiple concurrent calls supported via request_id correlation
- Graceful degradation: if client never responds, timeout kicks in

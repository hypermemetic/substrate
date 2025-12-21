# MCP-9: Tools Call + SSE Streaming

## Metadata
- **blocked_by:** [MCP-3, MCP-7]
- **unlocks:** [MCP-10, MCP-11]
- **priority:** Critical (on critical path)

## Scope

Implement `tools/call` with native SSE streaming. No buffering - stream Plexus events directly as MCP progress notifications.

## Protocol

**Request:**
```http
POST /mcp HTTP/1.1
Content-Type: application/json
Accept: text/event-stream

{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"claudecode.chat","arguments":{...}}}
```

**Response (SSE stream):**
```http
HTTP/1.1 200 OK
Content-Type: text/event-stream
Mcp-Session-Id: abc123

event: message
id: evt-1
data: {"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"xyz","message":"Hello"}}

event: message
id: evt-2
data: {"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"xyz","message":" world"}}

event: message
id: evt-3
data: {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Hello world"}],"isError":false}}
```

## Implementation

```rust
// src/mcp/handlers/tools_call.rs

impl McpInterface {
    /// Stream tools/call as SSE - no buffering
    pub fn handle_tools_call_sse(
        &self,
        request_id: Value,
        params: ToolsCallParams,
    ) -> impl Stream<Item = SseEvent> {
        let progress_token = Uuid::new_v4().to_string();
        let plexus = self.plexus.clone();

        async_stream::stream! {
            // Parse tool name
            let (namespace, method) = match params.name.split_once('.') {
                Some(parts) => parts,
                None => {
                    yield error_sse(request_id, "Invalid tool name format");
                    return;
                }
            };

            // Get stream from Plexus
            let stream = match plexus.call(&format!("{}.{}", namespace, method), params.arguments).await {
                Ok(s) => s,
                Err(e) => {
                    yield error_sse(request_id, &e.to_string());
                    return;
                }
            };

            let mut event_id = 0;
            let mut final_content = Vec::new();

            pin_mut!(stream);
            while let Some(event) = stream.next().await {
                event_id += 1;

                match event.event_type() {
                    EventType::Content => {
                        if let Some(text) = event.text() {
                            final_content.push(text.clone());
                            yield progress_sse(&progress_token, event_id, &text);
                        }
                    }
                    EventType::ToolUse => {
                        if let Some(info) = event.tool_info() {
                            yield progress_sse(&progress_token, event_id,
                                &format!("Using tool: {}", info.name));
                        }
                    }
                    EventType::Error => {
                        yield error_sse(request_id.clone(),
                            &event.error_message().unwrap_or_default());
                        return;
                    }
                    EventType::Complete => {
                        yield result_sse(request_id.clone(), &final_content.join(""), false);
                        return;
                    }
                    _ => {}
                }
            }

            // Stream ended without Complete event
            yield result_sse(request_id, &final_content.join(""), false);
        }
    }
}

fn progress_sse(token: &str, id: usize, message: &str) -> SseEvent {
    SseEvent {
        id: format!("evt-{}", id),
        data: json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {
                "progressToken": token,
                "progress": id,
                "message": message
            }
        }),
    }
}

fn result_sse(request_id: Value, text: &str, is_error: bool) -> SseEvent {
    SseEvent {
        id: "final".into(),
        data: json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "content": [{ "type": "text", "text": text }],
                "isError": is_error
            }
        }),
    }
}
```

## HTTP Handler

```rust
// src/transport/http.rs

async fn handle_mcp_post(req: Request, mcp: Arc<McpInterface>) -> Response {
    let body: JsonRpcRequest = req.json().await?;

    match body.method.as_str() {
        "tools/call" => {
            // Always stream via SSE
            let params: ToolsCallParams = serde_json::from_value(body.params)?;
            let stream = mcp.handle_tools_call_sse(body.id, params);

            Response::builder()
                .header("Content-Type", "text/event-stream")
                .header("Cache-Control", "no-cache")
                .header("Mcp-Session-Id", Uuid::new_v4().to_string())
                .body(Body::from_stream(stream))
        }
        _ => {
            // Other methods return JSON directly
            let result = mcp.handle(&body.method, body.params).await?;
            Response::json(json!({
                "jsonrpc": "2.0",
                "id": body.id,
                "result": result
            }))
        }
    }
}
```

## Files to Create/Modify

- Create `src/mcp/handlers/tools_call.rs`
- Create `src/mcp/sse.rs` (SSE event types)
- Create `src/transport/http.rs`
- Update `src/main.rs` for HTTP server

## Acceptance Criteria

- [ ] `tools/call` streams SSE events
- [ ] Progress notifications include message field
- [ ] Final result ends the stream
- [ ] Error events close stream with `isError: true`
- [ ] Mcp-Session-Id header for tracking
- [ ] Works with MCP 2025-03-26 Streamable HTTP spec

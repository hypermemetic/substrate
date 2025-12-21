# MCP-12: Streamable HTTP Transport (SSE)

## Metadata
- **blocked_by:** [MCP-10]
- **unlocks:** [MCP-14, MCP-15]
- **priority:** Medium

## Scope

Implement MCP 2025-03-26 Streamable HTTP transport with SSE for real-time progress.

## Endpoints

```
POST /mcp          - JSON-RPC requests (may return SSE stream)
GET  /mcp          - Server-initiated messages (optional)
DELETE /mcp        - Session termination
```

## Request/Response Flow

**Simple JSON response (short operations):**
```http
POST /mcp HTTP/1.1
Content-Type: application/json

{"jsonrpc":"2.0","id":1,"method":"ping"}

---

HTTP/1.1 200 OK
Content-Type: application/json

{"jsonrpc":"2.0","id":1,"result":{}}
```

**SSE streaming response (long operations):**
```http
POST /mcp HTTP/1.1
Content-Type: application/json
Accept: application/json, text/event-stream

{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"claudecode.chat",...}}

---

HTTP/1.1 200 OK
Content-Type: text/event-stream
Mcp-Session-Id: abc123

event: message
id: evt-1
data: {"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"xyz","message":"Hello"}}

event: message
id: evt-2
data: {"jsonrpc":"2.0","id":1,"result":{"content":[...],"isError":false}}
```

## Implementation

```rust
// src/transport/http.rs

pub struct HttpMcpTransport {
    mcp: McpInterface,
}

impl HttpMcpTransport {
    async fn handle_post(&self, req: Request) -> Response {
        let accepts_sse = req.headers()
            .get("Accept")
            .map(|v| v.to_str().unwrap_or("").contains("text/event-stream"))
            .unwrap_or(false);

        let body: JsonRpcRequest = req.json().await?;

        if accepts_sse && self.is_streamable(&body.method) {
            // Return SSE stream
            self.stream_response(body).await
        } else {
            // Return buffered JSON
            self.json_response(body).await
        }
    }

    fn is_streamable(&self, method: &str) -> bool {
        method == "tools/call"
    }

    async fn stream_response(&self, request: JsonRpcRequest) -> Response {
        let session_id = Uuid::new_v4().to_string();

        // Get Plexus stream and convert to SSE
        let sse_stream = self.mcp.stream_tools_call(request).await;

        Response::builder()
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Mcp-Session-Id", &session_id)
            .body(Body::from_stream(sse_stream))
    }
}
```

## Files to Create/Modify

- Create `src/transport/http.rs`
- Create `src/mcp/streaming.rs` (SSE event generation)
- Modify `src/main.rs` for HTTP server option

## Acceptance Criteria

- [ ] POST /mcp accepts JSON-RPC requests
- [ ] Short operations return JSON
- [ ] Long operations stream SSE with progress
- [ ] Mcp-Session-Id header for session tracking
- [ ] Progress notifications include message field
- [ ] Final result ends the SSE stream
- [ ] Works with MCP 2025-03-26 clients

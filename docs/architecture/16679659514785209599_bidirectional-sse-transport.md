# Bidirectional SSE Transport for Plexus

## Goal

Expose a native Plexus HTTP+SSE endpoint (Option C) and make it accessible through MCP (Option B), enabling:
1. Direct Plexus access via lightweight SSE without MCP overhead
2. MCP clients to leverage the same streaming infrastructure
3. Backwards compatibility with existing WebSocket and stdio transports

## Architecture Overview

```
                              ┌─────────────────────────────────────────┐
                              │              Substrate                   │
                              │                                          │
 ┌──────────────┐             │  ┌────────────────────────────────────┐ │
 │  WebSocket   │────────────────▶│       jsonrpsee RpcModule         │ │
 │  Client      │  :4444      │  │         (JSON-RPC 2.0)             │ │
 └──────────────┘             │  └──────────────┬─────────────────────┘ │
                              │                 │                        │
 ┌──────────────┐             │  ┌──────────────▼─────────────────────┐ │
 │    MCP       │────────────────▶│     StreamableHttpService         │ │
 │   Client     │  :4445/mcp  │  │   (PlexusMcpBridge + rmcp)         │ │
 └──────────────┘             │  └──────────────┬─────────────────────┘ │
                              │                 │                        │
 ┌──────────────┐             │  ┌──────────────▼─────────────────────┐ │
 │  SSE Client  │────────────────▶│     PlexusSseService (NEW)        │ │
 │  (EventSource)│ :4445/sse  │  │   (Native HTTP+SSE endpoint)       │ │
 └──────────────┘             │  └──────────────┬─────────────────────┘ │
                              │                 │                        │
                              │  ┌──────────────▼─────────────────────┐ │
                              │  │           Plexus::route()          │ │
                              │  │                                     │ │
                              │  │  ┌─────────┐ ┌─────────┐ ┌───────┐ │ │
                              │  │  │ health  │ │  cone   │ │ arbor │ │ │
                              │  │  └─────────┘ └─────────┘ └───────┘ │ │
                              │  └────────────────────────────────────┘ │
                              └─────────────────────────────────────────┘
```

## Design Principles

### 1. Caller-Wraps Streaming (Existing)

All activations return typed domain streams. Plexus wraps them with `PlexusStreamItem`:

```rust
pub enum PlexusStreamItem {
    Data { metadata, content_type, content },
    Progress { metadata, message, percentage },
    Error { metadata, message, code, recoverable },
    Done { metadata },
}
```

The SSE transport consumes this directly - no additional transformation needed.

### 2. Content Negotiation

Single endpoint supports multiple response modes via `Accept` header:

| Accept Header | Response Mode | Use Case |
|---------------|---------------|----------|
| `text/event-stream` | SSE stream | Real-time streaming clients |
| `application/json` | Buffered JSON | Simple HTTP clients |
| `application/x-ndjson` | Newline-delimited JSON | Log processors |

### 3. MCP Passthrough

MCP clients can call Plexus methods directly. The SSE endpoint is exposed as an MCP resource:

```
plexus://sse/stream
```

---

## Implementation Plan

### Phase 1: Native SSE Endpoint (`/sse`)

#### 1.1 New Module: `src/sse_transport.rs`

```rust
//! Native HTTP+SSE transport for Plexus
//!
//! Provides direct Plexus access without MCP protocol overhead.
//! Supports bidirectional communication via POST (client→server) + SSE (server→client).

use axum::{
    extract::{State, Json},
    response::{sse::{Event, Sse}, IntoResponse, Response},
    http::{HeaderMap, StatusCode},
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::convert::Infallible;

use crate::plexus::{Plexus, PlexusError};
use crate::plexus::types::PlexusStreamItem;

// ============================================================================
// Request/Response Types
// ============================================================================

/// JSON-RPC 2.0 request for Plexus methods
#[derive(Debug, Deserialize)]
pub struct PlexusRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response wrapper
#[derive(Debug, Serialize)]
pub struct PlexusResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ============================================================================
// SSE Event Formatting
// ============================================================================

/// Convert PlexusStreamItem to SSE Event
fn stream_item_to_event(item: PlexusStreamItem) -> Event {
    let event_type = match &item {
        PlexusStreamItem::Data { .. } => "data",
        PlexusStreamItem::Progress { .. } => "progress",
        PlexusStreamItem::Error { .. } => "error",
        PlexusStreamItem::Done { .. } => "done",
    };

    Event::default()
        .event(event_type)
        .data(serde_json::to_string(&item).unwrap_or_default())
}

// ============================================================================
// Content Negotiation
// ============================================================================

/// Determine response mode from Accept header
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResponseMode {
    Sse,           // text/event-stream
    Json,          // application/json (buffered)
    NdJson,        // application/x-ndjson (streaming JSON lines)
}

impl ResponseMode {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let accept = headers
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if accept.contains("text/event-stream") {
            Self::Sse
        } else if accept.contains("application/x-ndjson") {
            Self::NdJson
        } else {
            Self::Json
        }
    }
}

// ============================================================================
// Handler Implementation
// ============================================================================

/// Main SSE endpoint handler with content negotiation
pub async fn sse_handler(
    State(plexus): State<Arc<Plexus>>,
    headers: HeaderMap,
    Json(request): Json<PlexusRequest>,
) -> Response {
    let mode = ResponseMode::from_headers(&headers);
    let request_id = request.id.clone();

    // Route to Plexus
    let stream_result = plexus.route(&request.method, request.params).await;

    match stream_result {
        Ok(stream) => match mode {
            ResponseMode::Sse => sse_response(stream, request_id).into_response(),
            ResponseMode::NdJson => ndjson_response(stream, request_id).into_response(),
            ResponseMode::Json => json_response(stream, request_id).await.into_response(),
        },
        Err(e) => error_response(e, request_id, mode).into_response(),
    }
}

/// SSE streaming response
fn sse_response(
    stream: crate::plexus::PlexusStream,
    _request_id: Option<serde_json::Value>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let event_stream = stream.map(|item| {
        Ok(stream_item_to_event(item))
    });

    Sse::new(event_stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping")
        )
}

/// Newline-delimited JSON streaming response
fn ndjson_response(
    stream: crate::plexus::PlexusStream,
    _request_id: Option<serde_json::Value>,
) -> impl IntoResponse {
    let body_stream = stream.map(|item| {
        let mut json = serde_json::to_string(&item).unwrap_or_default();
        json.push('\n');
        Ok::<_, Infallible>(json)
    });

    let body = axum::body::Body::from_stream(body_stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/x-ndjson")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(body)
        .unwrap()
}

/// Buffered JSON response (collects stream then returns)
async fn json_response(
    stream: crate::plexus::PlexusStream,
    request_id: Option<serde_json::Value>,
) -> impl IntoResponse {
    use futures::StreamExt;

    let mut data_items = Vec::new();
    let mut errors = Vec::new();

    tokio::pin!(stream);
    while let Some(item) = stream.next().await {
        match item {
            PlexusStreamItem::Data { content, .. } => {
                data_items.push(content);
            }
            PlexusStreamItem::Error { message, .. } => {
                errors.push(message);
            }
            PlexusStreamItem::Done { .. } => break,
            _ => {}
        }
    }

    let response = if errors.is_empty() {
        PlexusResponse {
            jsonrpc: "2.0".into(),
            id: request_id,
            result: Some(if data_items.len() == 1 {
                data_items.into_iter().next().unwrap()
            } else {
                serde_json::Value::Array(data_items)
            }),
            error: None,
        }
    } else {
        PlexusResponse {
            jsonrpc: "2.0".into(),
            id: request_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: errors.join("; "),
                data: None,
            }),
        }
    };

    Json(response)
}

/// Error response for routing failures
fn error_response(
    error: PlexusError,
    request_id: Option<serde_json::Value>,
    mode: ResponseMode,
) -> Response {
    let (code, message) = match &error {
        PlexusError::ActivationNotFound(name) => (-32601, format!("Method not found: {}", name)),
        PlexusError::MethodNotFound { activation, method } =>
            (-32601, format!("Method not found: {}.{}", activation, method)),
        PlexusError::InvalidParams(reason) => (-32602, reason.clone()),
        PlexusError::ExecutionError(msg) => (-32000, msg.clone()),
        PlexusError::HandleNotSupported(name) => (-32000, format!("Handle not supported: {}", name)),
    };

    match mode {
        ResponseMode::Sse => {
            let error_item = PlexusStreamItem::Error {
                metadata: crate::plexus::types::StreamMetadata {
                    provenance: vec!["plexus".into()],
                    plexus_hash: String::new(),
                    timestamp: chrono::Utc::now().timestamp(),
                },
                message: message.clone(),
                code: Some(code.to_string()),
                recoverable: false,
            };

            let stream = futures::stream::once(async move {
                Ok::<_, Infallible>(stream_item_to_event(error_item))
            });

            Sse::new(stream).into_response()
        }
        _ => {
            Json(PlexusResponse {
                jsonrpc: "2.0".into(),
                id: request_id,
                result: None,
                error: Some(JsonRpcError { code, message, data: None }),
            }).into_response()
        }
    }
}

// ============================================================================
// Router Builder
// ============================================================================

/// Build the SSE router
pub fn build_sse_router(plexus: Arc<Plexus>) -> axum::Router {
    axum::Router::new()
        .route("/", axum::routing::post(sse_handler))
        .with_state(plexus)
}
```

#### 1.2 Dependencies to Add

```toml
# Cargo.toml additions
axum-extra = { version = "0.9", features = ["typed-header"] }
```

#### 1.3 Main.rs Integration

```rust
// In main.rs, after building mcp_service:

// Build SSE service with same Plexus instance
let sse_plexus = build_plexus().await;
let sse_router = substrate::sse_transport::build_sse_router(sse_plexus);

// Combined router: MCP at /mcp, SSE at /sse
let combined_app = axum::Router::new()
    .nest("/sse", sse_router)
    .nest_service("/mcp", mcp_service);

// Single HTTP server on port 4445
let http_listener = tokio::net::TcpListener::bind(mcp_addr).await?;
let http_handle = tokio::spawn(async move {
    axum::serve(http_listener, combined_app).await
});

tracing::info!("  SSE:      http://{}/sse", mcp_addr);
tracing::info!("  MCP HTTP: http://{}/mcp", mcp_addr);
```

---

### Phase 2: MCP Bridge Enhancement

#### 2.1 Expose SSE Endpoint as MCP Resource

Add to `mcp_bridge.rs`:

```rust
impl ServerHandler for PlexusMcpBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_logging()
                .enable_resources()  // NEW: Enable resources capability
                .build(),
            // ...
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                Resource {
                    uri: "plexus://transport/sse".into(),
                    name: "Plexus SSE Transport".into(),
                    description: Some("Direct streaming endpoint for Plexus methods".into()),
                    mime_type: Some("text/event-stream".into()),
                    annotations: None,
                    meta: None,
                },
                Resource {
                    uri: "plexus://schema".into(),
                    name: "Plexus Schema".into(),
                    description: Some("Full schema of all registered activations".into()),
                    mime_type: Some("application/json".into()),
                    annotations: None,
                    meta: None,
                },
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        match request.uri.as_str() {
            "plexus://transport/sse" => {
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::Text {
                        uri: request.uri,
                        mime_type: Some("text/plain".into()),
                        text: format!(
                            "SSE Endpoint: POST http://localhost:4445/sse\n\n\
                             Headers:\n\
                               Accept: text/event-stream (streaming) | application/json (buffered)\n\
                               Content-Type: application/json\n\n\
                             Body (JSON-RPC 2.0):\n\
                             {{\n\
                               \"jsonrpc\": \"2.0\",\n\
                               \"id\": 1,\n\
                               \"method\": \"health.check\",\n\
                               \"params\": {{}}\n\
                             }}\n\n\
                             Event Types: data, progress, error, done"
                        ),
                        meta: None,
                    }],
                    meta: None,
                })
            }
            "plexus://schema" => {
                let schemas = self.plexus.list_plugin_schemas();
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::Text {
                        uri: request.uri,
                        mime_type: Some("application/json".into()),
                        text: serde_json::to_string_pretty(&schemas).unwrap_or_default(),
                        meta: None,
                    }],
                    meta: None,
                })
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown resource: {}", request.uri),
                None,
            )),
        }
    }
}
```

#### 2.2 SSE Proxy Tool (Optional)

Add a tool that proxies requests through the SSE endpoint:

```rust
// In list_tools(), add:
Tool::new(
    "plexus.stream".into(),
    "Call any Plexus method with SSE streaming (returns endpoint info for client-side streaming)".into(),
    Arc::new(serde_json::json!({
        "type": "object",
        "properties": {
            "method": { "type": "string", "description": "Plexus method (e.g., bash.execute)" },
            "params": { "type": "object", "description": "Method parameters" }
        },
        "required": ["method"]
    }).as_object().unwrap().clone())
)

// In call_tool(), handle:
"plexus.stream" => {
    let method = arguments.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = arguments.get("params").cloned().unwrap_or(json!({}));

    // Return streaming instructions for client
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Stream endpoint: POST http://localhost:4445/sse\n\
         Method: {}\n\
         Params: {}\n\n\
         curl -X POST http://localhost:4445/sse \\\n\
           -H 'Accept: text/event-stream' \\\n\
           -H 'Content-Type: application/json' \\\n\
           -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"{}\",\"params\":{}}}'",
        method,
        serde_json::to_string_pretty(&params).unwrap_or_default(),
        method,
        serde_json::to_string(&params).unwrap_or_default()
    ))]))
}
```

---

### Phase 3: Session Management (Optional)

For stateful streaming (e.g., subscriptions), add session tracking:

```rust
// src/sse_transport.rs additions

use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Session state for SSE connections
pub struct SseSession {
    pub id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_event_id: Option<String>,
}

/// Session manager for SSE connections
pub struct SseSessionManager {
    sessions: RwLock<HashMap<Uuid, SseSession>>,
}

impl SseSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_session(&self) -> Uuid {
        let session = SseSession {
            id: Uuid::new_v4(),
            created_at: chrono::Utc::now(),
            last_event_id: None,
        };
        let id = session.id;
        self.sessions.write().await.insert(id, session);
        id
    }

    pub async fn update_last_event(&self, session_id: Uuid, event_id: String) {
        if let Some(session) = self.sessions.write().await.get_mut(&session_id) {
            session.last_event_id = Some(event_id);
        }
    }
}
```

---

## SSE Event Format

### Wire Format

```
event: data
id: evt_abc123
data: {"type":"data","metadata":{"provenance":["plexus","health"],"plexus_hash":"abc...","timestamp":1234567890},"content_type":"health.status","content":{"status":"healthy"}}

event: progress
id: evt_abc124
data: {"type":"progress","metadata":{...},"message":"Processing...","percentage":50.0}

event: error
id: evt_abc125
data: {"type":"error","metadata":{...},"message":"Failed to connect","code":"E_CONN","recoverable":true}

event: done
id: evt_abc126
data: {"type":"done","metadata":{...}}
```

### Event Types

| Event | When Sent | Client Action |
|-------|-----------|---------------|
| `data` | Each result chunk | Append to output |
| `progress` | Long operations | Update progress UI |
| `error` | Failures | Show error (may continue if recoverable) |
| `done` | Stream complete | Close connection |

### Keep-Alive

SSE connections send `:ping` comments every 15 seconds to prevent proxy timeouts:

```
:ping

event: data
data: {...}
```

---

## Client Usage

### JavaScript (EventSource)

```javascript
// Simple GET-based streaming (requires GET endpoint)
const eventSource = new EventSource('http://localhost:4445/sse/health.check');

eventSource.addEventListener('data', (e) => {
  const item = JSON.parse(e.data);
  console.log('Data:', item.content);
});

eventSource.addEventListener('done', () => {
  eventSource.close();
});
```

### JavaScript (Fetch + ReadableStream)

```javascript
// POST-based streaming with full control
async function streamPlexus(method, params) {
  const response = await fetch('http://localhost:4445/sse', {
    method: 'POST',
    headers: {
      'Accept': 'text/event-stream',
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method,
      params,
    }),
  });

  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += value;
    const lines = buffer.split('\n\n');
    buffer = lines.pop(); // Keep incomplete event

    for (const event of lines) {
      const dataMatch = event.match(/^data: (.+)$/m);
      if (dataMatch) {
        const item = JSON.parse(dataMatch[1]);
        console.log(item.type, item);
        if (item.type === 'done') return;
      }
    }
  }
}

// Usage
await streamPlexus('bash.execute', { command: 'ls -la' });
```

### curl

```bash
# Streaming
curl -X POST http://localhost:4445/sse \
  -H 'Accept: text/event-stream' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"bash.execute","params":{"command":"ls -la"}}'

# Buffered JSON
curl -X POST http://localhost:4445/sse \
  -H 'Accept: application/json' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"health.check","params":{}}'
```

---

## Feature Flag

Add to `Cargo.toml`:

```toml
[features]
default = ["sse-transport"]
sse-transport = ["axum-extra"]
```

Conditionally compile in `lib.rs`:

```rust
#[cfg(feature = "sse-transport")]
pub mod sse_transport;
```

---

## Migration Path

### Current State
- WebSocket: `:4444` (JSON-RPC via jsonrpsee)
- MCP HTTP: `:4445/mcp` (Streamable HTTP via rmcp)
- Stdio: MCP-compatible line-delimited JSON-RPC

### After Implementation
- WebSocket: `:4444` (unchanged)
- MCP HTTP: `:4445/mcp` (unchanged, with resources enabled)
- SSE: `:4445/sse` (new native endpoint)
- Stdio: unchanged

### Breaking Changes
None. All existing transports remain compatible.

---

## Testing

### Unit Tests

```rust
#[tokio::test]
async fn test_sse_content_negotiation() {
    let plexus = build_plexus().await;
    let app = build_sse_router(plexus);

    // Test SSE response
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("Accept", "text/event-stream")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"health.check","params":{}}"#))
                .unwrap()
        )
        .await
        .unwrap();

    assert_eq!(response.headers().get("content-type").unwrap(), "text/event-stream");
}
```

### Integration Tests

```bash
# Test SSE streaming
curl -N -X POST http://localhost:4445/sse \
  -H 'Accept: text/event-stream' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"bash.execute","params":{"command":"for i in 1 2 3; do echo $i; sleep 1; done"}}'

# Expected output:
# event: data
# data: {"type":"data",...,"content":"1\n"}
#
# event: data
# data: {"type":"data",...,"content":"2\n"}
#
# event: data
# data: {"type":"data",...,"content":"3\n"}
#
# event: done
# data: {"type":"done",...}
```

---

## File Summary

| File | Purpose | Lines (est.) |
|------|---------|--------------|
| `src/sse_transport.rs` | SSE handler, content negotiation | ~250 |
| `src/mcp_bridge.rs` | Add resources capability | +80 |
| `src/main.rs` | Router integration | +15 |
| `src/lib.rs` | Module export | +3 |
| `Cargo.toml` | Feature flag | +2 |

**Total: ~350 new lines**

---

## References

- [MCP Streamable HTTP Transport](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports)
- [Why MCP Deprecated SSE](https://blog.fka.dev/blog/2025-06-06-why-mcp-deprecated-sse-and-go-with-streamable-http/)
- [Centrifugo Bidirectional SSE](https://centrifugal.dev/docs/transports/sse)
- [bidi-sse Library](https://github.com/WebReflection/bidi-sse)
- [MDN Server-Sent Events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events)

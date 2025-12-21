# RMCP-Plexus Integration: Unknowns and Design Considerations

## Executive Summary

This document analyzes the feasibility and unknowns of using the official MCP Rust SDK (`rmcp`) as the MCP layer in front of Plexus, replacing our hand-rolled `src/mcp/` implementation. Plexus is the activation system (bash, arbor, cone, claudecode) and cannot be altered; we need a bridge layer.

## Current State

### What We Have
- **Plexus**: Activation system with streaming responses (`PlexusStream`)
- **Hand-rolled MCP layer** (`src/mcp/`): Incomplete implementation
  - `McpInterface`: routes methods, handles state machine
  - `McpState`: Uninitialized → Initializing → Ready
  - MCP-4 through MCP-8 implemented (initialize, initialized, ping, tools/list)
  - MCP-9 (`tools/call`) stub only - not streaming
- **HTTP Transport**: Custom axum router at `/mcp`
- **Streaming Model**: Plexus returns `PlexusStream = Pin<Box<dyn Stream<Item = PlexusStreamItem>>>`

### What RMCP Provides
- Complete MCP protocol types (`model/`)
- `ServerHandler` trait with `call_tool`, `list_tools`, etc.
- `#[tool]` and `#[tool_router]` macros for defining tools
- `StreamableHttpService`: tower-compatible HTTP transport with SSE
- Session management, authentication, progress notifications
- Full 2025-03-26 spec compliance with Streamable HTTP

## Critical Unknowns

### 1. Streaming Impedance Mismatch ✅ RESOLVED

**The core challenge**: RMCP tools return `Result<CallToolResult, McpError>`, but Plexus returns `PlexusStream`.

```rust
// RMCP expectation (single result)
async fn call_tool(&self, params: CallToolRequestParam) -> Result<CallToolResult, McpError>

// Plexus reality (streaming)
async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>
```

#### Resolution: Dual-Channel Streaming via Notifications

RMCP provides **two notification mechanisms** callable during tool execution:

1. **`notifications/progress`** - For progress updates with numeric progress
   ```rust
   // From rmcp/src/model.rs:856-869
   pub struct ProgressNotificationParam {
       pub progress_token: ProgressToken,
       pub progress: f64,
       pub total: Option<f64>,
       pub message: Option<String>,  // String only
   }
   ```

2. **`notifications/message`** (logging) - For structured data
   ```rust
   // From rmcp/src/model.rs:1055-1066
   pub struct LoggingMessageNotificationParam {
       pub level: LoggingLevel,
       pub logger: Option<String>,
       pub data: Value,  // Full JSON value - can hold PlexusStreamEvent!
   }
   ```

#### Key Finding: RequestContext.peer Available During call_tool

From `rmcp/src/service/server.rs:386-416`, the `Peer<RoleServer>` provides:

```rust
impl Peer<RoleServer> {
    // Progress notifications (line 410)
    pub async fn notify_progress(&self, params: ProgressNotificationParam) -> Result<(), ServiceError>;

    // Logging notifications - STRUCTURED DATA (line 411)
    pub async fn notify_logging_message(&self, params: LoggingMessageNotificationParam) -> Result<(), ServiceError>;
}
```

The `RequestContext` passed to `call_tool` contains:
- `ctx.peer: Peer<RoleServer>` - for sending notifications during execution
- `ctx.meta: Meta` - contains `progressToken` from client request
- `ctx.ct: CancellationToken` - for cancellation propagation

#### Implementation Pattern (Truly Unbuffered)

```rust
impl ServerHandler for PlexusMcpBridge {
    async fn call_tool(
        &self,
        request: CallToolRequest,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // 1. Extract progress token from client request
        let progress_token = ctx.meta.get_progress_token();

        // 2. Start Plexus stream
        let stream = self.plexus.call(&request.params.name, request.params.arguments).await?;

        // 3. Stream each event via notifications - NO BUFFERING
        let mut step = 0u64;
        let mut had_error = false;

        tokio::pin!(stream);
        while let Some(item) = stream.next().await {
            // Check cancellation on each iteration
            if ctx.ct.is_cancelled() {
                return Err(McpError::internal_error("Cancelled", None));
            }

            match &item.event {
                PlexusStreamEvent::Progress { message, percentage, .. } => {
                    if let Some(token) = &progress_token {
                        ctx.peer.notify_progress(ProgressNotificationParam {
                            progress_token: token.clone(),
                            progress: percentage.map(|p| p as f64).unwrap_or(step as f64),
                            total: None,
                            message: Some(message.clone()),
                        }).await.ok();
                    }
                }

                PlexusStreamEvent::Data { data, content_type, provenance } => {
                    // Stream data immediately - client receives via notification
                    ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                        level: LoggingLevel::Info,
                        logger: Some("plexus.stream".into()),
                        data: serde_json::json!({
                            "type": "data",
                            "content_type": content_type,
                            "data": data,
                            "provenance": provenance,
                            "plexus_hash": &item.plexus_hash,
                        }),
                    }).await.ok();
                    // NO push to buffer - data already delivered
                }

                PlexusStreamEvent::Error { error, recoverable, .. } => {
                    ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                        level: LoggingLevel::Error,
                        logger: Some("plexus.stream".into()),
                        data: serde_json::json!({
                            "type": "error",
                            "error": error,
                            "recoverable": recoverable,
                        }),
                    }).await.ok();

                    if !recoverable {
                        had_error = true;
                        break;
                    }
                }

                PlexusStreamEvent::Done { .. } => break,

                PlexusStreamEvent::Guidance { .. } => {
                    ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                        level: LoggingLevel::Warning,
                        logger: Some("plexus.guidance".into()),
                        data: serde_json::to_value(&item.event).unwrap_or_default(),
                    }).await.ok();
                }
            }
            step += 1;
        }

        // 4. Return minimal completion marker - all data already streamed via notifications
        if had_error {
            Ok(CallToolResult::error(vec![Content::text(
                "Stream completed with errors - see notifications for details"
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                format!("Stream completed: {} events delivered via notifications", step)
            )]))
        }
    }
}
```

#### Key Insight: CallToolResult as Completion Marker

The `CallToolResult` is **not** the data carrier - it's just the JSON-RPC response that signals "request complete". All real data flows through notifications:

```
Client                              Server
  |                                    |
  |--- tools/call (with progressToken) -->
  |                                    |
  |<-- notifications/progress (step 0) |
  |<-- notifications/message (data 0)  |
  |<-- notifications/progress (step 1) |
  |<-- notifications/message (data 1)  |
  |<-- notifications/message (data 2)  |
  |    ...                             |
  |<-- CallToolResult (completion)     |
  |                                    |
```

The final `CallToolResult` contains only:
- Success/error status
- Event count for debugging
- No buffered data

#### Why This Works

1. **SSE Delivery**: RMCP's `StreamableHttpService` sends notifications as SSE events
2. **Non-blocking**: `notify_*` methods are async but we `.ok()` to ignore send errors
3. **Structured Data**: `LoggingMessageNotification.data` is `Value`, not just `String`
4. **Token Correlation**: Client sends `progressToken` in `_meta`, we echo it back
5. **Cancellation**: `ctx.ct` allows cooperative cancellation mid-stream

#### Client Request Format

Client must include progress token to receive progress notifications:
```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
        "name": "bash.execute",
        "arguments": { "command": "ls -la" }
    },
    "_meta": { "progressToken": "unique-token-123" }
}
```

#### Remaining Questions

- **Claude Code Support**: Verify Claude Code handles `notifications/message` for structured streaming
- **Backpressure**: If client can't consume fast enough, notifications may queue in channel
- **Error Recovery**: Current pattern ignores notification send failures - acceptable for streaming

### 2. Tool Registration Model

**UNKNOWN 2.1**: How to register Plexus activations as RMCP tools without using `#[tool]` macro?

RMCP's `#[tool_router]` macro generates tools statically at compile time:
```rust
#[tool_router]
impl Counter {
    #[tool(description = "Increment counter")]
    async fn increment(&self) -> Result<CallToolResult, McpError> { ... }
}
```

But Plexus activations are:
- Registered dynamically at runtime
- Define their own schemas via `MethodEnumSchema` trait
- Have streaming response semantics

**Options**:
1. **Manual `ToolRouter` construction**: Build routes programmatically
   ```rust
   let router = ToolRouter::new();
   for activation in plexus.list_activations() {
       for method in activation.methods() {
           router.add_route(format!("{}.{}", namespace, method), handler);
       }
   }
   ```
   - Need to verify: Is `ToolRouter` constructible without macros?

# User Choice: I believe we should simply implement this piece manually. macro route removes the control we need. this is a thin routing layer so we can afford to avoid abstractions when they would increase the abstraction burden. we don't want to force ourselves through abstractions when the execution path becomes more complex 
2. **Implement `ServerHandler` directly**: Skip the router, handle in `call_tool`
   ```rust
   impl ServerHandler for PlexusMcpBridge {
       async fn call_tool(&self, params: CallToolRequestParam, ctx: RequestContext)
           -> Result<CallToolResult, McpError>
       {
           let stream = self.plexus.call(&params.name, params.arguments).await?;
           // Buffer or stream via progress notifications
       }

       async fn list_tools(&self, ...) -> Result<ListToolsResult, McpError> {
           let schemas = self.plexus.list_full_schemas();
           // Transform to RMCP Tool format
       }
   }
   ```
   - More flexible, less magic
   - Manually implement schema transformation

**Resolution path**: Check if `ToolRouter::add_route` or similar API exists for dynamic registration.

### 3. Schema Transformation

**UNKNOWN 3.1**: Is our schema format compatible with RMCP's expectations?

Plexus generates schemas via `schemars`:
```rust
ActivationFullSchema {
    namespace: "bash",
    methods: [MethodSchemaInfo {
        name: "execute",
        params: Some(schemars::Schema { ... }),
        returns: Some(schemars::Schema { ... }),
    }]
}
```

RMCP expects:
```rust
Tool {
    name: "bash.execute".into(),
    description: Some("...".into()),
    input_schema: ToolInputSchema { ... }, // Arc<serde_json::Value>
}
```

**Key questions**:
- Does `schemars::Schema` serialize to the same JSON as RMCP expects?
- What about `required` fields handling?
- How does RMCP handle nested `$defs`?

**Resolution path**: Compare JSON output from both systems.

### 4. Transport Integration

**UNKNOWN 4.1**: How to integrate RMCP's `StreamableHttpService` with our existing axum setup?

Current setup:
```rust
// main.rs
let mcp_app = mcp_router(mcp_interface);  // Our custom router
axum::serve(listener, mcp_app).await
```

RMCP provides:
```rust
let service = StreamableHttpService::new(
    || Ok(MyHandler::new()),
    LocalSessionManager::default().into(),
    StreamableHttpServerConfig::default(),
);
let router = axum::Router::new().nest_service("/mcp", service);
```

**Key questions**:
- Can we run both WebSocket (jsonrpsee) and RMCP HTTP on same server?
- Does RMCP's session management conflict with our needs?
- How does stateful vs stateless mode affect Claude Code integration?

**Resolution path**: Test RMCP's tower service with our axum infrastructure.

### 5. State Machine Ownership

**UNKNOWN 5.1**: RMCP handles initialize/initialized internally - do we lose control?

Our current implementation:
```rust
// McpInterface owns state machine
pub struct McpInterface {
    state: McpStateMachine,  // We control transitions
}
```

RMCP's `Service` trait:
```rust
impl<H: ServerHandler> Service<RoleServer> for H {
    async fn handle_request(&self, request: ClientRequest, context: RequestContext) {
        match request {
            ClientRequest::InitializeRequest(r) => self.initialize(r.params, context).await,
            // ... RMCP handles state internally
        }
    }
}
```

**Key questions**:
- Does RMCP enforce state machine or just call our handlers?
- Can we inject state validation in our `ServerHandler` impl?
- What about custom error handling for state violations?

**Resolution path**: Trace RMCP's initialization flow in service.rs.

### 6. Cancellation Model

**UNKNOWN 6.1**: How does RMCP's cancellation interact with Plexus streams?

RMCP provides:
```rust
RequestContext {
    ct: CancellationToken,  // Cancelled when notifications/cancelled received
    ...
}
```

Plexus streams need explicit cancellation:
```rust
// We need to propagate ct.cancelled() to stream termination
```

**Key questions**:
- Can we wrap `PlexusStream` to respect `ct`?
- What cleanup is needed when cancelled mid-stream?
- Does RMCP send proper cancellation notifications on HTTP disconnect?

### 7. Type System Compatibility

**UNKNOWN 7.1**: schemars 0.8 vs 1.x compatibility

Our `Cargo.toml`:
```toml
schemars = { version = "1.1", features = ["derive", "uuid1"] }
```

RMCP's `Cargo.toml`:
```toml
schemars = { version = "1.0", optional = true, features = ["chrono04"] }
```

Both use 1.x, should be compatible. But verify enum representation matches.

## Architecture Options

### Option A: RMCP as Transport Only

Keep our `McpInterface` but use RMCP's `StreamableHttpService` for HTTP transport.

```
┌─────────────────────────────────────────────────────────────┐
│                    RMCP Transport Layer                      │
│  StreamableHttpService → SSE streaming                       │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    Our McpInterface                          │
│  handle() → route methods → state machine                    │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                        Plexus                                │
│  activations (bash, arbor, cone, claudecode)                 │
└─────────────────────────────────────────────────────────────┘
```

**Pros**: Minimal RMCP dependency, keep our logic
**Cons**: Still need to map our types to RMCP's transport types

### Option B: RMCP as Full Handler

Implement `ServerHandler` directly, delegate to Plexus.

```
┌─────────────────────────────────────────────────────────────┐
│                    RMCP Full Stack                           │
│  StreamableHttpService + ServerHandler                       │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                  PlexusMcpBridge                             │
│  impl ServerHandler for PlexusMcpBridge {                    │
│      call_tool() → plexus.call() → buffer → CallToolResult   │
│      list_tools() → plexus.list_full_schemas() → transform   │
│  }                                                           │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                        Plexus                                │
│  activations (unmodified)                                    │
└─────────────────────────────────────────────────────────────┘
```

**Pros**: Full RMCP compliance, proven implementation
**Cons**: Streaming challenge, need to buffer or use progress notifications

### Option C: Hybrid - RMCP Types, Custom Transport

Use RMCP's types (`model::*`) but keep our transport layer.

```rust
// Use RMCP types
use rmcp::model::{
    CallToolRequestParam, CallToolResult, Tool, ToolInputSchema,
    ServerInfo, ServerCapabilities,
};

// Our custom transport that streams
async fn handle_tools_call_sse(
    &self,
    request_id: Value,
    params: CallToolRequestParam,
) -> impl Stream<Item = SseEvent> {
    // Stream Plexus events as SSE
}
```

**Pros**: Standard types, custom streaming
**Cons**: Partial benefit, still custom code

## Recommended Investigation Steps

1. **Test RMCP progress notifications during tool execution**
   ```rust
   async fn call_tool(&self, params: CallToolRequestParam, ctx: RequestContext) {
       ctx.peer.notify_progress(ProgressNotificationParam { ... }).await;
   }
   ```

2. **Build minimal PlexusMcpBridge prototype**
   - Implement `ServerHandler` with simple buffering
   - Test with Claude Code as client
   - Measure latency impact

3. **Compare schema JSON output**
   ```bash
   # Our schema
   cargo run --bin substrate -- --stdio <<< '{"method":"plexus_activation_schema","params":["bash"]}'

   # RMCP tool schema
   # Compare Tool::input_schema format
   ```

4. **Test RMCP session management**
   - Stateful mode with session persistence
   - Stateless mode (one request per connection)
   - Impact on Claude Code reconnection

## Open Questions Requiring User Input

1. **Streaming priority**: Is real-time progress essential, or is buffering acceptable?
   - If buffering OK: Option B is straightforward
   - If streaming needed: Must resolve progress notification approach

2. **Protocol version**: Target 2024-11-05 only, or also 2025-03-26?
   - RMCP supports both, but streaming differs

3. **Session management**: Do we need stateful sessions?
   - Claude Code typically uses stateless (one-shot)
   - But session IDs enable request correlation

4. **Error handling**: Custom error types or pure RMCP errors?
   - Our `PlexusError` has rich guidance information
   - Need mapping strategy to `McpError`

## Next Steps

### ✅ Completed

1. **Added `rmcp` to dependencies** (dev-dependencies for now):
   ```toml
   rmcp = { version = "0.12", features = ["server", "transport-io", "transport-streamable-http-server"] }
   ```

2. **Created working stub server**: `examples/rmcp_mcp_server.rs`
   - Implements `ServerHandler` directly (no macros)
   - Demonstrates unbuffered streaming via `notify_logging_message()`
   - Uses `notify_progress()` for progress updates
   - Returns minimal `CallToolResult` as completion marker
   - Tested: initialize, tools/list, tools/call all working

### Remaining

3. **Integrate Plexus into `examples/rmcp_mcp_server.rs`**:
   - Replace `StubMcpHandler` with `PlexusMcpBridge` wrapping `Arc<Plexus>`
   - Transform `Plexus.list_full_schemas()` → `Vec<Tool>`
   - Stream `PlexusStreamEvent` → `notify_logging_message()`
   - Map `PlexusError` → `McpError`

4. **Move to production**: Once validated, move from example to `src/mcp/rmcp_bridge.rs`

5. **Replace main.rs transport**: Swap `mcp_router` with RMCP's `StreamableHttpService`

6. **Remove hand-rolled `src/mcp/`** once validated

## References

- [RMCP README](https://github.com/modelcontextprotocol/rust-sdk/tree/main/crates/rmcp)
- [MCP Specification 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26)
- [Current MCP Epic](../plans/MCP/MCP-1.md)
- [MCP Compatibility Spec](16680473255155665663_mcp-compatibility.md)

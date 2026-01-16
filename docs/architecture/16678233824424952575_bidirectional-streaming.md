# Bidirectional Streaming Architecture

**BIDIR-10: Architecture Documentation**
**Date:** 2026-01-15

## Overview

Bidirectional streaming extends Plexus's unidirectional streaming architecture to enable **server-to-client requests** during method execution. This allows activations to interactively prompt users for confirmation, input, or selection without blocking the entire call.

### Why Bidirectional?

Traditional RPC is request-response: client sends, server replies. Streaming extends this to allow multiple server responses per request. Bidirectional goes further:

```
Traditional RPC:        Streaming:              Bidirectional:

 Client  Server         Client  Server          Client  Server
   │        │             │        │              │        │
   │──req──▶│             │──req──▶│              │──req──▶│
   │◀──res──│             │◀─data──│              │◀─data──│
   │        │             │◀─data──│              │◀──ASK──│ ← Server asks client
   │        │             │◀─done──│              │──ANS──▶│ ← Client responds
   │        │             │        │              │◀─data──│
   │        │             │        │              │◀─done──│
```

### Use Cases

1. **Confirmations**: "Delete 47 repositories? [y/N]"
2. **Input prompts**: "Enter GitHub token:"
3. **Multi-select**: "Which branches to sync? [x] main [ ] dev [ ] staging"
4. **Wizard flows**: Multi-step processes with checkpoints
5. **Progressive refinement**: "Found 100 matches. Filter by [date/author/status]?"

---

## Design Goals

| Goal | Description |
|------|-------------|
| **Backward Compatible** | All existing unidirectional methods work unchanged |
| **Opt-in** | Bidirectional is a method-level attribute, not required |
| **Transport Agnostic** | Works over MCP, WebSocket, and future transports |
| **Type Safe** | Compile-time guarantees for request/response matching |
| **Graceful Degradation** | Methods can handle non-interactive transports |

---

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ACTIVATION LAYER                                   │
│                                                                              │
│  #[hub_method(bidirectional)]                                               │
│  pub async fn sync(&self, ctx: &BidirChannel, dry_run: bool)                │
│      -> impl Stream<Item = SyncEvent> {                                      │
│                                                                              │
│      // Can now interact with client mid-stream:                            │
│      if ctx.confirm("Proceed with sync?").await? {                          │
│          // User confirmed, continue                                        │
│      }                                                                       │
│  }                                                                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                            CORE LAYER                                        │
│                                                                              │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐           │
│  │   RequestType    │  │ ResponsePayload  │  │  BidirChannel    │           │
│  │                  │  │                  │  │                  │           │
│  │  - Confirm       │  │  - Confirmed     │  │  - request()     │           │
│  │  - Prompt        │  │  - Text          │  │  - confirm()     │           │
│  │  - Select        │  │  - Selected      │  │  - prompt()      │           │
│  │  - Custom        │  │  - Custom        │  │  - select()      │           │
│  │                  │  │  - Cancelled     │  │  - handle_response() │       │
│  │                  │  │  - Timeout       │  │                  │           │
│  └──────────────────┘  └──────────────────┘  └──────────────────┘           │
│                                                                              │
│  PlexusStreamItem::Request { request_id, request_type, timeout_ms, ... }    │
├─────────────────────────────────────────────────────────────────────────────┤
│                          TRANSPORT LAYER                                     │
│                                                                              │
│  ┌────────────────────────────┐    ┌────────────────────────────┐           │
│  │          MCP               │    │       WebSocket            │           │
│  │                            │    │                            │           │
│  │  notifications/logging     │    │  plexus_subscribe          │           │
│  │  { type: "request", ... }  │    │  SubscriptionMessage::Request │         │
│  │                            │    │                            │           │
│  │  _plexus_respond tool      │    │  plexus_respond RPC        │           │
│  │  { request_id, payload }   │    │  { sub_id, req_id, payload } │          │
│  └────────────────────────────┘    └────────────────────────────┘           │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Protocol Flow

### Unidirectional (Existing)

```
Client                                          Server
  │                                               │
  │───── tools/call { name: "list" } ───────────▶│
  │                                               │
  │◀──── notification/progress { "Loading..." }──│
  │◀──── notification/progress { "Found 5" } ────│
  │◀──── Data { items: [...] } ──────────────────│
  │◀──── Done ───────────────────────────────────│
  │                                               │
```

### Bidirectional (New)

```
Client                                          Server
  │                                               │
  │───── tools/call { name: "sync" } ────────────▶│
  │                                               │
  │◀──── Progress { "Analyzing changes..." } ────│
  │                                               │
  │◀──── Request {                               │
  │        request_id: "abc-123",                │
  │        request_type: Confirm {               │
  │          message: "Delete 5 files?"          │
  │        },                                    │
  │        timeout_ms: 30000                     │
  │      } ──────────────────────────────────────│
  │                                               │
  │───── _plexus_respond {                       │
  │        request_id: "abc-123",                │ ← User says yes
  │        payload: Confirmed(true)              │
  │      } ─────────────────────────────────────▶│
  │                                               │
  │◀──── Progress { "Deleting files..." } ───────│
  │◀──── Data { deleted: [...] } ────────────────│
  │◀──── Done ───────────────────────────────────│
  │                                               │
```

---

## Request Types

All request types are defined in `hub-core/src/plexus/types.rs`:

### Confirm

Simple yes/no question with optional default.

```rust
RequestType::Confirm {
    message: "Continue with sync?".into(),
    default: Some(true),  // Pre-select "yes"
}
```

**Wire format:**
```json
{
  "type": "confirm",
  "message": "Continue with sync?",
  "default": true
}
```

**Response:** `ResponsePayload::Confirmed(bool)`

### Prompt

Free-form text input with optional defaults.

```rust
RequestType::Prompt {
    message: "Enter GitHub token:".into(),
    default: None,
    placeholder: Some("ghp_xxxx...".into()),
}
```

**Wire format:**
```json
{
  "type": "prompt",
  "message": "Enter GitHub token:",
  "placeholder": "ghp_xxxx..."
}
```

**Response:** `ResponsePayload::Text(String)`

### Select

Single or multi-select from predefined options.

```rust
RequestType::Select {
    message: "Select release channel:".into(),
    options: vec![
        SelectOption {
            value: "stable".into(),
            label: "Stable".into(),
            description: Some("Production ready".into()),
        },
        SelectOption {
            value: "beta".into(),
            label: "Beta".into(),
            description: None,
        },
    ],
    multi_select: false,
}
```

**Wire format:**
```json
{
  "type": "select",
  "message": "Select release channel:",
  "options": [
    { "value": "stable", "label": "Stable", "description": "Production ready" },
    { "value": "beta", "label": "Beta" }
  ],
  "multi_select": false
}
```

**Response:** `ResponsePayload::Selected(Vec<String>)`

### Custom

Escape hatch for domain-specific interactions with optional JSON schema.

```rust
RequestType::Custom {
    type_name: "file_picker".into(),
    schema: Some(json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" },
            "filter": { "type": "string" }
        }
    })),
}
```

**Response:** `ResponsePayload::Custom(Value)`

---

## Transport Mappings

### MCP Transport

MCP doesn't natively support server-initiated requests. We work around this using:

1. **Logging notifications** to send requests to the client
2. **`_plexus_respond` tool** for clients to send responses back

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         MCP Bidirectional Flow                             │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  1. Server sends request via logging notification:                         │
│                                                                            │
│     {                                                                      │
│       "jsonrpc": "2.0",                                                    │
│       "method": "notifications/logging",                                   │
│       "params": {                                                          │
│         "level": "info",                                                   │
│         "logger": "plexus.bidir",                                          │
│         "data": {                                                          │
│           "type": "request",                                               │
│           "request_id": "abc-123",                                         │
│           "request_type": { "type": "confirm", "message": "Continue?" },   │
│           "timeout_ms": 30000                                              │
│         }                                                                  │
│       }                                                                    │
│     }                                                                      │
│                                                                            │
│  2. Client responds via _plexus_respond tool:                              │
│                                                                            │
│     {                                                                      │
│       "jsonrpc": "2.0",                                                    │
│       "id": 42,                                                            │
│       "method": "tools/call",                                              │
│       "params": {                                                          │
│         "name": "_plexus_respond",                                         │
│         "arguments": {                                                     │
│           "request_id": "abc-123",                                         │
│           "payload": { "type": "confirmed", "0": true }                    │
│         }                                                                  │
│       }                                                                    │
│     }                                                                      │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### WebSocket Transport

WebSocket natively supports bidirectional communication:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                       WebSocket Bidirectional Flow                         │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  1. Client subscribes:                                                     │
│                                                                            │
│     {                                                                      │
│       "jsonrpc": "2.0",                                                    │
│       "id": 1,                                                             │
│       "method": "plexus_subscribe",                                        │
│       "params": ["sync", { "dry_run": false }]                             │
│     }                                                                      │
│                                                                            │
│  2. Server sends request via subscription:                                 │
│                                                                            │
│     {                                                                      │
│       "jsonrpc": "2.0",                                                    │
│       "method": "plexus_subscription",                                     │
│       "params": {                                                          │
│         "subscription": "sub-001",                                         │
│         "result": {                                                        │
│           "type": "request",                                               │
│           "request_id": "abc-123",                                         │
│           "request_type": { "type": "confirm", "message": "Continue?" },   │
│           "timeout_ms": 30000                                              │
│         }                                                                  │
│       }                                                                    │
│     }                                                                      │
│                                                                            │
│  3. Client responds via plexus_respond RPC:                                │
│                                                                            │
│     {                                                                      │
│       "jsonrpc": "2.0",                                                    │
│       "id": 2,                                                             │
│       "method": "plexus_respond",                                          │
│       "params": ["sub-001", "abc-123", { "type": "confirmed", "0": true }] │
│     }                                                                      │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

**WebSocket Message Types** (`hub-core/src/plexus/transport/websocket.rs`):

```rust
// Server -> Client
pub enum SubscriptionMessage {
    Progress { message, percentage },
    Data { content_type, content },
    Request { request_id, request_type, timeout_ms },
    Error { message, code, recoverable },
    Done,
}

// Client -> Server
pub enum ClientMessage {
    Response { request_id, payload },
    Cancel,
}
```

---

## Usage Patterns

### Basic Confirmation

```rust
#[hub_method(bidirectional)]
pub async fn delete_files(
    &self,
    ctx: &BidirChannel,
    paths: Vec<String>,
) -> impl Stream<Item = DeleteEvent> {
    stream! {
        // Ask for confirmation
        match ctx.confirm(format!("Delete {} files?", paths.len())).await {
            Ok(true) => {
                // User confirmed - proceed
                for path in paths {
                    std::fs::remove_file(&path).ok();
                    yield DeleteEvent::Deleted { path };
                }
            }
            Ok(false) => {
                yield DeleteEvent::Cancelled { reason: "User declined".into() };
            }
            Err(BidirError::NotSupported) => {
                // Non-interactive mode - skip confirmation
                for path in paths {
                    std::fs::remove_file(&path).ok();
                    yield DeleteEvent::Deleted { path };
                }
            }
            Err(e) => {
                yield DeleteEvent::Error { message: format!("{}", e) };
            }
        }
    }
}
```

### Graceful Degradation with BidirWithFallback

```rust
use hub_core::plexus::bidirectional::BidirWithFallback;

#[hub_method(bidirectional)]
pub async fn sync_repos(
    &self,
    ctx: &BidirChannel,
    force: Option<bool>,
) -> impl Stream<Item = SyncEvent> {
    let force = force.unwrap_or(false);

    stream! {
        // Use fallback wrapper for graceful degradation
        let bidir = BidirWithFallback::new(ctx)
            .auto_confirm()           // Auto-yes when non-interactive
            .with_default("all");     // Default prompt value

        // This won't block if transport doesn't support bidir
        if bidir.confirm("Sync all repositories?").await {
            let filter = bidir.prompt("Filter pattern:").await
                .unwrap_or_else(|| "*".into());

            // ... perform sync with filter
        }
    }
}
```

### Multi-Step Wizard

```rust
#[hub_method(bidirectional)]
pub async fn setup_project(
    &self,
    ctx: &BidirChannel,
) -> impl Stream<Item = SetupEvent> {
    stream! {
        // Step 1: Get project name
        yield SetupEvent::Step { num: 1, total: 3, desc: "Project name".into() };

        let name = match ctx.prompt("Enter project name:").await {
            Ok(n) => n,
            Err(BidirError::NotSupported) => "my-project".into(),
            Err(e) => {
                yield SetupEvent::Error { message: format!("{}", e) };
                return;
            }
        };

        // Step 2: Select template
        yield SetupEvent::Step { num: 2, total: 3, desc: "Template".into() };

        let template = ctx.select("Choose template:", vec![
            SelectOption { value: "minimal".into(), label: "Minimal".into(), description: None },
            SelectOption { value: "full".into(), label: "Full".into(), description: Some("With tests and CI".into()) },
        ]).await.unwrap_or_else(|_| "minimal".into());

        // Step 3: Confirm
        yield SetupEvent::Step { num: 3, total: 3, desc: "Confirm".into() };

        let summary = format!("Create '{}' with '{}' template?", name, template);
        if ctx.confirm(&summary).await.unwrap_or(true) {
            // Create project...
            yield SetupEvent::Created { name, template };
        } else {
            yield SetupEvent::Cancelled { reason: "User declined".into() };
        }
    }
}
```

---

## Error Handling

### BidirError Variants

```rust
pub enum BidirError {
    /// Client cancelled the request (user hit Escape/Cancel)
    Cancelled,

    /// Request timed out waiting for response
    Timeout,

    /// Transport doesn't support bidirectional (e.g., stdio, non-interactive MCP)
    NotSupported,

    /// Response type doesn't match request type
    /// e.g., sent Confirm, got Text response
    TypeMismatch { expected: String, got: String },

    /// Transport-level error (channel closed, network issue)
    Transport(String),
}
```

### Helper Function

```rust
use hub_core::plexus::bidirectional::bidir_error_message;

match ctx.confirm("Continue?").await {
    Ok(confirmed) => { /* ... */ }
    Err(e) => {
        yield MyEvent::Error {
            message: bidir_error_message(&e)
        };
    }
}

// Output messages:
// - "Operation cancelled by user"
// - "Timed out waiting for response"
// - "Interactive mode not supported"
// - "Expected Confirmed response, got Text"
// - "Communication error: channel closed"
```

### Error Recovery Patterns

```rust
// Pattern 1: Retry on timeout
let mut attempts = 0;
let confirmed = loop {
    match ctx.request_with_timeout(
        RequestType::Confirm { message: "Continue?".into(), default: None },
        Duration::from_secs(10),
    ).await {
        Ok(ResponsePayload::Confirmed(v)) => break v,
        Err(BidirError::Timeout) if attempts < 3 => {
            attempts += 1;
            yield MyEvent::Warning { message: "No response, retrying...".into() };
            continue;
        }
        Err(e) => {
            yield MyEvent::Error { message: format!("{}", e) };
            return;
        }
        _ => break false,
    }
};

// Pattern 2: Fallback to default on NotSupported
let answer = match ctx.prompt("Enter value:").await {
    Ok(s) => s,
    Err(BidirError::NotSupported) => "default_value".into(),
    Err(e) => {
        yield MyEvent::Error { message: format!("{}", e) };
        return;
    }
};
```

---

## Testing Patterns

### Unit Testing with Mock Channel

```rust
#[tokio::test]
async fn test_bidirectional_confirmation() {
    // Create channel
    let (tx, mut rx) = mpsc::channel(10);
    let channel = BidirChannel::new(
        tx,
        true,  // bidirectional supported
        vec!["test".into()],
        "hash".into(),
    );

    // Spawn the method call
    let channel_clone = channel.clone();
    let task = tokio::spawn(async move {
        channel_clone.confirm("Continue?").await
    });

    // Receive the request
    let item = rx.recv().await.unwrap();
    let request_id = match item {
        PlexusStreamItem::Request { request_id, request_type, .. } => {
            // Verify request
            assert!(matches!(request_type, RequestType::Confirm { .. }));
            request_id
        }
        _ => panic!("Expected Request"),
    };

    // Send response
    channel.handle_response(ClientResponse {
        request_id,
        payload: ResponsePayload::Confirmed(true),
    }).unwrap();

    // Verify result
    let result = task.await.unwrap();
    assert_eq!(result.unwrap(), true);
}
```

### Testing Timeout Behavior

```rust
#[tokio::test]
async fn test_request_timeout() {
    let (tx, _rx) = mpsc::channel(10);
    let channel = BidirChannel::new(tx, true, vec![], String::new());

    let start = std::time::Instant::now();
    let result = channel.request_with_timeout(
        RequestType::Confirm { message: "test".into(), default: None },
        Duration::from_millis(50),
    ).await;

    // Should timeout
    assert!(matches!(result, Err(BidirError::Timeout)));
    assert!(start.elapsed() >= Duration::from_millis(50));

    // Pending should be cleaned up
    assert_eq!(channel.pending_count(), 0);
}
```

### Testing NotSupported Fallback

```rust
#[tokio::test]
async fn test_unidirectional_returns_not_supported() {
    let (tx, _rx) = mpsc::channel(1);
    let channel = BidirChannel::unidirectional(tx);

    let result = channel.confirm("test").await;
    assert!(matches!(result, Err(BidirError::NotSupported)));

    // With fallback wrapper
    let fallback = BidirWithFallback::new(&channel).auto_confirm();
    let result = fallback.confirm("test").await;
    assert!(result); // Returns auto_confirm value
}
```

### Integration Testing with Full Activation

```rust
#[tokio::test]
async fn test_interactive_wizard_flow() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(tx, true, vec![], String::new()));

    let interactive = Interactive;
    let stream = interactive.wizard(&channel, Some(false)).await;
    tokio::pin!(stream);

    let mut events = Vec::new();
    let mut completed = false;

    while !completed {
        tokio::select! {
            Some(event) = stream.next() => {
                events.push(event.clone());
                if matches!(event, InteractiveEvent::Result { .. }) {
                    completed = true;
                }
            }
            Some(item) = rx.recv() => {
                if let PlexusStreamItem::Request { request_id, request_type, .. } = item {
                    // Auto-respond based on request type
                    let payload = match request_type {
                        RequestType::Prompt { .. } => ResponsePayload::Text("test-input".into()),
                        RequestType::Select { options, .. } => {
                            ResponsePayload::Selected(vec![options[0].value.clone()])
                        }
                        RequestType::Confirm { .. } => ResponsePayload::Confirmed(true),
                        _ => ResponsePayload::Cancelled,
                    };
                    channel.handle_response(ClientResponse { request_id, payload }).unwrap();
                }
            }
        }
    }

    // Verify all steps completed
    let step_count = events.iter()
        .filter(|e| matches!(e, InteractiveEvent::StepStarted { .. }))
        .count();
    assert_eq!(step_count, 5);
}
```

---

## Implementation Status

### Completed (BIDIR-2 through BIDIR-8)

| Component | Location | Status |
|-----------|----------|--------|
| **Core Types** | `hub-core-bidir/src/plexus/types.rs` | Complete |
| - `RequestType` enum | Confirm, Prompt, Select, Custom | Complete |
| - `ResponsePayload` enum | Confirmed, Text, Selected, Custom, Cancelled, Timeout | Complete |
| - `PlexusStreamItem::Request` | New variant for bidirectional | Complete |
| - `ClientResponse` struct | Request ID + payload correlation | Complete |
| **BidirChannel** | `hub-core-bidir/src/plexus/bidirectional.rs` | Complete |
| - `BidirectionalContext` trait | `request()`, `request_with_timeout()`, `is_bidirectional()` | Complete |
| - `BidirExt` trait | `confirm()`, `prompt()`, `select()` helpers | Complete |
| - `BidirChannel` struct | Channel implementation with pending map | Complete |
| - `handle_response()` | Route responses to waiting tasks | Complete |
| **Helper Patterns** | `hub-core-bidir/src/plexus/bidirectional.rs` | Complete |
| - `BidirWithFallback` | Graceful degradation wrapper | Complete |
| - `TimeoutConfig` | Request-type-specific timeouts | Complete |
| - `bidir_error_message()` | User-friendly error messages | Complete |
| **Macro Support** | `hub-macro-bidir/src/parse.rs` | Complete |
| - `#[hub_method(bidirectional)]` | Attribute parsing | Complete |
| - `MethodInfo.bidirectional` | Flag propagation | Complete |
| - `MethodInfo.context_param` | BidirChannel parameter detection | Complete |
| - Context parameter filtering | Excluded from schema | Complete |
| **WebSocket Types** | `hub-core-bidir/src/plexus/transport/websocket.rs` | Complete |
| - `SubscriptionMessage` enum | Server-to-client wire format | Complete |
| - `ClientMessage` enum | Client-to-server messages | Complete |
| - `From<PlexusStreamItem>` | Conversion for transport | Complete |
| **Tests** | Various | Complete |
| - Channel roundtrip tests | Full request-response cycle | Complete |
| - Error handling tests | NotSupported, Timeout, TypeMismatch | Complete |
| - Concurrent request tests | Multiple pending requests | Complete |
| - Fallback pattern tests | BidirWithFallback behavior | Complete |

### Pending

| Component | Status | Notes |
|-----------|--------|-------|
| **MCP Transport Integration** | Planned (BIDIR-6) | `_plexus_respond` tool, logging notifications |
| **WebSocket Transport Integration** | Planned (BIDIR-7) | `plexus_respond` RPC, subscription routing |
| **Example Interactive Activation** | Planned (BIDIR-8) | Demo methods: confirm_demo, prompt_demo, wizard |
| **Integration Tests** | Planned (BIDIR-9) | End-to-end tests over both transports |

---

## File Reference

### Core Types and Channel

```
hub-core-bidir/src/plexus/
├── types.rs              # PlexusStreamItem::Request, RequestType, ResponsePayload
├── bidirectional.rs      # BidirChannel, BidirExt, BidirWithFallback, TimeoutConfig
├── transport/
│   ├── mod.rs
│   └── websocket.rs      # SubscriptionMessage, ClientMessage
└── mod.rs                # Module exports
```

### Macro Support

```
hub-macro-bidir/src/
├── parse.rs              # HubMethodAttrs.bidirectional, MethodInfo.context_param
└── codegen/
    ├── activation.rs     # Dispatch generation for bidirectional methods
    └── method_enum.rs    # Schema generation (excludes context param)
```

### Transport Integration (Planned)

```
substrate-bidir/src/
├── mcp_bridge.rs         # _plexus_respond tool, logging notifications
└── ws_server.rs          # plexus_respond RPC, subscription registry
```

---

## References

- [BIDIR-1: Epic Overview](/Users/user/dev/controlflow/hypermemetic/substrate/plans/BIDIR/BIDIR-1.md)
- [MCP Protocol Specification](https://modelcontextprotocol.io/specification)
- [jsonrpsee Subscriptions](https://docs.rs/jsonrpsee/latest/jsonrpsee/)
- [Caller-Wraps Streaming Architecture](/Users/user/dev/controlflow/hypermemetic/substrate-bidir/docs/architecture/16680179837700061695_caller-wraps-streaming.md)

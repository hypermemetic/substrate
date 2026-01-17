# Bidirectional Streaming Architecture

## Overview

Bidirectional streaming extends substrate's streaming architecture to support **server-initiated requests during method execution**. This enables interactive workflows where activations can ask clients for confirmation, text input, or selection from options mid-stream.

```
Traditional RPC:        Streaming:              Bidirectional:
  Client -> Server       Client -> Server        Client -> Server
  Client <- Server       Client <- Data          Client <- Data
                         Client <- Data          Client <- Request (confirm?)
                         Client <- Done          Client -> Response (yes)
                                                 Client <- Data
                                                 Client <- Done
```

## Design Goals

| Goal | Description |
|------|-------------|
| **Backward Compatible** | Existing unidirectional methods work unchanged |
| **Opt-in** | Bidirectional is a method-level attribute, not required |
| **Transport Agnostic** | Works over MCP and WebSocket transports |
| **Type Safe** | Compile-time guarantees for request/response matching |
| **Graceful Degradation** | Falls back when bidirectional not supported |

## Architecture Layers

```
+---------------------------------------------------------------------+
|                         Activation Layer                             |
|  +---------------------------------------------------------------+  |
|  |  #[hub_method(description = "...", bidirectional)]            |  |
|  |  async fn wizard(&self, ctx: &BidirChannel) -> Stream<Event>  |  |
|  +---------------------------------------------------------------+  |
+---------------------------------------------------------------------+
                                  |
                                  v
+---------------------------------------------------------------------+
|                         Core Layer (hub-core)                        |
|  +---------------+  +----------------+  +------------------------+  |
|  | RequestType   |  | ResponsePayload|  | BidirectionalContext   |  |
|  | - Confirm     |  | - Confirmed    |  | - request()            |  |
|  | - Prompt      |  | - Text         |  | - request_with_timeout |  |
|  | - Select      |  | - Selected     |  | - is_bidirectional()   |  |
|  | - Custom      |  | - Custom       |  +------------------------+  |
|  +---------------+  | - Cancelled    |                              |
|                     | - Timeout      |                              |
|                     +----------------+                              |
|  +---------------------------------------------------------------+  |
|  |                        BidirChannel                            |  |
|  |  - stream_tx: mpsc::Sender<PlexusStreamItem>                  |  |
|  |  - pending: HashMap<String, oneshot::Sender<ResponsePayload>> |  |
|  |  - handle_response(ClientResponse) -> Result<(), BidirError>  |  |
|  +---------------------------------------------------------------+  |
+---------------------------------------------------------------------+
                                  |
                                  v
+---------------------------------------------------------------------+
|                        Transport Layer                               |
|  +---------------------------+  +--------------------------------+  |
|  |           MCP             |  |          WebSocket             |  |
|  | - Request via logging     |  | - Request in subscription msg  |  |
|  |   notification            |  | - Response via plexus_respond  |  |
|  | - Response via            |  |   RPC call                     |  |
|  |   _plexus_respond tool    |  |                                |  |
|  +---------------------------+  +--------------------------------+  |
+---------------------------------------------------------------------+
```

## Protocol Flow

### Unidirectional (Existing)

```
Client                           Server
  |                                |
  |---- tools/call --------------->|
  |                                |
  |<--- Progress -----------------|
  |<--- Data --------------------|
  |<--- Done --------------------|
  |<--- Result ------------------|
  |                                |
```

### Bidirectional (New)

```
Client                           Server
  |                                |
  |---- tools/call --------------->|
  |                                |
  |<--- Progress -----------------|
  |<--- Request {                 |  <- Server asks client
  |       request_id: "abc",      |
  |       request_type: Confirm,  |
  |       message: "Continue?"    |
  |     } ------------------------|
  |                                |
  |---- Response {                |  <- Client responds
  |       request_id: "abc",      |
  |       payload: Confirmed(true)|
  |     } ----------------------->|
  |                                |
  |<--- Data --------------------|
  |<--- Done --------------------|
  |<--- Result ------------------|
  |                                |
```

## Request Types

### Confirm

Binary yes/no decision.

```rust
RequestType::Confirm {
    message: "Delete 5 files?".into(),
    default: Some(false),
}
// Response: ResponsePayload::Confirmed(true/false)
```

### Prompt

Free-form text input.

```rust
RequestType::Prompt {
    message: "Enter project name:".into(),
    default: Some("my-project".into()),
    placeholder: Some("project-name".into()),
}
// Response: ResponsePayload::Text(String)
```

### Select

Choose from options.

```rust
RequestType::Select {
    message: "Select environment:".into(),
    options: vec![
        SelectOption { value: "dev".into(), label: "Development".into(), description: None },
        SelectOption { value: "prod".into(), label: "Production".into(), description: Some("Requires approval".into()) },
    ],
    multi_select: false,
}
// Response: ResponsePayload::Selected(Vec<String>)
```

### Custom

Extensibility escape hatch for domain-specific requests.

```rust
RequestType::Custom {
    type_name: "file_picker".into(),
    schema: Some(json!({ "type": "object", "properties": { "path": { "type": "string" } } })),
}
// Response: ResponsePayload::Custom(Value)
```

## Transport Mappings

### MCP Transport

MCP is fundamentally request->streaming response, so bidirectional uses a notification + tool pattern:

```
+---------------------------------------------------------------------+
| 1. Client: tools/call { name: "wizard", arguments: {} }              |
|                                                                      |
| 2. Server sends logging notification:                                |
|    {                                                                 |
|      method: "notifications/logging",                                |
|      params: {                                                       |
|        level: "info",                                                |
|        logger: "plexus.bidir",                                       |
|        data: {                                                       |
|          type: "request",                                            |
|          request_id: "abc-123",                                      |
|          request_type: { "Confirm": { "message": "Continue?" } },    |
|          timeout_ms: 30000                                           |
|        }                                                             |
|      }                                                               |
|    }                                                                 |
|                                                                      |
| 3. Client: tools/call {                                              |
|      name: "_plexus_respond",                                        |
|      arguments: {                                                    |
|        request_id: "abc-123",                                        |
|        payload: { "Confirmed": true }                                |
|      }                                                               |
|    }                                                                 |
|                                                                      |
| 4. Server continues streaming, returns final result                  |
+---------------------------------------------------------------------+
```

### WebSocket Transport

WebSocket is naturally bidirectional:

```
+---------------------------------------------------------------------+
| 1. Client: plexus_subscribe("wizard", {})                            |
|    -> Subscription ID: "sub-1"                                       |
|                                                                      |
| 2. Server subscription message:                                      |
|    { "type": "request", "request_id": "abc", "request_type": {...} } |
|                                                                      |
| 3. Client RPC: plexus_respond("sub-1", "abc", { "Confirmed": true }) |
|                                                                      |
| 4. Server subscription messages:                                     |
|    { "type": "data", "content": {...} }                              |
|    { "type": "done" }                                                |
+---------------------------------------------------------------------+
```

## Usage Patterns

### Basic Confirmation

```rust
#[hub_method(description = "Delete items", bidirectional)]
pub async fn delete(
    &self,
    ctx: &BidirChannel,
    ids: Vec<String>,
) -> impl Stream<Item = DeleteEvent> + Send + 'static {
    stream! {
        // Ask for confirmation
        match ctx.confirm(&format!("Delete {} items?", ids.len())).await {
            Ok(true) => { /* proceed */ }
            Ok(false) => {
                yield DeleteEvent::Cancelled;
                return;
            }
            Err(BidirError::NotSupported) => {
                yield DeleteEvent::Error { message: "Interactive mode required".into() };
                return;
            }
            Err(e) => {
                yield DeleteEvent::Error { message: bidir_error_message(&e) };
                return;
            }
        }

        for id in ids {
            // ... delete logic ...
            yield DeleteEvent::Deleted { id };
        }
    }
}
```

### Graceful Degradation

```rust
// Use BidirWithFallback when confirmation is optional
let bidir = BidirWithFallback::new(ctx)
    .auto_confirm()           // Return true if bidir not supported
    .with_default("anonymous"); // Return this for prompts if not supported

if bidir.confirm("Proceed?").await {
    let name = bidir.prompt("Name?").await.unwrap_or_default();
    // Continues even if transport doesn't support bidir
}
```

### Multi-Step Wizard

```rust
#[hub_method(description = "Setup wizard", bidirectional)]
pub async fn setup(&self, ctx: &BidirChannel) -> impl Stream<Item = SetupEvent> + Send + 'static {
    stream! {
        // Step 1: Collect name
        yield SetupEvent::StepStarted { step: 1, description: "Project name".into() };
        let name = match ctx.prompt("Project name:").await {
            Ok(n) => n,
            Err(BidirError::NotSupported) => "default-project".into(),
            Err(e) => {
                yield SetupEvent::Error { message: bidir_error_message(&e) };
                return;
            }
        };

        // Step 2: Select template
        yield SetupEvent::StepStarted { step: 2, description: "Template".into() };
        let template = ctx.select("Template:", vec![
            SelectOption { value: "minimal".into(), label: "Minimal".into(), description: None },
            SelectOption { value: "full".into(), label: "Full".into(), description: None },
        ]).await.unwrap_or("minimal".into());

        // Step 3: Checkpoint confirmation
        yield SetupEvent::StepStarted { step: 3, description: "Confirm".into() };
        if !ctx.confirm(&format!("Create '{}' with '{}'?", name, template)).await.unwrap_or(true) {
            yield SetupEvent::Cancelled;
            return;
        }

        // Step 4: Execute
        yield SetupEvent::Created { name, template };
    }
}
```

## Error Handling

### BidirError Variants

| Error | Description | Typical Recovery |
|-------|-------------|------------------|
| `NotSupported` | Transport doesn't support bidirectional | Use fallback value or fail |
| `Timeout` | Client didn't respond in time | Retry or cancel operation |
| `Cancelled` | Client explicitly cancelled | Clean up and return |
| `TypeMismatch` | Response type doesn't match request | Bug in client - log error |
| `Transport` | Communication failure | Depends on cause |

### Helper Function

```rust
use hub_core::plexus::bidir_error_message;

match ctx.confirm("Continue?").await {
    Ok(v) => { /* use v */ }
    Err(e) => {
        yield MyEvent::Error {
            message: bidir_error_message(&e),  // User-friendly message
            recoverable: matches!(e, BidirError::Timeout),
        };
    }
}
```

## Testing Bidirectional Methods

```rust
#[tokio::test]
async fn test_my_bidir_method() {
    let (stream_tx, mut stream_rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(
        stream_tx,
        true,  // bidirectional_supported
        vec!["test".into()],  // provenance
        "test-hash".into(),   // plexus_hash
    ));

    // Run method in background
    let stream = my_activation.my_method(&channel, args).await;
    tokio::pin!(stream);

    let mut events = Vec::new();
    loop {
        tokio::select! {
            Some(event) = stream.next() => {
                events.push(event.clone());
                if matches!(event, MyEvent::Done) {
                    break;
                }
            }
            Some(item) = stream_rx.recv() => {
                if let PlexusStreamItem::Request { request_id, request_type, .. } = item {
                    // Auto-respond based on request type
                    let payload = match request_type {
                        RequestType::Confirm { .. } => ResponsePayload::Confirmed(true),
                        RequestType::Prompt { .. } => ResponsePayload::Text("test".into()),
                        RequestType::Select { options, .. } => {
                            ResponsePayload::Selected(vec![options[0].value.clone()])
                        }
                        _ => ResponsePayload::Cancelled,
                    };
                    channel.handle_response(ClientResponse { request_id, payload }).unwrap();
                }
            }
        }
    }

    // Assert on collected events
    assert!(events.iter().any(|e| matches!(e, MyEvent::Done)));
}
```

## Implementation Status

| Component | Location | Status |
|-----------|----------|--------|
| Core Types | hub-core/src/plexus/types.rs | Complete |
| BidirChannel | hub-core/src/plexus/bidirectional.rs | Complete |
| BidirExt trait | hub-core/src/plexus/bidirectional.rs | Complete |
| BidirWithFallback | hub-core/src/plexus/bidirectional.rs | Complete |
| TimeoutConfig | hub-core/src/plexus/bidirectional.rs | Complete |
| Hub-macro support | hub-macro/src/parse.rs, codegen/ | Complete |
| MCP transport | substrate/src/mcp_bridge.rs | Complete |
| WebSocket transport | substrate/src/ws_bidir.rs | Complete |
| Interactive example | substrate/src/activations/interactive/ | Complete |
| Test coverage | 184 tests across hub-core, substrate | Complete |

## Implementation Branch

All implementation work is on the `feature/bidirectional-streaming` branch in:
- substrate
- hub-core
- hub-macro

## Future Extensions

- [ ] Request priority levels
- [ ] Request batching for multiple questions
- [ ] Client capability negotiation
- [ ] Server-initiated request cancellation
- [ ] Progress updates during request wait

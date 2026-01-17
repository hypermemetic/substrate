# BIDIR-10: Architecture Documentation

**Status:** Planning
**Blocked By:** BIDIR-8
**Unlocks:** -

## Scope

Document the bidirectional streaming architecture in substrate's architecture docs. This serves as the canonical reference for the design decisions, protocol flow, transport mappings, and usage patterns.

## Acceptance Criteria

- [ ] Architecture doc created in `docs/architecture/`
- [ ] Uses naming convention: `(u64::MAX - nanotime)_bidirectional-streaming.md`
- [ ] Documents design decisions and rationale
- [ ] Includes protocol flow diagrams
- [ ] Covers both MCP and WebSocket transport mappings
- [ ] Provides usage patterns and examples
- [ ] Documents error handling and edge cases

## Implementation Notes

### Document Structure

```markdown
# Bidirectional Streaming Architecture

## Overview

Bidirectional streaming enables server-to-client requests during method execution,
allowing for interactive workflows like confirmations, prompts, and multi-step
wizards.

## Design Goals

1. **Backward Compatible**: Existing unidirectional methods work unchanged
2. **Opt-in**: Bidirectional is a method-level feature
3. **Transport Agnostic**: Works over MCP and WebSocket
4. **Type Safe**: Compile-time guarantees for request/response matching
5. **Graceful Degradation**: Falls back when bidirectional not supported

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Activation Layer                            │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  #[hub_method(bidirectional)]                                │   │
│  │  async fn wizard(&self, ctx: &BidirChannel) -> Stream       │   │
│  └─────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────┤
│                         Core Layer (hub-core)                       │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────┐    │
│  │ RequestType  │  │ResponsePayload│  │ BidirectionalContext  │    │
│  │ - Confirm    │  │ - Confirmed  │  │ - request()           │    │
│  │ - Prompt     │  │ - Text       │  │ - request_with_timeout│    │
│  │ - Select     │  │ - Selected   │  │ - is_bidirectional()  │    │
│  │ - Custom     │  │ - Custom     │  └────────────────────────┘    │
│  └──────────────┘  │ - Cancelled  │                                │
│                    │ - Timeout    │                                │
│                    └──────────────┘                                │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                      BidirChannel                            │   │
│  │  - stream_tx: mpsc::Sender<PlexusStreamItem>                │   │
│  │  - pending: HashMap<String, oneshot::Sender>                │   │
│  │  - handle_response(ClientResponse)                          │   │
│  └─────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────┤
│                       Transport Layer                               │
│  ┌───────────────────────┐     ┌───────────────────────────────┐   │
│  │         MCP           │     │          WebSocket            │   │
│  │ - Request via logging │     │ - Request in subscription     │   │
│  │ - Response via tool   │     │ - Response via RPC call       │   │
│  │   _plexus_respond     │     │   plexus_respond              │   │
│  └───────────────────────┘     └───────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

## Protocol Flow

### Unidirectional (Existing)

```
Client                           Server
  │                                │
  │──── tools/call ───────────────>│
  │                                │
  │<──── Progress ─────────────────│
  │<──── Data ─────────────────────│
  │<──── Done ─────────────────────│
  │<──── Result ───────────────────│
  │                                │
```

### Bidirectional (New)

```
Client                           Server
  │                                │
  │──── tools/call ───────────────>│
  │                                │
  │<──── Progress ─────────────────│
  │<──── Request (confirm) ────────│  ← Server asks client
  │                                │
  │──── Response (true) ──────────>│  ← Client responds
  │                                │
  │<──── Data ─────────────────────│
  │<──── Done ─────────────────────│
  │<──── Result ───────────────────│
  │                                │
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
        SelectOption { value: "dev", label: "Development", description: None },
        SelectOption { value: "prod", label: "Production", description: Some("Requires approval") },
    ],
    multi_select: false,
}
// Response: ResponsePayload::Selected(Vec<String>)
```

### Custom

Extensibility escape hatch.

```rust
RequestType::Custom {
    type_name: "file_picker".into(),
    schema: Some(json!({...})),
}
// Response: ResponsePayload::Custom(Value)
```

## Transport Mappings

### MCP Transport

MCP is fundamentally request→streaming response, so bidirectional uses a workaround:

1. Server sends Request as logging notification
2. Client responds via `_plexus_respond` tool call
3. Server correlates response by request_id

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. Client: tools/call { name: "wizard", arguments: {} }         │
│                                                                 │
│ 2. Server notification:                                         │
│    {                                                            │
│      method: "notifications/logging",                           │
│      params: {                                                  │
│        level: "info",                                           │
│        data: {                                                  │
│          type: "request",                                       │
│          request_id: "abc-123",                                 │
│          request_type: { "Confirm": { message: "Continue?" } }  │
│        }                                                        │
│      }                                                          │
│    }                                                            │
│                                                                 │
│ 3. Client: tools/call { name: "_plexus_respond",                │
│                         arguments: {                            │
│                           request_id: "abc-123",                │
│                           payload: { "Confirmed": true }        │
│                         } }                                     │
│                                                                 │
│ 4. Server continues streaming, final result                     │
└─────────────────────────────────────────────────────────────────┘
```

### WebSocket Transport

WebSocket is naturally bidirectional, so the mapping is cleaner:

1. Server sends Request as subscription message
2. Client responds via `plexus_respond` RPC call
3. Subscription continues

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. Client: plexus_subscribe("wizard", {})                       │
│    → Subscription ID: "sub-1"                                   │
│                                                                 │
│ 2. Server subscription message:                                 │
│    { type: "request", request_id: "abc", request_type: {...} }  │
│                                                                 │
│ 3. Client RPC: plexus_respond("sub-1", "abc", {...})           │
│                                                                 │
│ 4. Server subscription message:                                 │
│    { type: "data", content: {...} }                             │
│    { type: "done" }                                             │
└─────────────────────────────────────────────────────────────────┘
```

## Usage Patterns

### Basic Confirmation

```rust
#[hub_method(description = "Delete items", bidirectional)]
pub async fn delete(&self, ctx: &BidirChannel, ids: Vec<String>) -> impl Stream<Item = Event> {
    stream! {
        // Ask for confirmation
        if !ctx.confirm(&format!("Delete {} items?", ids.len())).await.unwrap_or(false) {
            yield Event::Cancelled;
            return;
        }

        // Proceed with deletion
        for id in ids {
            // ...delete logic...
            yield Event::Deleted { id };
        }
    }
}
```

### Graceful Degradation

```rust
// Use fallback when bidirectional not supported
let bidir = BidirWithFallback::new(ctx).auto_confirm();
if bidir.confirm("Proceed?").await {
    // Continues even if transport doesn't support bidir
}
```

### Multi-Step Wizard

```rust
#[hub_method(description = "Setup wizard", bidirectional)]
pub async fn setup(&self, ctx: &BidirChannel) -> impl Stream<Item = Event> {
    stream! {
        // Step 1: Collect name
        let name = ctx.prompt("Project name:").await?;

        // Step 2: Select template
        let template = ctx.select("Template:", options).await?;

        // Step 3: Checkpoint
        if !ctx.confirm(&format!("Create '{}' with '{}'?", name, template)).await? {
            yield Event::Cancelled;
            return;
        }

        // Step 4: Execute
        yield Event::Created { name, template };
    }
}
```

## Error Handling

### Error Types

| Error | Description | Recovery |
|-------|-------------|----------|
| `NotSupported` | Transport doesn't support bidir | Use fallback or fail |
| `Timeout` | Client didn't respond in time | Retry or cancel |
| `Cancelled` | Client explicitly cancelled | Clean up and return |
| `TypeMismatch` | Wrong response type | Bug in client |
| `Transport` | Communication failure | Depends on cause |

### Best Practices

1. **Always handle NotSupported**: Use fallback patterns for optional interactions
2. **Set appropriate timeouts**: Quick confirmations vs. complex decisions
3. **Provide context in messages**: Help users make informed decisions
4. **Emit progress events**: Keep users informed during waits

## Testing

### Mock Channel

```rust
#[tokio::test]
async fn test_bidir_method() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(tx, true));

    // Run method
    let stream = my_method(&channel).await;
    tokio::pin!(stream);

    // Handle requests
    while let Some(item) = rx.recv().await {
        if let PlexusStreamItem::Request { request_id, .. } = item {
            channel.handle_response(ClientResponse {
                request_id,
                payload: ResponsePayload::Confirmed(true),
            }).unwrap();
        }
    }

    // Collect results
    let events: Vec<_> = stream.collect().await;
}
```

## Performance Considerations

- Request timeout defaults to 30 seconds
- Channel buffer size is 32 items
- Pending requests stored in HashMap (O(1) lookup)
- Response handling uses oneshot channels (zero-copy)

## Future Extensions

- [ ] Request priority levels
- [ ] Request batching
- [ ] Client capability negotiation
- [ ] Request cancellation from server
- [ ] Progress updates during request wait
```

### File Generation Script

```python
# generate_doc_filename.py
import time

nanotime = int(time.time() * 1_000_000_000)
filename = (2**64 - 1) - nanotime
print(f'{filename}_bidirectional-streaming.md')
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `substrate/docs/architecture/<timestamp>_bidirectional-streaming.md` | NEW: Architecture doc |
| `hub-core/docs/architecture/<timestamp>_bidirectional-streaming.md` | NEW: Linked doc (if applicable) |

## Testing

Documentation is validated by:

1. **Code examples compile**: All Rust snippets should be testable
2. **Diagrams are accurate**: Match actual protocol flow
3. **Links work**: Cross-references to code and other docs
4. **Review**: Technical review by implementers

## Notes

- Use the timestamp naming convention for chronological bubbling
- Link to this doc from relevant code comments
- Update if implementation details change
- Consider adding to hyperforge's user-facing docs for activation authors
- Include in any onboarding materials for contributors

# Bidirectional Communication in Plexus

This document explains how to implement bidirectional communication in Plexus activations, allowing servers to request input from clients during stream execution.

## Overview

Traditional RPC is unidirectional: clients send requests, servers respond. **Bidirectional communication** extends this by allowing the server to send requests back to the client during stream execution.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Traditional RPC (Unidirectional)                 │
│                                                                     │
│  Client ──── Request ────► Server                                   │
│         ◄─── Response ────                                          │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                    Bidirectional RPC                                │
│                                                                     │
│  Client ──── Request ────► Server                                   │
│         ◄─── Stream Item ──                                         │
│         ◄─── Request ────── (server asks client)                    │
│         ──── Response ───►  (client responds)                       │
│         ◄─── Stream Item ──                                         │
│         ◄─── Done ─────────                                         │
└─────────────────────────────────────────────────────────────────────┘
```

### Use Cases

- **User confirmations** before destructive operations ("Delete 3 files?")
- **Interactive prompts** for missing information ("Enter project name:")
- **Selection menus** for choosing options ("Select template:")
- **Multi-step wizards** that guide users through complex workflows

## Defining Bidirectional Methods

Use the `#[hub_method(bidirectional)]` attribute to mark a method as bidirectional:

```rust
use plexus_macros::{hub_methods, hub_method};
use plexus_core::plexus::bidirectional::{StandardBidirChannel, SelectOption, BidirError};
use std::sync::Arc;

#[hub_methods(namespace = "myapp", version = "1.0.0")]
impl MyActivation {
    /// Interactive setup wizard
    #[hub_method(bidirectional, streaming)]
    async fn setup_wizard(
        &self,
        ctx: &Arc<StandardBidirChannel>,  // Injected by macro
    ) -> impl Stream<Item = WizardEvent> + Send + 'static {
        let ctx = ctx.clone();
        stream! {
            // Step 1: Get project name
            let name = match ctx.prompt("Enter project name:").await {
                Ok(n) => n,
                Err(BidirError::NotSupported) => {
                    yield WizardEvent::Error {
                        message: "Interactive mode required".into()
                    };
                    return;
                }
                Err(e) => {
                    yield WizardEvent::Error { message: format!("{}", e) };
                    return;
                }
            };

            yield WizardEvent::NameCollected { name: name.clone() };

            // Step 2: Select template
            let templates = vec![
                SelectOption::new("minimal", "Minimal")
                    .with_description("Bare-bones starter"),
                SelectOption::new("full", "Full Featured")
                    .with_description("All features included"),
            ];

            let template = match ctx.select("Choose template:", templates).await {
                Ok(selected) => selected.into_iter().next().unwrap_or_default(),
                Err(BidirError::Cancelled) => {
                    yield WizardEvent::Cancelled;
                    return;
                }
                Err(e) => {
                    yield WizardEvent::Error { message: format!("{}", e) };
                    return;
                }
            };

            yield WizardEvent::TemplateSelected { template };

            // Step 3: Confirm
            match ctx.confirm("Create project?").await {
                Ok(true) => yield WizardEvent::Created,
                Ok(false) | Err(BidirError::Cancelled) => yield WizardEvent::Cancelled,
                Err(e) => yield WizardEvent::Error { message: format!("{}", e) },
            }
        }
    }
}
```

## StandardBidirChannel Helpers

The `StandardBidirChannel` provides three convenience methods for common UI patterns:

### confirm() - Yes/No Questions

```rust
// Simple confirmation
if ctx.confirm("Delete this file?").await? {
    // User confirmed
}

// With error handling
match ctx.confirm("Proceed with deployment?").await {
    Ok(true) => deploy(),
    Ok(false) => println!("Deployment cancelled"),
    Err(BidirError::NotSupported) => {
        // Non-interactive transport - use safe default
        println!("Skipping deployment in non-interactive mode");
    }
    Err(BidirError::Cancelled) => println!("User cancelled"),
    Err(e) => eprintln!("Error: {}", e),
}
```

### prompt() - Text Input

```rust
// Simple prompt
let name = ctx.prompt("Enter your name:").await?;

// Prompt with validation
loop {
    let email = ctx.prompt("Enter email:").await?;
    if email.contains('@') {
        break email;
    }
    // Invalid - will prompt again
}
```

### select() - Selection Menus

```rust
use plexus_core::plexus::bidirectional::SelectOption;

// Single selection
let options = vec![
    SelectOption::new("dev", "Development")
        .with_description("Local development environment"),
    SelectOption::new("staging", "Staging")
        .with_description("Pre-production testing"),
    SelectOption::new("prod", "Production")
        .with_description("Live environment - requires approval"),
];

let selected = ctx.select("Choose environment:", options).await?;
// selected is Vec<String> with one element for single-select
let env = selected.into_iter().next().unwrap();
```

## Timeout Handling

All bidirectional requests have a default 30-second timeout. For custom timeouts:

```rust
use std::time::Duration;
use plexus_core::plexus::bidirectional::{StandardRequest, TimeoutConfig};

// Using request_with_timeout directly
let response = ctx.request_with_timeout(
    StandardRequest::Confirm {
        message: "Complex decision?".into(),
        default: None,
    },
    Duration::from_secs(120)  // 2 minutes
).await?;

// Using TimeoutConfig presets
let config = TimeoutConfig::patient();  // 60 second timeouts
// Quick: 10s, Normal: 30s, Patient: 60s, Extended: 5min
```

### Handling Timeouts

```rust
match ctx.confirm("Approve changes?").await {
    Ok(approved) => { /* handle response */ }
    Err(BidirError::Timeout(ms)) => {
        // User didn't respond in time
        println!("No response after {}ms, using default", ms);
    }
    Err(e) => { /* other errors */ }
}
```

## Error Handling Best Practices

Always handle `BidirError::NotSupported` to support non-interactive transports:

```rust
use plexus_core::plexus::bidirectional::{BidirError, bidir_error_message};

async fn safe_delete(ctx: &StandardBidirChannel, paths: &[String]) -> Result<(), String> {
    // Try to get confirmation
    let confirmed = match ctx.confirm(&format!("Delete {} files?", paths.len())).await {
        Ok(c) => c,
        Err(BidirError::NotSupported) => {
            // Non-interactive: don't delete without confirmation
            return Err("Cannot delete without user confirmation".into());
        }
        Err(BidirError::Cancelled) => false,
        Err(e) => return Err(bidir_error_message(&e)),
    };

    if confirmed {
        // Proceed with deletion
        Ok(())
    } else {
        Err("Deletion cancelled".into())
    }
}
```

## Example: Interactive Activation

See the complete interactive activation example in `src/activations/interactive/`:

```
src/activations/interactive/
├── activation.rs    # Bidirectional method implementations
├── types.rs         # Event types (WizardEvent, DeleteEvent, etc.)
└── mod.rs           # Module documentation and exports
```

### The Wizard Method

The wizard demonstrates all three bidirectional patterns:

1. **Prompt** for project name
2. **Select** template from options
3. **Confirm** before creation

```rust
#[hub_method(bidirectional, streaming)]
async fn wizard(
    &self,
    ctx: &Arc<StandardBidirChannel>,
) -> impl Stream<Item = WizardEvent> + Send + 'static {
    let ctx = ctx.clone();
    stream! {
        yield WizardEvent::Started;

        // Step 1: Prompt for name
        let name = ctx.prompt("Enter project name:").await?;
        yield WizardEvent::NameCollected { name };

        // Step 2: Select template
        let templates = vec![
            SelectOption::new("minimal", "Minimal"),
            SelectOption::new("full", "Full Featured"),
        ];
        let selected = ctx.select("Choose template:", templates).await?;
        yield WizardEvent::TemplateSelected { template: selected[0].clone() };

        // Step 3: Confirm
        if ctx.confirm("Create project?").await? {
            yield WizardEvent::Created;
        } else {
            yield WizardEvent::Cancelled;
        }

        yield WizardEvent::Done;
    }
}
```

## Wire Protocol

### Request Format (PlexusStreamItem_Request)

```json
{
  "type": "request",
  "requestId": "550e8400-e29b-41d4-a716-446655440000",
  "requestData": {
    "type": "confirm",
    "message": "Delete 3 files?",
    "default": false
  },
  "timeoutMs": 30000
}
```

### StandardRequest Types

```json
// Confirm
{ "type": "confirm", "message": "...", "default": true }

// Prompt
{ "type": "prompt", "message": "...", "default": "value", "placeholder": "hint" }

// Select
{
  "type": "select",
  "message": "...",
  "options": [
    { "value": "opt1", "label": "Option 1", "description": "..." }
  ],
  "multiSelect": false
}
```

### StandardResponse Types

```json
// Confirmed (response to confirm)
{ "type": "confirmed", "value": true }

// Text (response to prompt)
{ "type": "text", "value": "user input" }

// Selected (response to select)
{ "type": "selected", "values": ["opt1", "opt2"] }

// Cancelled (response to any)
{ "type": "cancelled" }
```

## Testing Bidirectional Methods

Use the test helpers from `plexus_core::plexus::bidirectional`:

```rust
use plexus_core::plexus::bidirectional::{
    auto_respond_channel, StandardRequest, StandardResponse
};
use futures::StreamExt;

#[tokio::test]
async fn test_wizard_success() {
    // Create channel with predetermined responses
    let ctx = auto_respond_channel(|req: &StandardRequest| {
        match req {
            StandardRequest::Prompt { .. } => StandardResponse::Text("my-project".into()),
            StandardRequest::Select { options, .. } => {
                StandardResponse::Selected(vec![options[0].value.clone()])
            }
            StandardRequest::Confirm { .. } => StandardResponse::Confirmed(true),
        }
    });

    // Run the wizard
    let activation = Interactive::new();
    let events: Vec<WizardEvent> = activation.wizard(&ctx).await.collect().await;

    // Verify results
    assert!(matches!(events.last(), Some(WizardEvent::Done)));
    assert!(events.iter().any(|e| matches!(e, WizardEvent::Created { .. })));
}

#[tokio::test]
async fn test_wizard_cancelled() {
    let ctx = auto_respond_channel(|_: &StandardRequest| StandardResponse::Cancelled);

    let activation = Interactive::new();
    let events: Vec<WizardEvent> = activation.wizard(&ctx).await.collect().await;

    assert!(matches!(events.last(), Some(WizardEvent::Cancelled)));
}
```

## Transport Support

| Transport | Bidirectional Support | Mechanism |
|-----------|----------------------|-----------|
| WebSocket | Yes | Request as stream item, response via dedicated call |
| MCP | Yes | Request as logging notification, response via `_plexus_respond` tool |
| HTTP | No | Stateless - returns `BidirError::NotSupported` |

For MCP transport, the flow is:

1. Server sends logging notification with `level="warning"` and `data.type="request"`
2. Client receives request data in notification
3. Client calls `_plexus_respond` tool with `request_id` and `response`
4. Server receives response and continues execution

## Custom Request/Response Types

For domain-specific interactions, define custom types:

```rust
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use plexus_core::plexus::bidirectional::BidirChannel;

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageProcessRequest {
    ConfirmOverwrite { path: String, size_bytes: u64 },
    ChooseQuality { min: u8, max: u8, default: u8 },
    SelectFormat { available: Vec<String> },
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageProcessResponse {
    Confirmed { value: bool },
    Quality { value: u8 },
    Format { value: String },
    Cancelled,
}

// Use custom channel type
type ImageChannel = BidirChannel<ImageProcessRequest, ImageProcessResponse>;

#[hub_method(bidirectional, streaming)]
async fn process_images(
    &self,
    ctx: &Arc<ImageChannel>,
    paths: Vec<String>,
) -> impl Stream<Item = ProcessEvent> + Send + 'static {
    let ctx = ctx.clone();
    stream! {
        for path in paths {
            let quality = ctx.request(ImageProcessRequest::ChooseQuality {
                min: 50, max: 100, default: 85,
            }).await?;

            if let ImageProcessResponse::Quality { value } = quality {
                yield ProcessEvent::Processing { path, quality: value };
            }
        }
    }
}
```

## Summary

1. Mark methods with `#[hub_method(bidirectional)]`
2. Accept `&Arc<StandardBidirChannel>` (or custom channel type) as parameter
3. Use `confirm()`, `prompt()`, `select()` for standard patterns
4. Always handle `BidirError::NotSupported` for non-interactive transports
5. Use `auto_respond_channel()` for testing

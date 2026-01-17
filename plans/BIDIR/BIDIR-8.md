# BIDIR-8: Example Interactive Activation

**Status:** Planning
**Blocked By:** BIDIR-6, BIDIR-7
**Unlocks:** BIDIR-9, BIDIR-10

## Scope

Build a complete example activation demonstrating bidirectional patterns. This serves as both documentation and validation that the bidirectional infrastructure works end-to-end over both MCP and WebSocket transports.

## Acceptance Criteria

- [ ] `Interactive` activation with demonstration methods
- [ ] `confirm_demo` - shows confirmation flow
- [ ] `prompt_demo` - shows text input flow
- [ ] `wizard` - multi-step workflow with checkpoints
- [ ] Works over both MCP and WebSocket transports
- [ ] Events for each step to observe progress
- [ ] Error handling demonstrations
- [ ] Timeout behavior demonstrations

## Implementation Notes

### Interactive Activation

```rust
// hyperforge/src/activations/interactive.rs

use async_stream::stream;
use hub_macro::hub_activation;
use hub_core::plexus::{
    BidirChannel, BidirExt, BidirError, BidirWithFallback,
    RequestType, ResponsePayload, SelectOption,
    bidir_error_message, InteractiveWorkflow,
};
use serde::{Deserialize, Serialize};

/// Events emitted by the interactive activation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum InteractiveEvent {
    /// Step started
    StepStarted {
        step: usize,
        total: usize,
        description: String,
    },

    /// User confirmed/declined
    ConfirmResult {
        question: String,
        confirmed: bool,
    },

    /// User provided input
    PromptResult {
        question: String,
        answer: String,
    },

    /// User selected option(s)
    SelectResult {
        question: String,
        selected: Vec<String>,
    },

    /// Workflow checkpoint reached
    Checkpoint {
        step: usize,
        name: String,
        approved: bool,
    },

    /// Final result
    Result {
        success: bool,
        summary: String,
        data: Value,
    },

    /// Error occurred
    Error {
        message: String,
        recoverable: bool,
    },

    /// Operation cancelled
    Cancelled {
        reason: String,
    },
}

#[hub_activation(namespace = "interactive")]
pub struct Interactive;

impl Interactive {
    /// Demonstrates a simple confirmation flow
    #[hub_method(
        description = "Demonstrate confirmation dialog",
        bidirectional,
    )]
    pub async fn confirm_demo(
        &self,
        ctx: &BidirChannel,
        message: Option<String>,
        require_confirmation: Option<bool>,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let message = message.unwrap_or_else(|| "Do you want to proceed?".into());
        let require = require_confirmation.unwrap_or(true);

        stream! {
            yield InteractiveEvent::StepStarted {
                step: 1,
                total: 2,
                description: "Requesting confirmation".into(),
            };

            let result = if require {
                // Must have confirmation, fail if not supported
                match ctx.confirm(&message).await {
                    Ok(confirmed) => {
                        yield InteractiveEvent::ConfirmResult {
                            question: message.clone(),
                            confirmed,
                        };
                        if !confirmed {
                            yield InteractiveEvent::Cancelled {
                                reason: "User declined".into(),
                            };
                            return;
                        }
                        true
                    }
                    Err(BidirError::NotSupported) => {
                        yield InteractiveEvent::Error {
                            message: "This demo requires interactive mode".into(),
                            recoverable: false,
                        };
                        return;
                    }
                    Err(e) => {
                        yield InteractiveEvent::Error {
                            message: bidir_error_message(&e),
                            recoverable: false,
                        };
                        return;
                    }
                }
            } else {
                // Use fallback pattern
                let bidir = BidirWithFallback::new(ctx).auto_confirm();
                let confirmed = bidir.confirm(&message).await;
                yield InteractiveEvent::ConfirmResult {
                    question: message.clone(),
                    confirmed,
                };
                confirmed
            };

            yield InteractiveEvent::StepStarted {
                step: 2,
                total: 2,
                description: "Completing operation".into(),
            };

            yield InteractiveEvent::Result {
                success: result,
                summary: if result {
                    "Operation confirmed and completed".into()
                } else {
                    "Operation completed with auto-confirm".into()
                },
                data: json!({ "confirmed": result }),
            };
        }
    }

    /// Demonstrates text input prompt flow
    #[hub_method(
        description = "Demonstrate text input prompt",
        bidirectional,
    )]
    pub async fn prompt_demo(
        &self,
        ctx: &BidirChannel,
        questions: Option<Vec<String>>,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let questions = questions.unwrap_or_else(|| vec![
            "What is your name?".into(),
            "What is your favorite color?".into(),
        ]);

        stream! {
            let mut answers = Vec::new();
            let total = questions.len();

            for (i, question) in questions.iter().enumerate() {
                yield InteractiveEvent::StepStarted {
                    step: i + 1,
                    total,
                    description: format!("Asking: {}", question),
                };

                match ctx.prompt(question).await {
                    Ok(answer) => {
                        yield InteractiveEvent::PromptResult {
                            question: question.clone(),
                            answer: answer.clone(),
                        };
                        answers.push(json!({
                            "question": question,
                            "answer": answer,
                        }));
                    }
                    Err(BidirError::Cancelled) => {
                        yield InteractiveEvent::Cancelled {
                            reason: "User cancelled input".into(),
                        };
                        return;
                    }
                    Err(BidirError::Timeout) => {
                        yield InteractiveEvent::Error {
                            message: format!("Timed out waiting for answer to: {}", question),
                            recoverable: true,
                        };
                        // Continue with empty answer
                        answers.push(json!({
                            "question": question,
                            "answer": "",
                            "timed_out": true,
                        }));
                    }
                    Err(e) => {
                        yield InteractiveEvent::Error {
                            message: bidir_error_message(&e),
                            recoverable: false,
                        };
                        return;
                    }
                }
            }

            yield InteractiveEvent::Result {
                success: true,
                summary: format!("Collected {} answers", answers.len()),
                data: json!({ "answers": answers }),
            };
        }
    }

    /// Demonstrates selection from options
    #[hub_method(
        description = "Demonstrate selection dialog",
        bidirectional,
    )]
    pub async fn select_demo(
        &self,
        ctx: &BidirChannel,
        multi_select: Option<bool>,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let multi = multi_select.unwrap_or(false);

        stream! {
            yield InteractiveEvent::StepStarted {
                step: 1,
                total: 2,
                description: "Presenting options".into(),
            };

            let options = vec![
                SelectOption {
                    value: "alpha".into(),
                    label: "Alpha Release".into(),
                    description: Some("Latest experimental features".into()),
                },
                SelectOption {
                    value: "beta".into(),
                    label: "Beta Release".into(),
                    description: Some("Stable features, some bugs".into()),
                },
                SelectOption {
                    value: "stable".into(),
                    label: "Stable Release".into(),
                    description: Some("Production ready".into()),
                },
            ];

            let request_type = RequestType::Select {
                message: if multi {
                    "Select release channels to enable:".into()
                } else {
                    "Select your preferred release channel:".into()
                },
                options,
                multi_select: multi,
            };

            match ctx.request(request_type).await {
                Ok(ResponsePayload::Selected(selections)) => {
                    yield InteractiveEvent::SelectResult {
                        question: "Release channel".into(),
                        selected: selections.clone(),
                    };

                    yield InteractiveEvent::StepStarted {
                        step: 2,
                        total: 2,
                        description: "Applying selection".into(),
                    };

                    yield InteractiveEvent::Result {
                        success: true,
                        summary: format!("Selected: {}", selections.join(", ")),
                        data: json!({ "selected": selections }),
                    };
                }
                Ok(ResponsePayload::Cancelled) => {
                    yield InteractiveEvent::Cancelled {
                        reason: "Selection cancelled".into(),
                    };
                }
                Ok(other) => {
                    yield InteractiveEvent::Error {
                        message: format!("Unexpected response: {:?}", other),
                        recoverable: false,
                    };
                }
                Err(e) => {
                    yield InteractiveEvent::Error {
                        message: bidir_error_message(&e),
                        recoverable: false,
                    };
                }
            }
        }
    }

    /// Multi-step workflow with checkpoints
    #[hub_method(
        description = "Demonstrate multi-step wizard workflow",
        bidirectional,
    )]
    pub async fn wizard(
        &self,
        ctx: &BidirChannel,
        simulate_long_steps: Option<bool>,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let simulate = simulate_long_steps.unwrap_or(false);

        stream! {
            let mut workflow = InteractiveWorkflow::<InteractiveEvent>::new(ctx, 5);

            // Step 1: Collect project name
            yield InteractiveEvent::StepStarted {
                step: 1,
                total: 5,
                description: "Collecting project information".into(),
            };

            let project_name = match ctx.prompt("Enter project name:").await {
                Ok(name) => name,
                Err(BidirError::NotSupported) => "default-project".into(),
                Err(e) => {
                    yield InteractiveEvent::Error {
                        message: bidir_error_message(&e),
                        recoverable: false,
                    };
                    return;
                }
            };

            yield InteractiveEvent::PromptResult {
                question: "Enter project name:".into(),
                answer: project_name.clone(),
            };

            // Step 2: Select template
            yield InteractiveEvent::StepStarted {
                step: 2,
                total: 5,
                description: "Selecting template".into(),
            };

            let template = match ctx.select(
                "Select project template:",
                vec![
                    SelectOption { value: "minimal".into(), label: "Minimal".into(), description: None },
                    SelectOption { value: "standard".into(), label: "Standard".into(), description: None },
                    SelectOption { value: "full".into(), label: "Full Featured".into(), description: None },
                ],
            ).await {
                Ok(t) => t,
                Err(BidirError::NotSupported) => "standard".into(),
                Err(e) => {
                    yield InteractiveEvent::Error {
                        message: bidir_error_message(&e),
                        recoverable: false,
                    };
                    return;
                }
            };

            yield InteractiveEvent::SelectResult {
                question: "Select project template:".into(),
                selected: vec![template.clone()],
            };

            // Simulate work if requested
            if simulate {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }

            // Step 3: Checkpoint - confirm before creation
            yield InteractiveEvent::StepStarted {
                step: 3,
                total: 5,
                description: "Checkpoint: confirm creation".into(),
            };

            let confirm_msg = format!(
                "Create project '{}' with '{}' template?",
                project_name, template
            );

            let approved = match ctx.confirm(&confirm_msg).await {
                Ok(v) => v,
                Err(BidirError::NotSupported) => true, // Auto-approve
                Err(e) => {
                    yield InteractiveEvent::Error {
                        message: bidir_error_message(&e),
                        recoverable: false,
                    };
                    return;
                }
            };

            yield InteractiveEvent::Checkpoint {
                step: 3,
                name: "create_project".into(),
                approved,
            };

            if !approved {
                yield InteractiveEvent::Cancelled {
                    reason: "Project creation cancelled at checkpoint".into(),
                };
                return;
            }

            // Step 4: Simulate creation
            yield InteractiveEvent::StepStarted {
                step: 4,
                total: 5,
                description: format!("Creating project '{}'", project_name),
            };

            if simulate {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }

            // Step 5: Optional additional config
            yield InteractiveEvent::StepStarted {
                step: 5,
                total: 5,
                description: "Optional configuration".into(),
            };

            let enable_ci = match ctx.confirm("Enable CI/CD pipeline?").await {
                Ok(v) => v,
                Err(_) => false, // Default to no
            };

            yield InteractiveEvent::ConfirmResult {
                question: "Enable CI/CD pipeline?".into(),
                confirmed: enable_ci,
            };

            // Final result
            yield InteractiveEvent::Result {
                success: true,
                summary: format!("Created project '{}' successfully", project_name),
                data: json!({
                    "project_name": project_name,
                    "template": template,
                    "ci_enabled": enable_ci,
                    "path": format!("/projects/{}", project_name),
                }),
            };
        }
    }

    /// Demonstrates timeout behavior
    #[hub_method(
        description = "Demonstrate request timeout behavior",
        bidirectional,
    )]
    pub async fn timeout_demo(
        &self,
        ctx: &BidirChannel,
        timeout_seconds: Option<u64>,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let timeout = std::time::Duration::from_secs(timeout_seconds.unwrap_or(5));

        stream! {
            yield InteractiveEvent::StepStarted {
                step: 1,
                total: 1,
                description: format!("Waiting for response ({}s timeout)", timeout.as_secs()),
            };

            match ctx.request_with_timeout(
                RequestType::Confirm {
                    message: format!(
                        "You have {} seconds to respond (or let it timeout)",
                        timeout.as_secs()
                    ),
                    default: None,
                },
                timeout,
            ).await {
                Ok(ResponsePayload::Confirmed(v)) => {
                    yield InteractiveEvent::ConfirmResult {
                        question: "Timeout test".into(),
                        confirmed: v,
                    };
                    yield InteractiveEvent::Result {
                        success: true,
                        summary: format!("Response received: {}", v),
                        data: json!({ "responded": true, "confirmed": v }),
                    };
                }
                Ok(ResponsePayload::Timeout) => {
                    yield InteractiveEvent::Error {
                        message: "Request timed out (as expected in demo)".into(),
                        recoverable: true,
                    };
                    yield InteractiveEvent::Result {
                        success: true,
                        summary: "Timeout occurred as expected".into(),
                        data: json!({ "responded": false, "timed_out": true }),
                    };
                }
                Err(BidirError::Timeout) => {
                    yield InteractiveEvent::Error {
                        message: "Request timed out".into(),
                        recoverable: true,
                    };
                    yield InteractiveEvent::Result {
                        success: true,
                        summary: "Timeout behavior demonstrated".into(),
                        data: json!({ "responded": false, "timed_out": true }),
                    };
                }
                Err(e) => {
                    yield InteractiveEvent::Error {
                        message: bidir_error_message(&e),
                        recoverable: false,
                    };
                }
            }
        }
    }
}
```

### Registration

```rust
// hyperforge/src/lib.rs

mod activations {
    pub mod interactive;
    // ... other activations
}

pub fn register_activations(plexus: &mut Plexus) {
    plexus.register(Interactive);
    // ... other registrations
}
```

### MCP Integration Test Script

```bash
#!/bin/bash
# test_interactive_mcp.sh

# Start the MCP server
./target/release/hyperforge mcp &
MCP_PID=$!
sleep 2

# Test confirm_demo
echo "Testing confirm_demo..."
curl -X POST http://localhost:3000/mcp/tools/call \
  -H "Content-Type: application/json" \
  -d '{"name": "interactive.confirm_demo", "arguments": {"message": "Test confirm?"}}'

# Watch for request notification and respond
# (In practice, Claude Code handles this automatically)

kill $MCP_PID
```

### WebSocket Integration Test Script

```typescript
// test_interactive_ws.ts

import WebSocket from 'ws';

async function testInteractiveWizard() {
  const ws = new WebSocket('ws://localhost:9944');

  await new Promise(resolve => ws.on('open', resolve));

  // Subscribe to wizard
  const subscribeMsg = {
    jsonrpc: '2.0',
    id: 1,
    method: 'plexus_subscribe',
    params: ['interactive.wizard', {}]
  };
  ws.send(JSON.stringify(subscribeMsg));

  ws.on('message', async (data) => {
    const msg = JSON.parse(data.toString());

    if (msg.method === 'plexus_subscription') {
      const event = msg.params.result;
      console.log('Event:', event);

      // Auto-respond to requests for testing
      if (event.type === 'request') {
        const responseMsg = {
          jsonrpc: '2.0',
          id: 2,
          method: 'plexus_respond',
          params: [
            msg.params.subscription,
            event.request_id,
            autoResponse(event.request_type)
          ]
        };
        ws.send(JSON.stringify(responseMsg));
      }
    }
  });
}

function autoResponse(requestType: any) {
  if (requestType.Confirm) return { Confirmed: true };
  if (requestType.Prompt) return { Text: 'test-input' };
  if (requestType.Select) return { Selected: [requestType.Select.options[0].value] };
  return { Cancelled: null };
}

testInteractiveWizard();
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `hyperforge/src/activations/interactive.rs` | NEW: Interactive activation |
| `hyperforge/src/activations/mod.rs` | Export interactive module |
| `hyperforge/src/lib.rs` | Register Interactive activation |
| `hyperforge/tests/interactive_mcp.rs` | Integration tests for MCP |
| `hyperforge/tests/interactive_ws.rs` | Integration tests for WebSocket |

## Testing

```rust
// hyperforge/tests/interactive_mcp.rs

use hyperforge::activations::interactive::{Interactive, InteractiveEvent};
use hub_core::plexus::{BidirChannel, ResponsePayload, ClientResponse};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

#[tokio::test]
async fn test_confirm_demo_confirmed() {
    let (stream_tx, mut stream_rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(stream_tx, true));

    let interactive = Interactive;
    let stream = interactive.confirm_demo(&channel, Some("Test?".into()), Some(true)).await;
    tokio::pin!(stream);

    // Collect events until we get a request
    let mut events = Vec::new();
    loop {
        tokio::select! {
            Some(event) = stream.next() => {
                events.push(event);
            }
            Some(item) = stream_rx.recv() => {
                if let PlexusStreamItem::Request { request_id, .. } = item {
                    // Respond with confirmation
                    channel.handle_response(ClientResponse {
                        request_id,
                        payload: ResponsePayload::Confirmed(true),
                    }).unwrap();
                }
            }
        }

        // Check if we got final result
        if events.iter().any(|e| matches!(e, InteractiveEvent::Result { .. })) {
            break;
        }
    }

    // Verify events
    assert!(events.iter().any(|e| matches!(e, InteractiveEvent::ConfirmResult { confirmed: true, .. })));
    assert!(events.iter().any(|e| matches!(e, InteractiveEvent::Result { success: true, .. })));
}

#[tokio::test]
async fn test_confirm_demo_declined() {
    let (stream_tx, mut stream_rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(stream_tx, true));

    let interactive = Interactive;
    let stream = interactive.confirm_demo(&channel, None, Some(true)).await;
    tokio::pin!(stream);

    // Handle request with decline
    let mut events = Vec::new();
    loop {
        tokio::select! {
            Some(event) = stream.next() => {
                events.push(event.clone());
                if matches!(event, InteractiveEvent::Cancelled { .. }) {
                    break;
                }
            }
            Some(item) = stream_rx.recv() => {
                if let PlexusStreamItem::Request { request_id, .. } = item {
                    channel.handle_response(ClientResponse {
                        request_id,
                        payload: ResponsePayload::Confirmed(false),
                    }).unwrap();
                }
            }
        }
    }

    assert!(events.iter().any(|e| matches!(e, InteractiveEvent::Cancelled { .. })));
}

#[tokio::test]
async fn test_wizard_full_flow() {
    let (stream_tx, mut stream_rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(stream_tx, true));

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
            Some(item) = stream_rx.recv() => {
                if let PlexusStreamItem::Request { request_id, request_type, .. } = item {
                    // Auto-respond based on type
                    let payload = match request_type {
                        RequestType::Prompt { .. } => ResponsePayload::Text("test-project".into()),
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

    // Verify we got all expected events
    let step_count = events.iter().filter(|e| matches!(e, InteractiveEvent::StepStarted { .. })).count();
    assert_eq!(step_count, 5);

    // Verify final result
    let result = events.iter().find(|e| matches!(e, InteractiveEvent::Result { .. }));
    assert!(result.is_some());
}

#[tokio::test]
async fn test_timeout_demo() {
    let (stream_tx, _stream_rx) = mpsc::channel(32);
    let channel = Arc::new(BidirChannel::new(stream_tx, true));

    let interactive = Interactive;
    let stream = interactive.timeout_demo(&channel, Some(1)).await; // 1 second timeout
    tokio::pin!(stream);

    let events: Vec<_> = stream.collect().await;

    // Should have timeout error
    assert!(events.iter().any(|e| matches!(
        e,
        InteractiveEvent::Error { message, .. } if message.contains("timeout") || message.contains("Timeout")
    )));
}
```

## Notes

- The Interactive activation serves as living documentation for bidirectional patterns
- All methods demonstrate graceful degradation when bidirectional not supported
- The `wizard` method shows a complete multi-step workflow pattern
- Events provide visibility into what's happening at each step
- Timeout demo helps verify timeout behavior works correctly
- Tests can run without actual user interaction by programmatically responding
- Consider adding this to hyperforge's standard activation set for end-user testing

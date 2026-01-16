//! Interactive activation demonstrating bidirectional communication patterns.
//!
//! This activation shows how to use `BidirChannel` for:
//! - Confirmation prompts
//! - Text input prompts
//! - Fallback patterns when bidirectional isn't supported
//!
//! The `bidirectional` attribute on methods indicates they may send Request
//! stream items to the client and await responses.

use super::types::InteractiveEvent;
use async_stream::stream;
use futures::Stream;
use hub_core::plexus::{bidir_error_message, BidirChannel, BidirError, BidirExt, BidirWithFallback};
use serde_json::json;

/// Interactive activation - demonstrates bidirectional communication
#[derive(Clone)]
pub struct Interactive;

impl Interactive {
    pub fn new() -> Self {
        Interactive
    }
}

impl Default for Interactive {
    fn default() -> Self {
        Self::new()
    }
}

/// Hub-macro generates all the boilerplate for this impl block
#[hub_macro::hub_methods(
    namespace = "interactive",
    version = "1.0.0",
    description = "Demonstrate bidirectional communication patterns"
)]
impl Interactive {
    /// Demonstrate a confirmation dialog
    ///
    /// Shows how to use `ctx.confirm()` for yes/no prompts.
    /// If bidirectional is not supported, returns an error event.
    #[hub_macro::hub_method(
        description = "Demonstrate confirmation dialog",
        params(message = "Custom message to display (optional)"),
        bidirectional
    )]
    async fn confirm_demo(
        &self,
        ctx: &BidirChannel,
        message: Option<String>,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let message = message.unwrap_or_else(|| "Do you want to proceed?".into());
        let ctx = ctx.clone();

        stream! {
            yield InteractiveEvent::StepStarted {
                step: 1,
                total: 2,
                description: "Requesting confirmation".into(),
            };

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
                }
                Err(BidirError::NotSupported) => {
                    yield InteractiveEvent::Error {
                        message: "Interactive mode not supported by current transport".into(),
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

            yield InteractiveEvent::StepStarted {
                step: 2,
                total: 2,
                description: "Completing operation".into(),
            };

            yield InteractiveEvent::Result {
                success: true,
                summary: "Operation confirmed and completed".into(),
                data: json!({ "confirmed": true }),
            };
        }
    }

    /// Demonstrate a text input prompt
    ///
    /// Shows how to use `ctx.prompt()` for free-form text input.
    #[hub_macro::hub_method(
        description = "Demonstrate text input prompt",
        bidirectional
    )]
    async fn prompt_demo(
        &self,
        ctx: &BidirChannel,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let ctx = ctx.clone();

        stream! {
            yield InteractiveEvent::StepStarted {
                step: 1,
                total: 1,
                description: "Asking for input".into(),
            };

            match ctx.prompt("What is your name?").await {
                Ok(answer) => {
                    yield InteractiveEvent::PromptResult {
                        question: "What is your name?".into(),
                        answer: answer.clone(),
                    };
                    yield InteractiveEvent::Result {
                        success: true,
                        summary: format!("Hello, {}!", answer),
                        data: json!({ "name": answer }),
                    };
                }
                Err(BidirError::Cancelled) => {
                    yield InteractiveEvent::Cancelled {
                        reason: "User cancelled input".into(),
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

    /// Demonstrate the fallback pattern
    ///
    /// Uses `BidirWithFallback` to work even when bidirectional isn't supported.
    /// With `auto_confirm()`, confirmations return true automatically.
    /// With `with_default()`, prompts return the default value.
    #[hub_macro::hub_method(
        description = "Demonstrate fallback pattern for non-interactive transports",
        bidirectional
    )]
    async fn fallback_demo(
        &self,
        ctx: &BidirChannel,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let ctx = ctx.clone();

        stream! {
            // Create a fallback helper that:
            // - Auto-confirms when bidirectional not supported
            // - Uses "anonymous" as default for prompts
            let bidir = BidirWithFallback::new(&ctx)
                .auto_confirm()
                .with_default("anonymous");

            yield InteractiveEvent::StepStarted {
                step: 1,
                total: 2,
                description: "Requesting with fallback".into(),
            };

            // This will return true if:
            // - User confirms interactively, OR
            // - Bidirectional not supported (auto_confirm enabled)
            let confirmed = bidir.confirm("Continue with operation?").await;
            yield InteractiveEvent::ConfirmResult {
                question: "Continue with operation?".into(),
                confirmed,
            };

            yield InteractiveEvent::StepStarted {
                step: 2,
                total: 2,
                description: "Getting name with fallback".into(),
            };

            // This will return:
            // - User's input if interactive, OR
            // - "anonymous" if bidirectional not supported
            let name = bidir.prompt("Enter your name:").await.unwrap_or_default();
            yield InteractiveEvent::PromptResult {
                question: "Enter your name:".into(),
                answer: name.clone(),
            };

            yield InteractiveEvent::Result {
                success: true,
                summary: format!("Completed for {}", name),
                data: json!({ "confirmed": confirmed, "name": name }),
            };
        }
    }

    /// Multi-step operation with checkpoints
    ///
    /// Demonstrates a more complex workflow with multiple confirmation points.
    #[hub_macro::hub_method(
        description = "Multi-step operation with approval checkpoints",
        params(steps = "Number of steps to execute (default: 3)"),
        bidirectional
    )]
    async fn multi_step_demo(
        &self,
        ctx: &BidirChannel,
        steps: Option<usize>,
    ) -> impl Stream<Item = InteractiveEvent> + Send + 'static {
        let steps = steps.unwrap_or(3).max(1).min(10);
        let ctx = ctx.clone();

        stream! {
            let bidir = BidirWithFallback::new(&ctx).auto_confirm();

            for step in 1..=steps {
                yield InteractiveEvent::StepStarted {
                    step,
                    total: steps,
                    description: format!("Executing step {}", step),
                };

                // Every other step is a checkpoint requiring approval
                if step % 2 == 0 {
                    let checkpoint_name = format!("checkpoint_{}", step);
                    let approved = bidir
                        .confirm(&format!("Approve {} before continuing?", checkpoint_name))
                        .await;

                    yield InteractiveEvent::Checkpoint {
                        step,
                        name: checkpoint_name,
                        approved,
                    };

                    if !approved {
                        yield InteractiveEvent::Cancelled {
                            reason: format!("Checkpoint at step {} not approved", step),
                        };
                        return;
                    }
                }

                // Simulate some work
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            yield InteractiveEvent::Result {
                success: true,
                summary: format!("Completed all {} steps", steps),
                data: json!({ "steps_completed": steps }),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use hub_core::plexus::{PlexusStreamItem, ResponsePayload};
    use tokio::sync::mpsc;

    /// Helper to create a test channel that doesn't support bidirectional
    fn create_unidirectional_channel() -> (BidirChannel, mpsc::Receiver<PlexusStreamItem>) {
        let (tx, rx) = mpsc::channel(32);
        let channel = BidirChannel::new(tx, false, vec!["test".into()], "test_hash".into());
        (channel, rx)
    }

    /// Helper to create a test channel that supports bidirectional
    fn create_bidirectional_channel() -> (BidirChannel, mpsc::Receiver<PlexusStreamItem>) {
        let (tx, rx) = mpsc::channel(32);
        let channel = BidirChannel::new(tx, true, vec!["test".into()], "test_hash".into());
        (channel, rx)
    }

    #[tokio::test]
    async fn test_confirm_demo_not_supported() {
        let interactive = Interactive::new();
        let (ctx, _rx) = create_unidirectional_channel();

        let mut stream = Box::pin(interactive.confirm_demo(&ctx, None).await);
        let events: Vec<_> = stream.by_ref().collect().await;

        // Should emit: StepStarted, Error (not supported)
        assert!(events.len() >= 2);

        // First event should be StepStarted
        match &events[0] {
            InteractiveEvent::StepStarted { step, total, .. } => {
                assert_eq!(*step, 1);
                assert_eq!(*total, 2);
            }
            other => panic!("Expected StepStarted, got {:?}", other),
        }

        // Second event should be Error
        match &events[1] {
            InteractiveEvent::Error {
                message,
                recoverable,
            } => {
                assert!(message.contains("not supported"));
                assert!(!recoverable);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_confirm_demo_confirmed() {
        let interactive = Interactive::new();
        let (ctx, mut rx) = create_bidirectional_channel();

        // Spawn task to handle the confirm request
        let ctx_clone = ctx.clone();
        let responder = tokio::spawn(async move {
            // Wait for the request to arrive
            if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
                // Respond with confirmation
                ctx_clone
                    .handle_response(hub_core::plexus::ClientResponse {
                        request_id,
                        payload: ResponsePayload::Confirmed(true),
                    })
                    .unwrap();
            }
        });

        let stream = interactive.confirm_demo(&ctx, None).await;
        let events: Vec<_> = stream.collect().await;

        responder.await.unwrap();

        // Should emit: StepStarted, ConfirmResult(true), StepStarted, Result
        assert!(events.len() >= 3, "Got {} events: {:?}", events.len(), events);

        // Check for ConfirmResult with confirmed=true
        let has_confirm_result = events.iter().any(|e| {
            matches!(e, InteractiveEvent::ConfirmResult { confirmed: true, .. })
        });
        assert!(has_confirm_result, "Expected ConfirmResult with confirmed=true");

        // Check for successful Result
        let has_result = events
            .iter()
            .any(|e| matches!(e, InteractiveEvent::Result { success: true, .. }));
        assert!(has_result, "Expected successful Result");
    }

    #[tokio::test]
    async fn test_confirm_demo_declined() {
        let interactive = Interactive::new();
        let (ctx, mut rx) = create_bidirectional_channel();

        // Spawn task to handle the confirm request - respond with false
        let ctx_clone = ctx.clone();
        let responder = tokio::spawn(async move {
            if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
                ctx_clone
                    .handle_response(hub_core::plexus::ClientResponse {
                        request_id,
                        payload: ResponsePayload::Confirmed(false),
                    })
                    .unwrap();
            }
        });

        let stream = interactive.confirm_demo(&ctx, None).await;
        let events: Vec<_> = stream.collect().await;

        responder.await.unwrap();

        // Should emit: StepStarted, ConfirmResult(false), Cancelled
        let has_cancelled = events
            .iter()
            .any(|e| matches!(e, InteractiveEvent::Cancelled { .. }));
        assert!(has_cancelled, "Expected Cancelled event when user declines");

        // Should NOT have successful Result
        let has_result = events
            .iter()
            .any(|e| matches!(e, InteractiveEvent::Result { success: true, .. }));
        assert!(!has_result, "Should not have successful Result when declined");
    }

    #[tokio::test]
    async fn test_prompt_demo_with_response() {
        let interactive = Interactive::new();
        let (ctx, mut rx) = create_bidirectional_channel();

        let ctx_clone = ctx.clone();
        let responder = tokio::spawn(async move {
            if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
                ctx_clone
                    .handle_response(hub_core::plexus::ClientResponse {
                        request_id,
                        payload: ResponsePayload::Text("Alice".into()),
                    })
                    .unwrap();
            }
        });

        let stream = interactive.prompt_demo(&ctx).await;
        let events: Vec<_> = stream.collect().await;

        responder.await.unwrap();

        // Check for PromptResult with the answer
        let has_prompt_result = events.iter().any(|e| {
            matches!(e, InteractiveEvent::PromptResult { answer, .. } if answer == "Alice")
        });
        assert!(has_prompt_result, "Expected PromptResult with answer 'Alice'");

        // Check for Result containing the name
        let has_result = events.iter().any(|e| {
            matches!(e, InteractiveEvent::Result { summary, .. } if summary.contains("Alice"))
        });
        assert!(has_result, "Expected Result mentioning Alice");
    }

    #[tokio::test]
    async fn test_fallback_demo_unidirectional() {
        let interactive = Interactive::new();
        let (ctx, _rx) = create_unidirectional_channel();

        let stream = interactive.fallback_demo(&ctx).await;
        let events: Vec<_> = stream.collect().await;

        // Should complete successfully with fallback values
        // - auto_confirm should return true
        // - default should return "anonymous"

        let has_confirm = events.iter().any(|e| {
            matches!(e, InteractiveEvent::ConfirmResult { confirmed: true, .. })
        });
        assert!(has_confirm, "Expected ConfirmResult with auto-confirmed true");

        let has_prompt = events.iter().any(|e| {
            matches!(e, InteractiveEvent::PromptResult { answer, .. } if answer == "anonymous")
        });
        assert!(has_prompt, "Expected PromptResult with fallback 'anonymous'");

        let has_result = events.iter().any(|e| {
            matches!(e, InteractiveEvent::Result { success: true, data, .. }
                if data.get("name").and_then(|v| v.as_str()) == Some("anonymous"))
        });
        assert!(has_result, "Expected successful Result with name='anonymous'");
    }

    #[tokio::test]
    async fn test_multi_step_demo_completes() {
        let interactive = Interactive::new();
        let (ctx, _rx) = create_unidirectional_channel();

        // With 3 steps and auto_confirm, should complete all steps
        let stream = interactive.multi_step_demo(&ctx, Some(3)).await;
        let events: Vec<_> = stream.collect().await;

        // Should have step started events for each step
        let step_count = events
            .iter()
            .filter(|e| matches!(e, InteractiveEvent::StepStarted { .. }))
            .count();
        assert_eq!(step_count, 3, "Expected 3 StepStarted events");

        // Should have successful completion
        let has_result = events.iter().any(|e| {
            matches!(e, InteractiveEvent::Result { success: true, data, .. }
                if data.get("steps_completed").and_then(|v| v.as_u64()) == Some(3))
        });
        assert!(has_result, "Expected successful Result with 3 steps completed");
    }
}

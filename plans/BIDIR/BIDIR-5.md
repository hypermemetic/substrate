# BIDIR-5: Helper Functions and Patterns

**Status:** Planning
**Blocked By:** BIDIR-2
**Unlocks:** BIDIR-6, BIDIR-7

## Scope

Create helper functions and common patterns for bidirectional communication that make it easy to build interactive activations.

## Acceptance Criteria

- [ ] Convenience macros for common request patterns
- [ ] Error handling helpers
- [ ] Timeout configuration utilities
- [ ] Fallback patterns for non-bidirectional transports

## Implementation Notes

### Convenience Macros

```rust
// hub-core/src/plexus/bidir_macros.rs

/// Confirm or skip pattern - common for destructive operations
///
/// Usage:
/// ```rust
/// confirm_or_skip!(ctx, "Delete {} files?", count);
/// // If cancelled or not supported, returns early with Cancelled event
/// ```
#[macro_export]
macro_rules! confirm_or_skip {
    ($ctx:expr, $($arg:tt)*) => {{
        let message = format!($($arg)*);
        match $ctx.confirm(&message).await {
            Ok(true) => (), // Continue
            Ok(false) => {
                return; // User said no
            }
            Err(BidirError::NotSupported) => {
                // Non-interactive transport, continue without confirmation
            }
            Err(BidirError::Cancelled) => {
                return; // User cancelled
            }
            Err(e) => {
                tracing::warn!("Confirmation failed: {:?}", e);
                // Continue anyway
            }
        }
    }};
}

/// Confirm or fail pattern - for operations that require explicit consent
///
/// Usage:
/// ```rust
/// confirm_or_fail!(ctx, event_type, "Proceed with irreversible action?");
/// // If not confirmed, yields error event and returns
/// ```
#[macro_export]
macro_rules! confirm_or_fail {
    ($ctx:expr, $event:ty, $($arg:tt)*) => {{
        let message = format!($($arg)*);
        match $ctx.confirm(&message).await {
            Ok(true) => (), // Continue
            Ok(false) | Err(BidirError::Cancelled) => {
                yield <$event>::Cancelled {
                    message: "User declined".into(),
                };
                return;
            }
            Err(BidirError::NotSupported) => {
                yield <$event>::Error {
                    message: "This operation requires interactive confirmation".into(),
                    recoverable: false,
                };
                return;
            }
            Err(e) => {
                yield <$event>::Error {
                    message: format!("Confirmation failed: {:?}", e),
                    recoverable: false,
                };
                return;
            }
        }
    }};
}

/// Prompt with default pattern
///
/// Usage:
/// ```rust
/// let name = prompt_or_default!(ctx, "Enter name", "default_value");
/// ```
#[macro_export]
macro_rules! prompt_or_default {
    ($ctx:expr, $message:expr, $default:expr) => {{
        match $ctx.prompt($message).await {
            Ok(s) if !s.is_empty() => s,
            Ok(_) | Err(BidirError::NotSupported) | Err(BidirError::Timeout) => {
                $default.to_string()
            }
            Err(BidirError::Cancelled) => {
                $default.to_string()
            }
            Err(e) => {
                tracing::warn!("Prompt failed: {:?}", e);
                $default.to_string()
            }
        }
    }};
}
```

### Fallback Patterns

```rust
// hub-core/src/plexus/bidirectional.rs

/// Wrapper that provides fallback values when bidirectional not supported
pub struct BidirWithFallback<'a> {
    ctx: &'a BidirChannel,
    auto_confirm: bool,
    default_text: Option<String>,
}

impl<'a> BidirWithFallback<'a> {
    pub fn new(ctx: &'a BidirChannel) -> Self {
        Self {
            ctx,
            auto_confirm: false,
            default_text: None,
        }
    }

    /// Auto-confirm when bidirectional not supported
    pub fn auto_confirm(mut self) -> Self {
        self.auto_confirm = true;
        self
    }

    /// Use default text when prompt not supported
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_text = Some(default.into());
        self
    }

    /// Confirm with fallback behavior
    pub async fn confirm(&self, message: &str) -> bool {
        match self.ctx.confirm(message).await {
            Ok(v) => v,
            Err(BidirError::NotSupported) => self.auto_confirm,
            Err(_) => false,
        }
    }

    /// Prompt with fallback behavior
    pub async fn prompt(&self, message: &str) -> Option<String> {
        match self.ctx.prompt(message).await {
            Ok(s) => Some(s),
            Err(BidirError::NotSupported) => self.default_text.clone(),
            Err(_) => None,
        }
    }
}

// Usage:
// let bidir = BidirWithFallback::new(ctx).auto_confirm().with_default("default");
// if bidir.confirm("Continue?").await {
//     let name = bidir.prompt("Name?").await.unwrap_or_default();
// }
```

### Progress with Request Pattern

```rust
// hub-core/src/plexus/bidirectional.rs

/// Builder for multi-step interactive workflows
pub struct InteractiveWorkflow<'a, E> {
    ctx: &'a BidirChannel,
    step: usize,
    total_steps: usize,
    _phantom: std::marker::PhantomData<E>,
}

impl<'a, E> InteractiveWorkflow<'a, E>
where
    E: From<WorkflowEvent>,
{
    pub fn new(ctx: &'a BidirChannel, total_steps: usize) -> Self {
        Self {
            ctx,
            step: 0,
            total_steps,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Advance to next step with confirmation
    pub async fn next_step(&mut self, description: &str) -> Result<E, BidirError> {
        self.step += 1;
        let progress = (self.step as f32 / self.total_steps as f32) * 100.0;

        // Emit progress event
        let event = E::from(WorkflowEvent::Progress {
            step: self.step,
            total: self.total_steps,
            description: description.to_string(),
            percentage: progress,
        });

        Ok(event)
    }

    /// Checkpoint that requires confirmation to proceed
    pub async fn checkpoint(&mut self, message: &str) -> Result<bool, BidirError> {
        self.ctx.confirm(message).await
    }

    /// Collect input at a step
    pub async fn collect_input(&mut self, prompt: &str) -> Result<String, BidirError> {
        self.ctx.prompt(prompt).await
    }
}

#[derive(Debug, Clone)]
pub enum WorkflowEvent {
    Progress {
        step: usize,
        total: usize,
        description: String,
        percentage: f32,
    },
    Checkpoint {
        step: usize,
        message: String,
        confirmed: bool,
    },
    Input {
        step: usize,
        prompt: String,
        value: String,
    },
}
```

### Timeout Configuration

```rust
// hub-core/src/plexus/bidirectional.rs

/// Configure timeouts for different request types
pub struct TimeoutConfig {
    pub confirm: Duration,
    pub prompt: Duration,
    pub select: Duration,
    pub custom: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            confirm: Duration::from_secs(30),
            prompt: Duration::from_secs(60),
            select: Duration::from_secs(45),
            custom: Duration::from_secs(120),
        }
    }
}

impl TimeoutConfig {
    pub fn for_request_type(&self, request_type: &RequestType) -> Duration {
        match request_type {
            RequestType::Confirm { .. } => self.confirm,
            RequestType::Prompt { .. } => self.prompt,
            RequestType::Select { .. } => self.select,
            RequestType::Custom { .. } => self.custom,
        }
    }

    /// Short timeouts for quick interactions
    pub fn quick() -> Self {
        Self {
            confirm: Duration::from_secs(10),
            prompt: Duration::from_secs(30),
            select: Duration::from_secs(20),
            custom: Duration::from_secs(60),
        }
    }

    /// Long timeouts for complex decisions
    pub fn patient() -> Self {
        Self {
            confirm: Duration::from_secs(120),
            prompt: Duration::from_secs(300),
            select: Duration::from_secs(180),
            custom: Duration::from_secs(600),
        }
    }
}
```

### Error Event Helpers

```rust
// hub-core/src/plexus/bidirectional.rs

/// Convert BidirError to a user-friendly event
pub fn bidir_error_message(error: &BidirError) -> String {
    match error {
        BidirError::Cancelled => "Operation cancelled by user".into(),
        BidirError::Timeout => "Timed out waiting for response".into(),
        BidirError::NotSupported => "Interactive mode not supported".into(),
        BidirError::TypeMismatch { expected, got } => {
            format!("Expected {} response, got {}", expected, got)
        }
        BidirError::Transport(msg) => format!("Communication error: {}", msg),
    }
}

/// Trait for events that can represent bidir errors
pub trait BidirErrorEvent: Sized {
    fn from_bidir_error(error: BidirError, context: &str) -> Self;
}

// Example implementation:
impl BidirErrorEvent for RepoEvent {
    fn from_bidir_error(error: BidirError, context: &str) -> Self {
        RepoEvent::Error {
            org_name: String::new(),
            repo_name: None,
            message: format!("{}: {}", context, bidir_error_message(&error)),
        }
    }
}
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `hub-core/src/plexus/bidir_macros.rs` | NEW: Convenience macros |
| `hub-core/src/plexus/bidirectional.rs` | Add fallback patterns, workflow builder |
| `hub-core/src/plexus/mod.rs` | Export macros |

## Testing

```rust
#[tokio::test]
async fn test_bidir_with_fallback_auto_confirm() {
    let (tx, _rx) = mpsc::channel(32);
    let ctx = BidirChannel::unidirectional(tx);
    let fallback = BidirWithFallback::new(&ctx).auto_confirm();

    assert!(fallback.confirm("Test?").await);
}

#[tokio::test]
async fn test_bidir_with_fallback_default_text() {
    let (tx, _rx) = mpsc::channel(32);
    let ctx = BidirChannel::unidirectional(tx);
    let fallback = BidirWithFallback::new(&ctx).with_default("fallback");

    assert_eq!(fallback.prompt("Name?").await, Some("fallback".into()));
}

#[test]
fn test_timeout_config() {
    let config = TimeoutConfig::quick();
    assert_eq!(
        config.for_request_type(&RequestType::Confirm { message: "".into(), default: None }),
        Duration::from_secs(10)
    );
}
```

## Notes

- Macros use `$crate` paths for proper module resolution
- Fallback patterns enable graceful degradation
- Workflow builder standardizes multi-step interactions
- All helpers are optional - direct BidirChannel use still works

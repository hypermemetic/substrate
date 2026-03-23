//! Echo activation - demonstrates new activation macro usage
//!
//! This is a minimal example showing how to create an activation using the
//! new `#[activation]` macro. The macro generates:
//!
//! - Activation trait implementation
//! - Method enum with JSON schemas
//! - Automatic dispatch routing
//!
//! Event types are plain domain types (no special traits needed).
//! The macro handles wrapping with `wrap_stream()` at the call site.

use super::types::EchoEvent;
use async_stream::stream;
use futures::Stream;
use std::time::Duration;

// Import the activation macro - use crate::activation since we're inside plexus-substrate
use crate::activation;

// Imports needed for generated RPC code
use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::{proc_macros::rpc, PendingSubscriptionSink};

/// Echo activation - echoes messages back
#[derive(Clone)]
pub struct Echo;

impl Echo {
    pub fn new() -> Self {
        Echo
    }
}

impl Default for Echo {
    fn default() -> Self {
        Self::new()
    }
}

/// New activation macro - much cleaner! No need for #[hub_method] on each method.
/// All public async functions are automatically included as methods.
#[activation(
    namespace = "echo",
    version = "1.0.0",
    description = "Echo messages back - demonstrates plexus-derive usage",
    plexus  // Enable Plexus JSON-RPC transport
)]
impl Echo {
    /// Echo a message back the specified number of times
    async fn echo(
        &self,
        message: String,
        count: u32,
    ) -> impl Stream<Item = EchoEvent> + Send + 'static {
        let count = if count == 0 { 1 } else { count };
        stream! {
            for i in 0..count {
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                yield EchoEvent::Echo {
                    message: message.clone(),
                    count: i + 1,
                };
            }
        }
    }

    /// Echo a message once
    async fn once(&self, message: String) -> impl Stream<Item = EchoEvent> + Send + 'static {
        stream! {
            yield EchoEvent::Echo {
                message,
                count: 1,
            };
        }
    }

    /// Ping — returns a Pong response
    async fn ping(&self) -> impl Stream<Item = EchoEvent> + Send + 'static {
        stream! {
            yield EchoEvent::Pong;
        }
    }
}

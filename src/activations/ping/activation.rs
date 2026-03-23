//! Ping activation - simple test activation using plexus-derive

use super::types::PingEvent;
use async_stream::stream;
use futures::Stream;
use std::time::Duration;

// Import the activation macro
use crate::activation;

// Imports needed for generated RPC code
use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::{proc_macros::rpc, PendingSubscriptionSink};

/// Ping activation - simple RPC test
#[derive(Clone)]
pub struct Ping;

impl Ping {
    pub fn new() -> Self {
        Ping
    }
}

impl Default for Ping {
    fn default() -> Self {
        Self::new()
    }
}

/// Ping activation using the new #[activation] macro
#[activation(
    namespace = "ping",
    version = "1.0.0",
    description = "Simple ping/pong test activation",
    plexus  // Enable Plexus JSON-RPC transport
)]
impl Ping {
    /// Simple ping that returns a pong
    async fn pong(&self, message: String) -> impl Stream<Item = PingEvent> + Send + 'static {
        stream! {
            yield PingEvent::Pong { message };
        }
    }

    /// Echo a message multiple times
    async fn echo(
        &self,
        message: String,
        count: u32,
    ) -> impl Stream<Item = PingEvent> + Send + 'static {
        let count = if count == 0 { 1 } else { count.min(10) }; // Max 10 echoes
        stream! {
            for i in 0..count {
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                yield PingEvent::Echo {
                    message: message.clone(),
                    index: i + 1,
                    total: count,
                };
            }
        }
    }
}

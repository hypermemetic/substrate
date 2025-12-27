use super::methods::HealthMethod;
use super::types::HealthEvent;
use crate::plexus::{wrap_stream, PlexusError, PlexusStream, Activation, PlexusStreamItem, StreamMetadata, PlexusContext};
use async_stream::stream;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use jsonrpsee::core::{server::Methods, SubscriptionResult};
use jsonrpsee::{proc_macros::rpc, PendingSubscriptionSink};
use serde_json::Value;
use std::pin::Pin;
use std::time::Instant;

/// Health RPC interface
#[rpc(server, namespace = "health")]
pub trait HealthRpc {
    /// Check health status (streaming subscription)
    #[subscription(name = "check", unsubscribe = "unsubscribe_check", item = serde_json::Value)]
    async fn check(&self) -> SubscriptionResult;
}

/// Health activation - minimal reference implementation
///
/// This activation demonstrates the caller-wraps architecture.
/// The `check_stream` method returns typed domain events (HealthEvent),
/// and the `call` method wraps them using `wrap_stream`.
#[derive(Clone)]
pub struct Health {
    start_time: Instant,
}

impl Health {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }

    /// Returns typed stream - caller will wrap with metadata
    fn check_stream(
        &self,
    ) -> Pin<Box<dyn Stream<Item = HealthEvent> + Send + 'static>> {
        let uptime = self.start_time.elapsed().as_secs();

        Box::pin(stream! {
            yield HealthEvent::Status {
                status: "healthy".to_string(),
                uptime_seconds: uptime,
                timestamp: chrono::Utc::now().timestamp(),
            };
        })
    }
}

impl Default for Health {
    fn default() -> Self {
        Self::new()
    }
}

/// RPC server implementation
#[async_trait]
impl HealthRpcServer for Health {
    async fn check(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let sink = pending.accept().await?;

        // Get wrapped stream
        let stream = self.check_stream();
        let wrapped = wrap_stream(stream, "health.status", vec!["health".into()]);

        // Forward all items to sink
        tokio::spawn(async move {
            let mut stream = wrapped;
            while let Some(item) = stream.next().await {
                if let Ok(raw_value) = serde_json::value::to_raw_value(&item) {
                    if sink.send(raw_value).await.is_err() {
                        break;
                    }
                }
            }
            // Send done event
            let done = PlexusStreamItem::Done {
                metadata: StreamMetadata::new(vec!["health".into()], PlexusContext::hash()),
            };
            if let Ok(raw_value) = serde_json::value::to_raw_value(&done) {
                let _ = sink.send(raw_value).await;
            }
        });

        Ok(())
    }
}

/// Activation trait implementation - unified interface for Plexus
#[async_trait]
impl Activation for Health {
    type Methods = HealthMethod;

    fn namespace(&self) -> &str {
        "health"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Check hub health and uptime"
    }

    fn methods(&self) -> Vec<&str> {
        vec!["check", "schema"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        match method {
            "schema" => Some("Get this plugin's schema".to_string()),
            _ => HealthMethod::description(method).map(|s| s.to_string()),
        }
    }

    async fn call(&self, method: &str, _params: Value) -> Result<PlexusStream, PlexusError> {
        match method {
            "check" => {
                let stream = self.check_stream();
                Ok(wrap_stream(stream, "health.status", vec!["health".into()]))
            }
            "schema" => {
                let schema = self.plugin_schema();
                Ok(wrap_stream(
                    futures::stream::once(async move { schema }),
                    "health.schema",
                    vec!["health".into()]
                ))
            }
            _ => Err(PlexusError::MethodNotFound {
                activation: "health".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn into_rpc_methods(self) -> Methods {
        // Register RPC subscription methods
        self.into_rpc().into()
    }
}

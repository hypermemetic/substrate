use super::methods::HealthMethod;
use super::types::HealthEvent;
use crate::{
    plexus::{into_plexus_stream, Provenance, PlexusError, PlexusStream, Activation, Schema},
    plugin_system::conversion::{IntoSubscription, SubscriptionResult},
};
use async_stream::stream;
use async_trait::async_trait;
use jsonrpsee::{core::server::Methods, proc_macros::rpc, PendingSubscriptionSink};
use serde_json::Value;
use std::time::Instant;

/// Health plugin RPC interface
#[rpc(server, namespace = "health")]
pub trait HealthRpc {
    /// Check health status (streaming)
    #[subscription(name = "check", unsubscribe = "unsubscribe_check", item = serde_json::Value)]
    async fn check(&self) -> SubscriptionResult;
}

/// Health plugin implementation
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

    /// Internal method that returns tightly-typed stream
    async fn check_stream(
        &self,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = HealthEvent> + Send + 'static>> {
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

#[async_trait]
impl HealthRpcServer for Health {
    async fn check(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.check_stream().await;
        let path = Provenance::root("health");
        stream.into_subscription(pending, path).await
    }
}

impl Default for Health {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin trait implementation - unified interface for hub
#[async_trait]
impl Activation for Health {
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
        vec!["check"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        HealthMethod::description(method).map(|s| s.to_string())
    }

    fn enrich_schema(&self) -> Schema {
        let schema_json = HealthMethod::schema();
        serde_json::from_value(schema_json)
            .expect("CRITICAL: Failed to parse schema - Health activation incorrectly defined")
    }

    async fn call(&self, method: &str, _params: Value) -> Result<PlexusStream, PlexusError> {
        match method {
            "check" => {
                let stream = self.check_stream().await;
                let path = Provenance::root("health");
                Ok(into_plexus_stream(stream, path))
            }
            _ => Err(PlexusError::MethodNotFound {
                activation: "health".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn into_rpc_methods(self) -> Methods {
        self.into_rpc().into()
    }
}

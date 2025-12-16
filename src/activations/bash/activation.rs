use super::executor::BashExecutor;
use super::methods::BashMethod;
use super::types::BashOutput;
use crate::{
    plexus::{into_plexus_stream, Provenance, PlexusError, PlexusStream, Activation, GuidanceSuggestion},
    plugin_system::conversion::{IntoSubscription, SubscriptionResult},
};
use async_trait::async_trait;
use futures::Stream;
use jsonrpsee::{core::server::Methods, proc_macros::rpc, PendingSubscriptionSink};
use serde_json::Value;
use std::pin::Pin;

/// RPC adapter trait - defines the JSON-RPC interface for bash execution
#[rpc(server, namespace = "bash")]
pub trait BashRpc {
    /// Execute a bash command and stream the output
    #[subscription(
        name = "execute",
        unsubscribe = "unsubscribe_execute",
        item = serde_json::Value
    )]
    async fn execute(&self, command: String) -> SubscriptionResult;
}

/// RPC adapter plugin - thin wrapper over BashExecutor core system
#[derive(Clone)]
pub struct Bash {
    executor: BashExecutor,
}

impl Bash {
    pub fn new() -> Self {
        Self {
            executor: BashExecutor::new(),
        }
    }

    /// Thin wrapper method - delegates to core system
    ///
    /// This method exists at the plugin layer to provide a bridge between
    /// the RPC signature (which needs PendingSubscriptionSink) and the
    /// core system method (which returns a pure stream).
    async fn execute_stream(
        &self,
        command: String,
    ) -> Pin<Box<dyn Stream<Item = BashOutput> + Send + 'static>> {
        // Simply delegate to the core system
        self.executor.execute(&command).await
    }
}

/// RPC adapter implementation - bridges core system to RPC
#[async_trait]
impl BashRpcServer for Bash {
    async fn execute(&self, pending: PendingSubscriptionSink, command: String) -> SubscriptionResult {
        // Get the stream from the core system
        let stream = self.execute_stream(command).await;

        // Create plugin path for tracking
        let path = Provenance::root("bash");

        // Convert to subscription using the trait
        stream.into_subscription(pending, path).await
    }
}

impl Default for Bash {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin trait implementation - unified interface for hub
#[async_trait]
impl Activation for Bash {
    type Methods = BashMethod;

    fn namespace(&self) -> &str {
        "bash"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Execute bash commands and stream output"
    }

    fn methods(&self) -> Vec<&str> {
        vec!["execute"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        BashMethod::description(method).map(|s| s.to_string())
    }

    fn custom_guidance(&self, method: &str, error: &PlexusError) -> Option<GuidanceSuggestion> {
        match (method, error) {
            ("execute", PlexusError::InvalidParams(_)) => {
                // Provide helpful example for invalid params
                Some(GuidanceSuggestion::TryMethod {
                    method: "bash_execute".to_string(),
                    example_params: Some(serde_json::json!("echo 'Hello, World!'")),
                })
            }
            _ => None, // Use default guidance for other cases
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        match method {
            "execute" => {
                // Extract command from params
                let command = match params {
                    Value::String(s) => s,
                    Value::Object(map) => map
                        .get("command")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| PlexusError::InvalidParams("missing 'command' field".into()))?
                        .to_string(),
                    Value::Array(arr) => arr
                        .first()
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| PlexusError::InvalidParams("expected command string".into()))?
                        .to_string(),
                    _ => return Err(PlexusError::InvalidParams("expected string or object with 'command'".into())),
                };

                let stream = self.execute_stream(command).await;
                let path = Provenance::root("bash");
                Ok(into_plexus_stream(stream, path))
            }
            _ => Err(PlexusError::MethodNotFound {
                activation: "bash".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn into_rpc_methods(self) -> Methods {
        self.into_rpc().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_custom_guidance() {
        let bash = Bash::new();
        let error = PlexusError::InvalidParams("missing command".to_string());

        // Contract: Should provide custom guidance for execute method
        let guidance = bash.custom_guidance("execute", &error);
        assert!(guidance.is_some(), "Should provide custom guidance for execute");

        match guidance.unwrap() {
            GuidanceSuggestion::TryMethod { method, example_params } => {
                assert_eq!(method, "bash_execute");
                assert!(example_params.is_some(), "Should include example params");
                assert_eq!(
                    example_params.unwrap(),
                    serde_json::json!("echo 'Hello, World!'")
                );
            }
            _ => panic!("Expected TryMethod suggestion"),
        }
    }

    #[test]
    fn test_bash_no_guidance_for_other_errors() {
        let bash = Bash::new();
        let error = PlexusError::ExecutionError("runtime error".to_string());

        // Should return None for errors other than InvalidParams
        let guidance = bash.custom_guidance("execute", &error);
        assert!(guidance.is_none(), "Should not provide guidance for ExecutionError");
    }

    #[test]
    fn test_bash_no_guidance_for_other_methods() {
        let bash = Bash::new();
        let error = PlexusError::InvalidParams("test".to_string());

        // Should return None for methods other than execute
        let guidance = bash.custom_guidance("other", &error);
        assert!(guidance.is_none(), "Should not provide guidance for unknown methods");
    }
}

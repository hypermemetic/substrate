//! DEPRECATED: Stream-based guidance replaces middleware
//!
//! This module is kept for historical reference only. Error guidance is now
//! provided via `PlexusStreamEvent::Guidance` events in error streams.
//!
//! **Migration:** Frontends should handle guidance events instead of parsing
//! JSON-RPC error data. See the frontend migration guide at:
//! `docs/architecture/16680880693241553663_frontend-guidance-migration.md`
//!
//! **Architecture:** See stream-based guidance design at:
//! `docs/architecture/16680881573410764543_guidance-stream-based-errors.md`
//!
//! ---
//!
//! ## Legacy Documentation (RPC middleware for guided error responses)
//!
//! This middleware intercepted JSON-RPC error responses and enriched them
//! with a `try` field containing a suggested next request.

#![allow(dead_code)]

use super::errors::GuidedError;
use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::types::{Id, Request};
use jsonrpsee::MethodResponse;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Activation info needed for generating guided errors
#[derive(Clone, Debug)]
pub struct ActivationRegistry {
    /// List of available activation namespaces
    pub activations: Vec<String>,
}

impl ActivationRegistry {
    pub fn new(activations: Vec<String>) -> Self {
        Self { activations }
    }
}

/// Middleware that enriches error responses with guided `try` suggestions
#[derive(Clone)]
pub struct GuidedErrorMiddleware<S> {
    inner: S,
    registry: Arc<ActivationRegistry>,
}

impl<S> GuidedErrorMiddleware<S> {
    pub fn new(inner: S, registry: Arc<ActivationRegistry>) -> Self {
        Self { inner, registry }
    }
}

impl<'a, S> RpcServiceT<'a> for GuidedErrorMiddleware<S>
where
    S: RpcServiceT<'a> + Send + Sync + Clone + 'static,
{
    type Future = Pin<Box<dyn Future<Output = MethodResponse> + Send + 'a>>;

    fn call(&self, req: Request<'a>) -> Self::Future {
        let inner = self.inner.clone();
        let registry = self.registry.clone();
        let method_name = req.method_name().to_string();
        let req_id = req.id.clone();

        Box::pin(async move {
            tracing::debug!(
                method = %method_name,
                activations = ?registry.activations,
                "GuidedErrorMiddleware: processing request"
            );

            // Check BEFORE calling inner service if this activation exists
            // This catches unknown activations before jsonrpsee returns "Method not found"
            if let Some(guided_error) = check_activation_exists(&method_name, &req_id, &registry) {
                tracing::debug!(
                    method = %method_name,
                    "GuidedErrorMiddleware: returning guided error for unknown activation"
                );
                return guided_error;
            }

            let response = inner.call(req).await;

            tracing::debug!(
                method = %method_name,
                is_error = response.is_error(),
                is_success = response.is_success(),
                "GuidedErrorMiddleware: got response"
            );

            // If this is an error response, try to enrich it (fallback for other errors)
            if response.is_error() {
                tracing::debug!(
                    method = %method_name,
                    "GuidedErrorMiddleware: got error response"
                );
            }

            response
        })
    }
}

/// Check if the activation exists BEFORE calling the method
/// Returns a guided error if the activation namespace is unknown
fn check_activation_exists(method_name: &str, req_id: &Id<'_>, registry: &ActivationRegistry) -> Option<MethodResponse> {
    // Parse method name to check if it's an activation method (namespace_method)
    let parts: Vec<&str> = method_name.splitn(2, '_').collect();

    if parts.len() == 2 {
        let namespace = parts[0];

        // Skip plexus-level methods
        if namespace == "plexus" {
            return None;
        }

        // Check if activation exists
        if !registry.activations.iter().any(|a| a == namespace) {
            // Activation not found - return guided error
            let error = GuidedError::activation_not_found(
                namespace,
                registry.activations.clone(),
            );
            tracing::debug!(
                namespace = %namespace,
                available = ?registry.activations,
                "Activation not found, returning guided error"
            );
            return Some(MethodResponse::error(req_id.clone(), error));
        }
    }

    None
}

/// Try to enrich an error response based on the method name (legacy, kept for reference)
#[allow(dead_code)]
fn enrich_error(method_name: &str, req_id: &Id<'_>, registry: &ActivationRegistry) -> Option<MethodResponse> {
    // Parse method name to check if it's an activation method (namespace_method)
    let parts: Vec<&str> = method_name.splitn(2, '_').collect();

    if parts.len() == 2 {
        let namespace = parts[0];
        let _method = parts[1];

        // Check if activation exists
        if !registry.activations.iter().any(|a| a == namespace) {
            // Activation not found - return guided error
            let error = GuidedError::activation_not_found(
                namespace,
                registry.activations.clone(),
            );
            return Some(MethodResponse::error(req_id.clone(), error));
        }
    }

    None
}

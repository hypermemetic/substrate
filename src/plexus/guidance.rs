//! Stream-based error guidance system
//!
//! This module provides the guidance system that helps users resolve errors by
//! including helpful context (available methods, schemas, suggestions) as stream events.

use super::{
    path::Provenance,
    plexus::{PlexusError, PlexusStream},
    schema::Schema,
    types::{GuidanceErrorType, GuidanceSuggestion, PlexusStreamItem},
};
use std::sync::Arc;

/// Trait for activations to provide custom guidance
pub trait CustomGuidance {
    fn custom_guidance(&self, _method: &str, _error: &PlexusError) -> Option<GuidanceSuggestion> {
        None
    }
}

/// Minimal interface needed from ActivationObject for guidance
pub trait ActivationGuidanceInfo: CustomGuidance {
    fn methods(&self) -> Vec<&str>;
    fn schema(&self) -> Schema;
}

/// Create an error stream with guidance
///
/// Returns a stream with three items: Guidance → Error → Done
pub fn error_stream_with_guidance<T: ActivationGuidanceInfo + ?Sized>(
    plexus_hash: String,
    provenance: Provenance,
    error: PlexusError,
    activation_info: Option<&Arc<T>>,
    attempted_method: Option<&str>,
) -> PlexusStream {
    use futures::stream;

    let (error_type, available_methods, method_schema, mut suggestion) = match &error {
        PlexusError::ActivationNotFound(name) => {
            let error_type = GuidanceErrorType::ActivationNotFound {
                activation: name.clone(),
            };
            let suggestion = GuidanceSuggestion::CallPlexusSchema;
            (error_type, None, None, suggestion)
        }

        PlexusError::MethodNotFound { activation, method } => {
            let available: Option<Vec<String>> = activation_info
                .map(|a| a.methods().iter().map(|s| s.to_string()).collect());

            let suggestion = if let Some(ref methods) = available {
                if let Some(first_method) = methods.first() {
                    GuidanceSuggestion::TryMethod {
                        method: format!("{}_{}", activation, first_method),
                        example_params: None,
                    }
                } else {
                    GuidanceSuggestion::CallActivationSchema {
                        namespace: activation.clone(),
                    }
                }
            } else {
                GuidanceSuggestion::CallActivationSchema {
                    namespace: activation.clone(),
                }
            };

            let error_type = GuidanceErrorType::MethodNotFound {
                activation: activation.clone(),
                method: method.clone(),
            };

            (error_type, available, None, suggestion)
        }

        PlexusError::InvalidParams(msg) => {
            // Try to get method schema if we have activation info
            let method_schema = activation_info
                .and_then(|a| attempted_method.and_then(|m| a.schema().get_method_schema(m)));

            let error_type = GuidanceErrorType::InvalidParams {
                method: attempted_method.unwrap_or("unknown").to_string(),
                reason: msg.clone(),
            };

            // Get namespace from provenance for suggestion
            let namespace = provenance
                .segments()
                .get(1)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let suggestion = GuidanceSuggestion::CallActivationSchema { namespace };

            (error_type, None, method_schema, suggestion)
        }

        PlexusError::ExecutionError(_) => {
            // No guidance for runtime errors - just return simple error stream
            let error_item = PlexusStreamItem::error(
                plexus_hash.clone(),
                provenance.clone(),
                error.to_string(),
                false,
            );
            let done_item = PlexusStreamItem::done(plexus_hash, provenance);
            return Box::pin(stream::iter(vec![error_item, done_item]));
        }
    };

    // Check for custom guidance from activation (Contract 3)
    if let Some(activation_obj) = activation_info {
        if let Some(custom) = attempted_method.and_then(|m| activation_obj.custom_guidance(m, &error)) {
            suggestion = custom;
        }
    }

    // Build guidance stream
    let guidance_item = PlexusStreamItem::guidance(
        plexus_hash.clone(),
        provenance.clone(),
        error_type,
        available_methods,
        method_schema,
        suggestion,
    );

    let error_item = PlexusStreamItem::error(
        plexus_hash.clone(),
        provenance.clone(),
        error.to_string(),
        false,
    );

    let done_item = PlexusStreamItem::done(plexus_hash, provenance);

    Box::pin(stream::iter(vec![guidance_item, error_item, done_item]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_error_stream_helper_contract() {
        let hash = "test123".to_string();
        let prov = Provenance::root("test");
        let error = PlexusError::ActivationNotFound("foo".to_string());

        let stream =
            error_stream_with_guidance::<dyn ActivationGuidanceInfo>(hash, prov, error, None, None);
        let items: Vec<_> = stream.collect().await;

        // Contract: Must yield 3 items (Guidance, Error, Done)
        assert_eq!(items.len(), 3, "Should yield Guidance → Error → Done");

        // Verify first item is Guidance
        assert!(
            matches!(
                &items[0].event,
                super::super::types::PlexusStreamEvent::Guidance { .. }
            ),
            "First event should be Guidance"
        );

        // Verify second item is Error
        assert!(
            matches!(
                &items[1].event,
                super::super::types::PlexusStreamEvent::Error { .. }
            ),
            "Second event should be Error"
        );

        // Verify third item is Done
        assert!(
            matches!(
                &items[2].event,
                super::super::types::PlexusStreamEvent::Done { .. }
            ),
            "Third event should be Done"
        );
    }

    #[tokio::test]
    async fn test_execution_error_no_guidance() {
        let hash = "test123".to_string();
        let prov = Provenance::root("test");
        let error = PlexusError::ExecutionError("runtime error".to_string());

        let stream =
            error_stream_with_guidance::<dyn ActivationGuidanceInfo>(hash, prov, error, None, None);
        let items: Vec<_> = stream.collect().await;

        // ExecutionError returns only Error → Done (no Guidance)
        assert_eq!(items.len(), 2, "ExecutionError should yield Error → Done");

        // Verify first item is Error
        assert!(matches!(
            &items[0].event,
            super::super::types::PlexusStreamEvent::Error { .. }
        ));

        // Verify second item is Done
        assert!(matches!(
            &items[1].event,
            super::super::types::PlexusStreamEvent::Done { .. }
        ));
    }
}

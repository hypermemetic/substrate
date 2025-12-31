//! Streaming helpers for the caller-wraps architecture
//!
//! These functions are used by the Plexus routing layer to wrap activation
//! responses with metadata. Activations return typed domain events, and
//! the caller uses these helpers to create PlexusStreamItems.

use futures::stream::{self, Stream, StreamExt};
use serde::Serialize;
use std::pin::Pin;

use super::context::PlexusContext;
use super::types::{PlexusStreamItem, StreamMetadata};

/// Type alias for boxed stream of PlexusStreamItem
pub type PlexusStream = Pin<Box<dyn Stream<Item = PlexusStreamItem> + Send>>;

/// Wrap a typed stream into PlexusStream with automatic Done event
///
/// This is the core helper for the caller-wraps architecture.
/// Activations return typed domain events (e.g., HealthEvent),
/// and the caller wraps them with metadata. A Done event is
/// automatically appended when the stream completes.
///
/// # Example
///
/// ```ignore
/// let stream = health.check();  // Returns Stream<Item = HealthEvent>
/// let wrapped = wrap_stream(stream, "health.status", vec!["health".into()]);
/// // Stream will emit: Data, Data, ..., Done
/// ```
pub fn wrap_stream<T: Serialize + Send + 'static>(
    stream: impl Stream<Item = T> + Send + 'static,
    content_type: &'static str,
    provenance: Vec<String>,
) -> PlexusStream {
    let plexus_hash = PlexusContext::hash();
    let metadata = StreamMetadata::new(provenance.clone(), plexus_hash.clone());
    let done_metadata = StreamMetadata::new(provenance, plexus_hash);

    let data_stream = stream.map(move |item| PlexusStreamItem::Data {
        metadata: metadata.clone(),
        content_type: content_type.to_string(),
        content: serde_json::to_value(item).expect("serialization failed"),
    });

    let done_stream = stream::once(async move { PlexusStreamItem::Done {
        metadata: done_metadata,
    }});

    Box::pin(data_stream.chain(done_stream))
}

/// Wrap a typed stream and append a Done event
///
/// Alias for `wrap_stream` - both now include automatic Done events.
#[deprecated(since = "0.2.0", note = "Use wrap_stream instead, which now includes Done")]
pub fn wrap_stream_with_done<T: Serialize + Send + 'static>(
    stream: impl Stream<Item = T> + Send + 'static,
    content_type: &'static str,
    provenance: Vec<String>,
) -> PlexusStream {
    wrap_stream(stream, content_type, provenance)
}

/// Create an error stream
///
/// Returns a single-item stream containing an error event.
pub fn error_stream(
    message: String,
    provenance: Vec<String>,
    recoverable: bool,
) -> PlexusStream {
    let metadata = StreamMetadata::new(provenance, PlexusContext::hash());

    Box::pin(stream::once(async move {
        PlexusStreamItem::Error {
            metadata,
            message,
            code: None,
            recoverable,
        }
    }))
}

/// Create an error stream with error code
///
/// Returns a single-item stream containing an error event with a code.
pub fn error_stream_with_code(
    message: String,
    code: String,
    provenance: Vec<String>,
    recoverable: bool,
) -> PlexusStream {
    let metadata = StreamMetadata::new(provenance, PlexusContext::hash());

    Box::pin(stream::once(async move {
        PlexusStreamItem::Error {
            metadata,
            message,
            code: Some(code),
            recoverable,
        }
    }))
}

/// Create a done stream
///
/// Returns a single-item stream containing a done event.
pub fn done_stream(provenance: Vec<String>) -> PlexusStream {
    let metadata = StreamMetadata::new(provenance, PlexusContext::hash());

    Box::pin(stream::once(async move {
        PlexusStreamItem::Done { metadata }
    }))
}

/// Create a progress stream
///
/// Returns a single-item stream containing a progress event.
pub fn progress_stream(
    message: String,
    percentage: Option<f32>,
    provenance: Vec<String>,
) -> PlexusStream {
    let metadata = StreamMetadata::new(provenance, PlexusContext::hash());

    Box::pin(stream::once(async move {
        PlexusStreamItem::Progress {
            metadata,
            message,
            percentage,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestEvent {
        value: i32,
    }

    #[tokio::test]
    async fn test_wrap_stream() {
        let events = vec![TestEvent { value: 1 }, TestEvent { value: 2 }];
        let input_stream = stream::iter(events);

        let wrapped = wrap_stream(input_stream, "test.event", vec!["test".into()]);
        let items: Vec<_> = wrapped.collect().await;

        // 2 data items + 1 done
        assert_eq!(items.len(), 3);

        // Check first item
        match &items[0] {
            PlexusStreamItem::Data {
                content_type,
                content,
                metadata,
            } => {
                assert_eq!(content_type, "test.event");
                assert_eq!(content["value"], 1);
                assert_eq!(metadata.provenance, vec!["test"]);
            }
            _ => panic!("Expected Data item"),
        }

        // Check done at end
        assert!(matches!(items[2], PlexusStreamItem::Done { .. }));
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_wrap_stream_with_done() {
        let events = vec![TestEvent { value: 1 }];
        let input_stream = stream::iter(events);

        let wrapped = wrap_stream_with_done(input_stream, "test.event", vec!["test".into()]);
        let items: Vec<_> = wrapped.collect().await;

        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], PlexusStreamItem::Data { .. }));
        assert!(matches!(items[1], PlexusStreamItem::Done { .. }));
    }

    #[tokio::test]
    async fn test_error_stream() {
        let stream = error_stream("Something failed".into(), vec!["test".into()], false);
        let items: Vec<_> = stream.collect().await;

        assert_eq!(items.len(), 1);
        match &items[0] {
            PlexusStreamItem::Error {
                message,
                recoverable,
                code,
                ..
            } => {
                assert_eq!(message, "Something failed");
                assert!(!recoverable);
                assert!(code.is_none());
            }
            _ => panic!("Expected Error item"),
        }
    }

    #[tokio::test]
    async fn test_error_stream_with_code() {
        let stream = error_stream_with_code(
            "Not found".into(),
            "NOT_FOUND".into(),
            vec!["test".into()],
            true,
        );
        let items: Vec<_> = stream.collect().await;

        assert_eq!(items.len(), 1);
        match &items[0] {
            PlexusStreamItem::Error {
                message,
                code,
                recoverable,
                ..
            } => {
                assert_eq!(message, "Not found");
                assert_eq!(code.as_deref(), Some("NOT_FOUND"));
                assert!(recoverable);
            }
            _ => panic!("Expected Error item"),
        }
    }
}

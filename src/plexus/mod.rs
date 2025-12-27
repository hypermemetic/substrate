// Guidance system removed during caller-wraps streaming architecture refactor
// pub mod guidance;

pub mod context;
pub mod errors;
pub mod method_enum;
pub mod middleware;
pub mod path;
pub mod plexus;
pub mod schema;
pub mod streaming;
pub mod types;

pub use context::PlexusContext;
#[deprecated(note = "Use GuidanceErrorType and GuidanceSuggestion from stream events instead")]
pub use errors::{GuidedError, GuidedErrorData, TryRequest};
#[deprecated(note = "Middleware removed - guidance provided via PlexusStreamEvent::Guidance")]
pub use middleware::{ActivationRegistry, GuidedErrorMiddleware};
pub use path::Provenance;
pub use plexus::{Activation, ActivationInfo, ChildRouter, Plexus, PlexusError, route_to_child};
#[allow(deprecated)]
pub use plexus::ActivationFullSchema;
pub use crate::types::Handle;
pub use schema::{ChildSummary, MethodSchema, PluginSchema, Schema, SchemaProperty, SchemaType};
pub use types::{PlexusStreamItem, StreamMetadata};
pub use method_enum::MethodEnumSchema;
pub use streaming::{PlexusStream, wrap_stream, wrap_stream_with_done, error_stream, done_stream, progress_stream};
pub use plexus::PlexusMethod;

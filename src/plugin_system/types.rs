use crate::plexus::{Provenance, PlexusStreamItem};
use serde::Serialize;

/// Trait that all behavior stream items must implement
pub trait ActivationStreamItem: Serialize + Send + 'static {
    /// The content type identifier for this behavior's events (e.g., "bash.event")
    fn content_type() -> &'static str
    where
        Self: Sized,
    {
        std::any::type_name::<Self>()
    }

    /// Convert to body's unified type
    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem;

    /// Whether this event indicates the stream should end
    fn is_terminal(&self) -> bool {
        false
    }
}

/// Wrapper for behavior-specific stream items with provenance tracking
#[derive(Debug, Clone, Serialize)]
pub struct TrackedItem<T> {
    #[serde(skip)]
    pub provenance: Provenance,
    #[serde(flatten)]
    pub item: T,
}

impl<T> ActivationStreamItem for TrackedItem<T>
where
    T: Serialize + Send + 'static,
{
    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem {
        PlexusStreamItem::Data {
            provenance,
            content_type: std::any::type_name::<T>().to_string(),
            data: serde_json::to_value(self.item).unwrap(),
        }
    }
}

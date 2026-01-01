/// Identifier types for the Cone plugin
///
/// Provides flexible identification of cones by name or UUID.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifier for a cone - either by name or UUID
///
/// CLI usage: Just pass the name or UUID directly (e.g., "my-assistant" or "550e8400-...")
/// The CLI/API will handle the conversion to the appropriate lookup type.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConeIdentifier {
    /// Lookup cone by its human-readable name
    ByName {
        /// Cone name (supports partial matching, e.g., "assistant" or "assistant#550e")
        name: String,
    },
    /// Lookup cone by its UUID
    ById {
        /// Cone UUID
        id: Uuid,
    },
}

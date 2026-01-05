//! Mustache activation types

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Information about a registered template
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TemplateInfo {
    /// Unique template ID
    pub id: String,
    /// Plugin that owns this template
    pub plugin_id: Uuid,
    /// Method this template is for
    pub method: String,
    /// Template name (e.g., "default", "compact", "verbose")
    pub name: String,
    /// When the template was created (Unix timestamp)
    pub created_at: i64,
    /// When the template was last updated (Unix timestamp)
    pub updated_at: i64,
}

/// Error type for Mustache operations
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum MustacheError {
    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Render error: {0}")]
    RenderError(String),

    #[error("Invalid template: {0}")]
    InvalidTemplate(String),
}

impl From<String> for MustacheError {
    fn from(s: String) -> Self {
        MustacheError::StorageError(s)
    }
}

impl From<&str> for MustacheError {
    fn from(s: &str) -> Self {
        MustacheError::StorageError(s.to_string())
    }
}

/// Events from mustache operations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MustacheEvent {
    /// Template rendered successfully
    Rendered {
        /// The rendered output
        output: String,
    },

    /// Template registered successfully
    Registered {
        /// Template info
        template: TemplateInfo,
    },

    /// Template retrieved
    Template {
        /// The template content
        template: String,
    },

    /// Template not found
    NotFound {
        /// Description of what was not found
        message: String,
    },

    /// List of templates
    Templates {
        /// The templates
        templates: Vec<TemplateInfo>,
    },

    /// Template deleted
    Deleted {
        /// Number of templates deleted
        count: usize,
    },

    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

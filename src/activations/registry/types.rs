use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Source of a backend registration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BackendSource {
    /// Automatically registered (self-registration at startup)
    Auto,
    /// Loaded from config file
    File,
    /// Manually registered via RPC
    Manual,
    /// Loaded from environment variable
    Env,
}

impl BackendSource {
    pub fn as_str(&self) -> &str {
        match self {
            BackendSource::Auto => "auto",
            BackendSource::File => "file",
            BackendSource::Manual => "manual",
            BackendSource::Env => "env",
        }
    }
}

/// Backend connection information
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackendInfo {
    /// Unique identifier (UUID)
    pub id: String,
    /// Human-readable name (unique)
    pub name: String,
    /// Host address
    pub host: String,
    /// Port number
    pub port: u16,
    /// Protocol (ws or wss)
    pub protocol: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional routing namespace
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Plexus version (from hash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// JSON metadata (extensibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    /// Source of registration
    pub source: BackendSource,
    /// Whether this backend is active
    pub is_active: bool,
    /// Timestamp when registered (Unix seconds)
    pub registered_at: i64,
    /// Timestamp when last seen (Unix seconds, for health checks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<i64>,
    /// Timestamp when created (Unix seconds)
    pub created_at: i64,
    /// Timestamp when last updated (Unix seconds)
    pub updated_at: i64,
}

impl BackendInfo {
    /// Build WebSocket URL from backend info
    pub fn url(&self) -> String {
        format!("{}://{}:{}", self.protocol, self.host, self.port)
    }
}

/// Events emitted by registry methods
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum RegistryEvent {
    /// Backend was registered
    #[serde(rename = "backend_registered")]
    BackendRegistered { backend: BackendInfo },

    /// Backend was updated
    #[serde(rename = "backend_updated")]
    BackendUpdated { backend: BackendInfo },

    /// Backend was deleted
    #[serde(rename = "backend_deleted")]
    BackendDeleted { name: String },

    /// List of backends
    #[serde(rename = "backends")]
    Backends { backends: Vec<BackendInfo> },

    /// Single backend info
    #[serde(rename = "backend")]
    Backend { backend: BackendInfo },

    /// Ping response
    #[serde(rename = "ping")]
    Ping { name: String, success: bool, message: String },

    /// Config reloaded
    #[serde(rename = "reloaded")]
    Reloaded { count: usize },

    /// Error occurred
    #[serde(rename = "error")]
    Error { message: String },
}

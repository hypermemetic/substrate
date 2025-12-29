//! Substrate-level core types
//!
//! These types are shared across all activations and the plexus layer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Handle pointing to external data with versioning
///
/// Display format: `{plugin_id}@{version}::{method}:meta[0]:meta[1]:...`
///
/// Examples:
/// - `550e8400-e29b-41d4-a716-446655440000@1.0.0::chat:msg-123:user:bob`
/// - `123e4567-e89b-12d3-a456-426614174000@1.0.0::execute:cmd-789`
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Handle {
    /// Stable plugin instance identifier (UUID)
    pub plugin_id: Uuid,

    /// Plugin version (semantic version: "MAJOR.MINOR.PATCH")
    /// Used for schema/type lookup
    pub version: String,

    /// Creation method that produced this handle (e.g., "chat", "execute")
    pub method: String,

    /// Metadata parts - variable length list of strings
    /// For messages: typically [message_uuid, role, optional_extra...]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta: Vec<String>,

    /// Legacy plugin name for backwards compatibility
    /// Used during migration period for name-based resolution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

impl Handle {
    /// Create a new handle with plugin UUID
    pub fn new(plugin_id: Uuid, version: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            plugin_id,
            version: version.into(),
            method: method.into(),
            meta: Vec::new(),
            plugin_name: None,
        }
    }

    /// Create a handle from a plugin name (for backwards compatibility)
    /// The plugin_id is generated deterministically from name@version
    pub fn from_name(name: impl Into<String>, version: impl Into<String>, method: impl Into<String>) -> Self {
        let name = name.into();
        let version = version.into();
        let plugin_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("{}@{}", name, version).as_bytes());
        Self {
            plugin_id,
            version,
            method: method.into(),
            meta: Vec::new(),
            plugin_name: Some(name),
        }
    }

    /// Add metadata to the handle
    pub fn with_meta(mut self, meta: Vec<String>) -> Self {
        self.meta = meta;
        self
    }

    /// Add a single metadata item
    pub fn push_meta(mut self, item: impl Into<String>) -> Self {
        self.meta.push(item.into());
        self
    }

    /// Set the legacy plugin name
    pub fn with_plugin_name(mut self, name: impl Into<String>) -> Self {
        self.plugin_name = Some(name.into());
        self
    }

    /// Get the origin for this handle (for provenance tracking)
    pub fn origin(&self) -> Origin {
        Origin {
            plugin_id: self.plugin_id,
            method: self.method.clone(),
        }
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format: {plugin_id}@{version}::{method}:meta[0]:meta[1]:...
        write!(f, "{}@{}::{}", self.plugin_id, self.version, self.method)?;
        for m in &self.meta {
            write!(f, ":{}", m)?;
        }
        Ok(())
    }
}

impl FromStr for Handle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Parse: {plugin_id}@{version}::{method}:meta[0]:meta[1]:...
        // Also supports legacy format: {plugin_name}@{version}::...

        // Split on @ to get plugin identifier and rest
        let (plugin_part, rest) = s.split_once('@')
            .ok_or_else(|| format!("Invalid handle format, missing '@': {}", s))?;

        // Split on :: to get version and method+meta
        let (version, method_and_meta) = rest.split_once("::")
            .ok_or_else(|| format!("Invalid handle format, missing '::': {}", s))?;

        // Split method and meta on :
        let mut parts = method_and_meta.split(':');
        let method = parts.next()
            .ok_or_else(|| format!("Invalid handle format, missing method: {}", s))?;

        let meta: Vec<String> = parts.map(|s| s.to_string()).collect();

        // Try to parse as UUID first, fall back to name-based
        let (plugin_id, plugin_name) = if let Ok(uuid) = plugin_part.parse::<Uuid>() {
            (uuid, None)
        } else {
            // Legacy name-based format - generate deterministic UUID
            let uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("{}@{}", plugin_part, version).as_bytes());
            (uuid, Some(plugin_part.to_string()))
        };

        Ok(Handle {
            plugin_id,
            version: version.to_string(),
            method: method.to_string(),
            meta,
            plugin_name,
        })
    }
}

// ============================================================================
// Envelope and Origin - Provenance Tracking
// ============================================================================

/// Origin of a value - tracks which plugin/method created it
///
/// Used for:
/// - Finding the right template for rendering
/// - Tracing where values came from
/// - Routing back to the source plugin
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Origin {
    /// Plugin that created/owns this value
    pub plugin_id: Uuid,

    /// Method that produced the value (for schema/template lookup)
    pub method: String,
}

impl Origin {
    /// Create a new origin
    pub fn new(plugin_id: Uuid, method: impl Into<String>) -> Self {
        Self {
            plugin_id,
            method: method.into(),
        }
    }
}

/// Envelope wrapping a value with its provenance
///
/// Values flowing through the system carry their origin for:
/// - Rendering: find the right template based on origin
/// - Debugging: trace where values came from
/// - Routing: know which plugin to call for further operations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Envelope<T> {
    /// Where this value came from
    pub origin: Origin,

    /// The actual data
    pub data: T,
}

impl<T> Envelope<T> {
    /// Create a new envelope
    pub fn new(origin: Origin, data: T) -> Self {
        Self { origin, data }
    }

    /// Create envelope from a handle (uses handle's origin)
    pub fn from_handle(handle: &Handle, data: T) -> Self {
        Self {
            origin: handle.origin(),
            data,
        }
    }

    /// Map the data while preserving origin
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Envelope<U> {
        Envelope {
            origin: self.origin,
            data: f(self.data),
        }
    }

    /// Get reference to the data
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get reference to the origin
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Unwrap the envelope, returning the data
    pub fn into_inner(self) -> T {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Handle tests - updated for UUID-based handles
    // ========================================================================

    #[test]
    fn test_handle_display() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let handle = Handle::new(uuid, "1.0.0", "chat")
            .push_meta("msg-123")
            .push_meta("user");
        assert_eq!(handle.to_string(), "550e8400-e29b-41d4-a716-446655440000@1.0.0::chat:msg-123:user");
    }

    #[test]
    fn test_handle_parse_uuid() {
        let handle: Handle = "550e8400-e29b-41d4-a716-446655440000@1.0.0::chat:msg-123:user".parse().unwrap();
        assert_eq!(handle.plugin_id, Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        assert_eq!(handle.version, "1.0.0");
        assert_eq!(handle.method, "chat");
        assert_eq!(handle.meta, vec!["msg-123", "user"]);
        assert!(handle.plugin_name.is_none());
    }

    #[test]
    fn test_handle_parse_legacy_name() {
        // Legacy format with plugin name instead of UUID
        let handle: Handle = "cone@1.0.0::chat:msg-123".parse().unwrap();

        // Should generate deterministic UUID
        let expected_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, b"cone@1.0.0");
        assert_eq!(handle.plugin_id, expected_uuid);
        assert_eq!(handle.plugin_name, Some("cone".to_string()));
        assert_eq!(handle.method, "chat");
    }

    #[test]
    fn test_handle_from_name() {
        let handle = Handle::from_name("cone", "1.0.0", "chat");

        let expected_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, b"cone@1.0.0");
        assert_eq!(handle.plugin_id, expected_uuid);
        assert_eq!(handle.plugin_name, Some("cone".to_string()));
    }

    #[test]
    fn test_handle_parse_no_meta() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let handle: Handle = format!("{}@1.0.0::execute", uuid).parse().unwrap();
        assert_eq!(handle.plugin_id, uuid);
        assert_eq!(handle.method, "execute");
        assert!(handle.meta.is_empty());
    }

    // ========================================================================
    // INVARIANT: Handle roundtrip - parse(display(h)) == h (for UUID handles)
    // ========================================================================

    #[test]
    fn invariant_handle_roundtrip_with_meta() {
        let uuid = Uuid::new_v4();
        let original = Handle::new(uuid, "1.0.0", "chat")
            .with_meta(vec!["msg-550e8400".into(), "user".into(), "bob".into()]);

        let serialized = original.to_string();
        let parsed: Handle = serialized.parse().unwrap();

        assert_eq!(original.plugin_id, parsed.plugin_id);
        assert_eq!(original.version, parsed.version);
        assert_eq!(original.method, parsed.method);
        assert_eq!(original.meta, parsed.meta);
    }

    #[test]
    fn invariant_handle_roundtrip_no_meta() {
        let uuid = Uuid::new_v4();
        let original = Handle::new(uuid, "1.0.0", "execute");

        let serialized = original.to_string();
        let parsed: Handle = serialized.parse().unwrap();

        assert_eq!(original.plugin_id, parsed.plugin_id);
        assert_eq!(original.method, parsed.method);
    }

    // ========================================================================
    // INVARIANT: Handle equality
    // ========================================================================

    #[test]
    fn invariant_handle_equality_reflexive() {
        let uuid = Uuid::new_v4();
        let h = Handle::new(uuid, "1.0.0", "chat").push_meta("msg-1");
        assert_eq!(h, h.clone());
    }

    #[test]
    fn invariant_handle_equality_symmetric() {
        let uuid = Uuid::new_v4();
        let h1 = Handle::new(uuid, "1.0.0", "chat").push_meta("msg-1");
        let h2 = Handle::new(uuid, "1.0.0", "chat").push_meta("msg-1");
        assert_eq!(h1, h2);
        assert_eq!(h2, h1);
    }

    #[test]
    fn invariant_handle_inequality_different_plugin_id() {
        let h1 = Handle::new(Uuid::new_v4(), "1.0.0", "chat");
        let h2 = Handle::new(Uuid::new_v4(), "1.0.0", "chat");
        assert_ne!(h1, h2);
    }

    #[test]
    fn invariant_handle_inequality_different_version() {
        let uuid = Uuid::new_v4();
        let h1 = Handle::new(uuid, "1.0.0", "chat");
        let h2 = Handle::new(uuid, "2.0.0", "chat");
        assert_ne!(h1, h2);
    }

    #[test]
    fn invariant_handle_inequality_different_method() {
        let uuid = Uuid::new_v4();
        let h1 = Handle::new(uuid, "1.0.0", "chat");
        let h2 = Handle::new(uuid, "1.0.0", "create");
        assert_ne!(h1, h2);
    }

    // ========================================================================
    // INVARIANT: Parse error cases
    // ========================================================================

    #[test]
    fn invariant_parse_error_missing_at() {
        let result: Result<Handle, _> = "550e8400-e29b-41d4-a716-446655440000-1.0.0::chat".parse();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing '@'"));
    }

    #[test]
    fn invariant_parse_error_missing_double_colon() {
        let result: Result<Handle, _> = "550e8400-e29b-41d4-a716-446655440000@1.0.0:chat".parse();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing '::'"));
    }

    // ========================================================================
    // INVARIANT: JSON serialization roundtrip
    // ========================================================================

    #[test]
    fn invariant_json_roundtrip() {
        let uuid = Uuid::new_v4();
        let original = Handle::new(uuid, "1.0.0", "chat")
            .with_meta(vec!["msg-123".into(), "user".into()]);

        let json = serde_json::to_string(&original).unwrap();
        let parsed: Handle = serde_json::from_str(&json).unwrap();

        assert_eq!(original, parsed);
    }

    #[test]
    fn invariant_json_empty_meta_omitted() {
        let uuid = Uuid::new_v4();
        let handle = Handle::new(uuid, "1.0.0", "execute");
        let json = serde_json::to_string(&handle).unwrap();

        // Empty meta should be skipped in serialization
        assert!(!json.contains("\"meta\""), "empty meta should be omitted: {}", json);
    }

    // ========================================================================
    // Origin tests
    // ========================================================================

    #[test]
    fn test_origin_from_handle() {
        let uuid = Uuid::new_v4();
        let handle = Handle::new(uuid, "1.0.0", "chat");
        let origin = handle.origin();

        assert_eq!(origin.plugin_id, uuid);
        assert_eq!(origin.method, "chat");
    }

    // ========================================================================
    // Envelope tests
    // ========================================================================

    #[test]
    fn test_envelope_creation() {
        let uuid = Uuid::new_v4();
        let origin = Origin::new(uuid, "chat");
        let envelope = Envelope::new(origin.clone(), "hello world");

        assert_eq!(envelope.origin().plugin_id, uuid);
        assert_eq!(envelope.origin().method, "chat");
        assert_eq!(*envelope.data(), "hello world");
    }

    #[test]
    fn test_envelope_from_handle() {
        let uuid = Uuid::new_v4();
        let handle = Handle::new(uuid, "1.0.0", "execute");
        let envelope = Envelope::from_handle(&handle, 42);

        assert_eq!(envelope.origin().plugin_id, uuid);
        assert_eq!(envelope.origin().method, "execute");
        assert_eq!(*envelope.data(), 42);
    }

    #[test]
    fn test_envelope_map() {
        let uuid = Uuid::new_v4();
        let origin = Origin::new(uuid, "chat");
        let envelope = Envelope::new(origin, 10);

        let mapped = envelope.map(|x| x * 2);

        assert_eq!(mapped.origin().plugin_id, uuid);
        assert_eq!(*mapped.data(), 20);
    }

    #[test]
    fn test_envelope_into_inner() {
        let uuid = Uuid::new_v4();
        let origin = Origin::new(uuid, "chat");
        let envelope = Envelope::new(origin, vec![1, 2, 3]);

        let data = envelope.into_inner();
        assert_eq!(data, vec![1, 2, 3]);
    }
}

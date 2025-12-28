//! Substrate-level core types
//!
//! These types are shared across all activations and the plexus layer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Handle pointing to external data with versioning
///
/// Display format: `plugin@version::method:meta[0]:meta[1]:...`
///
/// Examples:
/// - `cone@1.0.0::chat:msg-123:user:bob`
/// - `claudecode@1.0.0::chat:msg-456:assistant`
/// - `bash@1.0.0::execute:cmd-789`
/// - `cone@1.0.0::chat` (empty meta)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Handle {
    /// Plugin identifier (e.g., "cone", "claudecode", "bash")
    pub plugin: String,

    /// Plugin version (semantic version: "MAJOR.MINOR.PATCH")
    pub version: String,

    /// Creation method that produced this handle (e.g., "chat", "execute")
    pub method: String,

    /// Metadata parts - variable length list of strings
    /// For messages: typically [message_uuid, role, optional_extra...]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta: Vec<String>,
}

impl Handle {
    /// Create a new handle
    pub fn new(plugin: impl Into<String>, version: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            plugin: plugin.into(),
            version: version.into(),
            method: method.into(),
            meta: Vec::new(),
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
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format: plugin@version::method:meta[0]:meta[1]:...
        write!(f, "{}@{}::{}", self.plugin, self.version, self.method)?;
        for m in &self.meta {
            write!(f, ":{}", m)?;
        }
        Ok(())
    }
}

impl FromStr for Handle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Parse: plugin@version::method:meta[0]:meta[1]:...

        // Split on @ to get plugin and rest
        let (plugin, rest) = s.split_once('@')
            .ok_or_else(|| format!("Invalid handle format, missing '@': {}", s))?;

        // Split on :: to get version and method+meta
        let (version, method_and_meta) = rest.split_once("::")
            .ok_or_else(|| format!("Invalid handle format, missing '::': {}", s))?;

        // Split method and meta on :
        let mut parts = method_and_meta.split(':');
        let method = parts.next()
            .ok_or_else(|| format!("Invalid handle format, missing method: {}", s))?;

        let meta: Vec<String> = parts.map(|s| s.to_string()).collect();

        Ok(Handle {
            plugin: plugin.to_string(),
            version: version.to_string(),
            method: method.to_string(),
            meta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_display() {
        let handle = Handle::new("cone", "1.0.0", "chat")
            .push_meta("msg-123")
            .push_meta("user");
        assert_eq!(handle.to_string(), "cone@1.0.0::chat:msg-123:user");
    }

    #[test]
    fn test_handle_parse() {
        let handle: Handle = "cone@1.0.0::chat:msg-123:user".parse().unwrap();
        assert_eq!(handle.plugin, "cone");
        assert_eq!(handle.version, "1.0.0");
        assert_eq!(handle.method, "chat");
        assert_eq!(handle.meta, vec!["msg-123", "user"]);
    }

    #[test]
    fn test_handle_parse_no_meta() {
        let handle: Handle = "bash@1.0.0::execute".parse().unwrap();
        assert_eq!(handle.plugin, "bash");
        assert_eq!(handle.method, "execute");
        assert!(handle.meta.is_empty());
    }

    // ========================================================================
    // INVARIANT: Handle roundtrip - parse(display(h)) == h
    // ========================================================================

    #[test]
    fn invariant_handle_roundtrip_with_meta() {
        let original = Handle::new("cone", "1.0.0", "chat")
            .with_meta(vec!["msg-550e8400".into(), "user".into(), "bob".into()]);

        let serialized = original.to_string();
        let parsed: Handle = serialized.parse().unwrap();

        assert_eq!(original, parsed, "roundtrip must preserve handle exactly");
    }

    #[test]
    fn invariant_handle_roundtrip_no_meta() {
        let original = Handle::new("bash", "1.0.0", "execute");

        let serialized = original.to_string();
        let parsed: Handle = serialized.parse().unwrap();

        assert_eq!(original, parsed, "roundtrip must preserve handle with empty meta");
    }

    #[test]
    fn invariant_handle_roundtrip_single_meta() {
        let original = Handle::new("arbor", "2.0.0", "tree_get")
            .push_meta("tree-123");

        let serialized = original.to_string();
        let parsed: Handle = serialized.parse().unwrap();

        assert_eq!(original, parsed);
    }

    // ========================================================================
    // INVARIANT: Handle equality - same components = equal handles
    // ========================================================================

    #[test]
    fn invariant_handle_equality_reflexive() {
        let h = Handle::new("cone", "1.0.0", "chat").push_meta("msg-1");
        assert_eq!(h, h.clone());
    }

    #[test]
    fn invariant_handle_equality_symmetric() {
        let h1 = Handle::new("cone", "1.0.0", "chat").push_meta("msg-1");
        let h2 = Handle::new("cone", "1.0.0", "chat").push_meta("msg-1");
        assert_eq!(h1, h2);
        assert_eq!(h2, h1);
    }

    #[test]
    fn invariant_handle_inequality_different_plugin() {
        let h1 = Handle::new("cone", "1.0.0", "chat");
        let h2 = Handle::new("bash", "1.0.0", "chat");
        assert_ne!(h1, h2);
    }

    #[test]
    fn invariant_handle_inequality_different_version() {
        let h1 = Handle::new("cone", "1.0.0", "chat");
        let h2 = Handle::new("cone", "2.0.0", "chat");
        assert_ne!(h1, h2);
    }

    #[test]
    fn invariant_handle_inequality_different_method() {
        let h1 = Handle::new("cone", "1.0.0", "chat");
        let h2 = Handle::new("cone", "1.0.0", "create");
        assert_ne!(h1, h2);
    }

    #[test]
    fn invariant_handle_inequality_different_meta() {
        let h1 = Handle::new("cone", "1.0.0", "chat").push_meta("msg-1");
        let h2 = Handle::new("cone", "1.0.0", "chat").push_meta("msg-2");
        assert_ne!(h1, h2);
    }

    #[test]
    fn invariant_handle_inequality_meta_vs_no_meta() {
        let h1 = Handle::new("cone", "1.0.0", "chat");
        let h2 = Handle::new("cone", "1.0.0", "chat").push_meta("msg-1");
        assert_ne!(h1, h2);
    }

    // ========================================================================
    // INVARIANT: Parse error cases - invalid formats fail cleanly
    // ========================================================================

    #[test]
    fn invariant_parse_error_missing_at() {
        let result: Result<Handle, _> = "cone1.0.0::chat".parse();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing '@'"));
    }

    #[test]
    fn invariant_parse_error_missing_double_colon() {
        let result: Result<Handle, _> = "cone@1.0.0:chat".parse();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing '::'"));
    }

    #[test]
    fn invariant_parse_error_empty_string() {
        let result: Result<Handle, _> = "".parse();
        assert!(result.is_err());
    }

    // ========================================================================
    // INVARIANT: Meta field handling - order preserved, content exact
    // ========================================================================

    #[test]
    fn invariant_meta_order_preserved() {
        let handle = Handle::new("cone", "1.0.0", "chat")
            .push_meta("first")
            .push_meta("second")
            .push_meta("third");

        assert_eq!(handle.meta[0], "first");
        assert_eq!(handle.meta[1], "second");
        assert_eq!(handle.meta[2], "third");
    }

    #[test]
    fn invariant_meta_empty_string_allowed() {
        let handle = Handle::new("test", "1.0.0", "method")
            .push_meta("")
            .push_meta("nonempty");

        let serialized = handle.to_string();
        let parsed: Handle = serialized.parse().unwrap();

        assert_eq!(parsed.meta.len(), 2);
        assert_eq!(parsed.meta[0], "");
        assert_eq!(parsed.meta[1], "nonempty");
    }

    #[test]
    fn invariant_meta_with_meta_replaces() {
        let handle = Handle::new("test", "1.0.0", "method")
            .push_meta("will-be-replaced")
            .with_meta(vec!["new1".into(), "new2".into()]);

        assert_eq!(handle.meta, vec!["new1", "new2"]);
    }

    // ========================================================================
    // INVARIANT: JSON serialization roundtrip
    // ========================================================================

    #[test]
    fn invariant_json_roundtrip() {
        let original = Handle::new("cone", "1.0.0", "chat")
            .with_meta(vec!["msg-123".into(), "user".into()]);

        let json = serde_json::to_string(&original).unwrap();
        let parsed: Handle = serde_json::from_str(&json).unwrap();

        assert_eq!(original, parsed);
    }

    #[test]
    fn invariant_json_empty_meta_omitted() {
        let handle = Handle::new("bash", "1.0.0", "execute");
        let json = serde_json::to_string(&handle).unwrap();

        // Empty meta should be skipped in serialization
        assert!(!json.contains("meta"), "empty meta should be omitted: {}", json);
    }

    #[test]
    fn invariant_json_deserialize_missing_meta() {
        // JSON without meta field should deserialize to empty meta
        let json = r#"{"plugin":"bash","version":"1.0.0","method":"execute"}"#;
        let handle: Handle = serde_json::from_str(json).unwrap();

        assert!(handle.meta.is_empty());
    }
}

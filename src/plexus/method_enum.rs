//! Method enum schema trait for activations
//!
//! Provides a trait that method enums can implement to support schema generation
//! with const discriminators for the method field.

use schemars::JsonSchema;
use serde_json::Value;

/// Trait for method enums that can generate schema with const discriminators
pub trait MethodEnumSchema: JsonSchema {
    /// Get all method names as static strings
    fn method_names() -> &'static [&'static str];

    /// Generate schema with const values for method discriminators
    ///
    /// This takes the base schemars schema and transforms it so that
    /// each variant's "method" field has a `const` value instead of
    /// just `type: string`.
    fn schema_with_consts() -> Value;
}

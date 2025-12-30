mod activation;
mod methods;
mod storage;
mod types;

pub use activation::{Cone, ConeMethod};
pub use methods::ConeIdentifier;
pub use storage::{ConeStorage, ConeStorageConfig};
pub use types::{
    ConeConfig, ConeError, ConeEvent, ConeId, ConeInfo, ChatUsage,
    Message, MessageId, MessageRole, Position,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that ConeEvent::Registry variant has proper schema with all fields.
    /// cllient now uses schemars 1.x so RegistryExport derives JsonSchema directly.
    #[test]
    fn test_cone_registry_schema_has_all_fields() {
        let schema = schemars::schema_for!(ConeEvent);
        let schema_value = serde_json::to_value(&schema).unwrap();

        // Find the registry variant
        let one_of = schema_value.get("oneOf").and_then(|v| v.as_array()).unwrap();
        let registry_variant = one_of.iter().find(|v| {
            v.get("properties")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.get("const"))
                .and_then(|c| c.as_str())
                == Some("registry")
        }).expect("Should have registry variant");

        // With schemars 1.x, the Registry variant references RegistryExport via $ref
        // Check that the variant has the expected structure (type field + reference to RegistryExport)
        let properties = registry_variant.get("properties").unwrap();
        assert!(properties.get("type").is_some(), "Should have type discriminant");

        // The RegistryExport fields should be in $defs and referenced
        let defs = schema_value.get("$defs").expect("Should have $defs");
        let registry_export = defs.get("RegistryExport").expect("Should have RegistryExport in $defs");
        let registry_props = registry_export.get("properties").unwrap();

        assert!(registry_props.get("families").is_some(), "RegistryExport should have families field");
        assert!(registry_props.get("models").is_some(), "RegistryExport should have models field");
        assert!(registry_props.get("services").is_some(), "RegistryExport should have services field");
        assert!(registry_props.get("stats").is_some(), "RegistryExport should have stats field");

        // Verify ModelExport is also in defs
        assert!(defs.get("ModelExport").is_some(), "Should have ModelExport in $defs");
    }

    /// Test that the registry method schema is properly filtered to only include
    /// the Registry variant (using returns(Registry) annotation).
    #[test]
    fn test_cone_registry_method_schema_filtered() {
        let method_schemas = ConeMethod::method_schemas();

        let registry = method_schemas.iter()
            .find(|m| m.name == "registry")
            .expect("Should have a registry method");

        let returns = registry.returns.as_ref().expect("Should have returns schema");
        let returns_value = serde_json::to_value(returns).unwrap();

        let one_of = returns_value.get("oneOf").and_then(|v| v.as_array()).unwrap();

        // Registry method should only return Registry variant (filtered from 12+ variants)
        assert_eq!(one_of.len(), 1, "Registry method should only return 1 variant (Registry)");

        let variant_name = one_of[0]
            .get("properties")
            .and_then(|p| p.get("type"))
            .and_then(|t| t.get("const"))
            .and_then(|c| c.as_str());
        assert_eq!(variant_name, Some("registry"), "Single variant should be 'registry'");
    }
}

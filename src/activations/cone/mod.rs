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
    // Schema types for RegistryExport (mirrors cllient types with schemars 1.x)
    RegistryExportSchema, ModelExportSchema, ServiceExportSchema,
    RegistryStatsSchema, CapabilitiesSchema, PricingSchema, ConstraintsSchema,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that ConeEvent::Registry variant has proper schema with all fields.
    ///
    /// This was added to fix the schema mismatch where cone.registry was returning
    /// a generic serde_json::Value schema instead of the proper RegistryExportSchema
    /// with families, models, services, and stats fields.
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

        // Check that registry variant has the expected fields
        let properties = registry_variant.get("properties").unwrap();
        assert!(properties.get("families").is_some(), "Should have families field");
        assert!(properties.get("models").is_some(), "Should have models field");
        assert!(properties.get("services").is_some(), "Should have services field");
        assert!(properties.get("stats").is_some(), "Should have stats field");

        // Verify the types are correct
        let models_ref = properties.get("models")
            .and_then(|m| m.get("items"))
            .and_then(|i| i.get("$ref"))
            .and_then(|r| r.as_str());
        assert_eq!(models_ref, Some("#/$defs/ModelExportSchema"), "models should reference ModelExportSchema");
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

mod activation;
mod methods;
mod storage;
mod types;

pub use activation::{Cone, ConeMethod};
pub use methods::ConeIdentifier;
pub use storage::{ConeStorage, ConeStorageConfig};
pub use types::{
    // Method-specific return types (preferred)
    ChatEvent, CreateResult, DeleteResult, GetResult, ListResult,
    RegistryResult, ResolveResult, SetHeadResult,
    // Shared types
    ChatUsage, ConeConfig, ConeError, ConeId, ConeInfo,
    Message, MessageId, MessageRole, Position,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that RegistryResult schema has proper structure with RegistryExport fields.
    #[test]
    fn test_registry_result_schema_has_all_fields() {
        let schema = schemars::schema_for!(RegistryResult);
        let schema_value = serde_json::to_value(&schema).unwrap();

        // RegistryResult has only one variant: Registry
        let one_of = schema_value.get("oneOf").and_then(|v| v.as_array()).unwrap();
        assert_eq!(one_of.len(), 1, "RegistryResult should have exactly 1 variant");

        let registry_variant = &one_of[0];
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
    }

    /// Test that each method returns its specific type, not a union of all types.
    #[test]
    fn test_method_specific_return_types() {
        let method_schemas = ConeMethod::method_schemas();

        // create -> CreateResult (2 variants: Created, Error)
        let create = method_schemas.iter().find(|m| m.name == "create").unwrap();
        let create_returns = serde_json::to_value(create.returns.as_ref().unwrap()).unwrap();
        let create_variants = create_returns.get("oneOf").and_then(|v| v.as_array()).unwrap();
        assert_eq!(create_variants.len(), 2, "CreateResult should have 2 variants");

        // list -> ListResult (2 variants: List, Error)
        let list = method_schemas.iter().find(|m| m.name == "list").unwrap();
        let list_returns = serde_json::to_value(list.returns.as_ref().unwrap()).unwrap();
        let list_variants = list_returns.get("oneOf").and_then(|v| v.as_array()).unwrap();
        assert_eq!(list_variants.len(), 2, "ListResult should have 2 variants");

        // chat -> ChatEvent (4 variants: Start, Content, Complete, Error)
        let chat = method_schemas.iter().find(|m| m.name == "chat").unwrap();
        let chat_returns = serde_json::to_value(chat.returns.as_ref().unwrap()).unwrap();
        let chat_variants = chat_returns.get("oneOf").and_then(|v| v.as_array()).unwrap();
        assert_eq!(chat_variants.len(), 4, "ChatEvent should have 4 variants");

        // registry -> RegistryResult (1 variant: Registry)
        let registry = method_schemas.iter().find(|m| m.name == "registry").unwrap();
        let registry_returns = serde_json::to_value(registry.returns.as_ref().unwrap()).unwrap();
        let registry_variants = registry_returns.get("oneOf").and_then(|v| v.as_array()).unwrap();
        assert_eq!(registry_variants.len(), 1, "RegistryResult should have 1 variant");
    }

    #[test]
    fn test_streaming_flag() {
        let method_schemas = ConeMethod::method_schemas();

        // chat is streaming (returns impl Stream<Item = ChatEvent>)
        let chat = method_schemas.iter().find(|m| m.name == "chat").unwrap();
        assert!(chat.streaming, "chat should be streaming");

        // create is NOT streaming (returns impl Stream but only yields one item)
        let create = method_schemas.iter().find(|m| m.name == "create").unwrap();
        assert!(!create.streaming, "create should NOT be streaming");
    }
}

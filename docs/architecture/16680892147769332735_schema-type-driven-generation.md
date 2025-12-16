# Schema Type-Driven Generation

## Summary

Schema generation is now fully automatic via schemars. By using proper types (`uuid::Uuid` instead of `String`) and doc comments, schemars generates complete JSON schemas with format annotations, descriptions, and required arrays. The manual enrichment system has been removed.

## Before: Manual Enrichment

Previously, schema generation required manual "enrichment" to add type information that schemars couldn't derive:

```rust
// OLD: Types were stringly-typed
pub enum ArborMethod {
    TreeGet {
        tree_id: String,  // No format: "uuid"
    }
}

// OLD: Manual enrichment needed for each method
impl ArborMethod {
    pub fn describe_by_name(method_name: &str) -> Option<MethodEnrichment> {
        match method_name {
            "tree_get" => Some(MethodEnrichment {
                method_name: "tree_get".to_string(),
                fields: vec![
                    FieldEnrichment::uuid("tree_id", "UUID of the tree", true),
                ],
            }),
            _ => None,
        }
    }
}

// OLD: enrich_schema() had to iterate and apply enrichments
fn enrich_schema(&self) -> Schema {
    let mut schema = ArborMethod::schema();
    for variant in schema.one_of.iter_mut() {
        if let Some(enrichment) = ArborMethod::describe_by_name(variant.method_name()) {
            variant.apply_enrichments(&enrichment);
        }
    }
    schema
}
```

This was error-prone: strings were duplicated, enrichments could get out of sync with actual types.

## After: Type-Driven Generation

Now we use proper types and schemars features:

```rust
// NEW: Use uuid::Uuid directly
pub enum ArborMethod {
    /// Get a complete tree with all nodes
    TreeGet {
        /// UUID of the tree to retrieve
        tree_id: Uuid,  // schemars adds format: "uuid" automatically
    }
}

// NEW: enrich_schema() just returns the schema
fn enrich_schema(&self) -> Schema {
    let schema_json = ArborMethod::schema();
    serde_json::from_value(schema_json).expect("...")
}
```

## What schemars Provides Automatically

With `uuid::Uuid` type and doc comments:

| Feature | Source | Example |
|---------|--------|---------|
| `format: "uuid"` | `uuid::Uuid` type | `"format": "uuid"` |
| `description` | Doc comments | `/// UUID of the tree` |
| `required` array | Non-Option fields | `"required": ["tree_id"]` |
| `type: ["string", "null"]` | `Option<T>` fields | Nullable types |

## Cargo.toml Change

Enable the `uuid1` feature for schemars:

```toml
schemars = { version = "1.0.4", features = ["derive", "uuid1"] }
```

## Generated Schema Example

For `node_create_text`:

```json
{
  "properties": {
    "method": { "const": "node_create_text" },
    "params": {
      "type": "object",
      "properties": {
        "tree_id": {
          "type": "string",
          "format": "uuid",
          "description": "UUID of the tree"
        },
        "parent": {
          "type": ["string", "null"],
          "format": "uuid",
          "description": "Parent node ID (None for root-level)"
        },
        "content": {
          "type": "string",
          "description": "Text content for the node"
        }
      },
      "required": ["tree_id", "content"]
    }
  }
}
```

## Removed Code

The following types/functions were removed as they're no longer needed:

- `FieldEnrichment` - format/description/required was specified here
- `MethodEnrichment` - collected field enrichments per method
- `Describe` trait - for providing enrichment data
- `SchemaVariant` - for applying enrichments to schema
- `describe_by_name()` - string-based enrichment lookup
- Complex `enrich_schema()` implementations with iteration

## Activations Updated

Both activations now use proper types:

### Arbor (`src/activations/arbor/methods.rs`)
- All `tree_id` fields: `String` → `Uuid`
- All `node_id` fields: `String` → `Uuid`
- All `parent` fields: `Option<String>` → `Option<Uuid>`

### Cone (`src/activations/cone/methods.rs`)
- `cone_id` fields: `String` → `Uuid`
- `node_id` field in SetHead: `String` → `Uuid`

## Benefits

1. **Single source of truth** - Types define the schema, no duplication
2. **Compile-time safety** - Wrong UUID type usage is a compile error
3. **Less code** - ~200 lines of enrichment code removed
4. **Automatic maintenance** - Adding a UUID field "just works"
5. **Consistent** - All activations use the same pattern

## Related

- Previous enrichment docs: `16680896323511347711_schema-enrichment-and-cache-invalidation.md`
- Self-documenting RPC: `16680998353176467711_self-documenting-rpc.md`

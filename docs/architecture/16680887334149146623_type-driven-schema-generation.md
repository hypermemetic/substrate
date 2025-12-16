# Type-Driven Schema Generation via Wrapper Pattern

## Context

Previously, activations manually generated their schemas using an "enrichment" pattern. Each activation implemented a `schema()` method that would:
1. Call `schemars::schema_for!(MethodEnum)` to get base schema
2. Manually "enrich" the schema with additional metadata (descriptions, UUID formats, required fields)

This approach had several problems:
- **String-based lookups**: `describe_by_name()` converted types to strings too early
- **Manual duplication**: Information already in type signatures had to be manually repeated in enrichment code
- **Maintenance burden**: Each new method required manual schema enrichment
- **Inconsistency risk**: Easy to forget to enrich fields or keep enrichment in sync with types

## Solution: Associated Types + Wrapper Pattern

### Type-Driven Schema Generation

Instead of manual enrichment, we now use proper types that schemars understands:
- `uuid::Uuid` instead of `String` → automatic `format: "uuid"` annotation
- Doc comments → automatic descriptions
- Non-`Option<T>` fields → automatic required array
- `#[serde(tag = "method", content = "params")]` → proper oneOf structure

Example:
```rust
#[derive(JsonSchema, Serialize)]
#[serde(tag = "method", content = "params")]
pub enum ArborMethod {
    /// Create a new text node
    #[serde(rename = "node_create_text")]
    NodeCreateText {
        /// The tree to add the node to
        tree_id: uuid::Uuid,  // Auto-generates format: "uuid"
        /// The text content
        content: String,      // Auto-required (not Option)
        metadata: Option<serde_json::Value>,  // Auto-optional
    },
}
```

Schemars generates the full correct schema automatically - no manual enrichment needed.

### Associated Type Pattern

Each activation declares its `Methods` type as an associated type:

```rust
trait InnerActivation {
    type Methods: JsonSchema + Serialize;
    // ... other methods
}

impl InnerActivation for Arbor {
    type Methods = ArborMethod;  // Compiler guarantees this type exists
    // No schema() method needed!
}
```

Benefits:
- Type safety: Compiler ensures `Methods` type exists and implements required traits
- No manual schema generation: Schema is auto-derived from the type
- Single source of truth: Method definitions are the schema

### Wrapper Pattern for Trait Objects

**Problem**: Rust trait objects (`dyn Trait`) cannot have associated types that vary per implementation.

```rust
// This doesn't work:
trait Activation {
    type Methods: JsonSchema;
}
let activations: HashMap<String, Arc<dyn Activation>> = ...;
// Error: "the value of the associated type `Methods` must be specified"
```

**Solution**: Split into inner trait (with associated type) and wrapper (trait-object-safe):

```rust
// Inner trait: has associated type, NOT trait-object-safe
trait InnerActivation {
    type Methods: JsonSchema + Serialize;
    fn namespace(&self) -> &str;
    async fn call(&self, method: &str, params: Value) -> Result<...>;
    // Note: NO schema() method here
}

// Wrapper: generic over InnerActivation, implements trait-object-safe Activation
struct ActivationWrapper<A: InnerActivation> {
    inner: A,
}

impl<A: InnerActivation> Activation for ActivationWrapper<A> {
    fn schema(&self) -> Schema {
        // Can access A::Methods here!
        let schema = schemars::schema_for!(A::Methods);
        serde_json::from_value(serde_json::to_value(schema).unwrap()).unwrap()
    }

    fn namespace(&self) -> &str { self.inner.namespace() }
    async fn call(&self, method: &str, params: Value) -> ... {
        self.inner.call(method, params).await
    }
}
```

**How registration works**:

```rust
impl Plexus {
    pub fn register<A: InnerActivation>(mut self, activation: A) -> Self {
        let namespace = activation.namespace().to_string();

        // Clone for RPC (consumes activation)
        let activation_for_rpc = activation.clone();

        // Wrap for storage (type-erases to dyn Activation)
        let wrapped = ActivationWrapper::new(activation);

        // Store as trait object
        self.activations.insert(namespace, Arc::new(wrapped));

        // Queue RPC conversion
        self.pending_rpc.push(Box::new(move ||
            activation_for_rpc.into_rpc_methods()
        ));

        self
    }
}
```

The wrapper:
1. Captures the concrete type `A` and its `Methods` associated type
2. Auto-generates schema from `A::Methods`
3. Delegates all other methods to `inner`
4. Type-erases to `Arc<dyn Activation>` for storage

## Benefits

### Before (Manual Enrichment)
```rust
impl Activation for Arbor {
    fn schema(&self) -> Schema {
        let mut schema = schemars::schema_for!(ArborMethod);

        // Manual enrichment for each method
        for variant in &mut schema.one_of {
            if let Some(method_name) = variant.get_method_name() {
                // Add description
                variant.description = ArborMethod::description(method_name);

                // Add UUID format annotations
                if let Some(params) = &mut variant.params {
                    for field in params.fields() {
                        if is_uuid_field(field.name) {
                            field.format = Some("uuid");
                        }
                    }
                }

                // Build required array
                let required: Vec<_> = params.fields()
                    .filter(|f| !f.is_optional)
                    .map(|f| f.name.clone())
                    .collect();
                params.required = required;
            }
        }

        Schema::from_root_schema(schema)
    }
}
```

### After (Type-Driven)
```rust
impl InnerActivation for Arbor {
    type Methods = ArborMethod;
    // Schema auto-generated by wrapper!
}
```

The schema generation happens automatically in `ActivationWrapper<Arbor>` by calling `schemars::schema_for!(ArborMethod)` - no manual code needed.

### Key Improvements

1. **Less Code**: Removed ~200+ lines of manual enrichment across all activations
2. **Single Source of Truth**: Type definitions ARE the schema
3. **Impossible to Forget**: Can't add a method without adding it to the enum
4. **Type Safety**: Compiler enforces schema-serializable Methods type
5. **No String Lookups**: No `describe_by_name()` or similar string-based reflection
6. **Maintainability**: New methods get correct schemas automatically

## Implementation Checklist

- [x] Add `uuid1` feature to schemars in Cargo.toml
- [x] Create `InnerActivation` trait with `type Methods` associated type
- [x] Create `ActivationWrapper<A>` struct
- [x] Implement `Activation` for `ActivationWrapper<A>` with auto schema generation
- [x] Update `Plexus::register()` to wrap `InnerActivation` implementations
- [x] Convert all activations to implement `InnerActivation`
- [x] Update method enums to use `uuid::Uuid` instead of `String`
- [x] Remove manual `schema()` implementations from all activations
- [x] Remove enrichment system (FieldEnrichment, MethodEnrichment, Describe trait)
- [x] All tests pass

## Files Modified

- `src/plexus/plexus.rs` - Added InnerActivation trait and ActivationWrapper
- `src/plexus/mod.rs` - Export InnerActivation
- `src/activations/arbor/activation.rs` - Changed to InnerActivation, removed schema()
- `src/activations/arbor/methods.rs` - Use uuid::Uuid type
- `src/activations/cone/activation.rs` - Changed to InnerActivation, removed schema()
- `src/activations/cone/methods.rs` - Use uuid::Uuid type
- `src/activations/bash/activation.rs` - Changed to InnerActivation, removed schema()
- `src/activations/health/activation.rs` - Changed to InnerActivation, removed schema()
- `Cargo.toml` - Added `uuid1` feature to schemars dependency

## Trade-offs

**Pros**:
- Much less code to maintain
- Impossible to have schema/type mismatches
- Compiler-enforced correctness
- New methods get correct schemas for free

**Cons**:
- Slightly more complex trait setup (InnerActivation + Wrapper pattern)
- Wrapper indirection adds one level of delegation
- Requires understanding of associated types and type erasure

The trade-off is strongly in favor of this approach - the complexity is centralized in one place (the wrapper), while making all activation code simpler and more maintainable.

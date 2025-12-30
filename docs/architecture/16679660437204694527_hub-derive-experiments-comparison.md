# Hub Derive Experiments: Comparison and Analysis

## Overview

Three experimental derive macros were built to explore different approaches for auto-generating Activation plugins. Each takes a fundamentally different approach to the same problem: reducing boilerplate when creating hub-core Activations.

| Experiment | Approach | Source of Truth | Completeness |
|------------|----------|-----------------|--------------|
| `hub-derive-generic` | Wrap struct as single-method plugin | The struct itself | Minimal |
| `hub-derive-crud` | Generate methods from field annotations | Field types + attributes | Full Activation |
| `hub-derive-methods` | Expose existing impl methods | Your impl block | Full Activation |

## The Three Approaches

### 1. hub-derive-generic: Struct → Plugin Wrapper

**Philosophy**: "I have data, wrap it minimally"

```rust
#[derive(HubStruct)]
#[hub_struct(namespace = "config")]
pub struct AppConfig { ... }

// Generates: AppConfigPlugin with get() method only
```

**What it generates**:
- Plugin wrapper struct
- Event enum (Data/Error variants)
- Single `get()` method

**What it does NOT generate**:
- Activation trait impl
- Method dispatch
- Multiple methods

**Best for**: Quick prototypes, learning, cases where you'll manually implement Activation anyway.

---

### 2. hub-derive-crud: Fields → Methods

**Philosophy**: "My data structure implies my API"

```rust
#[derive(HubPlugin)]
#[hub_plugin(namespace = "registry")]
pub struct RegistryStore {
    #[hub_field(list)]
    pub families: Vec<Family>,

    #[hub_field(get_by = "id")]
    pub models: HashMap<String, Model>,
}

// Generates: list_families(), get_model(id) + full Activation impl
```

**What it generates**:
- Plugin wrapper struct
- Event enum with typed variants per field
- Method enum for schema
- All CRUD-style methods based on field attributes
- **Complete Activation trait impl**

**What it does NOT generate**:
- Custom business logic
- Write operations (create/update/delete)
- Complex queries

**Best for**: Data-centric services, registries, catalogs, config stores.

---

### 3. hub-derive-methods: Impl → Plugin

**Philosophy**: "I have existing methods, expose them"

```rust
#[hub_methods]
impl Registry {
    pub fn list_models(&self) -> Vec<Model> { ... }
    pub fn complex_query(&self, filter: Filter) -> QueryResult { ... }
}

#[derive(HubPlugin)]
#[hub_plugin(namespace = "registry", delegate = "inner")]
pub struct RegistryPlugin {
    inner: Registry,
}

// Exposes your existing methods via Activation
```

**What it generates**:
- HubDispatch trait impl on your type
- Plugin wrapper with delegation
- **Complete Activation trait impl**

**What it does NOT generate**:
- The actual method implementations (that's your code)
- Typed method enum (placeholder only)

**Best for**: Wrapping existing business logic, complex operations, cases where field-based generation is too limiting.

---

## Comparison Matrix

| Feature | generic | crud | methods |
|---------|---------|------|---------|
| Full Activation impl | No | **Yes** | **Yes** |
| Zero manual impl | No | **Yes** | **Yes** |
| Custom business logic | No | No | **Yes** |
| Typed event enum | Partial | **Yes** | No (JSON) |
| Typed method enum | No | **Yes** | Placeholder |
| Parameter extraction | N/A | **Yes** | **Yes** |
| Multiple methods | No | **Yes** | **Yes** |
| Async support | N/A | Wrapped | Partial |
| Schema generation | No | Basic | Basic |

## Failure Modes and Limitations

### hub-derive-generic

1. **Too minimal**: Only generates `get()`, requires manual Activation impl
2. **No real use case**: Superseded by crud for most scenarios
3. **Incomplete**: Doesn't actually work standalone with Plexus

**Verdict**: Consider deprecating or merging into crud as a degenerate case.

### hub-derive-crud

1. **Read-only despite name**: No create/update/delete operations
2. **Naive singularization**: `families` → `family` works, but `matrices` → `matrice` fails
3. **Union event type**: All methods share one event enum, losing per-method typing
4. **HashMap key assumption**: Assumes keys can be extracted from JSON as strings
5. **No filtering**: Can't do `list_models(family_id: "anthropic")`

**Verdict**: Solid for read-only data services. Needs mutation support and better type handling.

### hub-derive-methods

1. **Placeholder method enum**: The `{Struct}Method` enum is fake, contains `__Placeholder`
2. **Can't introspect other types**: Proc macros can't see method signatures of other types
3. **Async handling incomplete**: Async methods may have lifetime issues
4. **Error handling weak**: Serialization failures become null, not errors
5. **Two-macro dance**: Requires both `#[hub_methods]` AND `#[derive(HubPlugin)]`

**Verdict**: Working prototype but needs refinement. The two-macro approach is awkward.

---

## Architectural Lessons

### 1. Proc Macro Boundaries

Rust proc macros can only see the tokens in their input. They cannot:
- Query types defined elsewhere
- Access trait implementations
- See method signatures of referenced types

This is why `hub-derive-methods` needs `#[hub_methods]` on the impl block itself - the derive macro on the wrapper can't see inside the inner type.

### 2. Generated Type Coordination

All three experiments generate multiple types that must work together:
- Plugin struct
- Event enum
- Method enum
- Activation impl

Getting the relationships right (especially generic bounds and trait requirements) is tricky.

### 3. Schema vs Runtime

There's tension between:
- **Compile-time schema** (method enum with typed variants)
- **Runtime dispatch** (matching on method name strings)

Ideally these would be unified, but proc macros can't generate the const data needed for runtime reflection.

---

## Recommendations

### Short Term

1. **Use hub-derive-crud** for new data-centric services
2. **Use hub-derive-methods** when you have existing impl blocks to expose
3. **Deprecate hub-derive-generic** or make it an alias for crud with no field attributes

### Medium Term

1. Merge learnings from all three into a single `hub-derive` crate
2. Support both field-based and impl-based generation in one macro
3. Add mutation operations to crud (`#[hub_field(create, update, delete)]`)

### Long Term

1. Investigate compile-time reflection (if Rust adds it)
2. Consider code generation instead of proc macros for cross-type awareness
3. Build tooling to validate schemas match runtime behavior

---

## Usage Decision Tree

```
Do you have existing impl methods to expose?
├── Yes → hub-derive-methods
└── No → Is your API defined by your data structure?
    ├── Yes → hub-derive-crud
    └── No → Write manual Activation impl
```

---

## File Locations

- `experiments/hub-derive-generic/` - Minimal wrapper approach
- `experiments/hub-derive-crud/` - Field-based generation
- `experiments/hub-derive-methods/` - Impl-based delegation
- `hub-core/src/plexus/dispatch.rs` - HubDispatch trait (added for methods experiment)

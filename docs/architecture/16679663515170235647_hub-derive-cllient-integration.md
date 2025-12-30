# Hub Derive: cllient Integration Plan

## Goal

Make `cllient::RegistryExport` natively convertible to an Activation plugin with minimal boilerplate:

```rust
#[derive(HubPlugin)]
#[hub_plugin(namespace = "registry", version = "1.0.0")]
pub struct RegistryExport {
    #[hub_field(list)]
    pub services: Vec<ServiceExport>,

    #[hub_field(list)]
    pub families: Vec<String>,

    #[hub_field(list)]
    pub models: Vec<ModelExport>,

    #[hub_field(get)]
    pub stats: RegistryStats,
}

// This should generate a fully working Activation with:
// - list_services(), list_families(), list_models(), get_stats()
// - Full Activation trait impl with call() dispatch
// - RegistryExportMethod enum for schema
// - plugin_schema() impl
```

## Success Criteria

1. cllient's `RegistryExport` can add `#[derive(HubPlugin)]` without any additional manual impl blocks
2. The generated plugin can be registered with `Plexus::with_namespace("registry").register(plugin)`
3. All methods are callable via JSON-RPC (`registry_call`, etc.)
4. Schema is correctly generated and queryable

## Current State

The `hub-derive-crud` macro in `experiments/hub-derive-crud/` currently generates:
- `{Struct}Plugin` wrapper struct
- `{Struct}Event` enum with variants for each field
- Methods like `list_{field}()`, `get_{field}()`, etc.

**Missing pieces:**
1. Auto-generated `{Struct}Method` enum for schema
2. Auto-generated `Activation` trait impl (currently manual in examples)
3. Auto-generated `call()` dispatch logic
4. Auto-generated `plugin_schema()` with proper method schemas
5. `into_rpc_methods()` that returns working jsonrpsee Methods (currently empty)

---

## Subagent Work Packages

### IMPORTANT: Subagent Operational Guidelines

**Context limitations:**
- Subagents cannot see this conversation history
- Subagents cannot ask clarifying questions back to the orchestrator
- Subagents have access to: Glob, Grep, Read, Edit, Write, Bash tools
- Subagents should produce complete, working code - not partial implementations

**Verification approach:**
- DO NOT get stuck waiting for manual testing or user feedback
- Use `cargo check` to verify compilation - this is sufficient for verification
- Use `cargo expand` to inspect generated code if needed
- If cargo check passes, the work package is complete
- Do not attempt to run full integration tests - leave that to orchestrator

**Output requirements:**
- Each subagent MUST end with a summary of files changed
- Each subagent MUST report `cargo check` results
- If cargo check fails, fix the errors before completing

---

### WP-1: Generate Method Enum

**Goal:** Auto-generate `{Struct}Method` enum from field attributes.

**Input example:**
```rust
#[derive(HubPlugin)]
#[hub_plugin(namespace = "registry")]
pub struct RegistryExport {
    #[hub_field(list)]
    pub services: Vec<ServiceExport>,
    #[hub_field(get)]
    pub stats: RegistryStats,
}
```

**Expected output:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum RegistryExportMethod {
    ListServices,
    GetStats,
}

impl MethodEnumSchema for RegistryExportMethod {
    fn method_names() -> &'static [&'static str] {
        &["list_services", "get_stats"]
    }
    fn schema_with_consts() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(RegistryExportMethod)).unwrap()
    }
}
```

**Files to modify:**
- `experiments/hub-derive-crud/src/lib.rs`

**Verification:**
```bash
cd experiments/hub-derive-crud && cargo check
```

---

### WP-2: Generate Activation Trait Impl

**Goal:** Auto-generate the full `Activation` trait impl.

**Expected output (conceptual):**
```rust
#[async_trait]
impl Activation for RegistryExportPlugin {
    type Methods = RegistryExportMethod;

    fn namespace(&self) -> &str { "registry" }
    fn version(&self) -> &str { "1.0.0" }
    fn description(&self) -> &str { "Auto-generated plugin" }

    fn methods(&self) -> Vec<&str> {
        vec!["list_services", "get_stats"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        match method {
            "list_services" => Some("List all services".to_string()),
            "get_stats" => Some("Get stats".to_string()),
            _ => None,
        }
    }

    fn plugin_id(&self) -> uuid::Uuid {
        Self::plugin_id()
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        match method {
            "list_services" => {
                let stream = self.list_services();
                Ok(wrap_stream(stream, "registry.list_services", vec!["registry".into()]))
            }
            "get_stats" => {
                let stream = self.get_stats();
                Ok(wrap_stream(stream, "registry.get_stats", vec!["registry".into()]))
            }
            _ => Err(PlexusError::MethodNotFound { ... })
        }
    }

    fn into_rpc_methods(self) -> Methods {
        Methods::new() // Can be empty for now
    }

    fn plugin_schema(&self) -> PluginSchema {
        PluginSchema::leaf(
            self.namespace(),
            self.version(),
            self.description(),
            Self::method_schemas(), // generated
        )
    }
}
```

**Key challenge:** The macro needs to import types from hub-core. Use `crate_path` attribute if needed.

**Files to modify:**
- `experiments/hub-derive-crud/src/lib.rs`

**Verification:**
```bash
cd experiments/hub-derive-crud && cargo check
```

---

### WP-3: Generate Method Schemas

**Goal:** Generate `MethodSchema` for each method with proper parameter types.

For a method like `get_model(id: String)`, generate:
```rust
MethodSchema::new("get_model", "Get model by id", "hash")
    .with_params(schemars::schema_for!(GetModelParams))
    .with_returns(schemars::schema_for!(Option<Model>))
```

**Files to modify:**
- `experiments/hub-derive-crud/src/lib.rs`

**Verification:**
```bash
cd experiments/hub-derive-crud && cargo check
```

---

### WP-4: Integration Example with cllient Types

**Goal:** Create an example that uses cllient-like types (copy the structs, don't depend on cllient).

Create `experiments/hub-derive-crud/examples/cllient_registry.rs` that:
1. Defines `ServiceExport`, `ModelExport`, `RegistryStats`, `RegistryExport` (copied from cllient)
2. Applies `#[derive(HubPlugin)]` to `RegistryExport`
3. Registers with Plexus and starts a server
4. NO manual Activation impl - everything derived

**Verification:**
```bash
cd experiments/hub-derive-crud && cargo build --example cllient_registry
```

---

## Orchestrator Responsibilities

The orchestrator (Claude in the main conversation) will:

1. **Dispatch work packages** to subagents with clear, self-contained instructions
2. **Review subagent output** - check that cargo check passed, review code quality
3. **Integrate changes** - if subagent made partial progress, complete the work
4. **Run integration tests** - actually start servers and query via WebSocket
5. **Iterate** - if a work package needs refinement, re-dispatch with clarifications

## Orchestrator Limitations with Subagents

1. **No back-and-forth:** Subagents complete in one shot. If they get stuck, the orchestrator must re-dispatch with more context.
2. **No shared state:** Each subagent starts fresh. Include all necessary context in the prompt.
3. **Output is text:** Subagents return a summary message. File changes are visible via Read/Glob.
4. **Tool access:** Subagents can read/write files, run bash, but cannot interact with user.
5. **Timeout risk:** Very long tasks may timeout. Break into smaller work packages.

## Execution Order

1. WP-1 (Method Enum) - foundation for schema
2. WP-2 (Activation Impl) - depends on WP-1
3. WP-3 (Method Schemas) - depends on WP-1
4. WP-4 (Integration Example) - depends on all above

WP-2 and WP-3 can potentially run in parallel after WP-1 completes.

## Notes

- The existing `crud_plugin.rs` example can serve as reference
- Hub-core is at `../../hub-core` relative to experiments
- The macro crate cannot depend on hub-core at compile time (proc-macro limitation), but examples can

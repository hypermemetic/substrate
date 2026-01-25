# Direct Method Transport - Removing the Meta-Method Pattern

## TL;DR

**Problem:** The current transport layer has an asymmetry between local and remote calls. Local Rust code calls `plexus.route("solar.help", params)` while remote clients must use a meta-method: `{"method": "plexus.call", "params": {"method": "solar.help", "params": {}}}`.

**Proposal:** Expose all activation methods directly at the JSON-RPC transport layer. Remote clients call `{"method": "plexus.solar.help", "params": {}}`, which internally routes to `hub.route("solar.help", params)`.

**Key Insight:** `.call()` is an implementation detail of the `Activation` trait, not a user-facing API method. The meta-method pattern leaks this implementation detail to clients.

**Status:** Proposal / Design Document

---

## Current Architecture

### Two Registration Mechanisms

The current system has a dual registration approach:

#### 1. Meta-Method Registration (`{namespace}.call`)

**File:** `hub-core/src/plexus/plexus.rs:806-819`

```rust
module.register_subscription(
    "plexus.call",  // ← Meta-method that routes to other methods
    call_method,
    call_unsub,
    move |params, pending, _ctx, _ext| {
        let hub = hub_for_call.clone();
        Box::pin(async move {
            let p: CallParams = params.parse()?;  // {method: string, params?: Value}
            let stream = hub.route(&p.method, p.params.unwrap_or_default()).await
                .map_err(|e| jsonrpsee::types::ErrorObject::owned(-32000, e.to_string(), None::<()>))?;
            pipe_stream_to_subscription(pending, stream).await
        })
    }
)?;

#[derive(Debug, serde::Deserialize)]
struct CallParams {
    method: String,
    #[serde(default)]
    params: Option<Value>,
}
```

This registers a **single** JSON-RPC method that accepts any activation method path and forwards it through the routing system.

#### 2. Direct Method Registration (Per-Activation)

**File:** `hub-core/src/plexus/plexus.rs:875-878`

```rust
// Register pending RPC methods from activations
let pending = std::mem::take(&mut *hub.inner.pending_rpc.lock().unwrap());
for factory in pending {
    module.merge(factory())?;  // ← Each activation registers direct methods
}
```

**File:** `hub-core/src/plexus/plexus.rs:496-497` (called during `.register()`)

```rust
inner.pending_rpc.lock().unwrap()
    .push(Box::new(move || activation_for_rpc.into_rpc_methods()));
```

Each activation implements `into_rpc_methods()` which returns a `jsonrpsee::core::server::Methods` instance containing direct JSON-RPC method registrations.

**Example:** `hub-core/src/activations/health/activation.rs:12-19`

```rust
#[rpc(server, namespace = "health")]
pub trait HealthRpc {
    /// Check health status (streaming subscription)
    #[subscription(name = "check", unsubscribe = "unsubscribe_check", item = serde_json::Value)]
    async fn check(&self) -> SubscriptionResult;
}
```

This generates a `"health.check"` JSON-RPC method directly. For activations registered in Plexus, this becomes **accessible at the activation's namespace level only**, not at the hub level.

### The Asymmetry

**Local Rust calls:**
```rust
// Direct routing
plexus.route("solar.help", params).await

// Or via Activation trait
Activation::call(&solar, "mercury.info", params).await
```

**Remote JSON-RPC calls (current):**
```json
// Via meta-method (wrapping required)
{"method": "plexus.call", "params": {"method": "solar.help", "params": {}}}

// Direct method (only works for non-nested activations)
{"method": "health.check", "params": {}}

// Nested methods (must use meta-method)
{"method": "plexus.call", "params": {"method": "solar.mercury.info", "params": {}}}
```

**Why the asymmetry exists:**

1. **Hub-level routing**: The `DynamicHub` (Plexus) manages routing across activations, but nested activations (like `solar.mercury.info`) aren't known at registration time
2. **Dynamic discovery**: Child routers implement `ChildRouter::get_child()` which is called at runtime, not during RPC registration
3. **Two-layer nesting**: Activations register their own methods (`health.check`), but the hub doesn't re-expose them at the hub namespace (`plexus.health.check`)

### Current Call Flow

```
Remote Client
    │
    ├─ {"method": "plexus.call", "params": {"method": "solar.mercury.info", ...}}
    │
    ▼
DynamicHub::arc_into_rpc_module
    │
    ├─ Registers "plexus.call" meta-method
    │      │
    │      ▼
    │   CallParams { method: "solar.mercury.info", params: {...} }
    │      │
    │      ▼
    │   hub.route("solar.mercury.info", params)
    │      │
    │      ▼
    │   DynamicHub::route()
    │      │
    │      ├─ Split: ("solar", "mercury.info")
    │      │
    │      ▼
    │   activations["solar"].call("mercury.info", params)
    │      │
    │      ▼
    │   Solar::call("mercury.info")
    │      │
    │      ├─ No local match
    │      │
    │      ▼
    │   route_to_child(self, "mercury.info")
    │      │
    │      ├─ Split: ("mercury", "info")
    │      ├─ get_child("mercury") → CelestialBodyActivation
    │      │
    │      ▼
    │   CelestialBodyActivation::router_call("info")
    │
    └─ Returns PlexusStream
```

---

## Proposed Architecture

### Direct Method Registration at Hub Namespace

**Goal:** Every activation method is registered as a direct JSON-RPC method using the fully-qualified path from the hub's perspective.

**Remote JSON-RPC calls (proposed):**
```json
// Hub methods (introspection)
{"method": "plexus.hash", "params": {}}
{"method": "plexus.schema", "params": {}}

// Top-level activation methods
{"method": "plexus.health.check", "params": {}}
{"method": "plexus.echo.once", "params": {"message": "hello"}}

// Nested activation methods (1 level)
{"method": "plexus.solar.observe", "params": {}}
{"method": "plexus.solar.mercury.info", "params": {}}

// Nested activation methods (2 levels)
{"method": "plexus.solar.earth.luna.info", "params": {}}

// Hub-with-children methods (direct access)
{"method": "plexus.solar.help", "params": {}}
```

**Local calls remain unchanged:**
```rust
plexus.route("solar.mercury.info", params).await
```

### Implementation Strategy

#### Phase 1: Method Tree Discovery

**At hub construction time**, walk the activation tree and discover all callable methods:

```rust
pub fn discover_all_methods(&self) -> Vec<(String, MethodInfo)> {
    let mut methods = Vec::new();

    // Add hub's own methods
    for method in Activation::methods(self) {
        methods.push((
            format!("{}.{}", self.runtime_namespace(), method),
            MethodInfo { /* schema info */ }
        ));
    }

    // Recursively discover activation methods
    for (namespace, activation) in &self.inner.activations {
        self.discover_activation_methods(namespace, activation, &mut methods);
    }

    methods
}

fn discover_activation_methods(
    &self,
    namespace: &str,
    activation: &Arc<ActivationWrapper>,
    methods: &mut Vec<(String, MethodInfo)>
) {
    let hub_ns = self.runtime_namespace();

    // Add this activation's direct methods
    for method in activation.methods() {
        methods.push((
            format!("{}.{}.{}", hub_ns, namespace, method),
            MethodInfo { /* schema info */ }
        ));
    }

    // If this is a hub activation, discover nested children
    if let Some(router) = self.inner.child_routers.get(namespace) {
        self.discover_child_methods(namespace, router, methods);
    }
}

fn discover_child_methods(
    &self,
    parent_path: &str,
    router: &Arc<dyn ChildRouter>,
    methods: &mut Vec<(String, MethodInfo)>
) {
    // Get plugin schema to find children
    let schema = router.plugin_schema();

    for child_schema in schema.children {
        let child_path = format!("{}.{}", parent_path, child_schema.namespace);

        // Add this child's methods
        for method in &child_schema.methods {
            methods.push((
                format!("{}.{}.{}", self.runtime_namespace(), child_path, method.name),
                MethodInfo { /* from method schema */ }
            ));
        }

        // Recursively discover grandchildren
        if child_schema.children.len() > 0 {
            // Need to get_child() at runtime - this is a challenge (see below)
        }
    }
}
```

#### Phase 2: RPC Registration

Register each discovered method directly:

```rust
pub fn arc_into_rpc_module(hub: Arc<Self>) -> Result<RpcModule<()>, RegisterMethodError> {
    let mut module = RpcModule::new(());

    PlexusContext::init(hub.compute_hash());

    // Discover all methods
    let all_methods = hub.discover_all_methods();

    // Register each method directly
    for (full_method_name, method_info) in all_methods {
        let hub_clone = hub.clone();
        let method_name_static: &'static str = Box::leak(full_method_name.clone().into_boxed_str());
        let unsub_name: &'static str = Box::leak(format!("{}_unsub", full_method_name).into_boxed_str());

        // Extract the activation path (everything after hub namespace)
        let activation_path = extract_activation_path(&full_method_name, hub.runtime_namespace());

        module.register_subscription(
            method_name_static,
            method_name_static,
            unsub_name,
            move |params, pending, _ctx, _ext| {
                let hub = hub_clone.clone();
                let path = activation_path.clone();
                Box::pin(async move {
                    // Internally route through the hub
                    let stream = hub.route(&path, params).await
                        .map_err(|e| jsonrpsee::types::ErrorObject::owned(-32000, e.to_string(), None::<()>))?;
                    pipe_stream_to_subscription(pending, stream).await
                })
            }
        )?;
    }

    Ok(module)
}

fn extract_activation_path(full_method: &str, hub_namespace: &str) -> String {
    // "plexus.solar.mercury.info" → "solar.mercury.info"
    full_method.strip_prefix(&format!("{}.", hub_namespace))
        .unwrap_or(full_method)
        .to_string()
}
```

#### Phase 3: Remove Meta-Method

The `{namespace}.call` method becomes **unnecessary** and should be removed. All routing happens through direct method registration.

**Compatibility option:** Keep `{namespace}.call` as a legacy method during transition, but mark as deprecated.

---

## Benefits

### 1. Symmetry Between Local and Remote

**Before:**
```rust
// Local
plexus.route("solar.help", params)

// Remote
{"method": "plexus.call", "params": {"method": "solar.help", "params": {}}}
```

**After:**
```rust
// Local (unchanged)
plexus.route("solar.help", params)

// Remote (symmetric!)
{"method": "plexus.solar.help", "params": {}}
```

Both use the same logical path.

### 2. RPC Method Discovery

Standard JSON-RPC introspection (if supported by jsonrpsee) would show all available methods:

```json
{
  "methods": [
    "plexus.hash",
    "plexus.schema",
    "plexus.health.check",
    "plexus.echo.once",
    "plexus.solar.observe",
    "plexus.solar.mercury.info",
    "plexus.solar.earth.info",
    "plexus.solar.earth.luna.info"
  ]
}
```

No more black box routing - clients can see exactly what's available.

### 3. Type-Safe Parameters

Each method gets its own JSON-RPC signature with proper parameter schemas. No generic `{method, params}` wrapper.

### 4. Better Codegen

Generated TypeScript/Python clients can expose typed methods directly:

```typescript
// Before
await client.call("solar.mercury.info", {})

// After
await client.plexus.solar.mercury.info({})
```

The nested structure reflects the actual activation hierarchy.

### 5. Cleaner Mental Model

`.call()` is exposed as what it truly is: an **internal routing implementation**, not a public API method.

---

## Technical Challenges

### Challenge 1: Dynamic Child Discovery

**Problem:** `ChildRouter::get_child()` is called at **runtime**, not during registration. The full activation tree isn't known statically.

**Example:** In `solar` activation:

```rust
async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
    self.bodies.get(name)
        .map(|c| Box::new(CelestialBodyActivation::new(c.clone())) as Box<dyn ChildRouter>)
}
```

The `CelestialBodyActivation` instances are created **on demand** when routing happens, not at startup.

**Impact:** We can't pre-register `"plexus.solar.mercury.info"` because we don't have access to the `mercury` child router at registration time.

**Potential Solutions:**

#### Option A: Schema-Based Discovery (Recommended)

Use the `PluginSchema` to discover children statically:

```rust
let schema = activation.plugin_schema();
for child_schema in schema.children {
    // Register methods based on schema
    for method in child_schema.methods {
        let method_path = format!("{}.{}", child_schema.namespace, method.name);
        // Register RPC method that routes through parent
    }
}
```

**Pros:**
- No need to instantiate child routers
- Schema already contains the full hierarchy
- Aligns with schema introspection

**Cons:**
- Schema must be complete and accurate
- Adds coupling between schema and RPC registration

#### Option B: Lazy Registration

Register a wildcard handler for nested patterns:

```rust
// Register: "plexus.solar.*"
// When "plexus.solar.mercury.info" is called:
//   - Extract "mercury.info"
//   - Call solar.router_call("mercury.info")
```

**Pros:**
- Handles dynamic children
- No pre-discovery needed

**Cons:**
- Can't enumerate methods upfront
- Loses RPC introspection benefits
- Complex wildcard matching logic

#### Option C: Eager Instantiation

Force all child routers to be instantiated at registration time:

```rust
impl Solar {
    fn instantiate_all_children(&self) -> Vec<Box<dyn ChildRouter>> {
        self.bodies.values()
            .map(|body| Box::new(CelestialBodyActivation::new(body.clone())) as Box<dyn ChildRouter>)
            .collect()
    }
}
```

**Pros:**
- Full tree available at registration
- Can walk the entire hierarchy

**Cons:**
- Memory overhead (all children in memory)
- Violates lazy-loading design
- Doesn't scale for large hierarchies (e.g., thousands of resources)

**Recommendation:** Use **Option A (Schema-Based Discovery)** for the initial implementation. The schema is already used for codegen and introspection, so it's the source of truth. Routing handlers would look up child routers at call time, just like the current `route_to_child()` implementation.

### Challenge 2: String Leaking for Static Lifetimes

**Problem:** jsonrpsee requires method names to have `'static` lifetime:

```rust
pub fn register_subscription<'a>(
    &mut self,
    subscribe_method_name: &'static str,  // ← 'static required
    // ...
)
```

**Current approach** (in `arc_into_rpc_module`):

```rust
let call_method: &'static str = Box::leak(format!("{}.call", ns).into_boxed_str());
```

This **intentionally leaks memory** to get a `'static` reference.

**Impact:** For a large number of methods (e.g., 1000+ nested methods), this leaks ~50-100KB of strings.

**Is this acceptable?**

- ✅ Memory leak is bounded (one string per method)
- ✅ Strings are never freed during the process lifetime anyway
- ✅ Alternative (Arc<str>) doesn't satisfy `'static` requirement
- ⚠️ Could accumulate if hubs are rebuilt frequently (dev mode)

**Mitigation:** Document that this is intentional and acceptable. In production, the hub is built once at startup.

### Challenge 3: Method Explosion

**Problem:** A deep activation hierarchy creates many RPC method registrations.

**Example:** Solar system with 8 planets, 200+ moons:

```
plexus.solar.mercury.info
plexus.solar.venus.info
plexus.solar.earth.info
plexus.solar.earth.luna.info
plexus.solar.mars.info
plexus.solar.mars.phobos.info
plexus.solar.mars.deimos.info
plexus.solar.jupiter.info
plexus.solar.jupiter.io.info
plexus.solar.jupiter.europa.info
plexus.solar.jupiter.ganymede.info
plexus.solar.jupiter.callisto.info
... (~250 methods)
```

**Impact:**
- RpcModule initialization time increases
- Memory usage for method registry increases
- JSON-RPC introspection payload size increases

**Is this a problem?**

- ✅ 250 methods × ~50 bytes per method = ~12KB (negligible)
- ✅ Registration happens once at startup
- ✅ Clients benefit from explicit method enumeration
- ⚠️ Could be an issue for 10,000+ methods (e.g., per-resource APIs)

**Mitigation:**
- For large hierarchies, use **prefixed wildcards** (Challenge 1, Option B)
- Or use a hybrid: direct registration for known static methods, wildcard for dynamic resources

### Challenge 4: Parameter Parsing Duplication

**Problem:** Each RPC method handler needs to parse params, but the schema is defined in the activation.

**Current (meta-method):**
```rust
module.register_subscription(
    "plexus.call",
    move |params, pending, _ctx, _ext| {
        let p: CallParams = params.parse()?;  // Parse once
        hub.route(&p.method, p.params.unwrap_or_default())
    }
)
```

**Proposed (direct methods):**
```rust
// For EACH method:
module.register_subscription(
    "plexus.solar.mercury.info",
    move |params, pending, _ctx, _ext| {
        // params is already the method's params (not wrapped)
        // But we don't validate schema here
        hub.route("solar.mercury.info", params)
    }
)
```

**Issue:** We lose the opportunity to validate params against the schema before routing.

**Solution:** Schema validation happens inside the activation's `call()` method anyway, so this isn't a regression. The RPC layer is just a thin wrapper over routing.

### Challenge 5: Hub Namespace Collision

**Problem:** What if an activation is named `"hash"` or `"schema"`? It would collide with hub methods.

**Example:**
```rust
Plexus::new("plexus")
    .register(Hash::new())  // Activation with namespace "hash"
```

This creates a collision:
- `plexus.hash` (hub method to get config hash)
- `plexus.hash.*` (hash activation methods)

**Current state:** This collision exists today with the meta-method pattern too.

**Solution:**
1. **Reserve hub method names** - document that activations can't use `"hash"`, `"schema"`, `"call"` as namespaces
2. **Prefix hub methods** - rename to `_hash`, `_schema`, `_call` (breaking change)
3. **Separate namespaces** - put hub methods under `plexus._internal.*`

**Recommendation:** Option 1 (reserve names) for initial implementation. This is already an implicit constraint.

---

## Implementation Plan

### Phase 1: Schema-Based Discovery (Non-Breaking)

**Goal:** Add method discovery without changing the transport layer yet.

1. Implement `discover_all_methods()` in `DynamicHub`
2. Write tests to verify all nested methods are discovered
3. Add logging to show discovered methods at startup

**Files:**
- `hub-core/src/plexus/plexus.rs` - add discovery methods
- `substrate/src/main.rs` - log discovered methods at startup

**Validation:** Run with solar activation, verify all `solar.*` methods appear in logs.

### Phase 2: Direct Registration (Breaking Change)

**Goal:** Register all methods directly in `arc_into_rpc_module()`.

1. Modify `arc_into_rpc_module()` to call `discover_all_methods()`
2. Register each method as a direct subscription
3. Keep `{namespace}.call` for backward compatibility (mark deprecated)

**Files:**
- `hub-core/src/plexus/plexus.rs:786-881` - rewrite `arc_into_rpc_module()`

**Testing:**
- Update integration tests to use direct method calls
- Verify nested routing still works
- Test with mcp-gateway client

### Phase 3: Update Clients

**Goal:** Update generated clients to use direct method paths.

1. Update `hub-codegen` to generate nested client methods
2. Update `synapse` CLI to use direct paths
3. Update `substrate-protocol` (Haskell transport) if needed

**Files:**
- `hub-codegen/` - TypeScript/Python generators
- `synapse/` - CLI client
- `substrate-protocol/Transport.hs` - Haskell client

### Phase 4: Remove Meta-Method (Breaking)

**Goal:** Remove `{namespace}.call` entirely.

1. Remove meta-method registration from `arc_into_rpc_module()`
2. Update documentation
3. Add migration guide

**Files:**
- `hub-core/src/plexus/plexus.rs` - remove `CallParams` and `{namespace}.call` registration
- `docs/migration/` - new migration guide

---

## Migration Path

### Backward Compatibility Window

**Recommended approach:** Support both patterns for 2-3 releases.

**Release N (Current):**
```json
{"method": "plexus.call", "params": {"method": "solar.help", "params": {}}}  // ✓ Works
{"method": "plexus.solar.help", "params": {}}  // ✗ Not found
```

**Release N+1 (Transition):**
```json
{"method": "plexus.call", "params": {"method": "solar.help", "params": {}}}  // ✓ Works (deprecated warning)
{"method": "plexus.solar.help", "params": {}}  // ✓ Works (recommended)
```

**Release N+2 (Final):**
```json
{"method": "plexus.call", "params": {"method": "solar.help", "params": {}}}  // ✗ Not found
{"method": "plexus.solar.help", "params": {}}  // ✓ Works
```

### Client Migration

**Old clients:**
```typescript
await client.subscribe("plexus.call", {
  method: "solar.mercury.info",
  params: {}
})
```

**New clients:**
```typescript
await client.subscribe("plexus.solar.mercury.info", {})
```

**Codegen changes:**
- Generated clients shift from `call(method, params)` wrapper to direct nested methods
- TypeScript example: `client.plexus.solar.mercury.info({})`

---

## Open Questions

### 1. Should we support both patterns long-term?

**Option A:** Deprecate and remove meta-method entirely
- ✅ Cleaner API surface
- ✅ One way to do things
- ❌ Breaking change for clients

**Option B:** Keep both patterns indefinitely
- ✅ Maximum flexibility
- ✅ No breaking changes
- ❌ Confusing to have two ways
- ❌ More maintenance burden

**Recommendation:** Option A - remove meta-method after transition period.

### 2. How to handle dynamic resource APIs?

For activations with thousands of dynamic resources (e.g., per-user, per-file), registering every method is impractical.

**Example:** File system activation
```
plexus.fs.read_file  // Generic method
  vs
plexus.fs.file.<file-id>.read  // Per-file method (10,000+ files)
```

**Solution:** Use **parameterized methods** instead of nested paths:
```json
{"method": "plexus.fs.read_file", "params": {"file_id": "..."}}
```

Reserve **nested paths** for static hierarchies only.

### 3. Should introspection methods stay at hub level?

Currently: `plexus.hash`, `plexus.schema`

**Alternative:** Move to `plexus._meta.hash`, `plexus._meta.schema` to avoid namespace pollution.

**Recommendation:** Keep at hub level for now. These are fundamental hub operations.

---

## Related Documents

- `16679517135570018559_multi-hub-transport-envelope.md` - Context on PlexusStreamItem envelope and `plexus.call` history
- `16679960320421152511_nested-plugin-routing.md` - `ChildRouter` trait and `route_to_child` implementation
- `16678373036159325695_plugin-development-guide.md` - Plugin development guide (shows `into_rpc_methods()`)

---

## References

**Key files:**
- `hub-core/src/plexus/plexus.rs:786-881` - `arc_into_rpc_module()` implementation
- `hub-core/src/plexus/plexus.rs:496-497` - Activation registration with RPC factory
- `hub-core/src/activations/health/activation.rs` - Example of direct RPC registration
- `substrate/src/main.rs:96-98` - Transport server initialization with `arc_into_rpc_module`
- `hub-transport/src/websocket.rs` - WebSocket transport layer

**External dependencies:**
- jsonrpsee 0.26 - JSON-RPC library requiring `'static` method names

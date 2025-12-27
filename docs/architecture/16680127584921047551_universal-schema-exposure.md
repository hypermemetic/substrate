# Universal Schema Exposure

## Problem

Currently only `plexus` exposes a `schema` method. Child plugins do not:

```bash
plexus_call("plexus.schema", {})  # ✓ works
plexus_call("echo.schema", {})    # ✗ "Method not found"
plexus_call("solar.schema", {})   # ✗ "Method not found"
```

This creates a **partial category** where:
- Methods compose fully: `solar.earth.luna.info` works
- Schemas do not: can only see one level at a time

## Solution

Every plugin should expose a `schema` method automatically via `hub-macro`.

## Implementation

### 1. Modify `hub-macro` to Generate `schema` Method

In the macro expansion, add a `schema` method to every activation:

```rust
// Auto-generated for every activation
async fn schema(&self) -> ShallowPluginSchema {
    ShallowPluginSchema {
        namespace: self.namespace().to_string(),
        version: self.version().to_string(),
        description: self.description().to_string(),
        hash: self.plugin_hash().to_string(),
        methods: self.method_schemas(),
        children: self.child_summaries(),
    }
}
```

### 2. Register in RPC Dispatch

The macro should register `{namespace}_schema` in the RPC match:

```rust
// In generated RPC handler
match method_name {
    // ... existing methods ...

    // Auto-generated schema endpoint
    "{namespace}_schema" => {
        let schema = self.schema().await;
        wrap_stream(once(schema), "schema", provenance)
    }
}
```

### 3. Ensure ChildRouter Forwards Schema Calls

The existing `ChildRouter` mechanism should already handle this, but verify that:

```rust
// When routing "solar.earth.schema":
// 1. plexus routes to solar
// 2. solar routes to earth
// 3. earth handles "schema" locally
```

## Files to Modify

```
hub-macro/src/lib.rs
├── Add schema() method generation
├── Add "{ns}_schema" to RPC dispatch
└── Include in method_schemas() output (so schema appears in parent's schema)
```

## Expected Result

After implementation:

```bash
# Root
plexus_call("plexus.schema", {})
# → {namespace: "plexus", methods: [...], children: [{namespace: "echo", ...}, ...]}

# First level
plexus_call("echo.schema", {})
# → {namespace: "echo", methods: [{name: "echo"}, {name: "once"}], children: null}

plexus_call("solar.schema", {})
# → {namespace: "solar", methods: [...], children: [{namespace: "mercury", ...}, ...]}

# Nested
plexus_call("solar.earth.schema", {})
# → {namespace: "earth", methods: [...], children: [{namespace: "luna", ...}]}

plexus_call("solar.earth.luna.schema", {})
# → {namespace: "luna", methods: [{name: "info"}], children: null}
```

## Category Theoretic Impact

With universal schema exposure:

| Before | After |
|--------|-------|
| Partial category | Free category |
| Schema morphisms partial | Schema morphisms total |
| Can't traverse full tree | Full tree traversable |
| Synapse broken with shallow | Synapse can recurse on-demand |

The schema graph becomes a proper category where:
- **Objects**: Plugin schemas (identified by hash)
- **Morphisms**: Child references (namespace + hash)
- **Identity**: `schema.namespace == self`
- **Composition**: `parent.children[i].schema` chains

## Synapse Implications

With universal schema, synapse can:

1. Fetch root: `plexus.schema`
2. Display methods + child summaries
3. On navigation to child: fetch `{child}.schema`
4. Recurse as needed

No need for full recursive fetch upfront - lazy evaluation via morphism composition.

## Alternative Considered

**`plexus.full_schema`** - returns entire recursive tree

Rejected because:
- Expensive for large trees
- Doesn't scale with depth
- Violates lazy evaluation principle
- Per-plugin schema is more composable

## Priority

High - this unblocks synapse CLI development with the new shallow schema format.

# Universal Schema Exposure

**Status: IMPLEMENTED ✓**

## Summary

Every plugin now exposes a `schema` method, enabling full recursive schema traversal.

## Verification

```bash
plexus.schema           ✓  # root with children: [echo, health, solar]
echo.schema             ✓  # leaf with methods: [echo, once]
solar.schema            ✓  # hub with children: [mercury...neptune]
solar.earth.schema      ✓  # planet with child: [luna]
solar.earth.luna.schema ✓  # moon (leaf)
```

## What Each Schema Returns

| Field | Type | Description |
|-------|------|-------------|
| `namespace` | string | Plugin namespace |
| `version` | string | Plugin version |
| `description` | string | Human-readable description |
| `hash` | string | Content hash for cache invalidation |
| `methods` | array | Method schemas with params/returns |
| `children` | array? | Child summaries (null for leaves) |

## Example Response

```json
// solar.earth.schema
{
  "namespace": "earth",
  "version": "1.0.0",
  "description": "Earth - planet",
  "hash": "776129dcff369203",
  "methods": [
    {"name": "info", "description": "Get information about Earth", "hash": "..."}
  ],
  "children": [
    {"namespace": "luna", "description": "Luna - moon of Earth", "hash": "fce77fb61aa8c9d2"}
  ]
}
```

## Category Theoretic Status

With universal schema exposure:

| Property | Status |
|----------|--------|
| Objects (schemas) | ✓ All fetchable |
| Morphisms (child refs) | ✓ All resolvable |
| Identity | ✓ `schema.namespace == self` |
| Composition | ✓ `parent.children[i]` → `child.schema` |

**Determination: FREE CATEGORY ✓**

## Synapse Integration

Synapse can now:

1. Fetch `plexus.schema` for root
2. Build CLI for current level (methods + child commands)
3. On child navigation, fetch `{child}.schema`
4. Recurse lazily as user navigates

This matches the coalgebraic design: unfold on demand.

## Related Documents

- [Category Verification Report](16680127584921047551_category-verification-report.md) - Protocol for verifying category properties
- [Nested Plugin Routing](16679960320421152511_nested-plugin-routing.md) - How `plexus_call` routes to nested plugins
- [Schema Type-Driven Generation](../../../synapse/docs/architecture/16680892147769332735_schema-type-driven-generation.md) - How schemars generates schemas from types

## Implementation Notes

The `hub-macro` now:
1. Generates a `schema` method for every activation
2. Registers `{namespace}_schema` in RPC dispatch
3. `ChildRouter` forwards `.schema` calls to children

No changes needed to synapse types - `ShallowPluginSchema` already matches this format.

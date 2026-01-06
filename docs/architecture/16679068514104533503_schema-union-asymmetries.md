# Schema Union Asymmetries and IR Testing

This document captures a gap discovered through integration testing: how the IR Builder handles different union representations in JSON Schema, and the testing approach that revealed the issue.

## Problem Discovery

Integration testing against a live plexus backend revealed an asymmetry in schema handling:

- `health.schema` returns `SchemaResult`
- `SchemaResult` uses `anyOf` (untagged union), not `oneOf` (tagged union)
- The IR Builder only handled `oneOf`, missing `anyOf` patterns entirely
- Test output reported: `[info] health.schema has unresolved return refs: SchemaResult`

This wasn't a failure per se, but an informational finding that exposed incomplete union handling.

## Tagged vs Untagged Unions

JSON Schema represents Rust enums in two distinct ways depending on their serde representation.

### Tagged Unions (`oneOf` with discriminator field)

Rust source:
```rust
#[serde(tag = "type")]
enum ConeIdentifier {
    ByName { name: String },
    ById { id: Uuid },
}
```

JSON Schema representation:
```json
{
  "oneOf": [
    {
      "type": "object",
      "properties": {
        "type": {"const": "by_name"},
        "name": {"type": "string"}
      },
      "required": ["type", "name"]
    },
    {
      "type": "object",
      "properties": {
        "type": {"const": "by_id"},
        "id": {"type": "string", "format": "uuid"}
      },
      "required": ["type", "id"]
    }
  ]
}
```

The discriminator is an explicit field value (`type: "by_name"` vs `type: "by_id"`). Each variant is a complete object schema.

### Untagged Unions (`anyOf` with `$ref`)

Rust source:
```rust
#[serde(untagged)]
enum SchemaResult {
    Plugin(PluginSchema),
    Method(MethodSchema),
}
```

JSON Schema representation:
```json
{
  "anyOf": [
    {"$ref": "#/$defs/PluginSchema"},
    {"$ref": "#/$defs/MethodSchema"}
  ]
}
```

Here there's no discriminator field. The type shape itself distinguishes variants. A parser must try each `$ref` until one matches.

## Key Insight: The Type Name IS the Discriminator

For `anyOf` with `$ref` variants, the **reference name itself serves as the discriminator**:

- `PluginSchema` vs `MethodSchema` distinguishes the variants
- No explicit tag field exists, but the schema structure implies variant identity
- CLI can present this as: "Returns: PluginSchema | MethodSchema"

This is different from tagged unions where you look for a specific field value. For untagged unions, you look at which schema structure the value conforms to.

## IR Representation

Both union styles can map to `KindEnum`, with different discriminator semantics:

```haskell
-- Tagged (explicit discriminator field)
KindEnum
  { keDiscriminator = "type"
  , keVariants =
      [ VariantDef "by_name" (Just "by_name") [FieldDef "name" (Primitive "string") True Nothing]
      , VariantDef "by_id" (Just "by_id") [FieldDef "id" (Primitive "uuid") True Nothing]
      ]
  }

-- Untagged (type name as implicit discriminator)
KindEnum
  { keDiscriminator = "$ref"  -- marker indicating "discriminate by type shape"
  , keVariants =
      [ VariantDef "PluginSchema" Nothing []  -- variant name IS the discriminator
      , VariantDef "MethodSchema" Nothing []
      ]
  }
```

The `"$ref"` discriminator value serves as a sentinel indicating that variant selection happens by matching against the referenced type's schema, not by examining a field value.

## Other Schema Asymmetries

The `anyOf` vs `oneOf` distinction is one of several asymmetries in how schemars/serde map Rust types to JSON Schema:

### 1. `oneOf` vs `anyOf` for Enums

- **`oneOf`**: Tagged unions with discriminator field
- **`anyOf`**: Untagged unions, discriminated by shape

### 2. `anyOf` for Nullable Types

```json
{
  "anyOf": [
    {"$ref": "#/$defs/SomeType"},
    {"type": "null"}
  ]
}
```

This represents `Option<SomeType>` - either the referenced type or null. Not a union of variants, but an optionality wrapper.

### 3. `type` Array for Nullable Primitives

```json
{
  "type": ["string", "null"]
}
```

This represents `Option<String>`. A shorthand for nullable primitives that doesn't use `anyOf` at all.

### 4. String Enums vs `oneOf` with `const`

Simple C-style enums:
```json
{
  "enum": ["opus", "sonnet", "haiku"]
}
```

Tagged enums with const discriminators:
```json
{
  "oneOf": [
    {"properties": {"type": {"const": "by_name"}, ...}},
    {"properties": {"type": {"const": "by_id"}, ...}}
  ]
}
```

Both represent enums but require different parsing strategies.

## The IR Integration Test Strategy

### Philosophy

The test approach validates the IR Builder against real schemas, not mocked examples:

1. **Live backend testing** - Connect to actual plexus instance
2. **Build IR once, verify exhaustively** - Single IR construction, multiple invariant checks
3. **Catch gaps through real data** - The `anyOf` issue surfaced because real methods use it

### What's Tested

```haskell
describe "Schema fetching" $ do
  it "builds IR from root" $ ...
  it "IR contains methods" $ ...

describe "Method coverage" $ do
  it "all methods have help text" $ ...
  it "all type refs resolve" $ ...

describe "Specific methods" $ do
  it "cone.chat expands ConeIdentifier" $ ...

describe "Type resolution" $ do
  it "no dangling RefNamed in params" $ ...
  -- [info] health.schema has unresolved return refs: SchemaResult
```

Key invariants checked:

1. **IR builds successfully** - Schema fetch and parse works
2. **All methods produce help text** - No parse failures in method schemas
3. **All `RefNamed` in params resolve** - Type definitions exist for all referenced types
4. **All methods have valid `SupportLevel`** - Each method categorizes correctly
5. **Specific expansions work** - Critical paths like `ConeIdentifier` expand correctly

### Configuration

- `PLEXUS_PORT` environment variable (default: 4444)
- Requires running plexus backend on localhost
- Tests skip gracefully if backend unreachable

### Example Test Output

```
Schema fetching
  ✓ builds IR from root
  ✓ IR contains methods (47 methods found)
Method coverage
  ✓ all methods have help text
  ✓ all type refs resolve in params
Specific methods
  ✓ cone.chat expands ConeIdentifier to 2 variants
Type resolution
  ✓ no dangling RefNamed in params
  [info] health.schema has unresolved return refs: SchemaResult
  [info] cone.registry has unresolved return refs: RegistryInfo

Tests passed: 7
Informational findings: 2
```

The informational findings don't fail the test but document gaps for future work.

## Benefits of Live Testing

### 1. Catches Real Gaps

The `anyOf` issue came from actual schema analysis, not a hypothetical scenario. `SchemaResult` genuinely uses `anyOf`, and the IR Builder genuinely couldn't handle it.

### 2. No Mock Drift

Tests validate against the actual schema emission pipeline. If schemars changes output format or a type annotation changes, the test catches it.

### 3. Comprehensive Coverage

Testing ALL methods means edge cases surface automatically. Hand-picked unit tests would never have covered `health.schema` specifically.

### 4. Documents Actual Behavior

Informational findings like "these methods have unresolved return refs" serve as living documentation of current limitations.

## Proposed Fix

To handle `anyOf` unions in the IR Builder:

```haskell
extractReturns :: Text -> Maybe Value -> (Map Text TypeDef, TypeRef, Bool)
extractReturns methodName (Just (Object o)) =
  let defs = extractDefs o
      typeName = extractTitle o `orElse` (methodName <> "Result")

      -- Check oneOf first (tagged), then anyOf (untagged)
      (typeDef, streaming) =
        case KM.lookup "oneOf" o of
          Just variants -> extractTaggedUnion typeName variants
          Nothing -> case KM.lookup "anyOf" o of
            Just variants -> extractUntaggedUnion typeName variants
            Nothing -> extractSimpleReturn typeName o
  in (mergeDefs defs typeDef, RefNamed typeName, streaming)

extractUntaggedUnion :: Text -> Array -> (Maybe TypeDef, Bool)
extractUntaggedUnion typeName variants =
  let -- Extract $ref names from each variant
      refs = mapMaybe extractRefName (V.toList variants)

      -- Filter out {type: "null"} for Option handling
      nonNullRefs = filter (/= "null") refs

      -- Create variant definitions from ref names
      variantDefs = map (\r -> VariantDef r Nothing []) nonNullRefs

      -- "$ref" as discriminator signals "by type shape"
  in (Just $ TypeDef typeName Nothing (KindEnum "$ref" variantDefs), False)

extractRefName :: Value -> Maybe Text
extractRefName (Object o) =
  case KM.lookup "$ref" o of
    Just (String ref) -> Just $ T.takeWhileEnd (/= '/') ref
    _ -> Nothing
extractRefName _ = Nothing
```

The fix recognizes `anyOf` as an alternative union representation and extracts variant names from the `$ref` paths.

## Relationship to Other Documents

- **`ir-based-cli.md`** - The IR approach this testing validates. Defines the IR data model being tested.
- **`method-schema-spec.md`** - Documents the JSON Schema patterns we're testing against. Source of truth for what schemas should look like.
- **`structured-params.md`** - IR is the client-side structured representation of these patterns.
- **`schema-client-consumption.md`** - Client-side schema processing, where these asymmetries must be handled.

## Conclusion

Schema union asymmetries are an inherent consequence of serde's flexible serialization options. The IR layer must handle both `oneOf` (tagged) and `anyOf` (untagged) unions to fully support all method return types. Live integration testing against real schemas is the most effective way to discover these gaps, as it exercises the actual schema emission pipeline rather than idealized examples.

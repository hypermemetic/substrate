# Method-Level Schema Query

## Summary

The `plexus_activation_schema` endpoint now accepts an optional method parameter, enabling progressive schema discovery for building dynamic CLIs. Schema implementation is now enforced at the type level - all activations must provide proper schema generation or fail to compile.

## Problem

Building a dynamic CLI requires querying schemas at different levels:
```
$ cli --help                          # What activations are available?
$ cli arbor --help                    # What methods does arbor have?
$ cli arbor node_create_text --help   # What params does this method need?
```

Previously:
- `plexus_activation_schema("arbor")` returned the full oneOf schema (all methods)
- Clients had to parse the entire schema to extract a single method
- No dedicated endpoint for method-level queries
- Activations could "opt out" of schema generation (default implementation)

## Solution

### 1. Optional Method Parameter

`plexus_activation_schema` now accepts an optional second parameter:

```rust
// Activation-level: returns full schema with oneOf array
plexus_activation_schema("arbor")
→ { oneOf: [...all methods...], $defs: {...} }

// Method-level: returns single method schema
plexus_activation_schema("arbor", "node_create_text")
→ { properties: { method: { const: "node_create_text" }, params: {...} } }
```

### 2. Schema Helper Methods

Added to `Schema` type:

```rust
impl Schema {
    /// Extract a single method's schema from the oneOf array
    pub fn get_method_schema(&self, method_name: &str) -> Option<Schema> {
        // Searches oneOf variants for matching method name
        // Checks both "const" and "enum" for compatibility
    }

    /// List all method names from the oneOf array
    pub fn list_methods(&self) -> Vec<String> {
        // Extracts method names from all oneOf variants
    }
}
```

### 3. Enforced Schema Implementation

Removed the default implementation of `enrich_schema()`:

```rust
// BEFORE: Optional
fn enrich_schema(&self) -> Schema {
    Schema::new(self.namespace(), "...") // Default empty schema
}

// AFTER: Required - compile error if missing
fn enrich_schema(&self) -> Schema;
```

All activations now have proper `*Method` enums:
- `ArborMethod` - 19 methods with UUID types
- `ConeMethod` - 7 methods with UUID types
- `BashMethod` - 1 method with String command
- `HealthMethod` - 1 method with no params

## Request/Response Examples

### Activation-Level Query

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "plexus_activation_schema",
  "params": ["arbor"],
  "id": 1
}
```

**Response:**
```json
{
  "plexus_hash": "7a1a43920ee194e1",
  "type": "data",
  "content_type": "plexus.activation_schema",
  "data": {
    "oneOf": [
      {
        "properties": {
          "method": { "const": "tree_create" },
          "params": { "properties": {...}, "required": [...] }
        }
      },
      {
        "properties": {
          "method": { "const": "node_create_text" },
          "params": { "properties": {...}, "required": ["tree_id", "content"] }
        }
      }
      // ... more methods
    ],
    "$defs": {...}
  }
}
```

### Method-Level Query

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "plexus_activation_schema",
  "params": ["arbor", "node_create_text"],
  "id": 1
}
```

**Response:**
```json
{
  "plexus_hash": "7a1a43920ee194e1",
  "type": "data",
  "content_type": "plexus.method_schema",
  "data": {
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
}
```

### Error: Unknown Method

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "plexus_activation_schema",
  "params": ["arbor", "nonexistent_method"],
  "id": 1
}
```

**Response:**
```json
{
  "plexus_hash": "7a1a43920ee194e1",
  "type": "error",
  "error": "Method 'nonexistent_method' not found in activation 'arbor'. Available: tree_create, tree_get, node_create_text, ...",
  "recoverable": false
}
```

## Dynamic CLI Flow

The progressive discovery pattern enables step-by-step help:

```bash
# Step 1: List activations
$ cli --help
# Calls: plexus_schema
# Shows: arbor, bash, cone, health

# Step 2: List methods
$ cli arbor --help
# Calls: plexus_activation_schema("arbor")
# Parses oneOf to show: tree_create, tree_get, node_create_text, ...

# Step 3: Show method details
$ cli arbor node_create_text --help
# Calls: plexus_activation_schema("arbor", "node_create_text")
# Shows:
#   Required: tree_id (uuid), content (string)
#   Optional: parent (uuid), metadata (object)
```

## Implementation Details

### Method Lookup Algorithm

```rust
pub fn get_method_schema(&self, method_name: &str) -> Option<Schema> {
    let variants = self.one_of.as_ref()?;

    for variant in variants {
        let props = variant.properties.as_ref()?;
        let method_prop = props.get("method")?;

        // Try "const" first (schemars uses this)
        if let Some(const_val) = method_prop.additional.get("const") {
            if const_val.as_str() == Some(method_name) {
                return Some(variant.clone());
            }
        }

        // Fall back to enum_values for compatibility
        if let Some(enum_vals) = &method_prop.enum_values {
            if enum_vals.first()?.as_str() == Some(method_name) {
                return Some(variant.clone());
            }
        }
    }
    None
}
```

### Content Type Differentiation

The endpoint returns different content types based on query:

| Query | Content Type | Data Structure |
|-------|-------------|----------------|
| `("arbor")` | `plexus.activation_schema` | Full schema with oneOf |
| `("arbor", "method")` | `plexus.method_schema` | Single method variant |

This allows clients to distinguish between activation-level and method-level schemas.

## Type-Level Enforcement

### Before: Optional Schema

```rust
trait Activation {
    fn enrich_schema(&self) -> Schema {
        // Default: empty schema - activations could opt out
        Schema::new(self.namespace(), "...")
    }
}
```

**Problem**: Activations without proper schemas would silently fail at runtime.

### After: Required Schema

```rust
trait Activation {
    /// MUST be implemented - no default
    fn enrich_schema(&self) -> Schema;
}
```

**Result**: Compile error if activation doesn't implement schema.

### Method Enum Pattern

All activations now follow the same pattern:

```rust
// 1. Define Method enum with schemars
#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ArborMethod {
    TreeCreate { /* fields */ },
    NodeCreateText {
        tree_id: Uuid,        // Auto format: "uuid"
        /// Text content        // Auto description
        content: String,       // Auto required (non-Option)
        parent: Option<Uuid>,  // Auto optional
    },
    // ...
}

// 2. Generate schema
impl ArborMethod {
    pub fn schema() -> serde_json::Value {
        schemars::schema_for!(ArborMethod).into()
    }
}

// 3. Implement enrich_schema
impl Activation for Arbor {
    fn enrich_schema(&self) -> Schema {
        let schema_json = ArborMethod::schema();
        serde_json::from_value(schema_json).expect("...")
    }
}
```

## Benefits

1. **Progressive Disclosure**: CLI can query at exactly the right level of detail
2. **Bandwidth Efficiency**: Fetch method schema without full activation schema
3. **Better Error Messages**: "Method X not found. Available: A, B, C"
4. **Type Safety**: Schema implementation enforced at compile time
5. **Consistency**: All activations use the same schema generation pattern
6. **Self-Documenting**: Doc comments become schema descriptions automatically

## Related

- Type-driven schema: `16680892147769332735_schema-type-driven-generation.md`
- Plexus hash: `16680894298193956607_plexus-hash-cache-invalidation.md`
- Self-documenting RPC: `16680998353176467711_self-documenting-rpc.md`

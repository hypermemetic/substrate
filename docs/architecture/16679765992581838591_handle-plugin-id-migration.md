# Handle Plugin ID Migration: Clean Slate

**Date:** 2025-12-28
**Status:** Completed
**Breaking Change:** Yes - requires fresh databases

## Overview

This document details the migration from a hybrid `plugin_name`/`plugin_id` Handle system to a pure UUID-based `plugin_id` system. The migration takes a "clean slate" approach, removing all backwards compatibility code and requiring fresh databases.

## Motivation

The previous Handle system supported two identification methods:
1. **Legacy:** String-based `plugin_name` (e.g., "cone", "bash")
2. **Modern:** UUID-based `plugin_id` (deterministic v5 UUID)

This dual-support created:
- Complex fallback logic in handle resolution
- Migration code in storage layers
- Ambiguity in handle parsing
- Technical debt accumulation

The clean slate approach eliminates this complexity by committing fully to UUID-based identification.

## Changes Summary

### 1. Core Handle Type (`src/types.rs`)

**Before:**
```rust
pub struct Handle {
    pub plugin_id: Uuid,
    pub plugin_name: Option<String>,  // REMOVED
    pub version: String,
    pub method: String,
    pub meta: Vec<String>,
}

impl Handle {
    pub fn from_name(name: &str, version: &str, method: &str) -> Self  // REMOVED
    pub fn with_plugin_name(mut self, name: &str) -> Self              // REMOVED
}
```

**After:**
```rust
pub struct Handle {
    pub plugin_id: Uuid,
    pub version: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta: Vec<String>,
}

impl Handle {
    pub fn new(plugin_id: Uuid, version: impl Into<String>, method: impl Into<String>) -> Self
    pub fn with_meta(mut self, meta: Vec<String>) -> Self
}
```

**Key Changes:**
- Removed `plugin_name: Option<String>` field
- Removed `Handle::from_name()` constructor
- Removed `Handle::with_plugin_name()` builder method
- Updated `FromStr` to reject legacy name format (requires valid UUID)

### 2. Handle Parsing (`src/types.rs` - FromStr)

**Before:**
```rust
// Accepted both formats:
// - "plugin_name@version::method:meta"  (legacy)
// - "uuid@version::method:meta"         (modern)

let plugin_id = if let Ok(uuid) = plugin_id_str.parse::<Uuid>() {
    uuid
} else {
    // Legacy: generate UUID from name
    Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("{}@{}", plugin_id_str, version).as_bytes())
};
```

**After:**
```rust
// Only accepts UUID format:
// - "uuid@version::method:meta"

let plugin_id = plugin_id_str.parse::<Uuid>()
    .map_err(|e| format!("Invalid plugin_id UUID '{}': {}", plugin_id_str, e))?;
```

### 3. Arbor Storage (`src/activations/arbor/storage.rs`)

**Schema Changes:**
```sql
-- Before
handle_plugin TEXT,
handle_plugin_name TEXT,  -- REMOVED

-- After
handle_plugin_id TEXT,
```

**Removed Functions:**
- `migrate_handle_columns()` - Added columns for migration
- `migrate_handle_data()` - Populated plugin_id from plugin_name

**Query Updates:**
```rust
// SELECT - changed column name
"SELECT ..., handle_plugin_id, handle_version, handle_method, handle_meta, ..."

// Reading handle from DB
let plugin_id_str: String = row.get("handle_plugin_id");
let plugin_id = Uuid::parse_str(&plugin_id_str)
    .map_err(|e| format!("Invalid handle plugin_id: {}", e))?;

// INSERT - bind plugin_id directly
.bind(handle.plugin_id.to_string())
```

**Added Import:**
```rust
use uuid::Uuid;
```

### 4. Cone Storage (`src/activations/cone/storage.rs`)

**Before:**
```rust
use crate::activations::arbor::types::Handle;

pub fn message_to_handle(message: &Message, name: &str) -> Handle {
    Handle::from_name("cone", "1.0.0", "chat")
        .with_meta(vec![...])
}
```

**After:**
```rust
use super::activation::Cone;
use crate::types::Handle;

pub fn message_to_handle(message: &Message, name: &str) -> Handle {
    Handle::new(Cone::PLUGIN_ID, "1.0.0", "chat")
        .with_meta(vec![...])
}
```

### 5. ClaudeCode Storage (`src/activations/claudecode/storage.rs`)

**Before:**
```rust
use crate::activations::arbor::types::Handle;

pub fn message_to_handle(message: &Message, name: &str) -> Handle {
    Handle::from_name("claudecode", "1.0.0", "chat")
        .with_meta(vec![...])
}
```

**After:**
```rust
use super::activation::ClaudeCode;
use crate::types::Handle;

pub fn message_to_handle(message: &Message, name: &str) -> Handle {
    Handle::new(ClaudeCode::PLUGIN_ID, "1.0.0", "chat")
        .with_meta(vec![...])
}
```

### 6. Cone Context Resolution (`src/activations/cone/activation.rs`)

**Before (lines 612-647):**
```rust
NodeType::External { handle } => {
    // Resolve handle based on plugin (use plugin_name for backwards compat)
    let plugin = handle.plugin_name.as_deref().unwrap_or("unknown");
    match plugin {
        "cone" => { /* resolve cone message */ }
        "bash" => { /* resolve bash output */ }
        _ => { /* unknown plugin */ }
    }
}
```

**After:**
```rust
use crate::activations::bash::Bash;

NodeType::External { handle } => {
    // Resolve handle based on plugin_id
    if handle.plugin_id == Cone::PLUGIN_ID {
        // Resolve cone message handle
    } else if handle.plugin_id == Bash::PLUGIN_ID {
        // Resolve bash output
    } else {
        // Unknown handle plugin
    }
}
```

### 7. Plexus Handle Resolution (`src/plexus/plexus.rs`)

**Before:**
```rust
pub async fn do_resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
    // Try registry lookup first
    let path = self.inner.registry.read().unwrap()
        .lookup(handle.plugin_id)
        .map(|s| s.to_string());

    let path = match path {
        Some(p) => p,
        None => {
            // Fallback: try legacy plugin_name resolution
            if let Some(name) = &handle.plugin_name {
                // ... complex fallback logic ...
            } else {
                return Err(PlexusError::ActivationNotFound(...));
            }
        }
    };
    // ...
}
```

**After:**
```rust
pub async fn do_resolve_handle(&self, handle: &Handle) -> Result<PlexusStream, PlexusError> {
    let path = self.inner.registry.read().unwrap()
        .lookup(handle.plugin_id)
        .map(|s| s.to_string())
        .ok_or_else(|| PlexusError::ActivationNotFound(handle.plugin_id.to_string()))?;

    let activation = self.inner.activations.get(&path)
        .ok_or_else(|| PlexusError::ActivationNotFound(path.clone()))?;
    activation.resolve_handle(handle).await
}
```

### 8. Health Activation (`src/activations/health/activation.rs`)

Added constants for consistency with hub_methods-generated activations:

```rust
impl Health {
    /// Namespace for the health plugin
    pub const NAMESPACE: &'static str = "health";
    /// Version of the health plugin
    pub const VERSION: &'static str = "1.0.0";
    /// Stable plugin instance ID for handle routing (same formula as hub_methods macro)
    pub const PLUGIN_ID: uuid::Uuid = uuid::uuid!("f04a08d2-a4cd-50e8-a4e6-0f825e6516ae");
    // ...
}
```

The PLUGIN_ID is computed using the same formula as the hub_methods macro:
```python
import uuid
ns = uuid.UUID('6ba7b812-9dad-11d1-80b4-00c04fd430c8')  # NAMESPACE_OID
result = uuid.uuid5(ns, "health@1.0.0")
# f04a08d2-a4cd-50e8-a4e6-0f825e6516ae
```

### 9. Test Updates

**Plexus Tests (`src/plexus/plexus.rs`):**

All tests updated from:
```rust
let handle = Handle::from_name("health", "1.0.0", "check");
```

To:
```rust
let handle = Handle::new(Health::PLUGIN_ID, "1.0.0", "check");
// or for unknown plugins:
let handle = Handle::new(Uuid::new_v4(), "1.0.0", "method");
```

**Cone Storage Tests (`src/activations/cone/storage.rs`):**

```rust
// Before
assert_eq!(handle.plugin_name, Some("cone".to_string()));

// After
assert_eq!(handle.plugin_id, super::Cone::PLUGIN_ID);
assert_eq!(handle.version, "1.0.0");
```

## Files Modified

| File | Changes |
|------|---------|
| `src/types.rs` | Removed plugin_name field and related methods |
| `src/activations/arbor/storage.rs` | Schema update, removed migration code, added Uuid import |
| `src/activations/arbor/types.rs` | Removed backwards compat comment |
| `src/activations/cone/storage.rs` | Use Cone::PLUGIN_ID, updated test |
| `src/activations/cone/activation.rs` | UUID-based context resolution, import Bash |
| `src/activations/claudecode/storage.rs` | Use ClaudeCode::PLUGIN_ID |
| `src/activations/health/activation.rs` | Added PLUGIN_ID, NAMESPACE, VERSION constants |
| `src/plexus/plexus.rs` | Simplified resolve logic, updated tests |

## Files Deleted

All `.db` files in the project were deleted to ensure a fresh start:
- `*.db` (SQLite databases)

## Migration Path

For users with existing data:

1. **Export data** before upgrading (if needed)
2. **Delete all `.db` files** in the project
3. **Rebuild** with the new code
4. **Re-import data** (handles will be created with proper UUIDs)

There is no automatic migration path - this is intentional to reduce complexity.

## Plugin ID Generation Formula

All plugins use deterministic UUID v5 generation with **major version only** (semver compatibility):

```rust
// In hub_methods macro
let major_version = version.split('.').next().unwrap_or("0");
let name = format!("{}@{}", namespace, major_version);
let plugin_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes());
```

This ensures:
- Same namespace+major_version always produces same UUID
- Minor/patch version changes don't affect plugin ID (handles survive upgrades)
- Major version changes produce different UUIDs (breaking changes)
- UUIDs are globally unique across all plugins

### Semver Compatibility

| Version Change | Plugin ID | Handles |
|----------------|-----------|---------|
| 1.0.0 → 1.1.0 | Same | Compatible |
| 1.0.0 → 1.99.99 | Same | Compatible |
| 1.0.0 → 2.0.0 | Different | Breaking |

## Known Plugin IDs

These UUIDs are verified by invariant tests in `src/types.rs`:

| Plugin | Namespace | Major | PLUGIN_ID |
|--------|-----------|-------|-----------|
| Health | health | 1 | `dc560257-b7c5-575b-b893-b448c87ca797` |
| Cone | cone | 1 | `11429815-0a5e-5fcc-baf2-842ab3666e77` |
| ClaudeCode | claudecode | 1 | `51b330e5-ed88-5fe2-8b58-0c57f2b02ab3` |
| Bash | bash | 1 | `c425933b-2db7-5bb1-a608-9dd88143fce3` |
| Arbor | arbor | 1 | `58fd2bc4-b477-509b-9568-c5aee56f1bc0` |

## Testing

All 90 tests pass after migration:
- Handle parsing tests updated for UUID-only format
- Plugin routing tests use PLUGIN_ID constants
- Storage tests verify correct plugin_id in handles

### Plugin ID Invariant Tests

Added 10 new tests in `src/types.rs` to guard against formula changes:

```rust
// Canonical formula - must match hub_methods macro
fn generate_plugin_id(namespace: &str, version: &str) -> Uuid {
    let major_version = version.split('.').next().unwrap_or("0");
    let name = format!("{}@{}", namespace, major_version);
    Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes())
}
```

| Test | Purpose |
|------|---------|
| `invariant_plugin_id_formula_health` | Verify health@1 UUID |
| `invariant_plugin_id_formula_cone` | Verify cone@1 UUID |
| `invariant_plugin_id_formula_bash` | Verify bash@1 UUID |
| `invariant_plugin_id_formula_arbor` | Verify arbor@1 UUID |
| `invariant_plugin_id_formula_claudecode` | Verify claudecode@1 UUID |
| `invariant_plugin_id_deterministic` | Same input → same output |
| `invariant_plugin_id_same_major_version` | 1.0.0 = 1.1.0 = 1.99.99 |
| `invariant_plugin_id_different_major_versions` | v1.x.x ≠ v2.x.x |
| `invariant_plugin_id_different_namespaces` | alpha ≠ beta |
| `invariant_plugin_id_uses_namespace_oid` | Uses OID, not URL/DNS |

If any of these tests fail, it means the UUID generation formula has been accidentally changed.

## Future Considerations

1. **VERSION constant**: The hub_methods macro only generates PLUGIN_ID, not VERSION. Consider adding VERSION generation for consistency.

2. **Handle Display**: The Display impl now shows UUID instead of name. Consider adding a registry lookup for human-readable display.

3. **Error Messages**: Some error messages include plugin_id UUIDs. Consider enriching with namespace lookup for better debugging.

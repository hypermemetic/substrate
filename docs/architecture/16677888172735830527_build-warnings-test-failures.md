# Build Warnings and Test Failures

**Date**: 2026-01-19
**Status**: Documented
**Priority**: Medium - Code quality and maintenance

## Summary

Current build warnings and test failures identified in the substrate workspace. These are non-critical but should be addressed for code quality and to avoid warning fatigue.

## Test Failures

### 1. Bash Plugin Schema Test
**Location**: `src/activations/bash/activation.rs:97`
**Test**: `test_plugin_schema_with_return_types`
**Issue**: Assertion failure - expected 1 method, found 2

```
assertion `left == right` failed
  left: 2
 right: 1
```

**Analysis**: The test expects only the `execute` method, but the plugin schema is returning 2 methods. This suggests either:
- A schema method was added to the plugin
- The hub_macro is generating an additional method
- The test expectation is outdated

**Impact**: Test suite fails, blocking CI/CD

## Build Warnings

### Dead Code Warnings

#### 1. hub-macro: `return_type` field unused
**Location**: `hub-macro/src/parse.rs:210`
```rust
pub struct MethodInfo {
    pub return_type: Type,  // Never read
}
```

#### 2. hyperforge: `org_config` field unused
**Location**: `hyperforge/src/activations/secrets/activation.rs:24`
```rust
pub struct SecretsActivation {
    org_config: OrgConfig,  // Never read
}
```

#### 3. substrate: `method_schemas` function unused
**Location**: `src/activations/solar/celestial.rs:182`
```rust
impl CelestialBodyMethod {
    pub fn method_schemas(body_name: &str) -> Vec<MethodSchema> {
        // Never called
    }
}
```

#### 4. substrate: `config` field unused
**Location**: `src/activations/arbor/storage.rs:45`
```rust
pub struct ArborStorage {
    config: ArborConfig,  // Never read
}
```

#### 5. substrate: `ConeEvent` enum unused
**Location**: `src/activations/cone/types.rs:261`
```rust
pub enum ConeEvent {  // Never used
    // (deprecated)
}
```

#### 6. substrate: `write_mcp_config` method unused
**Location**: `src/activations/claudecode/executor.rs:170`
```rust
impl ClaudeCodeExecutor {
    async fn write_mcp_config(&self, config: &Value) -> Result<String, String> {
        // Never called
    }
}
```

#### 7. mcp-gateway: `description` field unused
**Location**: `src/bin/mcp_gateway.rs:88`
```rust
struct PluginSchema {
    description: String,  // Never read
}
```

### Deprecated Code Warnings

#### 1. hub-core: `wrap_stream_with_done` deprecated
**Location**: `hub-core/src/plexus/mod.rs:29`
```rust
pub use streaming::{wrap_stream_with_done};  // Deprecated
```
**Message**: Use `wrap_stream` instead, which now includes Done

#### 2. substrate: `ConeEvent` enum deprecated
**Location**: `src/activations/cone/types.rs:261`
```rust
pub enum ConeEvent {  // Deprecated
}
```
**Message**: Use method-specific result types instead

### Unexpected cfg Condition Warnings

#### 1. cllient: `plugin` feature not declared
**Locations**: `cllient/src/lib.rs:20, 22, 60, 65`
```rust
#[cfg(feature = "plugin")]  // Feature not in Cargo.toml
```

#### 2. hyperforge: `test-support` feature not declared
**Location**: `hyperforge/src/bridge/mod.rs:16`
```rust
#[cfg(any(test, feature = "test-support"))]  // Feature not in Cargo.toml
```

### Unused Mutation Warning

#### 1. cllient: unnecessary `mut` in template
**Location**: `cllient/src/config.rs:450`
```rust
let mut template: StreamingConfigYaml = serde_yaml::from_str(&content)
    // mut not needed
```

### Unused Import Warnings

#### 1. substrate: StreamExt import unused
**Location**: `examples/claude_stream_test.rs:5`
```rust
use futures::StreamExt;  // Not used
```

## Test Results Summary

```
Test suite: 41 tests
Passed: 40
Failed: 1
Ignored: 0
```

**Overall**: 97.6% pass rate

## Resolution Plan

### High Priority
1. Fix failing bash plugin schema test - investigate method count discrepancy
2. Remove deprecated `wrap_stream_with_done` export from hub-core

### Medium Priority
3. Add missing `plugin` feature to cllient Cargo.toml OR remove cfg guards
4. Add missing `test-support` feature to hyperforge Cargo.toml OR remove cfg guard
5. Remove `mut` from cllient config.rs:450
6. Remove unused StreamExt import from example

### Low Priority (Clean Code)
7. Remove or use `return_type` field in hub-macro MethodInfo
8. Remove or use `org_config` field in SecretsActivation
9. Remove unused `method_schemas` function in CelestialBodyMethod
10. Remove or use `config` field in ArborStorage
11. Remove deprecated `ConeEvent` enum entirely
12. Remove unused `write_mcp_config` method from ClaudeCodeExecutor
13. Remove or use `description` field in mcp-gateway PluginSchema

## Impact Assessment

**Build Impact**: None - warnings don't block compilation
**Test Impact**: High - 1 failing test blocks merge
**Code Quality**: Medium - warning fatigue reduces visibility of new issues
**Maintenance**: Low - dead code adds confusion for new contributors

## References

- Plugin Development Guide: `docs/architecture/16678373036159325695_plugin-development-guide.md`
- Hub Macro System: Generates method enums and schema functions

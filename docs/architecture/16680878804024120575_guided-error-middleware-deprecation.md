# GuidedErrorMiddleware: Current Behavior and Removal Plan

## Overview

This document describes the current `GuidedErrorMiddleware` implementation, its limitations, and what will change when it's removed in favor of stream-based guidance (Phase 6).

## Current Architecture (Legacy)

### Components

**Files involved:**
- `src/plexus/middleware.rs` - Middleware interceptor (147 lines)
- `src/plexus/errors.rs` - GuidedError builders (230 lines)
- `src/main.rs` - Middleware setup (lines 84-97)

### How It Works Today

#### 1. Middleware Setup (main.rs)

```rust
// Extract activation namespaces
let activation_namespaces: Vec<String> = activations.iter()
    .map(|a| a.namespace.clone())
    .collect();

// Create registry with just namespace strings
let registry = Arc::new(ActivationRegistry::new(activation_namespaces));

// Build middleware chain
let rpc_middleware = RpcServiceBuilder::new()
    .layer_fn(move |service| {
        GuidedErrorMiddleware::new(service, registry.clone())
    });

// Server uses middleware
let server = Server::builder()
    .set_rpc_middleware(rpc_middleware)
    .build(addr)
    .await?;
```

**Data passed to middleware:**
- Only activation **namespaces** (Vec<String>)
- No schema information
- No method lists
- No way to access activation objects

#### 2. Request Interception (middleware.rs)

The middleware intercepts **every RPC request** before it reaches the handler:

```rust
impl<'a, S> RpcServiceT<'a> for GuidedErrorMiddleware<S> {
    fn call(&self, req: Request<'a>) -> Self::Future {
        let method_name = req.method_name().to_string();

        // PRE-CHECK: Does activation exist?
        if let Some(guided_error) = check_activation_exists(&method_name, &req_id, &registry) {
            // Return error WITHOUT calling the actual handler
            return guided_error;
        }

        // Pass through to actual handler
        let response = inner.call(req).await;

        // Could enrich other errors here, but currently doesn't
        response
    }
}
```

**What it checks:**
1. Parse method name as `namespace_method` (e.g., `bash_execute`)
2. Extract namespace (`bash`)
3. Check if namespace exists in registry (Vec<String>)
4. If not found: Return guided error **without calling handler**

#### 3. Error Generation (errors.rs)

When activation not found, generates a JSON-RPC error:

```rust
pub fn activation_not_found(activation: &str, available: Vec<String>) -> ErrorObjectOwned {
    let data = GuidedErrorData::with_context(
        TryRequest::schema(),  // Suggests calling plexus_schema
        json!({
            "activation": activation,
            "available_activations": available,
        }),
    );
    ErrorObjectOwned::owned(
        codes::METHOD_NOT_FOUND,  // -32601
        format!("Activation '{}' not found", activation),
        Some(data),
    )
}
```

**Error structure sent to client:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32601,
    "message": "Activation 'foo' not found",
    "data": {
      "try": {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "plexus_schema",
        "params": []
      },
      "activation": "foo",
      "available_activations": ["arbor", "bash", "cone", "health"]
    }
  }
}
```

## What The Middleware Covers (and Doesn't Cover)

### ✅ Covered: Activation Not Found

**Scenario:** Client calls `foo_bar` where `foo` doesn't exist

**Flow:**
1. Middleware intercepts request
2. Parses method name → namespace: "foo"
3. Checks registry: "foo" not in ["arbor", "bash", "cone", "health"]
4. **Returns error immediately** without calling Plexus
5. Error includes `try` field with `plexus_schema` suggestion

**Result:** Client gets guidance before request reaches the handler.

### ❌ NOT Covered: Method Not Found

**Scenario:** Client calls `bash_invalid` where `bash` exists but method doesn't

**Flow:**
1. Middleware intercepts request
2. Parses method name → namespace: "bash"
3. Checks registry: "bash" IS in list
4. **Passes request through to handler**
5. Activation's `call()` method returns `Err(PlexusError::MethodNotFound)`
6. Error is converted to JSON-RPC error **without guidance**

**Result:** Client gets plain error, no `try` field, no available methods.

**Why not covered:**
- Middleware only has namespace strings, not method lists
- Would need to duplicate method lookup logic from activations
- No access to activation objects or schemas

### ❌ NOT Covered: Invalid Params

**Scenario:** Client calls `bash_execute` with wrong parameters

**Flow:**
1. Middleware passes request through (activation exists)
2. Activation's `call()` parses params
3. Returns `Err(PlexusError::InvalidParams(...))`
4. Error converted to JSON-RPC error **without guidance**

**Result:** Client gets plain error, no schema, no suggestions.

**Why not covered:**
- Middleware doesn't know parameter schemas
- Can't suggest corrections without schema access
- Would need full schema parsing in middleware

### ❌ NOT Covered: Execution Errors

**Scenario:** Bash command fails during execution

**Flow:**
1. Middleware passes request through
2. Activation executes command
3. Command fails, returns `Err(PlexusError::ExecutionError(...))`

**Result:** Plain error (this is correct - no guidance needed for runtime failures)

## Limitations of Current Approach

### 1. **Incomplete Coverage**

Only catches **one error type** (activation not found):
- ✅ Activation not found: Guided
- ❌ Method not found: Plain error
- ❌ Invalid params: Plain error
- ❌ Execution error: Plain error (correct)

**Coverage**: ~25% of error scenarios

### 2. **Duplicates Logic**

Middleware checks activation existence:
```rust
// middleware.rs
if !registry.activations.iter().any(|a| a == namespace) {
    return error;
}
```

Plexus also checks activation existence:
```rust
// plexus.rs
let activation = self.activations.get(namespace)
    .ok_or_else(|| PlexusError::ActivationNotFound(...))?;
```

**Same check in two places** → maintenance burden

### 3. **Limited Context**

Registry only stores namespace strings:
```rust
pub struct ActivationRegistry {
    pub activations: Vec<String>,  // Just ["arbor", "bash", "cone", "health"]
}
```

**Missing information:**
- Method lists per activation
- Schema details
- Version information
- Descriptions

Cannot provide rich guidance without this context.

### 4. **Wrong Abstraction Layer**

Guidance logic sits at **JSON-RPC middleware layer**:
- Intercepting RPC protocol
- Returning JSON-RPC errors
- Tightly coupled to jsonrpsee

**Should be at:** Stream event layer (part of response content, not protocol)

### 5. **Activation-Level Customization Impossible**

Bash activation might want to provide:
```rust
"Try: bash.execute --help"
"Example: bash.execute 'echo hello'"
```

But middleware can't access activation-specific logic.

## What Removal Changes

### Files to Modify

1. **src/main.rs** (lines 84-97)
   - Remove `ActivationRegistry` creation
   - Remove middleware setup
   - Remove `.set_rpc_middleware()` call

2. **src/plexus/middleware.rs**
   - Mark entire module as deprecated
   - Add deprecation notice at top
   - Keep for historical reference

3. **src/plexus/errors.rs**
   - Mark as deprecated
   - GuidedError builders no longer used
   - Keep for historical reference

4. **src/plexus/mod.rs**
   - Mark exports as `#[deprecated]`

### Before and After

#### Before (Lines 84-97 in main.rs)

```rust
// Create activation registry for guided errors
let activation_namespaces: Vec<String> = activations.iter()
    .map(|a| a.namespace.clone())
    .collect();
let registry = Arc::new(ActivationRegistry::new(activation_namespaces));

// Convert plexus to RPC module for JSON-RPC server (consumes plexus)
let module = plexus.into_rpc_module()?;

// Build RPC middleware with guided error support
let rpc_middleware = RpcServiceBuilder::new()
    .layer_fn(move |service| {
        GuidedErrorMiddleware::new(service, registry.clone())
    });

// Start server with middleware
```

#### After (Simplified)

```rust
// Convert plexus to RPC module for JSON-RPC server (consumes plexus)
let module = plexus.into_rpc_module()?;

// Start server (no middleware needed - guidance is in stream events)
```

**Removed:**
- 14 lines of middleware setup
- ActivationRegistry creation
- Namespace extraction loop
- Middleware chain building

### Request Flow Changes

#### Before (With Middleware)

```
Client Request
    ↓
WebSocket JSON-RPC
    ↓
GuidedErrorMiddleware ←─── PRE-CHECK activation exists
    ├─ Not found → Return guided JSON-RPC error
    └─ Found → Pass through
        ↓
RPC Module (jsonrpsee)
    ↓
Plexus::call() ←─── RE-CHECK activation exists (duplicate)
    ├─ Err(ActivationNotFound) → JSON-RPC error (no guidance)
    ├─ Err(MethodNotFound) → JSON-RPC error (no guidance)
    └─ Ok(stream) → Stream events
        ↓
Client receives stream or error
```

**Issues:**
- Two activation checks (middleware + plexus)
- Middleware guidance only for activation not found
- Other errors have no guidance

#### After (Stream-Based)

```
Client Request
    ↓
WebSocket JSON-RPC
    ↓
RPC Module (jsonrpsee)
    ↓
Plexus::call() ←─── SINGLE check, returns Ok(stream) always
    ├─ Activation not found → Ok(Guidance → Error → Done stream)
    ├─ Method not found → Ok(Guidance → Error → Done stream)
    ├─ Invalid params → Ok(Guidance → Error → Done stream)
    └─ Success → Ok(Data → Data → ... → Done stream)
        ↓
Client receives stream (guidance embedded in stream events)
```

**Improvements:**
- Single activation check in Plexus
- All error types get guidance
- Guidance is stream events, not JSON-RPC errors
- Richer context (method lists, schemas, custom suggestions)

### Breaking Changes

**None** - This is backward compatible:

1. **Successful subscriptions** still work identically
2. **Error subscriptions** now include guidance events:
   - Old clients: See guidance + error events (or ignore guidance)
   - New clients: Use guidance to help users

3. **JSON-RPC protocol** unchanged:
   - Still subscribe → get subscription ID
   - Still receive stream events
   - Just added new event type (`guidance`)

### Impact on Frontends

**Before removal:**
```json
// Activation not found - JSON-RPC error with data.try field
{
  "error": {
    "code": -32601,
    "message": "Activation 'foo' not found",
    "data": {
      "try": { "method": "plexus_schema", ... },
      "available_activations": [...]
    }
  }
}
```

**After removal:**
```json
// Activation not found - successful subscription with guidance stream
{ "result": "subscription_123" }

// Stream event 1: Guidance
{
  "type": "guidance",
  "error_kind": "activation_not_found",
  "activation": "foo",
  "action": "call_plexus_schema"
}

// Stream event 2: Error
{
  "type": "error",
  "error": "Activation not found: foo"
}

// Stream event 3: Done
{ "type": "done" }
```

**Migration:** Frontends should handle `type: "guidance"` events (see [Frontend Migration Guide](./16680880693241553663_frontend-guidance-migration.md))

## Benefits of Removal

### 1. Eliminate Duplication

**Before:** Two checks for activation existence
- Middleware: Check namespace in Vec<String>
- Plexus: Check namespace in HashMap

**After:** One check in Plexus
- Single source of truth
- Less code to maintain

### 2. Complete Coverage

**Before:** Only activation not found gets guidance
**After:** All error types get guidance
- Activation not found ✅
- Method not found ✅ (new!)
- Invalid params ✅ (new!)
- Execution error: No guidance (correct)

Coverage: 25% → 75%

### 3. Richer Context

**Before:** Only namespace strings available
**After:** Full activation access
- Method lists
- Schemas
- Custom suggestions
- Version info

### 4. Simpler Architecture

**Before:**
- Middleware layer
- ActivationRegistry
- GuidedError builders
- Coordination between middleware and Plexus

**After:**
- Stream events
- Single error handling path
- Direct access to activation info

### 5. Activation Customization

**Before:** Impossible - middleware can't access activations
**After:** Activations can override `custom_guidance()`:

```rust
impl Activation for Bash {
    fn custom_guidance(&self, method: &str, error: &PlexusError) -> Option<GuidanceSuggestion> {
        match (method, error) {
            ("execute", PlexusError::InvalidParams(_)) => {
                Some(GuidanceSuggestion::TryMethod {
                    method: "bash_execute".to_string(),
                    example_params: Some(json!("echo 'Hello!'")),
                })
            }
            _ => None,
        }
    }
}
```

## Information Parity Verification

The stream-based guidance provides **all necessary information** for frontends to implement equivalent functionality:

### What Middleware Provided

1. **Which method to call next**
   ```json
   "try": {
     "method": "plexus_schema",
     "params": []
   }
   ```

2. **Context about the error**
   ```json
   "activation": "foo",
   "available_activations": ["arbor", "bash", "cone", "health"]
   ```

### What Stream Guidance Provides

1. **Which method to call next**
   ```json
   "action": "call_plexus_schema"
   ```

2. **Context about the error**
   ```json
   "error_kind": "activation_not_found",
   "activation": "foo"
   ```

3. **Additional context not in middleware**
   ```json
   "available_methods": ["execute"],
   "method_schema": { ... },
   "example_params": "echo hello"
   ```

### Frontend Adaptation

Frontends need to map guidance actions to RPC calls:

```typescript
// Old: Read from error.data.try
const nextMethod = error.data.try.method;
const nextParams = error.data.try.params;

// New: Map guidance.action to method call
function getNextCall(guidance: GuidanceEvent) {
  switch (guidance.action) {
    case "call_plexus_schema":
      return { method: "plexus_schema", params: [] };
    case "call_activation_schema":
      return { method: "plexus_activation_schema", params: [guidance.namespace] };
    case "try_method":
      return { method: guidance.method, params: [guidance.example_params] };
  }
}
```

**Information parity:** ✅ All information available (and more)

## Migration Approach (Direct Removal)

### Phase 6: Remove Middleware

**Goal:** Clean removal since frontends will adapt to new system.

**Changes:**

1. **src/main.rs** (lines 84-97)
   - Remove ActivationRegistry creation
   - Remove middleware setup
   - Remove `.set_rpc_middleware()` call

2. **src/plexus/middleware.rs**
   - Add deprecation notice at top
   - Mark entire file `#![allow(dead_code)]`

3. **src/plexus/errors.rs**
   - Add deprecation notice
   - Mark `#[allow(dead_code)]`

4. **src/plexus/mod.rs**
   - Mark exports as deprecated

### Migration Checklist

- [ ] **src/main.rs**
  - [ ] Remove lines 84-88 (ActivationRegistry creation)
  - [ ] Remove lines 94-97 (middleware setup)
  - [ ] Remove `.set_rpc_middleware(rpc_middleware)` from Server builder
  - [ ] Remove imports: `ActivationRegistry, GuidedErrorMiddleware`

- [ ] **src/plexus/middleware.rs**
  - [ ] Add header comment:
    ```rust
    //! DEPRECATED: Stream-based guidance replaces middleware
    //!
    //! This module is kept for historical reference.
    //! See: docs/architecture/16680881573410764543_guidance-stream-based-errors.md
    //!
    //! Frontends should handle PlexusStreamEvent::Guidance events instead.

    #![allow(dead_code)]
    ```

- [ ] **src/plexus/errors.rs**
  - [ ] Add similar deprecation notice

- [ ] **src/plexus/mod.rs**
  - [ ] Mark exports:
    ```rust
    #[deprecated(note = "Use PlexusStreamEvent::Guidance instead")]
    pub use middleware::{ActivationRegistry, GuidedErrorMiddleware};
    #[deprecated(note = "Use GuidanceErrorType and GuidanceSuggestion instead")]
    pub use errors::{GuidedError, GuidedErrorData, TryRequest};
    ```

### Testing

- [ ] Remove middleware setup
- [ ] Verify server starts
- [ ] Test activation not found returns guidance streams
- [ ] Test method not found returns guidance streams
- [ ] Run integration tests: `cargo test --test rpc_integration`

### Rollback Plan

If issues arise: `git revert` the removal commit.

**Safety:** Single atomic commit, easy to revert.

## Timeline

- **Phase 1-4:** Complete ✅ (Stream-based guidance implemented)
- **Phase 5:** Bash custom guidance example
- **Phase 6:** Remove middleware (this doc)
- **Phase 7:** Integration tests

**Middleware removal:** Direct removal, frontends adapt to new stream-based system

## Related Documentation

- [Stream-Based Guidance Architecture](./16680881573410764543_guidance-stream-based-errors.md) - New system design
- [Frontend Migration Guide](./16680880693241553663_frontend-guidance-migration.md) - How clients handle guidance events
- [Testing Strategy](./16680885909985432575_testing-strategy.md) - Test coverage for both systems

## Historical Context

The middleware was created when:
- Only needed to catch activation not found
- Didn't have stream event infrastructure
- Wanted quick solution to improve error messages

**Why it worked initially:**
- Simple single-error-type coverage
- Pre-check before handler reduces load

**Why it's being removed:**
- Stream events provide better abstraction
- Need coverage for all error types
- Want activation-level customization
- Avoid duplication and maintenance burden

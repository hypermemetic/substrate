# Guidance API Refactoring: Stream-Based Error Guidance

## Goal

Replace the duplicative GuidedErrorMiddleware with a stream-based guidance system where errors automatically include rich context (available methods, schemas, suggestions) as stream events.

## Current Problems

1. **Middleware duplication**: `GuidedErrorMiddleware` pre-checks activation existence, but activations also check method existence - two layers doing similar work
2. **Limited context**: Middleware only has activation namespaces (Vec<String>), not full schema/method info
3. **Wrong layer**: Guidance is in JSON-RPC error data, not in stream events like other responses
4. **Incomplete coverage**: No guidance for MethodNotFound/InvalidParams errors from activations

## Core Architecture Change

**Before:**
```
RPC Request → Middleware (checks activation) → Activation.call() → Result<PlexusStream, PlexusError>
```

**After:**
```
RPC Request → Plexus.call() → ALWAYS Ok(PlexusStream)
  On error: Stream yields Guidance → Error → Done
  On success: Stream yields Data → Done (unchanged)
```

Key insight: `Plexus::call()` returns `Ok(PlexusStream)` even for errors. Errors are expressed as streams containing guidance events.

## User Requirements

From user answers:
- **When**: Only on errors (not success responses)
- **What**: Available methods for this activation + Schema for the attempted method
- **Source**: Hybrid - Plexus infers from schemas, activations can override with custom suggestions
- **Structure**: Special event type in PlexusStreamEvent enum

## Synchronization Objects (Contracts Between Phases)

These are the minimum interfaces/structures that phases agree on to enable parallel development:

### Contract 1: GuidanceTypes
**Defines types that all other phases depend on**

```rust
// Agreed structure - can be stubbed during development
pub enum GuidanceErrorType {
    ActivationNotFound { activation: String },
    MethodNotFound { activation: String, method: String },
    InvalidParams { method: String, reason: String },
}

pub enum GuidanceSuggestion {
    CallPlexusSchema,
    CallActivationSchema { namespace: String },
    TryMethod { method: String, example_params: Option<Value> },
    Custom { message: String },
}

// PlexusStreamEvent gains this variant:
Guidance {
    provenance: Provenance,
    error_type: GuidanceErrorType,
    available_methods: Option<Vec<String>>,
    method_schema: Option<Schema>,
    suggestion: GuidanceSuggestion,
}
```

**Integration checkpoint**: Types compile, serialize/deserialize correctly

### Contract 2: ErrorStreamHelper
**Defines the helper function signature**

```rust
fn error_stream_with_guidance(
    plexus_hash: String,
    provenance: Provenance,
    error: PlexusError,
    activation_info: Option<&Arc<dyn ActivationObject>>,
    attempted_method: Option<&str>,
) -> PlexusStream;
```

**Behavior contract**:
- Returns stream with exactly 3 items: Guidance → Error → Done
- Guidance includes available_methods if activation_info.is_some()
- Guidance includes method_schema if method exists
- Suggestion varies by error type

**Stub implementation** (for parallel development):
```rust
fn error_stream_with_guidance(...) -> PlexusStream {
    // Minimal stub that returns empty stream or TODO items
    Box::pin(stream::iter(vec![
        PlexusStreamItem::guidance(...), // with stub data
        PlexusStreamItem::error(...),
        PlexusStreamItem::done(...),
    ]))
}
```

**Integration checkpoint**: Function compiles, returns PlexusStream, stream yields 3 items

### Contract 3: CustomGuidanceMethod
**Defines the trait method signature**

```rust
trait Activation {
    fn custom_guidance(
        &self,
        _method: &str,
        _error: &PlexusError,
    ) -> Option<GuidanceSuggestion> {
        None
    }
}
```

**Behavior contract**:
- Default returns None (no custom guidance)
- Activations can override to provide custom suggestions
- Called by error_stream_with_guidance() after default logic

**Integration checkpoint**: Trait method exists, default impl returns None, activations can override

### Contract 4: PlexusCallBehavior
**Defines new behavior of Plexus::call()**

**Old behavior**: Returns `Result<PlexusStream, PlexusError>`
**New behavior**: ALWAYS returns `Ok(PlexusStream)`, errors are stream events

**Stream shapes**:
- Success: Data → Data → ... → Done
- Error: Guidance → Error → Done

**Integration checkpoint**: call() compiles, always returns Ok, errors become streams

## Parallelization Strategy

### Dependency Graph

```
Phase 1 (Types)
    ├─→ Phase 2 (Helper) ─→ Phase 3 (call()) ─→ Phase 6 (Middleware)
    ├─→ Phase 4 (Trait) ──→ Phase 5 (Bash)
    └─→ Phase 7 (Tests) - can write anytime, run when ready

Independent paths:
- Path A: 1 → 2 → 3 → 6
- Path B: 1 → 4 → 5
- Path C: 7 (parallel to all)
```

### Parallel Execution Order

**Immediate start** (no dependencies):
- Phase 1: Add Guidance Event Types
- Phase 7: Write tests (won't pass until other phases done)

**After Phase 1 types exist** (can stub if not complete):
- Phase 2: Create Error Stream Helper (can stub GuidanceTypes)
- Phase 4: Add Activation Override (can stub GuidanceSuggestion)

**After Phase 2 OR Phase 4**:
- Phase 3: Modify Plexus::call() (can stub error_stream_with_guidance)
- Phase 5: Implement Bash example (needs trait method signature)

**After Phase 3**:
- Phase 6: Remove Middleware (needs working error streams)

**Final**:
- Phase 7: Run tests (validates all contracts)

### Stubbing Strategy for Maximum Parallelism

Each phase can start immediately by stubbing dependencies:

**Phase 2 stub** (if Phase 1 not ready):
```rust
fn error_stream_with_guidance(...) -> PlexusStream {
    // TODO: Add guidance when types are ready
    Box::pin(stream::iter(vec![error_item, done_item]))
}
```

**Phase 3 stub** (if Phase 2 not ready):
```rust
Err(error) => {
    // TODO: Use error_stream_with_guidance when ready
    return Err(error);  // Keep old behavior temporarily
}
```

**Phase 4 stub** (if Phase 1 not ready):
```rust
fn custom_guidance(...) -> Option<()> { None }  // Stub return type
```

### Integration Validation

After each phase, run its **Integration checkpoint** test to verify the contract:

```bash
# Phase 1
cargo test test_guidance_types_contract

# Phase 2
cargo test test_error_stream_helper_contract

# Phase 3
cargo test test_plexus_call_behavior_contract

# Phase 4
cargo test test_custom_guidance_contract

# Phase 5
cargo test test_bash_custom_guidance

# Phase 6
cargo build && cargo run  # Verify server starts

# Phase 7
cargo test guidance_tests
cargo test --test rpc_integration
```

### Communication Between Parallel Teams

Teams working on different phases communicate via:

1. **Contracts document** (this section) - defines interfaces
2. **Integration checkpoints** - verify assumptions
3. **Status checks** - quick test that phase is working

Example workflow:
- Team A works on Phase 1 (types)
- Team B works on Phase 2 (helper) using stubbed types
- Team A finishes → runs `cargo test test_guidance_types_contract`
- Team B replaces stubs with real types → runs `cargo test test_error_stream_helper_contract`
- Both teams verify integration

## Implementation Phases

See the full implementation plan at `/Users/shmendez/.claude/plans/kind-prancing-crane.md` for detailed implementation steps for each phase.

### Phase 1: Add Guidance Event Types
- **File**: src/plexus/types.rs
- Add GuidanceErrorType, GuidanceSuggestion enums
- Add Guidance variant to PlexusStreamEvent
- Add PlexusStreamItem::guidance() constructor

### Phase 2: Create Error Stream Helper
- **File**: src/plexus/plexus.rs
- Implement error_stream_with_guidance() function
- Handle ActivationNotFound, MethodNotFound, InvalidParams, ExecutionError

### Phase 3: Modify Plexus::call()
- **File**: src/plexus/plexus.rs
- Change to always return Ok(PlexusStream)
- Use error_stream_with_guidance() for errors

### Phase 4: Add Activation Override Mechanism
- **File**: src/plexus/plexus.rs
- Add custom_guidance() to Activation trait
- Add to ActivationObject trait
- Implement in ActivationWrapper

### Phase 5: Implement Example in Bash
- **File**: src/activations/bash/activation.rs
- Override custom_guidance() with example params

### Phase 6: Remove Middleware
- **Files**: src/main.rs, src/plexus/middleware.rs, src/plexus/mod.rs
- Remove ActivationRegistry and GuidedErrorMiddleware setup
- Add deprecation notices

### Phase 7: Update Tests
- **Files**: src/plexus/plexus.rs, tests/rpc_integration.rs
- Add unit tests for guidance streams
- Update integration tests

## Benefits

1. **Eliminates duplication**: Single error handling path in plexus
2. **Richer context**: Guidance includes methods, schemas, suggestions
3. **Activation control**: Custom suggestions for better UX
4. **Consistent protocol**: Guidance flows through streams like other events
5. **Type safety**: Structured guidance vs free-form error messages
6. **Parallelizable**: 7 phases can be developed concurrently

## Edge Cases

1. **Method parsing errors** (`"invalid"` with no dot): Return guidance suggesting plexus_schema
2. **Empty method names** (`"bash."`): Caught as MethodNotFound
3. **Long method lists** (100+ methods): Include all but suggest activation schema endpoint
4. **Missing schemas**: Skip method_schema field (it's optional)
5. **Custom guidance errors**: Trust activation authors, no validation

## Backward Compatibility

- **Stream-based clients**: Will see new Guidance events before Error events
- **Clients ignoring unknown types**: No breaking change
- **plexus_hash**: Still included in all events
- **Error events**: Structure unchanged

Migration: Deploy substrate → Update clients to handle Guidance (optional) → Remove middleware

## Related Documentation

- **[Frontend Migration Guide](./16680880693241553663_frontend-guidance-migration.md)** - How frontends (Symbols) should handle guidance events
- [Guided Errors (legacy)](./16680966217191669503_guided-errors.md) - Previous middleware-based approach
- [Dynamic CLI Type-Driven Schemas](./16680891033387373567_dynamic-cli-type-driven-schemas.md) - How clients use schemas
- [Testing Strategy](./16680885909985432575_testing-strategy.md) - How guidance is tested

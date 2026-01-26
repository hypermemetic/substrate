# ERROR-1: Structured Error Handling Epic

## Goal

Transform Plexus error handling from string-based messages to structured, actionable errors with field-level validation details, suggestions, and documentation links. Enable schema-based parameter validation that eliminates custom validation code and provides consistent error messages across all activations.

## Context

**Current Problems:**
- Errors are just strings: `PlexusError::InvalidParams("something wrong")`
- No structured details: which field failed? what was expected?
- Generic serde errors: `"invalid type: string \"five\", expected u32"`
- Each activation writes custom validation code
- Users only see first error (must fix one at a time)
- No suggestions or recovery guidance

**User Impact:**
```bash
# Before
$ synapse cone create --model invalid --name ""
Error: Invalid params

# After
$ synapse cone create --model invalid --name ""
Error: 2 validation error(s)

Validation errors:
  • name: Value is too short (minimum length: 1)
    Expected: string with 1-100 characters
    Actual: ""

  • model_id: Value does not match pattern
    Expected: string matching ^(claude-|gpt-|anthropic\.)
    Actual: "invalid"
    Suggestion: Valid examples: claude-3-haiku-20240307, gpt-4o-mini
                Run 'synapse cone registry' to see available models.
```

## Architecture

### Error Flow

```
User Request (JSON-RPC params)
       ↓
┌──────────────────────────────────────┐
│  hub-macro Generated Code            │
│  ┌────────────────────────────────┐  │
│  │ 1. Get JSON Schema for Type    │  │
│  │    schema_for!(CreateParams)   │  │
│  └────────────┬───────────────────┘  │
│               ↓                       │
│  ┌────────────────────────────────┐  │
│  │ 2. Validate Against Schema     │  │
│  │    validate_params_against_    │  │
│  │    schema(&params, &schema)    │  │
│  └────────────┬───────────────────┘  │
│               ↓                       │
│         [if invalid]                  │
│               ↓                       │
│  ┌────────────────────────────────┐  │
│  │ 3. Return Structured Error     │  │
│  │    PlexusError::InvalidParams  │  │
│  │    with ErrorDetails           │  │
│  └────────────┬───────────────────┘  │
│               ↓                       │
│  ┌────────────────────────────────┐  │
│  │ 4. Convert to Stream Error     │  │
│  │    PlexusStreamItem::Error     │  │
│  │    with ErrorDetails field     │  │
│  └────────────┬───────────────────┘  │
└───────────────┼───────────────────────┘
                ↓
         [if valid]
                ↓
  ┌──────────────────────────────┐
  │ 5. Deserialize to Type       │
  │    serde_json::from_value    │
  │    (should never fail)       │
  └──────────────┬───────────────┘
                 ↓
  ┌──────────────────────────────┐
  │ 6. Call Activation Method    │
  │    self.create(params)       │
  └──────────────────────────────┘
```

### Type System

```rust
// hub-core/src/plexus/error_codes.rs
pub enum ErrorCode {
    // JSON-RPC standard
    InvalidParams,      // -32602
    MethodNotFound,     // -32601
    InternalError,      // -32603

    // Plexus extensions
    ValidationError,    // -32001
    NotFound,          // -32002
    AlreadyExists,     // -32003
    Unauthorized,      // -32004
    Conflict,          // -32006
    // ...
}

pub enum ErrorCategory {
    Validation,
    NotFound,
    Authentication,
    Authorization,
    Conflict,
    // ...
}

// hub-core/src/plexus/types.rs
pub struct ErrorDetails {
    pub category: ErrorCategory,
    pub field: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<Value>,
    pub suggestion: Option<String>,
    pub docs_url: Option<String>,
    pub context: Option<Value>,
    pub related: Option<Vec<String>>,
}

pub enum PlexusStreamItem {
    Error {
        metadata: StreamMetadata,
        message: String,
        code: Option<ErrorCode>,        // NEW: structured
        recoverable: bool,
        details: Option<ErrorDetails>,  // NEW: structured details
    },
    // ...
}

// hub-core/src/plexus/plexus.rs
pub enum PlexusError {
    InvalidParams {
        message: String,
        details: ErrorDetails,  // NEW: structured
    },
    // ...
}
```

### Validation Approach

**Schema-Based Validation:**
```rust
// Developer writes types with schemars annotations
#[derive(Deserialize, JsonSchema)]
pub struct CreateParams {
    #[schemars(length(min = 1, max = 100))]
    pub name: String,

    #[schemars(regex(pattern = "^(claude-|gpt-)"))]
    pub model_id: String,
}

// hub-macro generates validation code automatically
#[hub_method]
async fn create(&self, params: CreateParams) -> impl Stream<Item = ConeEvent> {
    // params are pre-validated!
}

// Generated code:
let schema = schemars::schema_for!(CreateParams);
validate_params_against_schema(&params, &schema)?;  // ← validates
let params: CreateParams = serde_json::from_value(params)?;  // ← always succeeds
```

## Dependency DAG

```
                    ERROR-2 (ErrorCode, ErrorDetails, ErrorCategory)
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
          ERROR-3         ERROR-4         ERROR-5
       (Update          (Update         (Add validation
      PlexusError)   StreamItem)       to hub-core)
              │               │               │
              └───────────────┼───────────────┘
                              ▼
                          ERROR-6
                    (Update hub-macro
                     to validate)
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
          ERROR-7         ERROR-8         ERROR-9
       (Update Cone)   (Update Bash)  (Update ClaudeCode)
              │               │               │
              └───────────────┼───────────────┘
                              ▼
                         ERROR-10
                      (Documentation)
```

## Phases

### Phase 1: Core Types (Completed ✓)
| Ticket | Description | Status | Files Changed |
|--------|-------------|--------|---------------|
| ERROR-2 | Add ErrorCode, ErrorCategory, ErrorDetails types to hub-core | ✓ Done | `hub-core/src/plexus/error_codes.rs`, `types.rs` |

**Deliverables:**
- ✓ `ErrorCode` enum with JSON-RPC + Plexus codes
- ✓ `ErrorCategory` for programmatic handling
- ✓ `ErrorDetails` struct with builder pattern
- ✓ Updated `PlexusStreamItem::Error` to include `details` field
- ✓ Backward compatible (details is optional)

### Phase 2: Enhanced PlexusError
| Ticket | Description | Blocked By | Files Changed |
|--------|-------------|------------|---------------|
| ERROR-3 | Update PlexusError variants with ErrorDetails | ERROR-2 | `hub-core/src/plexus/plexus.rs` |
| ERROR-4 | Add stream error helpers with details | ERROR-2 | `hub-core/src/plexus/streaming.rs` |

**ERROR-3 Details:**
```rust
pub enum PlexusError {
    ActivationNotFound {
        namespace: String,
        suggestion: Option<String>,  // NEW
    },

    MethodNotFound {
        activation: String,
        method: String,
        available_methods: Vec<String>,  // NEW
    },

    InvalidParams {
        message: String,
        details: ErrorDetails,  // NEW
    },

    ExecutionError {
        message: String,
        code: ErrorCode,  // NEW
        details: Option<ErrorDetails>,  // NEW
    },

    // Add to_stream_error() method
}
```

**ERROR-4 Details:**
```rust
// New helper functions
pub fn error_stream_with_details(
    message: String,
    code: ErrorCode,
    details: ErrorDetails,
    provenance: Vec<String>,
) -> PlexusStream { ... }

pub fn validation_error_stream(...) -> PlexusStream { ... }
pub fn not_found_stream(...) -> PlexusStream { ... }
```

### Phase 3: Schema-Based Validation
| Ticket | Description | Blocked By | Files Changed |
|--------|-------------|------------|---------------|
| ERROR-5 | Add JSON Schema validation to hub-core | ERROR-2 | `hub-core/src/plexus/validation.rs` (new) |
| ERROR-6 | Update hub-macro to validate params | ERROR-5 | `hub-macro/src/codegen/activation.rs` |

**ERROR-5 Details:**
- Add `jsonschema` crate dependency
- Implement `validate_params_against_schema(params, schema)`
- Convert `jsonschema::ValidationError` → `ErrorDetails`
- Aggregate multiple validation errors
- Generate helpful suggestions based on schema constraints

**ERROR-6 Details:**
Update `generate_param_extraction()` to:
1. Generate schema for parameter type: `schema_for!(CreateParams)`
2. Validate params against schema before deserialization
3. Return structured error with all validation failures
4. Only deserialize if validation passes

Generated code pattern:
```rust
// For single struct param
let schema = schemars::schema_for!(#param_ty);
let schema_value = serde_json::to_value(&schema).expect("schema serialization");

validate_params_against_schema(&params, &schema_value)?;

let #param_name: #param_ty = serde_json::from_value(params.clone())
    .expect("deserialization should succeed after validation");
```

### Phase 4: Activation Updates
| Ticket | Description | Blocked By | Files Changed |
|--------|-------------|------------|---------------|
| ERROR-7 | Update Cone activation with structured errors | ERROR-6 | `substrate/src/activations/cone/` |
| ERROR-8 | Update Bash activation with structured errors | ERROR-6 | `substrate/src/activations/bash/` |
| ERROR-9 | Update ClaudeCode activation with structured errors | ERROR-6 | `substrate/src/activations/claudecode/` |

**Tasks per activation:**
1. Add schemars validation annotations to param types
2. Remove custom validation code (now handled by hub-macro)
3. Use `ErrorDetails` for business logic errors
4. Add helpful suggestions and related commands
5. Test all error paths

**Example:**
```rust
// Before
pub async fn create(&self, params: CreateParams) -> Result<...> {
    if params.name.is_empty() {
        return Err(PlexusError::InvalidParams("name cannot be empty".into()));
    }
    // ...
}

// After
#[derive(Deserialize, JsonSchema)]
pub struct CreateParams {
    #[schemars(length(min = 1, max = 100))]  // ← Validation here
    pub name: String,
    // ...
}

pub async fn create(&self, params: CreateParams) -> Result<...> {
    // No manual validation needed!
    // hub-macro already validated

    // Business logic errors still need ErrorDetails:
    if self.cone_exists(&params.name).await {
        return Err(PlexusError::ExecutionError {
            message: format!("Cone '{}' already exists", params.name),
            code: ErrorCode::AlreadyExists,
            details: Some(ErrorDetails::new(ErrorCategory::Conflict)
                .with_field("name")
                .with_suggestion(format!(
                    "Use a different name or delete with: synapse cone delete --id {}",
                    params.name
                ))),
        });
    }
}
```

### Phase 5: Documentation & Testing
| Ticket | Description | Blocked By | Files Changed |
|--------|-------------|------------|---------------|
| ERROR-10 | Documentation and comprehensive tests | ERROR-7, ERROR-8, ERROR-9 | Various |

**Deliverables:**
- Error code registry documentation
- Guide for adding validation to new activations
- Migration guide for existing activations
- Integration tests for error scenarios
- Wire format examples

## Benefits

### For Users (Synapse CLI)
- See **all** validation errors at once (not just first one)
- Get **specific** field-level error details
- Receive **actionable suggestions** for fixing
- Access **documentation links** for complex errors
- Understand **which command** to run next

### For Developers (Activation Authors)
- **No custom validation code** - use schemars annotations
- **Consistent error messages** across all activations
- **Schema is single source of truth** for validation
- **Automatic structured errors** from hub-macro
- **Easier maintenance** - validation in type definitions

### For Plexus Ecosystem
- **Better debugging** - errors include full context
- **Programmatic handling** - structured error codes
- **Cross-language** - error format is language-agnostic
- **Backward compatible** - old clients still work

## Wire Format Examples

### Before (Current)
```json
{
  "type": "error",
  "metadata": {...},
  "message": "Invalid params",
  "code": null,
  "recoverable": false
}
```

### After (With Structured Errors)
```json
{
  "type": "error",
  "metadata": {...},
  "message": "2 validation error(s)",
  "code": "VALIDATION_ERROR",
  "recoverable": false,
  "details": {
    "category": "validation",
    "context": {
      "errors": [
        {
          "field": "name",
          "constraint": "minLength",
          "expected": "string with at least 1 character",
          "actual": ""
        },
        {
          "field": "model_id",
          "constraint": "pattern",
          "expected": "string matching ^(claude-|gpt-|anthropic\\.)",
          "actual": "invalid-model",
          "suggestion": "Valid examples: claude-3-haiku-20240307, gpt-4o-mini. Run 'synapse cone registry' to see available models."
        }
      ]
    }
  }
}
```

## Migration Strategy

### Backward Compatibility
- `details` field is **optional** in `PlexusStreamItem::Error`
- Old Synapse clients ignore unknown fields
- New Synapse clients check for `details` and fall back to `message`
- No breaking protocol changes

### Rollout
1. **Week 1:** Deploy hub-core with new types (ERROR-2 ✓, ERROR-3, ERROR-4)
2. **Week 2:** Deploy hub-macro with validation (ERROR-5, ERROR-6)
3. **Week 3:** Update core activations (ERROR-7, ERROR-8, ERROR-9)
4. **Week 4:** Update Synapse CLI to render structured errors
5. **Week 5:** Documentation and remaining activations

### Testing Strategy
- Unit tests for all error types
- Integration tests for validation scenarios
- Backward compatibility tests (old Synapse + new Substrate)
- Forward compatibility tests (new Synapse + old Substrate)

## Success Metrics

### Quantitative
- [ ] 90% of errors include structured `ErrorDetails`
- [ ] Users see average of 2.3 validation errors per request (vs 1.0 currently)
- [ ] Support tickets mentioning "unclear error" reduced by 70%
- [ ] Activation validation code reduced by 80%

### Qualitative
- [ ] Users report errors are "actually helpful"
- [ ] Developers find validation "trivial to add"
- [ ] Error messages are consistent across activations
- [ ] Debugging is faster with full context

## Related Documents
- Architecture: `docs/architecture/16677364953367891711_error-handling-improvements.md`
- Implementation: `docs/architecture/16677363587163129599_substrate-error-improvements.md`
- Synapse Changes: See synapse repo for client-side rendering

## Status
- **Overall:** In Progress
- **Phase 1:** ✓ Completed
- **Phase 2:** Pending
- **Phase 3:** Pending
- **Phase 4:** Pending
- **Phase 5:** Pending

**Last Updated:** 2025-01-25

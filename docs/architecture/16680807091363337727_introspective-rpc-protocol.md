# Introspective Streaming RPC Protocol

## Status

This document describes a protocol for building **self-describing RPC services** and **reflective clients** that can dynamically adapt to server capabilities without compile-time knowledge of available operations.

## Abstract

This specification defines a streaming RPC protocol built on JSON-RPC 2.0 over WebSocket transport. The protocol enables clients to discover server capabilities at runtime through schema introspection, build user interfaces dynamically from type information, and receive structured guidance when errors occur. The design eliminates the need for clients to have prior knowledge of server operations while maintaining type safety through JSON Schema.

## Terminology

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in RFC 2119.

| Term | Definition |
|------|------------|
| Service | A server implementing this protocol |
| Module | A namespaced collection of related methods |
| Method | A callable operation within a module |
| Stream | An ordered sequence of events returned by a method invocation |
| Schema | JSON Schema describing method parameters and structure |
| Reflective Client | A client that discovers capabilities at runtime |

## 1. Transport Layer

### 1.1 Connection

Services MUST accept WebSocket connections. The default port is implementation-defined but SHOULD be configurable via environment variable.

```
ws://{host}:{port}
```

### 1.2 Message Format

All messages MUST conform to JSON-RPC 2.0 specification. Services MUST support the subscription pattern where method calls return streaming responses.

**Request Format:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "module_method",
  "params": [<parameters>]
}
```

**Subscription Notification Format:**
```json
{
  "jsonrpc": "2.0",
  "method": "<subscription_method>",
  "params": {
    "subscription": "<subscription_id>",
    "result": <stream_item>
  }
}
```

### 1.3 Method Naming Convention

Methods MUST be named using the pattern `{module}_{method}` where:
- `module` is the namespace identifier (lowercase, alphanumeric)
- `method` is the operation name (lowercase, underscores permitted)

Example: `storage_tree_create`, `shell_execute`

## 2. Stream Protocol

All method invocations MUST return a stream of events. The stream provides progress updates, data chunks, errors, and completion signals.

### 2.1 Stream Item Structure

Every stream item MUST include:
- `service_hash`: A deterministic hash for cache invalidation
- `type`: The event type discriminator
- `provenance`: Call chain tracking array

```json
{
  "service_hash": "a1b2c3d4",
  "type": "<event_type>",
  "provenance": ["module", "nested_module"],
  ...event_specific_fields
}
```

### 2.2 Event Types

#### 2.2.1 Progress Event

Indicates operation progress. Services MAY emit zero or more progress events.

```json
{
  "service_hash": "a1b2c3d4",
  "type": "progress",
  "provenance": ["shell"],
  "message": "Executing command...",
  "percentage": 0.5
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| message | string | REQUIRED | Human-readable progress message |
| percentage | number | OPTIONAL | Progress from 0.0 to 1.0 |

#### 2.2.2 Data Event

Contains operation output. Services MAY emit zero or more data events.

```json
{
  "service_hash": "a1b2c3d4",
  "type": "data",
  "provenance": ["shell"],
  "content_type": "shell.output",
  "data": {
    "stdout": "Hello, World!\n",
    "exit_code": 0
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| content_type | string | REQUIRED | Type identifier for the data payload |
| data | any | REQUIRED | The actual payload (type-specific) |

#### 2.2.3 Error Event

Indicates an error occurred. Non-recoverable errors SHOULD terminate the stream.

```json
{
  "service_hash": "a1b2c3d4",
  "type": "error",
  "provenance": ["storage"],
  "error": "Resource not found: tree-123",
  "recoverable": false
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| error | string | REQUIRED | Error message |
| recoverable | boolean | REQUIRED | Whether the operation can continue |

#### 2.2.4 Done Event

Signals successful stream completion. Every successful stream MUST end with exactly one done event.

```json
{
  "service_hash": "a1b2c3d4",
  "type": "done",
  "provenance": ["storage"]
}
```

#### 2.2.5 Guidance Event

Provides structured hints for error recovery. Services SHOULD emit guidance before error events for introspection-related failures.

```json
{
  "service_hash": "a1b2c3d4",
  "type": "guidance",
  "provenance": ["storage"],
  "error_kind": "method_not_found",
  "module": "storage",
  "method": "tree_destory",
  "available_methods": ["tree_create", "tree_get", "tree_delete"],
  "method_schema": { ... },
  "action": "try_method",
  "suggested_method": "tree_delete"
}
```

See Section 5 for complete guidance specification.

### 2.3 Stream Sequences

Valid stream sequences:

| Outcome | Sequence |
|---------|----------|
| Success with data | Progress* → Data* → Done |
| Success without data | Progress* → Done |
| Introspection error | Guidance → Error → Done |
| Execution error | Error → Done |

Where `*` indicates zero or more occurrences.

## 3. Introspection Protocol

The introspection protocol enables reflective clients to discover service capabilities at runtime.

### 3.1 List Modules

Returns all available modules and their methods.

**Method:** `service_schema`

**Parameters:** None (empty array)

**Response Stream:** Single data event with content_type `service.schema`

```json
{
  "modules": [
    {
      "namespace": "storage",
      "version": "1.0.0",
      "description": "Hierarchical data storage",
      "methods": ["tree_create", "tree_get", "tree_delete", "node_append"]
    },
    {
      "namespace": "shell",
      "version": "1.0.0",
      "description": "Command execution",
      "methods": ["execute"]
    }
  ],
  "total_methods": 5
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| modules | array | REQUIRED | List of module information objects |
| total_methods | integer | REQUIRED | Total count of all methods |

**Module Information:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| namespace | string | REQUIRED | Module identifier |
| version | string | REQUIRED | Semantic version |
| description | string | REQUIRED | Human-readable description |
| methods | array | REQUIRED | List of method names |

### 3.2 Get Module Schema

Returns the complete JSON Schema for a module's methods.

**Method:** `service_module_schema`

**Parameters:**
```json
["<namespace>"]
```

**Response Stream:** Single data event with content_type `service.module_schema`

The schema MUST be a valid JSON Schema (draft-07 or later) with the following structure:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "StorageMethods",
  "oneOf": [
    {
      "type": "object",
      "description": "Create a new tree",
      "properties": {
        "method": { "const": "tree_create" },
        "name": {
          "type": "string",
          "description": "Name for the new tree"
        }
      },
      "required": ["method", "name"]
    },
    {
      "type": "object",
      "description": "Retrieve a tree by ID",
      "properties": {
        "method": { "const": "tree_get" },
        "tree_id": {
          "type": "string",
          "format": "uuid",
          "description": "The tree identifier"
        }
      },
      "required": ["method", "tree_id"]
    }
  ],
  "$defs": {
    "TreeIdentifier": {
      "oneOf": [
        { "type": "object", "properties": { "id": { "type": "string", "format": "uuid" } } },
        { "type": "object", "properties": { "name": { "type": "string" } } }
      ]
    }
  }
}
```

**Schema Requirements:**

1. The root schema MUST contain a `oneOf` array where each element represents one method
2. Each method variant MUST have a `method` property with a `const` value matching the method name
3. The order of variants in `oneOf` MUST match the order of methods returned by `service_schema`
4. Complex types SHOULD be defined in `$defs` and referenced via `$ref`
5. Properties SHOULD include `description` fields
6. Type-specific formats (uuid, date-time, email) SHOULD be specified

### 3.3 Get Service Hash

Returns a deterministic hash representing the current service configuration.

**Method:** `service_hash`

**Parameters:** None (empty array)

**Response Stream:** Single data event with content_type `service.hash`

```json
{
  "hash": "a1b2c3d4e5f6"
}
```

The hash MUST change when:
- A module is added or removed
- A module version changes
- Methods are added or removed from a module

The hash MUST NOT change when:
- The service restarts with identical configuration
- Non-schema-affecting internal changes occur

### 3.4 Index-Based Method Resolution

Clients MUST correlate methods to schemas using array indices:

```
methods[i] from service_schema corresponds to oneOf[i] from service_module_schema
```

This enables efficient lookup without parsing schema internals.

## 4. Cache Invalidation Protocol

### 4.1 Opportunistic Invalidation

Every stream item includes `service_hash`. Clients SHOULD compare this hash against their cached schema hash. If different, the cache is stale.

### 4.2 Recommended Caching Strategy

1. On startup, call `service_hash`
2. Compare with cached hash
3. If match: use cached schema
4. If mismatch or no cache: fetch fresh schema via `service_schema` and `service_module_schema`
5. Store schema with hash for future sessions
6. During operation, monitor `service_hash` in responses for live invalidation

### 4.3 Stale Cache Fallback

If network errors prevent schema refresh, clients MAY use stale cached schema with degraded functionality. Clients SHOULD indicate to users when operating with potentially stale schema.

## 5. Guidance Protocol

The guidance protocol provides structured error recovery hints, enabling reflective clients to suggest corrections without hardcoded error handling.

### 5.1 Guidance Event Structure

```json
{
  "type": "guidance",
  "provenance": ["module"],
  "error_kind": "<error_type>",
  ...error_specific_fields,
  "action": "<suggestion_type>",
  ...suggestion_specific_fields,
  "available_methods": ["method1", "method2"],
  "method_schema": { ...json_schema }
}
```

### 5.2 Error Types

#### 5.2.1 Module Not Found

```json
{
  "error_kind": "module_not_found",
  "module": "storag"
}
```

#### 5.2.2 Method Not Found

```json
{
  "error_kind": "method_not_found",
  "module": "storage",
  "method": "tree_destory"
}
```

#### 5.2.3 Invalid Parameters

```json
{
  "error_kind": "invalid_params",
  "method": "storage.tree_get",
  "reason": "missing required field: tree_id"
}
```

### 5.3 Suggestion Types

#### 5.3.1 Call Service Schema

Suggests fetching the module list.

```json
{
  "action": "call_service_schema"
}
```

#### 5.3.2 Call Module Schema

Suggests fetching a specific module's schema.

```json
{
  "action": "call_module_schema",
  "namespace": "storage"
}
```

#### 5.3.3 Try Method

Suggests a specific method, optionally with example parameters.

```json
{
  "action": "try_method",
  "suggested_method": "storage.tree_delete",
  "example_params": { "tree_id": "..." }
}
```

#### 5.3.4 Custom

Module-specific guidance.

```json
{
  "action": "custom",
  "message": "The tree_id must be a valid UUID. Use tree_list to find available trees."
}
```

### 5.4 Contextual Information

Guidance events MAY include:

| Field | Type | Description |
|-------|------|-------------|
| available_methods | array | Methods available in the relevant module |
| method_schema | object | JSON Schema for the intended method |

## 6. Reflective Client Implementation

### 6.1 Capability Discovery Flow

```
                                    ┌─────────────────┐
                                    │  Client Start   │
                                    └────────┬────────┘
                                             │
                                             ▼
                                    ┌─────────────────┐
                                    │ Load Cached     │
                                    │ Schema (if any) │
                                    └────────┬────────┘
                                             │
                                             ▼
                              ┌──────────────────────────────┐
                              │ Call service_hash            │
                              └──────────────┬───────────────┘
                                             │
                          ┌──────────────────┴──────────────────┐
                          │                                     │
                          ▼                                     ▼
                 ┌─────────────────┐                   ┌─────────────────┐
                 │ Hash matches    │                   │ Hash differs    │
                 │ cached hash     │                   │ or no cache     │
                 └────────┬────────┘                   └────────┬────────┘
                          │                                     │
                          │                                     ▼
                          │                           ┌─────────────────┐
                          │                           │ Call            │
                          │                           │ service_schema  │
                          │                           └────────┬────────┘
                          │                                     │
                          │                                     ▼
                          │                           ┌─────────────────┐
                          │                           │ For each module:│
                          │                           │ Call            │
                          │                           │ service_module_ │
                          │                           │ schema          │
                          │                           └────────┬────────┘
                          │                                     │
                          │                                     ▼
                          │                           ┌─────────────────┐
                          │                           │ Cache schema    │
                          │                           │ with hash       │
                          │                           └────────┬────────┘
                          │                                     │
                          └──────────────┬──────────────────────┘
                                         │
                                         ▼
                              ┌─────────────────────┐
                              │ Build Dynamic UI    │
                              │ from Schema         │
                              └─────────────────────┘
```

### 6.2 Dynamic Interface Generation

Reflective clients SHOULD generate user interfaces from schema:

1. **Parse module list** - Create top-level navigation/commands per module
2. **Parse method schemas** - Create sub-commands/forms per method
3. **Map types to inputs**:
   - `string` → text input
   - `string` with `format: "uuid"` → UUID input with validation
   - `integer` → numeric input
   - `boolean` → toggle/checkbox
   - `array` → list input or JSON
   - `object` → nested form or JSON
4. **Apply required fields** - Mark required parameters, validate before submission
5. **Use descriptions** - Display as help text/tooltips

### 6.3 Schema Reference Resolution

Schemas MAY contain `$ref` references to `$defs`. Clients MUST resolve these references before generating interfaces.

**Resolution Algorithm:**

```
function resolveRefs(schema, defs):
  if schema has "$ref":
    refPath = schema["$ref"]  // e.g., "#/$defs/TreeIdentifier"
    defName = extractDefName(refPath)
    resolved = defs[defName]
    return resolveRefs(resolved, defs)

  if schema has "properties":
    for each property in schema.properties:
      schema.properties[property] = resolveRefs(property, defs)

  if schema has "oneOf":
    for each variant in schema.oneOf:
      variant = resolveRefs(variant, defs)

  return schema
```

### 6.4 Error Handling with Guidance

When receiving a guidance event:

1. Parse `error_kind` to understand the failure category
2. Parse `action` to determine suggested recovery
3. If `available_methods` present, offer list to user
4. If `method_schema` present, show correct parameter format
5. For `try_method` suggestions, pre-populate corrected invocation

## 7. Wire Format Examples

### 7.1 Complete Discovery Session

**Client Request - Get Service Schema:**
```json
{"jsonrpc":"2.0","id":1,"method":"service_schema","params":[]}
```

**Server Response - Subscription Created:**
```json
{"jsonrpc":"2.0","id":1,"result":"sub_001"}
```

**Server Notification - Data:**
```json
{
  "jsonrpc": "2.0",
  "method": "service_subscription",
  "params": {
    "subscription": "sub_001",
    "result": {
      "service_hash": "a1b2c3d4",
      "type": "data",
      "provenance": ["service"],
      "content_type": "service.schema",
      "data": {
        "modules": [
          {
            "namespace": "storage",
            "version": "1.0.0",
            "description": "Tree-based storage",
            "methods": ["tree_create", "tree_get"]
          }
        ],
        "total_methods": 2
      }
    }
  }
}
```

**Server Notification - Done:**
```json
{
  "jsonrpc": "2.0",
  "method": "service_subscription",
  "params": {
    "subscription": "sub_001",
    "result": {
      "service_hash": "a1b2c3d4",
      "type": "done",
      "provenance": ["service"]
    }
  }
}
```

### 7.2 Method Invocation with Guidance Error

**Client Request - Typo in Method:**
```json
{"jsonrpc":"2.0","id":2,"method":"storage_tree_destory","params":[{"tree_id":"123e4567-e89b-12d3-a456-426614174000"}]}
```

**Server Notification - Guidance:**
```json
{
  "jsonrpc": "2.0",
  "method": "service_subscription",
  "params": {
    "subscription": "sub_002",
    "result": {
      "service_hash": "a1b2c3d4",
      "type": "guidance",
      "provenance": ["storage"],
      "error_kind": "method_not_found",
      "module": "storage",
      "method": "tree_destory",
      "available_methods": ["tree_create", "tree_get", "tree_delete"],
      "action": "try_method",
      "suggested_method": "tree_delete"
    }
  }
}
```

**Server Notification - Error:**
```json
{
  "jsonrpc": "2.0",
  "method": "service_subscription",
  "params": {
    "subscription": "sub_002",
    "result": {
      "service_hash": "a1b2c3d4",
      "type": "error",
      "provenance": ["storage"],
      "error": "Method not found: tree_destory",
      "recoverable": false
    }
  }
}
```

**Server Notification - Done:**
```json
{
  "jsonrpc": "2.0",
  "method": "service_subscription",
  "params": {
    "subscription": "sub_002",
    "result": {
      "service_hash": "a1b2c3d4",
      "type": "done",
      "provenance": ["storage"]
    }
  }
}
```

## 8. Security Considerations

### 8.1 Schema Exposure

Services SHOULD consider what information schema introspection reveals. Sensitive operations MAY be excluded from public schema while remaining callable by authenticated clients.

### 8.2 Input Validation

Clients MUST NOT rely solely on schema validation. Services MUST validate all inputs server-side regardless of client-side schema validation.

### 8.3 Hash Predictability

The `service_hash` SHOULD NOT be cryptographically secure but MUST be deterministic. It is not intended for security purposes.

## 9. Implementation Notes

### 9.1 Server Implementation

1. **Automatic Schema Generation**: Use type system reflection (e.g., schemars in Rust, TypeBox in TypeScript, jsonschema in Python) to generate schemas from type definitions
2. **Doc Comment Propagation**: Type documentation SHOULD become schema descriptions
3. **Hash Computation**: Hash SHOULD be computed from sorted, deterministic representation of modules and methods
4. **Guidance Generation**: Implement middleware that intercepts errors and generates guidance before forwarding

### 9.2 Client Implementation

1. **Schema Caching**: Store in appropriate location (XDG_CACHE_HOME on Linux, ~/Library/Caches on macOS)
2. **Lazy Loading**: Fetch module schemas on-demand rather than all at once
3. **Graceful Degradation**: Handle schema fetch failures by falling back to raw JSON input
4. **Template Rendering**: Consider supporting output templates keyed by content_type

## 10. References

- [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119) - Key words for use in RFCs
- [JSON-RPC 2.0](https://www.jsonrpc.org/specification) - Base protocol specification
- [JSON Schema](https://json-schema.org/draft/2020-12/json-schema-core.html) - Schema specification
- [WebSocket Protocol](https://datatracker.ietf.org/doc/html/rfc6455) - Transport layer

## Appendix A: Content Type Registry

Services SHOULD use namespaced content types:

| Content Type | Description |
|--------------|-------------|
| `service.schema` | Module list response |
| `service.module_schema` | Module JSON Schema |
| `service.hash` | Service hash response |
| `{module}.{operation}` | Module-specific data |

## Appendix B: JSON Schema Type Mapping

| JSON Schema | Suggested UI |
|-------------|--------------|
| `{"type": "string"}` | Text input |
| `{"type": "string", "format": "uuid"}` | UUID input |
| `{"type": "string", "format": "date-time"}` | Datetime picker |
| `{"type": "integer"}` | Numeric input |
| `{"type": "number"}` | Decimal input |
| `{"type": "boolean"}` | Checkbox/toggle |
| `{"type": "array"}` | List builder or JSON |
| `{"type": "object"}` | Nested form or JSON |
| `{"type": ["string", "null"]}` | Optional text input |
| `{"enum": [...]}` | Dropdown/select |
| `{"oneOf": [...]}` | Discriminated form |

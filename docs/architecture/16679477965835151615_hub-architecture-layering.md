# Hub Architecture and Layering

## Overview

The hub system provides a tree-structured RPC namespace with schema introspection and cross-language code generation.

**Mental Model:**
```
                      ┌─────────────────────────────────────┐
    External Calls    │         Hub Backend (e.g. Plexus)   │
    ─────────────────►│                                     │
    namespace.method  │   ┌───────────────────────────────┐ │
                      │   │  Internal Self-References     │ │
                      │   │  (plugins call other plugins) │ │
                      │   └───────────────────────────────┘ │
                      │                                     │
                      │   ┌───────────────────────────────┐ │
                      │   │  Per-Backend Storage          │ │
                      │   └───────────────────────────────┘ │
                      └─────────────────────────────────────┘
```

- **Externally**: Users call into a tree-like namespace (`arbor.tree_create`, `cone.chat`)
- **Internally**: The hub references itself to compose functionality
- **Storage**: Each backend manages its own persistence
- **Multiplicity**: Multiple hub backends can coexist

## Layer Breakdown

```
┌────────────────────────────────────────────────────────────────┐
│                        Hub Backends                            │
│  (plexus, future: remote hubs via URL)                        │
├────────────────────────────────────────────────────────────────┤
│                         hub-macro                              │
│  #[hub_methods] #[hub_method(streaming)]                      │
│  → generates enums, schemas, streaming flags                  │
├────────────────────────────────────────────────────────────────┤
│                         hub-core                               │
│  Plexus routing, Activation trait, ChildRouter trait          │
│  PluginSchema, MethodSchema, streaming support                │
├────────────────────────────────────────────────────────────────┤
│                         substrate                              │
│  Foundation types, Handle, serialization                      │
└────────────────────────────────────────────────────────────────┘
```

### substrate
Foundation layer providing:
- Core types (`Handle`, `Value`)
- Serialization primitives
- No RPC knowledge

### hub-core
RPC routing and schema infrastructure:
- `Plexus`: Central router, method dispatch
- `Activation` trait: Interface for callable plugins
- `ChildRouter` trait: Nested plugin delegation
- `PluginSchema` / `MethodSchema`: Runtime schema introspection
- Streaming support infrastructure

### hub-macro
Procedural macros that generate boilerplate:
- `#[hub_methods]`: Marks an impl block as a plugin
- `#[hub_method(streaming)]`: Marks methods, enables streaming
- Generates: method enum variants, schema extraction, streaming flags

### Hub Backends
Concrete implementations (e.g., Plexus):
- Trees of plugins with internal self-references
- Backend-specific storage
- Future: remote hubs as plugins (URL-based)

## Tree Structure

### External Namespace

```
plexus
├── arbor
│   ├── tree_create
│   ├── tree_get
│   └── node_create_text
├── cone
│   ├── create
│   ├── chat
│   └── delete
├── echo
│   ├── echo
│   └── once
└── health
    ├── check
    └── schema
```

Calls use dot-separated paths: `arbor.tree_create`, `cone.chat`

### Internal Structure

```rust
trait ChildRouter {
    type Method;
    fn route_child(&self, method: &str) -> Option<Box<dyn Activation>>;
    fn child_summary(&self) -> ChildSummary;
}

struct ChildSummary {
    children: Vec<(String, ChildSummary)>,
    methods: Vec<MethodSummary>,
    hash: u64,  // Content-addressable schema hash
}
```

**Hash Propagation:**
- Each method has a hash derived from its schema
- Parent hashes incorporate child hashes
- Root hash changes when any descendant changes
- Enables cache invalidation and client version detection

## Schema Flow

```
┌──────────────────────────────────────────────────────────────────────┐
│                           Schema Pipeline                            │
└──────────────────────────────────────────────────────────────────────┘

   Rust Plugin                hub-macro              Runtime Schema
  ┌──────────┐              ┌──────────┐             ┌──────────┐
  │ impl Foo │──────────────│ proc-    │─────────────│ Plugin   │
  │ {        │  #[hub_      │ macro    │  generates  │ Schema   │
  │   fn x() │  methods]    │ expand   │  schema()   │ JSON     │
  │ }        │              │          │  method     │          │
  └──────────┘              └──────────┘             └──────────┘
                                                          │
                                                          ▼
                            ┌──────────────────────────────────────────┐
                            │               Synapse (Haskell)          │
                            │  Reads schema, parses, emits IR          │
                            │  synapse --emit-ir                       │
                            └──────────────────────────────────────────┘
                                                          │
                                                          ▼
                            ┌──────────────────────────────────────────┐
                            │               hub-codegen (Rust)         │
                            │  Consumes IR, generates TypeScript       │
                            └──────────────────────────────────────────┘
                                                          │
                                                          ▼
                            ┌──────────────────────────────────────────┐
                            │           TypeScript Client              │
                            │  Type-safe RPC calls                     │
                            └──────────────────────────────────────────┘
```

**Key Schema Types:**
```rust
struct PluginSchema {
    plugin_id: Uuid,
    methods: Vec<MethodSchema>,
    children: Vec<PluginSchema>,
    hash: u64,
}

struct MethodSchema {
    name: String,
    params: JsonSchema,
    returns: JsonSchema,
    streaming: bool,
}
```

## Multi-Hub Vision

**Current:** All plugins in-process, single hub backend.

**Future:** Hubs reference other hubs as plugins via URL.

```
┌─────────────────────┐          ┌─────────────────────┐
│     Local Hub       │          │    Remote Hub       │
│                     │  HTTP/   │                     │
│  ┌───────────────┐  │  SSE     │  ┌───────────────┐  │
│  │ local.plugin  │  │◄────────►│  │ remote.plugin │  │
│  └───────────────┘  │          │  └───────────────┘  │
│                     │          │                     │
│  ┌───────────────┐  │          └─────────────────────┘
│  │ remote@url    │──┼──────────────────────┘
│  │ (proxy)       │  │
│  └───────────────┘  │
│                     │
└─────────────────────┘
```

**Requirements for multi-hub:**
1. Transport envelope for cross-hub calls
2. Schema federation (remote schemas appear local)
3. Streaming across network boundary
4. Authentication/authorization layer

## Current State

| Component   | Status | Notes                                      |
|-------------|--------|--------------------------------------------|
| hub-core    | Done   | Plexus, Activation, ChildRouter, schemas   |
| hub-macro   | Done   | Streaming attribute works                  |
| synapse     | Done   | IR emission complete                       |
| hub-codegen | Partial| Types done, namespace generator pending    |
| Multi-hub   | Future | URL-based hub references not implemented   |

## See Also

- `16679517135570018559_multi-hub-transport-envelope.md` - Transport design for multi-hub
- `16679536519907867647_plexus-call-streaming.md` - Streaming implementation
- `16680975879064433663_substrate-architecture.md` - Foundation layer

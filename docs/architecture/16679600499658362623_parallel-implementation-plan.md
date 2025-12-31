# Parallel Implementation Plan: Typed Client Generation

**Status**: Phases 1-3 Complete
**Scope**: Multi-agent orchestrated implementation
**Updated**: 2025-12-31

## Progress Summary

| Agent | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
|-------|---------|---------|---------|---------|
| A (Protocol) | ✅ | ✅ | ✅ | Pending |
| B (Transpiler) | ✅ | Pending | Pending | Pending |
| C (Arbor) | ✅ Design | Pending | Pending | Pending |

### Completed Commits
- `hub-core:2c1d36c` - feat: add streaming field to MethodSchema
- `hub-macro:11e3b0b` - feat: emit streaming flag in method schemas
- `hub-macro:ee81b30` - feat: add explicit streaming attribute to hub_method
- `substrate-protocol:eeebff5` - feat: add streaming field to MethodSchema (Haskell)
- `substrate:a08ab38` - feat: mark streaming methods with explicit attribute

### Key Design Decision
Changed from **inferred streaming** (based on `stream_item_type.is_some()`) to **explicit streaming** attribute. All methods return `impl Stream`, so inference doesn't work. Methods must now use `#[hub_method(streaming)]` to mark multi-event streams.

## Overview

This plan maximizes parallelization by identifying independent workstreams that can proceed concurrently. We use multiple agents, each with a focused scope, coordinating through well-defined interfaces.

```
                    ┌─────────────────────────────────────────────┐
                    │              PHASE 1: FOUNDATION            │
                    │         (All agents start immediately)       │
                    └─────────────────────────────────────────────┘
                                         │
        ┌────────────────────────────────┼────────────────────────────────┐
        │                                │                                │
        ▼                                ▼                                ▼
   ┌─────────┐                     ┌─────────┐                     ┌─────────┐
   │ AGENT A │                     │ AGENT B │                     │ AGENT C │
   │ Protocol│                     │Transpiler                     │  Arbor  │
   │ Changes │                     │  Setup  │                     │ Cleanup │
   └────┬────┘                     └────┬────┘                     └────┬────┘
        │                                │                                │
        │ MethodSchema.streaming         │ Project scaffold               │ Design per-method
        │ + with_streaming()             │ + mock IR                      │ return types
        │                                │                                │
        ▼                                ▼                                ▼
                    ┌─────────────────────────────────────────────┐
                    │              PHASE 2: EMISSION              │
                    │      (Agents continue independently)         │
                    └─────────────────────────────────────────────┘
        │                                │                                │
        ▼                                ▼                                ▼
   hub-macro emits              Type generator                   Implement new
   streaming flag               (structs, enums)                 arbor types
        │                                │                                │
        ▼                                ▼                                ▼
                    ┌─────────────────────────────────────────────┐
                    │              PHASE 3: CONSUMPTION           │
                    │         (Integration points)                 │
                    └─────────────────────────────────────────────┘
        │                                │                                │
        ▼                                ▼                                ▼
   Synapse reads                 Namespace generator              Arbor tests
   streaming flag                + hub factory                    + validation
        │                                │                                │
        └────────────────────────────────┼────────────────────────────────┘
                                         │
                                         ▼
                    ┌─────────────────────────────────────────────┐
                    │              PHASE 4: INTEGRATION           │
                    │           (All streams converge)             │
                    └─────────────────────────────────────────────┘
                                         │
                                         ▼
                              End-to-end pipeline
                              synapse --emit-ir | hub-codegen
```

## Agent Assignments

### Agent A: Protocol (substrate + hub-macro)

**Scope**: Add streaming flag to schema, emit from macro

**Files**:
- `substrate/src/plexus/schema.rs`
- `hub-macro/src/codegen/method_enum.rs`
- `hub-macro/src/parse.rs` (if needed)

**Interface contract**: After Phase 2, `synapse --schema cone chat` returns JSON with `"streaming": true`

### Agent B: Transpiler (new hub-codegen package)

**Scope**: Build TypeScript code generator from IR

**Files**:
- `hub-codegen/` (new package)
- Can work with mock IR initially

**Interface contract**: Consumes IR JSON, produces `generated/` directory with compilable TypeScript

### Agent C: Arbor Cleanup (substrate)

**Scope**: Replace shared `ArborEvent` with per-method return types

**Files**:
- `substrate/src/activations/arbor/activation.rs`
- `substrate/src/activations/arbor/types.rs` (new)

**Interface contract**: Each arbor method has its own `FooResult` type with 1-2 variants

---

## Phase 1: Foundation

**Duration**: Can start immediately, all agents work in parallel

### Agent A: MethodSchema.streaming

```rust
// substrate/src/plexus/schema.rs

pub struct MethodSchema {
    pub name: String,
    pub description: String,
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<schemars::Schema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returns: Option<schemars::Schema>,
    pub streaming: bool,  // ADD
}

impl MethodSchema {
    pub fn new(name: String, description: String, hash: String) -> Self {
        Self {
            name,
            description,
            hash,
            params: None,
            returns: None,
            streaming: false,  // ADD default
        }
    }

    pub fn with_streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }
}
```

**Validation**: `cargo build` passes, existing tests pass

### Agent B: Transpiler Scaffold

```
hub-codegen/
├── Cargo.toml
├── src/
│   ├── main.rs           # CLI entry point
│   ├── lib.rs            # Library exports
│   ├── ir.rs             # IR types (mirror Synapse IR)
│   ├── generator/
│   │   ├── mod.rs
│   │   ├── types.rs      # Type generation
│   │   ├── namespaces.rs # Namespace generation
│   │   └── transport.rs  # Transport template
│   └── templates/        # Handlebars/string templates
└── tests/
    ├── mock_ir.json      # Snapshot of real IR for testing
    └── integration.rs
```

**Mock IR**: Capture current `synapse --emit-ir cone` output as test fixture

**Validation**: `cargo build` passes, can parse mock IR

### Agent C: Arbor Type Design

Design document (not code yet):

```rust
// Current (problematic):
pub enum ArborEvent {
    TreeCreated { ... },
    TreeDeleted { ... },
    NodeCreated { ... },
    // ... 30 variants total
    Error { message: String },
}

// Proposed (per-method):
pub enum TreeCreateResult {
    TreeCreated { tree_id: Uuid, root_node: Uuid },
    Error { message: String },
}

pub enum TreeGetResult {
    TreeData { tree: Tree },
    Error { message: String },
}

pub enum NodeCreateResult {
    NodeCreated { node_id: Uuid, tree_id: Uuid },
    Error { message: String },
}
// etc.
```

**Validation**: Design reviewed, variant names match method semantics

---

## Phase 2: Emission

**Depends on**: Phase 1 completion for each agent
**Agents continue independently**

### Agent A: hub-macro Emits Streaming ✅ COMPLETE

**Implementation**: Changed from inference to explicit attribute.

```rust
// hub-macro/src/parse.rs - Added to HubMethodAttrs and MethodInfo:
pub streaming: bool,

// hub-macro/src/codegen/method_enum.rs - Uses explicit flag:
let streaming_flags: Vec<bool> = methods
    .iter()
    .map(|m| m.streaming)  // From #[hub_method(streaming)] attribute
    .collect();

// substrate/src/activations/cone/activation.rs - Usage:
#[hub_macro::hub_method(
    streaming,  // <-- Explicit marker for multi-event streams
    params(...)
)]
async fn chat(...) -> impl Stream<Item = ChatEvent> { ... }
```

**Verified**:
```bash
synapse --schema cone | jq '.methods[] | {name, streaming}'
# chat: true, all others: false
```

### Agent B: Type Generator

```rust
// hub-codegen/src/generator/types.rs

pub fn generate_types(ir: &IR) -> String {
    let mut output = String::new();

    for (name, typedef) in &ir.types {
        match &typedef.kind {
            TypeKind::Struct { fields } => {
                output.push_str(&generate_interface(name, fields, &typedef.description));
            }
            TypeKind::Enum { discriminator, variants } => {
                output.push_str(&generate_union(name, discriminator, variants, &typedef.description));
                output.push_str(&generate_type_guards(name, variants));
            }
            TypeKind::Primitive { ty, format } => {
                output.push_str(&generate_type_alias(name, ty, format));
            }
            _ => {}
        }
    }

    output
}

fn generate_interface(name: &str, fields: &[FieldDef], desc: &Option<String>) -> String {
    // ... generate TypeScript interface
}

fn generate_union(name: &str, disc: &str, variants: &[VariantDef], desc: &Option<String>) -> String {
    // ... generate discriminated union + individual interfaces
}

fn generate_type_guards(name: &str, variants: &[VariantDef]) -> String {
    // ... generate isFoo() type guard functions
}
```

**Validation**: Generated `types.ts` compiles with `tsc --noEmit`

### Agent C: Implement Arbor Types

```rust
// substrate/src/activations/arbor/types.rs

use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeCreateResult {
    TreeCreated {
        tree_id: Uuid,
        root_node_id: Uuid,
    },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeGetResult {
    TreeData {
        tree: crate::arbor::Tree,
    },
    Error { message: String },
}

// ... one for each method
```

Update activation to use new types:
```rust
#[hub_method]
async fn tree_create(&self, ...) -> impl Stream<Item = TreeCreateResult> {
    stream! {
        yield TreeCreateResult::TreeCreated { tree_id, root_node_id };
    }
}
```

**Validation**: `cargo test -p substrate` passes, schemas show per-method types

---

## Phase 3: Consumption

**Depends on**: Phase 2 completion
**Integration begins**

### Agent A: Synapse Reads Streaming Flag ✅ COMPLETE

**Implementation**: Added `methodStreaming` to Haskell `MethodSchema` in substrate-protocol.

```haskell
-- substrate-protocol/src/Plexus/Schema/Recursive.hs

data MethodSchema = MethodSchema
  { methodName        :: Text
  , methodDescription :: Text
  , methodHash        :: PluginHash
  , methodParams      :: Maybe Value
  , methodReturns     :: Maybe Value
  , methodStreaming   :: Bool  -- ADDED
  } deriving stock (Show, Eq, Generic)

instance FromJSON MethodSchema where
  parseJSON = withObject "MethodSchema" $ \o -> MethodSchema
    <$> o .: "name"
    <*> o .: "description"
    <*> o .: "hash"
    <*> o .:? "params"
    <*> o .:? "returns"
    <*> o .:? "streaming" .!= False  -- Backwards compatible
```

**Verified**:
```bash
synapse --schema cone | jq '.methods[] | {name, streaming}'
# {name: "chat", streaming: true}
# {name: "create", streaming: false}
# ... all others false
```

### Agent B: Namespace Generator

```rust
// hub-codegen/src/generator/namespaces.rs

pub fn generate_namespace(plugin: &str, methods: &[&MethodDef], types: &HashMap<String, TypeDef>) -> String {
    let mut output = String::new();

    // Interface
    output.push_str(&format!("export interface {}Namespace {{\n", pascal_case(plugin)));
    for method in methods {
        let return_type = if method.streaming {
            format!("AsyncGenerator<{}>", type_ref_to_ts(&method.returns))
        } else {
            format!("Promise<{}>", type_ref_to_ts(&method.returns))
        };
        output.push_str(&format!(
            "  /** {} */\n  {}({}): {};\n",
            method.description.as_deref().unwrap_or(""),
            method.name,
            params_to_ts(&method.params),
            return_type
        ));
    }
    output.push_str("}\n\n");

    // Implementation
    output.push_str(&generate_namespace_impl(plugin, methods));

    output
}
```

**Validation**: Generated namespaces compile, match hub structure

### Agent C: Arbor Validation

```rust
// substrate/src/activations/arbor/tests.rs

#[tokio::test]
async fn test_tree_create_returns_typed_result() {
    let arbor = Arbor::new(...);
    let stream = arbor.tree_create("system", None).await;
    let items: Vec<_> = stream.collect().await;

    assert_eq!(items.len(), 1);
    match &items[0] {
        TreeCreateResult::TreeCreated { tree_id, root_node_id } => {
            assert!(!tree_id.is_nil());
            assert!(!root_node_id.is_nil());
        }
        TreeCreateResult::Error { message } => panic!("Unexpected error: {}", message),
    }
}
```

**Validation**: All arbor tests pass, IR shows distinct return types per method

---

## Phase 4: Integration

**Depends on**: All Phase 3 work complete
**All agents converge**

### End-to-End Pipeline

```bash
# 1. Start hub with all changes
cargo run --bin substrate &

# 2. Generate IR (streaming flags now explicit)
synapse --emit-ir > ir.json

# 3. Generate TypeScript client
cd hub-codegen && cargo run -- ../ir.json -o ../generated

# 4. Verify TypeScript compiles
cd ../generated && npx tsc --noEmit

# 5. Run integration test
npm test
```

### Integration Test

```typescript
// generated/tests/integration.test.ts
import { createHub } from "../";

describe("Generated Hub Client", () => {
  let hub: Hub;

  beforeAll(async () => {
    hub = await createHub({ url: "ws://localhost:4444" });
  });

  test("cone.create returns Promise (non-streaming)", async () => {
    const result = await hub.cone.create({
      name: "test-agent",
      model_id: "gpt-4o-mini",
    });
    expect(result.type).toBe("cone_created");
  });

  test("cone.chat returns AsyncGenerator (streaming)", async () => {
    const events: ChatEvent[] = [];
    for await (const event of hub.cone.chat({
      identifier: { by_name: "test-agent" },
      prompt: "Say hello",
    })) {
      events.push(event);
    }
    expect(events.some(e => e.type === "chat_start")).toBe(true);
    expect(events.some(e => e.type === "chat_complete")).toBe(true);
  });

  test("arbor.tree_create has typed result", async () => {
    const result = await hub.arbor.tree_create({ owner_id: "test" });
    // TypeScript knows this is TreeCreateResult, not ArborEvent
    if (result.type === "tree_created") {
      expect(result.tree_id).toBeDefined();
    }
  });
});
```

---

## Coordination Protocol

### Handoff Points

| From | To | Artifact | Signal |
|------|-----|----------|--------|
| Agent A Phase 1 | Agent A Phase 2 | `MethodSchema` with `streaming` field | PR merged to substrate |
| Agent B Phase 1 | Agent B Phase 2 | Scaffold with mock IR parsing | `cargo build` passes |
| Agent C Phase 1 | Agent C Phase 2 | Design doc approved | Doc committed |
| Agent A Phase 2 | Agent A Phase 3 | Schema emits `streaming` | `synapse --schema` shows flag |
| All Phase 3 | Phase 4 | All components ready | Green CI on all repos |

### Parallel Work Matrix

```
              │ Phase 1 │ Phase 2 │ Phase 3 │ Phase 4 │
──────────────┼─────────┼─────────┼─────────┼─────────┤
Agent A       │   ██    │   ██    │   ██    │         │
(Protocol)    │         │         │         │   ██    │
──────────────┼─────────┼─────────┼─────────┤ (all    │
Agent B       │   ██    │   ██    │   ██    │ agents  │
(Transpiler)  │         │         │         │ converge│
──────────────┼─────────┼─────────┼─────────┤    )    │
Agent C       │   ██    │   ██    │   ██    │         │
(Arbor)       │         │         │         │         │
──────────────┴─────────┴─────────┴─────────┴─────────┘
```

### Blocking Dependencies

```
Agent A Phase 1 ─────┬───► Agent A Phase 2 ───► Agent A Phase 3
                     │                                  │
                     │                                  ▼
                     │                          Synapse reads flag
                     │
Agent B Phase 1 ─────┼───► Agent B Phase 2 ───► Agent B Phase 3
(can use mock IR)    │     (can use mock IR)    (needs real IR)
                     │
Agent C Phase 1 ─────┴───► Agent C Phase 2 ───► Agent C Phase 3
(design only)              (implementation)     (validation)
```

**Critical path**: Agent A → Synapse reads flag → Agent B uses real IR

**Parallel paths**: Agent B can progress through Phase 2 with mock IR. Agent C is fully independent until Phase 4 integration.

---

## Risk Mitigation

### Risk: Mock IR diverges from real IR

**Mitigation**: Agent B captures real IR snapshot at Phase 1 start. Re-snapshot at Phase 3 start to catch any drift.

### Risk: Arbor changes break existing consumers

**Mitigation**: Agent C maintains backwards-compatible serialization (same JSON shape, just more specific types). Add deprecation warnings if needed.

### Risk: Streaming flag interpretation differs

**Mitigation**: Single source of truth in arch doc. All agents reference same definition:
- `streaming: true` → multiple events expected → `AsyncGenerator<T>`
- `streaming: false` → single event expected → `Promise<T>`

---

## Success Metrics

- [x] All three agents complete Phase 1 within same sprint
- [x] No blocking dependencies cause idle time > 1 day
- [ ] End-to-end pipeline works on first integration attempt
- [ ] Generated TypeScript has zero type errors
- [ ] Schema hash verification catches version mismatch

## Next Steps

1. **Agent B Phase 2**: Implement type generator using hub-codegen scaffold
2. **Agent B Phase 3**: Implement namespace generator with streaming-aware return types
3. **Agent C Phase 2**: Implement per-method return types for arbor (design in `16679574983150601727_arbor-per-method-types-design.md`)
4. **Phase 4**: End-to-end integration test

---

## Agent Launch Commands

```bash
# Agent A: Protocol
# Working directory: substrate + hub-macro
# Focus: MethodSchema.streaming, macro emission, synapse consumption

# Agent B: Transpiler
# Working directory: hub-codegen (new)
# Focus: IR parsing, type generation, namespace generation

# Agent C: Arbor
# Working directory: substrate/src/activations/arbor
# Focus: Per-method return types, backwards compatibility
```

Each agent should:
1. Read this plan and the compiler-architecture doc
2. Implement their Phase 1 work
3. Signal completion via commit message: `[PHASE-1-COMPLETE] <agent> <summary>`
4. Proceed to Phase 2 without waiting (except for explicit dependencies)

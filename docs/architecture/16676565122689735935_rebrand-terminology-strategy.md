# Rebrand Terminology Strategy

> **⚠️ HISTORICAL DOCUMENT**: This document explored multiple naming options. The final decision was **Plexus RPC**. See [16676565123400000000_plexus-rpc-ecosystem-naming.md](./16676565123400000000_plexus-rpc-ecosystem-naming.md) for the complete naming strategy.

## Executive Summary

This document establishes a clear, consistent terminology across the hypermemetic stack to prepare for public launch. It resolves naming conflicts, positions the project against competitors, and provides migration guidance.

**Key Decisions:**
- **"Plexus"** → Reserved for ONE SPECIFIC agent execution hub (not a generic term)
- **Protocol** → "Hub Protocol" (the RPC protocol itself)
- **Library Suite** → Keep "hub-*" names for technical continuity
- **Positioning** → "Write Rust services, get type-safe clients. Zero drift, instant streaming."

---

## The Problem: Current Terminology Confusion

### Overloaded Terms

| Term | Current Meanings | Confusion Level |
|------|-----------------|-----------------|
| **Plexus** | 1. Generic coordination layer<br>2. DynamicHub type alias<br>3. The substrate instance | HIGH |
| **Hub** | 1. Library prefix (hub-core)<br>2. Activation with children<br>3. Legacy term for coordination<br>4. Backend instance in synapse | HIGH |
| **Plugin** | Used informally for Activation | MEDIUM |
| **Protocol** | Unnamed (referred to as "WebSocket RPC", "the protocol", etc.) | HIGH |

### Technical Language Barriers

Current messaging is heavy on:
- Category theory references
- Academic terminology ("stratified systems theory", "physical symbol system")
- Architecture-first explanations (should be benefits-first)

---

## The Solution: New Terminology Standard

### Core Naming Conventions

```
┌─────────────────────────────────────────────────────────────────┐
│                    HYPERMEMETIC STACK                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Hub Protocol                          │  │
│  │  The RPC protocol: method routing, streaming, schemas   │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    hub-* Libraries                       │  │
│  │  • hub-core      - Activation trait, DynamicHub         │  │
│  │  • hub-macro     - Proc macro for Activations           │  │
│  │  • hub-transport - Transport implementations            │  │
│  │  • hub-codegen   - Client SDK generation                │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Substrate                             │  │
│  │  A specific backend server using Hub Protocol           │  │
│  │  Contains: Arbor, Cone, ClaudeCode, Bash, etc.         │  │
│  │  Uses: DynamicHub to route to activations               │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Plexus (Future)                       │  │
│  │  ONE SPECIFIC activation for agent execution            │  │
│  │  Not a generic term - a concrete agent orchestrator     │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Synapse                               │  │
│  │  CLI that auto-generates commands from Hub Protocol     │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Terminology Glossary

| Term | Definition | Usage |
|------|------------|-------|
| **Hub Protocol** | The RPC protocol for services with runtime schemas, streaming, and tree-structured namespaces | "Services communicate via Hub Protocol" |
| **Activation** | A service/capability that implements the `Activation` trait | "Arbor is an activation" |
| **DynamicHub** | An activation that routes to other activations via `.register()` | "Substrate uses DynamicHub to host multiple activations" |
| **Backend** | A server instance accessible via Hub Protocol | "Synapse connects to the substrate backend" |
| **Hub Activation** | An activation with children (implements `ChildRouter`) | "Solar is a hub activation with planet children" |
| **Plexus** | (Reserved) A future specific activation for agent execution | "Plexus manages multi-agent workflows" |
| **Substrate** | The main reference implementation backend | "Substrate provides Arbor, Cone, and other activations" |
| **Synapse** | Schema-driven CLI for Hub Protocol backends | "Use synapse to interact with any backend" |

### What Changes, What Stays

**CHANGE:**
- ❌ "Plexus" as generic term → ✅ "Hub Protocol" or "DynamicHub" (context-dependent)
- ❌ "Plugin" → ✅ "Activation" (consistently)
- ❌ "The hub" (ambiguous) → ✅ "Backend" or "DynamicHub" (context-dependent)
- ❌ Architecture-first docs → ✅ Benefits-first, architecture later

**KEEP:**
- ✅ `hub-core`, `hub-macro`, etc. (library names - changing would break dependencies)
- ✅ "Activation" trait (well-established, documented)
- ✅ Technical accuracy in architecture docs (just move them to internals)

---

## Positioning vs Competitors

### The One-Liner

**"Write Rust services, get type-safe clients. Zero drift, instant streaming."**

### Comparison Table

| Feature | Hub Protocol | gRPC | OpenAPI/REST | tRPC | GraphQL |
|---------|-------------|------|--------------|------|---------|
| **Schema source** | Rust code | .proto files | YAML/annotations | TypeScript code | .graphql schema |
| **Streaming** | Built-in (every method) | Bolt-on (special setup) | SSE (separate) | Limited | Subscriptions (complex) |
| **Type safety** | Rust → TS/clients | ✓ (with codegen) | ❌ (runtime only) | ✓ (TS only) | ✓ (with codegen) |
| **Schema drift** | Impossible (hash-based) | Easy (proto vs code) | Very easy | Easy (mono repo only) | Easy (schema vs resolvers) |
| **Runtime introspection** | Full (dynamic CLI) | Limited | Swagger UI | ❌ | GraphiQL |
| **Tree namespaces** | Native | ❌ | ❌ | ❌ | ❌ |
| **Progress events** | Built-in | Manual | Manual | Manual | Manual |
| **Error handling** | Structured (PlexusStreamItem) | Status codes | HTTP codes | Exceptions | Errors array |

### Key Differentiators

1. **Code IS Schema**: No separate IDL files. Your Rust types are the source of truth.
   ```rust
   // This is your schema AND implementation
   #[hub_method]
   async fn create_user(&self, email: String, name: String) -> impl Stream<Item = UserEvent>
   ```

2. **Streaming Everywhere**: Every method returns a stream. Non-streaming methods just emit one item.
   ```rust
   // Same infrastructure for both
   yield UserEvent::Created { id };                    // Simple
   yield UserEvent::Progress { percent: 50 };          // Streaming
   yield UserEvent::Error { message, recoverable };   // Structured errors
   ```

3. **Zero Drift Guarantee**: Content-addressed schemas. If the hash matches, the schema matches.

4. **Self-Documenting**: CLIs and clients auto-generate from runtime schemas.

### Target Audiences

| Audience | Pain Point | Hub Protocol Solution |
|----------|-----------|----------------------|
| **Backend devs** | "I change my API, forget to update OpenAPI, clients break" | Code IS schema - can't drift |
| **Frontend devs** | "The API changed, my types are wrong, I found out at runtime" | Hash-based cache invalidation, type-safe clients |
| **DevOps** | "Need to version gRPC protos, deploy coordinated updates" | Backwards compat via hash checking |
| **CLI users** | "Need to read API docs to know what commands exist" | `synapse --help` generates from runtime schema |

---

## Positioning Statement

### For Technical Users

> **Hub Protocol is an RPC framework where Rust services automatically expose type-safe schemas at runtime.**
>
> Write your service logic in Rust with regular types and doc comments. Hub Protocol generates JSON schemas, type-safe TypeScript clients, and even dynamic CLIs - all from your code. Every method streams by default, so progress reporting and error handling are built-in, not bolt-on.
>
> Unlike gRPC (`.proto` files) or OpenAPI (YAML annotations), there's no separate IDL to maintain. Unlike tRPC, you're not locked into TypeScript. Unlike GraphQL, you don't write resolvers separate from your schema.

### For Business Users

> **Ship features faster without breaking clients.**
>
> Traditional APIs require maintaining schema files separately from code, leading to drift and runtime errors. Hub Protocol eliminates schema files - your Rust code IS the schema. Type-safe clients auto-generate. CLIs auto-generate. API docs auto-generate. Change your code, push, done.

---

## Migration Plan

### Phase 1: Terminology Cleanup (No Breaking Changes)

**substrate project:**
1. Update docs to use "Hub Protocol" for the protocol itself
2. Update docs to use "DynamicHub" (not "Plexus") for the routing type
3. Add glossary to CLAUDE.md
4. Keep code unchanged (aliases remain)

**hub-core project:**
1. Add deprecation notices on `Plexus` type alias
2. Update README to explain DynamicHub clearly
3. Document "Hub Protocol" as the protocol name

**synapse project:**
1. Update README to say "Hub Protocol backends" not "hubs"
2. Keep CLI commands unchanged

### Phase 2: New Messaging (Content Changes)

**All projects:**
1. Root READMEs: Benefits-first, architecture later
2. Move category theory to internals
3. Add comparison tables vs gRPC/OpenAPI/tRPC
4. Update Cargo.toml descriptions

### Phase 3: Reserve "Plexus" Name (Future)

When creating the agent execution activation:
1. Name it `Plexus` (the specific activation)
2. Update docs: "Plexus is an activation for orchestrating multi-agent workflows"
3. This becomes the ONLY public use of "Plexus"

### Phase 4: Code Cleanup (Breaking Changes - Post 1.0)

Consider (much later):
1. Remove `Plexus` type alias from hub-core
2. Rename any remaining internal uses of "plexus" (modules, etc.)
3. Major version bump

---

## Messaging Guidelines

### DO:
- ✅ Lead with practical benefits ("Write Rust, get TypeScript SDKs")
- ✅ Show code examples early
- ✅ Compare to familiar tools (gRPC, OpenAPI)
- ✅ Use "activation" consistently
- ✅ Say "Hub Protocol" for the protocol
- ✅ Front-load value, architecture later

### DON'T:
- ❌ Use "Plexus" generically (except in existing code comments)
- ❌ Say "plugin" (use "activation")
- ❌ Lead with category theory or SST
- ❌ Assume users know what an "introspective RPC protocol" is
- ❌ Use "hub" ambiguously (specify: DynamicHub, backend, or hub-* library)

---

## Example Messaging

### Before (Current)

> "Substrate provides a hierarchical plugin architecture where all methods expose JSON schemas at runtime. Plugins organize into trees via dot-separated namespaces."

**Problems:**
- What's "substrate"? A library? A server?
- "Hierarchical plugin architecture" - too abstract
- Why should I care?

### After (New)

> "Substrate is a backend server built with Hub Protocol. It provides conversation trees (Arbor), LLM orchestration (Cone), and shell execution (Bash) as activations. Connect with the Synapse CLI or use generated TypeScript clients."

**Better:**
- Concrete: it's a backend server
- Shows what it does (trees, LLMs, shell)
- Explains how to use it (CLI or clients)

---

## Implementation Checklist

### Documentation Updates

- [ ] Create this terminology strategy doc
- [ ] Update substrate/CLAUDE.md with glossary
- [ ] Update substrate/README.md with new messaging
- [ ] Update substrate/docs/REBRAND.md to reference this doc
- [ ] Update hub-core/README.md to clarify DynamicHub
- [ ] Update synapse/README.md to say "Hub Protocol"
- [ ] Move category theory to docs/internals/

### Content Creation

- [ ] Write docs/COMPARISON.md (vs gRPC, OpenAPI, tRPC, GraphQL)
- [ ] Write docs/QUICKSTART.md (5-minute hello world)
- [ ] Write docs/CONCEPTS.md (Activation, DynamicHub, Hub Protocol, Streaming)
- [ ] Create root README.md with 30-second pitch

### Code Changes (Non-Breaking)

- [ ] Add deprecation comment on `pub type Plexus = DynamicHub`
- [ ] Update Cargo.toml descriptions across all crates
- [ ] Add `#[doc(alias = "Plexus")]` to DynamicHub for search

### Examples

- [ ] Create examples/hello-world/ (minimal activation)
- [ ] Create examples/streaming-progress/ (progress bars)
- [ ] Create examples/typescript-client/ (using generated SDK)

---

## Open Questions

1. **Should we rename the stack?** "Hypermemetic" is abstract. Consider:
   - "Hub Framework" (generic, searchable)
   - "Hubris" (clever but might be too cute)
   - Keep "Hypermemetic" (philosophical grounding)

   **Recommendation**: Keep "Hypermemetic" as the GitHub org/project name, but lead with "Hub Protocol" in docs.

2. **Protocol versioning?** Should it be:
   - "Hub Protocol v1"
   - "Hub Protocol 2025"
   - Just "Hub Protocol"

   **Recommendation**: Just "Hub Protocol" for now. Add versioning if breaking changes needed.

3. **Alternative to "Activation"?** If we find it's confusing users, consider:
   - "Service" (generic, familiar)
   - "Endpoint" (REST-familiar)
   - "Capability" (abstract but clear)

   **Recommendation**: Keep "Activation" - it's well-documented and distinctive.

---

## Success Metrics

After rebrand:
1. ✅ New developer can explain "what is this?" in 30 seconds
2. ✅ "Plexus" no longer causes confusion (reserved for specific activation)
3. ✅ Comparison with gRPC/OpenAPI is explicit
4. ✅ Hub Protocol is clearly named
5. ✅ All docs use consistent terminology
6. ✅ Category theory is in internals, not front matter

---

## Related Documents

- `docs/REBRAND.md` - Phased rebrand execution plan
- `docs/architecture/16681208204800674559_plexus-activation-terminology.md` - Original terminology (now deprecated)
- `docs/architecture/16678373036159325695_plugin-development-guide.md` - Developer guide (needs terminology update)

---

## Approval & Timeline

**Status**: DRAFT - Awaiting user approval

**Timeline** (post-approval):
- Phase 1 (Terminology cleanup): 1 week
- Phase 2 (New messaging): 1 week
- Phase 3 (Reserve Plexus): When agent activation is built
- Phase 4 (Code cleanup): Post-1.0 only

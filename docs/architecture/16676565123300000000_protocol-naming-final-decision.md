# Protocol Naming: Final Decision Framework

## Constraint: Preserve Current Architecture

**What stays the same:**
- ✅ Monolithic substrate server (one process)
- ✅ DynamicHub routing to multiple activations
- ✅ All activations registered in one server
- ✅ Single WebSocket endpoint
- ✅ All current code structure

**What we're deciding:**
- The protocol name (what to call the RPC protocol itself)
- How to talk about it consistently

---

## Option 1: "Substrate Protocol"

### How It Works

```
Substrate Protocol = The RPC protocol spec
substrate (server) = Reference implementation of Substrate Protocol
```

### Natural Language

✅ **"Substrate is the reference implementation of Substrate Protocol"**
✅ **"This implements Substrate Protocol"**
✅ **"Substrate Protocol uses streaming by default"**
⚠️  **"The substrate server"** - could mean any impl or THE substrate impl
⚠️  **"Connect to substrate"** - the protocol or the server?

### Documentation Style

```markdown
# Substrate

Substrate is the reference implementation of **Substrate Protocol**.

## What is Substrate Protocol?

Substrate Protocol is an RPC protocol where code IS schema...

## Running Substrate

To run the substrate server:

```bash
cargo run
```

## Building Your Own Substrate Protocol Server

Use the hub-* libraries to implement Substrate Protocol...
```

### Pros
- ✅ "Substrate" branding extends to protocol
- ✅ Reinforces "substrate" as foundation metaphor
- ✅ Simple: one name for the whole ecosystem

### Cons
- ⚠️ Some ambiguity: "substrate" means both server AND protocol
- ⚠️ Requires clarification in docs ("Substrate (the server)" vs "Substrate Protocol")
- ⚠️ If others implement the protocol, do they call themselves "substrate implementations"?

### Resolution Strategy

**Use explicit clarifiers:**
- "Substrate Protocol" = always the protocol
- "substrate server" or "substrate implementation" = the specific server
- In casual conversation, context disambiguates

---

## Option 2: "Plexus RPC"

### How It Works

```
Plexus RPC = The RPC protocol spec
substrate = A server implementing Plexus RPC
```

### Natural Language

✅ **"Substrate implements Plexus RPC"**
✅ **"Substrate is a Plexus RPC server"**
✅ **"Connect to the substrate server"** - unambiguous
✅ **"Build your own Plexus RPC server"** - clear you're not using substrate

### Documentation Style

```markdown
# Substrate

Substrate is a **Plexus RPC server** providing conversation trees and LLM orchestration.

## What is Plexus RPC?

Plexus RPC is a protocol where code IS schema...

## Running Substrate

```bash
cargo run
```

## Building Your Own Plexus RPC Server

Use the hub-* libraries to implement Plexus RPC...
```

### Pros
- ✅ Zero ambiguity: "substrate" = server, "Plexus RPC" = protocol
- ✅ "Plexus" has meaning (nerve network, coordination)
- ✅ Aligns with plexus_schema, plexus_call RPC methods
- ✅ Distinctive, searchable brand

### Cons
- ⚠️ Introduces new term ("Plexus") for the protocol
- ⚠️ "substrate" becomes just one implementation name

---

## Option 3: Other Names

### "Hub Protocol"
```
Hub Protocol = The RPC protocol
substrate = A Hub Protocol server
hub-core, hub-macro = Hub Protocol libraries
```

**Pros:** Aligns with hub-* library names
**Cons:** "Hub" is generic, not distinctive

### "Hypermemetic Protocol"
```
Hypermemetic Protocol = The RPC protocol
substrate = A Hypermemetic Protocol server
```

**Pros:** Uses existing project name
**Cons:** Abstract, hard to search, long to type

### "Membrane Protocol"
```
Membrane Protocol = The RPC protocol
substrate = A Membrane Protocol server
```

**Pros:** Relates to "schema as membrane" concept
**Cons:** Doesn't align with existing terminology

---

## Side-by-Side Comparison

### Documentation Readability

**Substrate Protocol:**
```markdown
Substrate is the reference implementation of Substrate Protocol.
Substrate Protocol enables type-safe RPC with runtime schemas.
The substrate server exposes activations via Substrate Protocol.

To connect to substrate:
  synapse connect localhost:4445
```

**Plexus RPC:**
```markdown
Substrate is a Plexus RPC server with conversation trees and LLM orchestration.
Plexus RPC enables type-safe RPC with runtime schemas.
The substrate server exposes activations via Plexus RPC.

To connect to substrate:
  synapse connect localhost:4445
```

### Library Descriptions

**Substrate Protocol:**
```toml
[package]
name = "hub-core"
description = "Core infrastructure for Substrate Protocol: Activation trait, routing, schemas"

[package]
name = "substrate"
description = "Reference Substrate Protocol server with conversation trees and LLM orchestration"
```

**Plexus RPC:**
```toml
[package]
name = "hub-core"
description = "Core infrastructure for Plexus RPC: Activation trait, routing, schemas"

[package]
name = "substrate"
description = "Plexus RPC server with conversation trees and LLM orchestration"
```

### Casual Conversation

**Substrate Protocol:**
```
dev1: "Is substrate running?"
dev2: "Yep, substrate on 4445"
dev1: "Cool, does it implement the latest Substrate Protocol changes?"
dev2: "Yeah, upgraded to Substrate Protocol v1.1"
```

**Plexus RPC:**
```
dev1: "Is substrate running?"
dev2: "Yep, substrate on 4445"
dev1: "Cool, does it implement the latest Plexus RPC changes?"
dev2: "Yeah, upgraded to Plexus RPC v1.1"
```

---

## Recommendations

### Recommendation A: "Substrate Protocol" (Unified Branding)

**Choose this if:**
- You want "substrate" to be THE brand for everything
- You're okay with some ambiguity (resolvable via context)
- You want maximum brand recognition

**Strategy:**
1. Always say "Substrate Protocol" when referring to the protocol
2. Say "substrate server" or "substrate implementation" for the server
3. Add glossary to docs clarifying the distinction
4. In code comments: use precise terms

**Positioning:**
"Substrate Protocol: Build services where code IS schema"

### Recommendation B: "Plexus RPC" (Clear Separation)

**Choose this if:**
- You want zero ambiguity between protocol and implementation
- You like the "plexus" metaphor (nerve network coordination)
- You want to encourage third-party implementations

**Strategy:**
1. Protocol is "Plexus RPC"
2. Substrate is "a Plexus RPC server" (the reference impl)
3. Libraries are "hub-*" (technical names, don't change)
4. Clear separation: Plexus RPC (protocol) ≠ substrate (implementation)

**Positioning:**
"Plexus RPC: Build services where code IS schema"
"Substrate: A Plexus RPC server for conversation trees and LLM orchestration"

---

## My Recommendation: Plexus RPC

### Why Plexus RPC Wins

1. **Zero Ambiguity**
   - "Plexus RPC" = protocol (always)
   - "substrate" = server (always)
   - No clarification needed

2. **Better for Ecosystem**
   - Others can build "Plexus RPC servers" without confusion
   - "I built a Plexus RPC server in Python" - clear
   - "I built a Substrate Protocol server" - sounds like you forked substrate

3. **Aligns with Existing Code**
   - `plexus_schema`, `plexus_call`, `plexus_hash` RPC methods
   - Makes sense: these are "Plexus RPC" methods

4. **Distinctive Brand**
   - "Plexus" has meaning (coordination, nerve network)
   - Searchable, unique
   - Not generic like "Hub Protocol"

5. **Natural Language**
   - "Substrate is a Plexus RPC server" flows naturally
   - "Connect to the substrate server" - unambiguous
   - "Build a Plexus RPC server" - clear invitation

### What About "Substrate" Branding?

**Substrate remains important:**
- It's the reference implementation
- It's the main server people will use
- It has strong positioning (foundation, substrate metaphor)

**But:**
- The protocol should have its own identity
- This enables ecosystem growth (other implementations)
- "Plexus RPC" and "Substrate" are complementary, not competing

---

## Implementation Strategy

### If We Choose "Substrate Protocol"

1. **Documentation:**
   - Always say "Substrate Protocol" for protocol
   - Add glossary: "Substrate (server)" vs "Substrate Protocol"
   - Update all docs to clarify

2. **Cargo.toml:**
   ```toml
   description = "Reference implementation of Substrate Protocol"
   ```

3. **README pattern:**
   ```markdown
   # Substrate

   The reference implementation of Substrate Protocol.

   ## What is Substrate Protocol?
   ...
   ```

### If We Choose "Plexus RPC"

1. **Documentation:**
   - Protocol docs: "Plexus RPC enables..."
   - Server docs: "Substrate is a Plexus RPC server..."
   - No ambiguity, no glossary needed

2. **Cargo.toml:**
   ```toml
   description = "Plexus RPC server with conversation trees and LLM orchestration"
   ```

3. **README pattern:**
   ```markdown
   # Substrate

   A Plexus RPC server for conversation trees and LLM orchestration.

   ## What is Plexus RPC?
   ...
   ```

---

## The Decision Point

**Question for you:**

Do you prefer:

**A) Substrate Protocol**
- Unified branding (substrate = everything)
- Some ambiguity (manageable with context)
- Stronger substrate brand identity

**B) Plexus RPC**
- Clear separation (protocol vs implementation)
- Zero ambiguity
- Enables ecosystem growth

**C) Something else?**
- Hub Protocol?
- Hypermemetic Protocol?
- Another name entirely?

---

## Next Steps (Once Decided)

1. **Update terminology strategy doc** with final choice
2. **Update CLAUDE.md** with glossary
3. **Begin Phase 1 rebrand** (docs only, no breaking changes)
4. **Write root README** with new messaging
5. **Update all crate descriptions**
6. **Create comparison docs** (vs gRPC, OpenAPI, tRPC)

---

## Quick Reference

### With "Substrate Protocol"

| Say this... | Not this... |
|-------------|-------------|
| "Substrate Protocol enables..." | "Substrate enables..." (ambiguous) |
| "substrate server on port 4445" | "substrate on port 4445" (ambiguous) |
| "Implements Substrate Protocol" | "Is a substrate" (unclear) |

### With "Plexus RPC"

| Say this... | Not this... |
|-------------|-------------|
| "Plexus RPC enables..." | Clear, no alternative needed |
| "substrate server on port 4445" | Clear, no alternative needed |
| "Implements Plexus RPC" | Clear, no alternative needed |

---

## My Vote: Plexus RPC

For all the reasons above, I recommend **Plexus RPC** as the protocol name, with **substrate** remaining the name of the reference implementation server.

But I'll implement whichever you prefer - what's your decision?

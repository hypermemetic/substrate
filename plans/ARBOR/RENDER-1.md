# RENDER-1: Arbor Render Enhancement Plan

## Goal

Enhance Arbor's `tree_render` to resolve external handles and display actual content instead of raw handle metadata.

## Current State

`tree_render` shows raw handles like:
```
└── [51b330e5-...@1.0.0::chat:msg-xxx:user:user]
    └── [51b330e5-...@1.0.0::chat:msg-yyy:assistant:assistant]
```

This is opaque and unhelpful for debugging or visualization.

## Desired State

New `tree_render_resolved` shows resolved content:
```
└── [user] Hello, how are you?
    └── [assistant] I'm doing well, thank you for asking...
```

## Dependency DAG

```
        RENDER-2 (Make Arbor generic over HubContext)
              │
      ┌───────┴───────┐
      ▼               ▼
  RENDER-3        RENDER-4
  (Expose         (Add tree_render_resolved
   resolve_handle  method)
   in ClaudeCode)
      │               │
      └───────┬───────┘
              ▼
          RENDER-5
          (Update builder.rs)
```

## Tickets

### RENDER-2: Make Arbor Generic Over HubContext
- **blocked_by:** []
- **unlocks:** [RENDER-3, RENDER-4]
- **Scope:** Add generic parameter and parent injection to Arbor
- **Reference:** `src/activations/claudecode/activation.rs` lines 21-68 for pattern

**Implementation:**
```rust
pub struct Arbor<P: HubContext = NoParent> {
    // existing fields...
    hub: Arc<OnceLock<P>>,
}

impl<P: HubContext> Arbor<P> {
    pub fn inject_parent(&self, parent: P) {
        self.hub.set(parent).ok();
    }
}
```

### RENDER-3: Expose resolve_handle in ClaudeCode
- **blocked_by:** [RENDER-2]
- **unlocks:** [RENDER-5]
- **Scope:** Make ClaudeCode's handle resolution accessible via HubContext

**Context:**
- ClaudeCode already has `resolve_message_handle()` in `storage.rs:516-532`
- Need to expose through activation layer for HubContext routing

### RENDER-4: Add tree_render_resolved Method
- **blocked_by:** [RENDER-2]
- **unlocks:** [RENDER-5]
- **Scope:** New Arbor method that resolves handles during render

**Implementation:**
```rust
pub async fn tree_render_resolved(&self, tree_id: Uuid) -> Result<String> {
    // Walk tree nodes
    // For External nodes: call hub.resolve_handle(handle)
    // Format as: [role] content_preview...
    // Gracefully degrade to raw handle on failure
}
```

**Format rules:**
- Text nodes: render content directly
- External nodes (resolved): `[{role}] {content_truncated_to_80_chars}...`
- External nodes (unresolved): fall back to existing raw handle format

### RENDER-5: Update builder.rs
- **blocked_by:** [RENDER-3, RENDER-4]
- **unlocks:** []
- **Scope:** Wire up Arbor with Plexus parent in builder

**Changes:**
```rust
// Change Arbor to Arbor<Weak<Plexus>>
let arbor = Arbor::<Weak<Plexus>>::new(...);

// In Arc::new_cyclic:
arbor.inject_parent(weak_plexus.clone());
```

## Acceptance Criteria

1. `tree_render` continues to work unchanged (backwards compatible)
2. `tree_render_resolved` shows human-readable content for chat messages
3. Resolution failures gracefully degrade to raw handle display
4. Works for both Cone and ClaudeCode message handles

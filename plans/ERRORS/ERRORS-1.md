# ERRORS-1: Typed Error Handling Across All Activations

## Problem

Error handling is inconsistent and often lossy across activations:

- **11 of 12 non-trivial activations** use ad-hoc `format!("Failed to X: {}", e)` string errors
- **Only Mustache** uses `thiserror` properly
- **~20+ silent `let _ =`** in ClaudeCode stream operations discard state update failures
- **Bash executor** panics on `.expect()` for stdio capture
- **Orcha** silently drops session lookup failures via `.ok()`
- No activation includes correlation IDs (session_id, tree_id) in errors

Errors are not machine-readable, not greppable by variant, and often stripped of the context needed to diagnose issues.

## Strategy

Standardize on `thiserror` enums using Mustache as the template. Each activation gets a typed error enum with contextual fields. Silent drops get replaced with logging or proper propagation.

**Tiers:**
- **Tier 1 (Critical):** Activations with silent error swallowing or panics — Bash, ClaudeCode streams, Orcha
- **Tier 2 (Core):** High-traffic activations with bare struct errors — Arbor, Cone
- **Tier 3 (Lightweight):** Simple activations that can stay as-is or get minimal treatment — Echo, Health, Interactive, Registry, Solar, Changelog

## Tickets

### ERRORS-2: Bash executor — typed errors + panic removal

**blocked_by:** []
**unlocks:** []

The Bash activation has the same problem ClaudeCode had (executor spawns process, errors are lossy) plus two `.expect()` calls that will panic.

1. Add `ExecutorError` thiserror enum to bash executor (mirror claudecode's `ExecutorError`)
   - `BinaryNotFound`, `WorkingDirNotFound`, `SpawnFailed`, `ProcessFailed`, `NoOutput`
2. Replace `.expect("Failed to capture stdout/stderr")` with match arms that yield error events
3. Add pre-flight validation for command and working_dir
4. Capture stderr in background task (same pattern as claudecode)
5. Check exit code after stream ends, surface errors with context

**Files:** `src/activations/bash/executor/mod.rs`, `src/activations/bash/types.rs`

---

### ERRORS-3: ClaudeCode — eliminate silent `let _ =` on stream ops

**blocked_by:** []
**unlocks:** []

~20+ instances where `stream_push_event`, `stream_set_status` failures are silently discarded. The client's stream is left in an inconsistent state with no indication.

1. Replace `let _ = storage.stream_push_event(...)` with `if let Err(e) = ... { tracing::error!(...) }`
2. Same for `stream_set_status`
3. For critical state transitions (status → Failed), add a fallback: if setting Failed status itself fails, log at error level with stream_id and session_id

**Files:** `src/activations/claudecode/activation.rs` (lines ~1114-1360)

---

### ERRORS-4: Arbor — thiserror enum

**blocked_by:** []
**unlocks:** []

Replace `ArborError { message: String }` with a thiserror enum. Arbor is foundational — every activation that uses trees hits these errors.

1. Define `ArborError` variants:
   - `TreeNotFound { tree_id }`, `NodeNotFound { node_id, tree_id }`, `StorageError { operation, source }`, `InvalidState { message }`, `InitError { source }`
2. Update storage.rs `map_err` calls to construct typed variants
3. Replace `let _ = self.hub.set(parent)` with logged error
4. Keep `From<String>` for backward compat during migration

**Files:** `src/activations/arbor/types.rs`, `src/activations/arbor/storage.rs`, `src/activations/arbor/activation.rs`

---

### ERRORS-5: Cone — thiserror enum

**blocked_by:** []
**unlocks:** []

Same pattern as Arbor. Replace `ConeError { message: String }` with typed variants.

1. Define `ConeError` variants:
   - `SessionNotFound { name }`, `StorageError { operation, source }`, `ArborError { source }`, `InvalidState { message }`
2. Update storage and activation
3. Replace `let _ = self.hub.set(parent)` with logged error

**Files:** `src/activations/cone/types.rs`, `src/activations/cone/storage.rs`, `src/activations/cone/activation.rs`

---

### ERRORS-6: Orcha — eliminate silent `.ok()` + typed errors

**blocked_by:** []
**unlocks:** []

Orcha silently drops session lookup failures and validation extraction failures via `.ok()`.

1. Replace `.await.ok()` session retrievals with proper error logging
2. Replace `.filter_map(|r| r.ok())` with `.filter_map(|r| match r { Ok(v) => Some(v), Err(e) => { tracing::warn!(...); None } })`
3. Replace `.ok()?` on regex/JSON parsing with logged fallbacks
4. Add `OrchaError` thiserror enum with `SessionNotFound`, `OrchestrationError`, `StorageError`, `ValidationError`

**Files:** `src/activations/orcha/types.rs`, `src/activations/orcha/storage.rs`, `src/activations/orcha/activation.rs`, `src/activations/orcha/orchestrator.rs`

---

### ERRORS-7: ClaudeCode Loopback + Changelog — lightweight cleanup

**blocked_by:** []
**unlocks:** []

Low-severity. Convert bare `Result<T, String>` to thiserror where it helps readability.

1. Loopback: Add `LoopbackError` enum (small — `SessionNotFound`, `StorageError`, `Timeout`)
2. Changelog: Add `ChangelogError` enum if justified, or leave as strings (it's simple enough)

**Files:** `src/activations/claudecode_loopback/types.rs`, `src/activations/changelog/types.rs`

---

### ERRORS-8: Skip list — activations that need no changes

No changes needed for:
- **Echo** — trivial, no error paths
- **Health** — trivial, no error paths
- **Interactive** — event-based, errors inline
- **Registry** — event-based, errors inline
- **Solar** — read-only data, `.expect()` in schema gen is acceptable
- **Mustache** — already exemplary thiserror usage

---

## Dependency DAG

```
ERRORS-2 (Bash)
ERRORS-3 (ClaudeCode silent drops)
ERRORS-4 (Arbor)
ERRORS-5 (Cone)
ERRORS-6 (Orcha)
ERRORS-7 (Loopback + Changelog)
```

**All tickets are independent** — they touch different activations with no shared code. All 6 can be done in parallel.

## Non-goals

- Changing RPC response types (the `CreateResult::Err { message }` pattern stays)
- Adding error codes or error registries
- Changing how errors are serialized over the wire
- Touching Mustache (already done right)

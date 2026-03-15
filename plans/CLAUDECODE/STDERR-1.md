# STDERR-1: Claude Code Executor Error Observability

## Problem

When `claudecode chat` invokes the `claude` CLI and it fails, the error is **completely invisible**:

- stderr is piped but **never read** (`executor.rs:299`)
- Exit code is discarded: `let _ = child.wait().await` (`executor.rs:366`)
- If stdout produces zero events, the stream loop exits silently
- `activation.rs` then yields a `Complete` event with empty/default values
- User sees: empty `claude_session_id`, null usage, no content — **no indication anything went wrong**

This makes every failure mode (bad API key, invalid model, network error, binary not found in PATH, working dir doesn't exist) look identical: silent nothing.

## Scope

Targeted fix to `executor.rs` — capture stderr, check exit code, surface errors. No changes to the activation-level event processing needed since it already handles `RawClaudeEvent::Result { is_error: Some(true), .. }` correctly.

## Tickets

### STDERR-2: Capture stderr in a background task

**blocked_by:** []
**unlocks:** [STDERR-3, STDERR-4]

In `executor.rs` `launch()`, after spawning the child process:

1. Take `child.stderr` the same way stdout is taken (line 326)
2. Spawn a `tokio::spawn` task that reads stderr lines into a `Arc<Mutex<Vec<String>>>` (or a bounded buffer — last N lines to avoid unbounded memory from a runaway process)
3. Cap at ~100 lines or ~64KB, whichever comes first
4. The buffer is shared with the main stream via an `Arc` so it can be read after the stdout loop ends

**Files:** `executor.rs` (lines ~295-327)

**Acceptance criteria:**
- stderr is fully consumed (prevents child process from blocking on a full stderr pipe)
- Buffer is accessible after stdout stream ends
- No unbounded memory growth

---

### STDERR-3: Yield error event on non-zero exit / empty stream

**blocked_by:** [STDERR-2]
**unlocks:** [STDERR-5]

After the stdout `while let` loop ends (line 363) and before cleanup:

1. `child.wait().await` — actually capture the `ExitStatus`
2. If exit code is non-zero **OR** no `Result` event was received from stdout:
   - Read the stderr buffer from STDERR-2
   - Yield a `RawClaudeEvent::Result` with `is_error: Some(true)` and `error` containing:
     - Exit code
     - stderr content (joined, trimmed)
     - If stderr is empty: a generic message like "Claude process exited with code {N} but produced no output"
3. If exit code is 0 but no events were received, still yield an error — a successful exit with no stream-json output is unexpected

**Files:** `executor.rs` (lines ~363-370)

**Acceptance criteria:**
- Non-zero exit always produces an error event with stderr content
- Zero exit with no stdout events produces a diagnostic error event
- The error event follows existing `RawClaudeEvent::Result` shape so activation.rs handles it without changes

---

### STDERR-4: Log stderr at debug/warn level via tracing

**blocked_by:** [STDERR-2]
**unlocks:** []

Parallel to STDERR-3. Regardless of whether an error event is yielded:

1. If stderr buffer is non-empty after stream ends, log it at `tracing::warn!` level
2. Always log the exit code at `tracing::debug!`
3. Log the full constructed command at `tracing::debug!` (already exists at line 292-293 — keep the existing `eprintln!` too for now)

This ensures operators can see failures in structured logs even if the error event is somehow lost downstream.

**Files:** `executor.rs`

**Acceptance criteria:**
- Non-empty stderr always appears in logs
- Exit code always logged
- No log spam on successful runs (debug level for normal exit)

---

### STDERR-5: Integration-level guard in activation.rs

**blocked_by:** [STDERR-3]
**unlocks:** []

Belt-and-suspenders check in `activation.rs` after the `raw_stream` loop ends (line 516):

If `response_content` is empty **and** `claude_session_id` is `None` **and** no error was yielded during the stream — yield a `ChatEvent::Err` with a diagnostic message like:

> "Claude process produced no response. Check substrate logs for details."

This catches any edge case where the executor somehow doesn't emit an error event but the stream was clearly empty.

**Files:** `activation.rs` (around line 516, before the "Store assistant response" section)

**Acceptance criteria:**
- Empty stream + no session ID = error event to user (not a silent `Complete` with empty fields)
- Non-empty stream still works as before (no false positives)

---

## Dependency DAG

```
STDERR-2 (capture stderr)
  ├── STDERR-3 (yield error events)  ──→  STDERR-5 (activation guard)
  └── STDERR-4 (tracing logs)
```

STDERR-2 is the foundation. STDERR-3 and STDERR-4 can be done in parallel after it. STDERR-5 is a safety net that can be done last (or in parallel with 3/4 since it's in a different file).

## Non-goals

- Changing how `activation.rs` processes error events (already works correctly)
- Adding retry logic
- Changing the claude CLI invocation args
- Diagnosing _why_ the specific `haiku` invocation failed (that's a runtime config issue, not a code issue — but this fix will make the _reason_ visible)

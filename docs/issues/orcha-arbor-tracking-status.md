# ORCHA-1: Arbor Tracking & Approval Interface - Status Report

**Date:** 2026-03-04
**Status:** Implementation Complete, Testing Blocked

## Summary

Added arbor tracking to Orcha orchestrator to provide visibility into orchestration events. Created OrchaContext abstraction to cleanly manage arbor tree operations. Implementation is complete and compiles successfully, but verification is blocked by synapse installation issues.

## What Works ✅

### 1. Session ID Transparency (Verified)
- **Test:** Direct Claude CLI → MCP Gateway → Loopback Backend
- **Result:** Session_id flows correctly end-to-end
- **Evidence:** Loopback logs show: `[LOOPBACK] permit: tool_use_id=... mapped to session_id=direct-test-1772646476`
- **Command used:**
  ```bash
  claude --permission-prompt-tool plexus.loopback.permit \
    --mcp-config /tmp/mcp-config.json \
    "curl example.com"
  ```

### 2. Orcha Session ID Propagation (Implemented)
- **Change:** Updated `orchestrator.rs` to pass `Some(session_id.clone())` to `claudecode.create()`
- **Result:** All Orcha-launched Claude Code sessions now include hierarchical session IDs
- **Format:** `orcha-{uuid}` → passes to child sessions

### 3. OrchaContext Abstraction (Complete)
- **File:** `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/context.rs`
- **Purpose:** Clean API for arbor tree operations without direct arbor calls
- **Methods:**
  - `OrchaContext::new()` - Creates arbor tree and writes initial session_started node
  - `claude_session_created()` - Records Claude Code session creation
  - `prompt_created()` - Records task prompt sent to Claude
  - `claude_session_complete()` - Records Claude session completion
  - `validation_started()` - Records validation test start
  - `validation_result()` - Records validation success/failure
  - `session_complete()` - Records final orchestration completion
  - `tool_use()` - Records tool use events
  - `tool_result()` - Records tool results
  - `claude_output()` - Records Claude's text output

### 4. Database Schema Updated (Complete)
- **Change:** Added `tree_id TEXT` column to `orcha_sessions` table
- **Migration:** Auto-migration runs on startup if column doesn't exist
- **Storage:** `SessionInfo` struct includes `tree_id: Option<String>` field

### 5. Compilation (Success)
- **Command:** `cargo build --release --package plexus-substrate`
- **Result:** Builds successfully with only warnings (unused variables, etc.)
- **Warnings fixed:** Removed `mut` from non-mutable variables

### 6. Substrate Service (Running)
- **Binary:** `target/release/plexus-substrate`
- **Ports:**
  - WebSocket: `ws://127.0.0.1:4444`
  - MCP HTTP: `http://127.0.0.1:4445/mcp`
- **Activations loaded:** 116 total methods including orcha
- **Orcha methods:**
  - `orcha_run_task`
  - `orcha_run_task_async`
  - `orcha_create_session`
  - `orcha_get_session`
  - `orcha_list_sessions`
  - `orcha_list_pending_approvals`
  - `orcha_approve_request`
  - `orcha_deny_request`
  - (and more...)

## What Doesn't Work ❌

### 1. Synapse Installation (Blocked)
- **Issue:** Cannot install synapse CLI tool
- **Error:** `cabal build` fails with dependency resolution errors
- **Attempted:**
  - `cabal install --installdir=/usr/local/bin` - Permission denied
  - `sudo cabal install` - Hangs indefinitely
  - `cabal build exe:synapse` - Missing websockets dependency
  - `cabal update && cabal build` - Interrupted/killed
- **Impact:** Cannot use synapse to interact with substrate, blocking all testing

### 2. Arbor Tracking Verification (Not Tested)
- **Issue:** Cannot verify that orchestration events are written to arbor trees
- **Reason:** Need synapse to query arbor trees
- **What we need to check:**
  - Arbor tree is created when Orcha session starts
  - Orchestration events appear as nodes in the tree
  - Tree_id is stored in orcha_sessions database
  - Events include: session_started, claude_session_created, prompt_created, claude_session_complete, validation_started, validation_result, session_complete

### 3. Orcha API Calls (Failing)
- **Issue:** Direct JSON-RPC calls to substrate return "Internal error"
- **Test attempted:**
  ```bash
  curl -X POST http://127.0.0.1:4444 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"orcha_run_task","params":{"request":{"model":"haiku","task":"echo test"}}}'
  ```
- **Response:** `{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Internal error"}}`
- **Note:** Even `health_check` returns internal error, suggesting JSON-RPC layer issue

### 4. MCP Endpoint Issues (Protocol Mismatch)
- **Issue:** MCP endpoint at `:4445/mcp` requires special handling
- **Error 1:** `Not Acceptable: Client must accept both application/json and text/event-stream`
- **Error 2:** `Unexpected message, expect initialize request`
- **Reason:** MCP protocol requires initialization handshake, not direct tool calls
- **Impact:** Cannot test Orcha via MCP endpoint without proper MCP client

### 5. Tool Use Visibility (Unknown)
- **Issue:** Cannot verify if Orcha-launched Claude Code sessions trigger tool use/approvals
- **Previous observation:** Orcha tasks completed in ~2 seconds with no tool use events
- **Hypothesis:** Either tasks complete without tool use, OR tool events not visible
- **Need to test:** Run task that definitely requires tools (e.g., "create a file")

## Architecture Summary

### Orcha Orchestration Flow with Arbor Tracking

```
1. User calls orcha.run_task
2. OrchaContext::new() creates arbor tree
   └─> Writes: session_started node
3. Storage.create_session() saves session with tree_id
4. ClaudeCode.create() spawns Claude session with session_id
   └─> Writes: claude_session_created node
5. ClaudeCode.chat() sends task prompt
   └─> Writes: prompt_created node
6. Claude processes task (with tool approval loop if needed)
7. Chat completes
   └─> Writes: claude_session_complete node
8. If validation artifact found:
   a. Run validation test
      └─> Writes: validation_started node
   b. Check result
      └─> Writes: validation_result node
   c. If failed and retries available: goto step 4
9. Mark session complete
   └─> Writes: session_complete node
```

### Session ID Hierarchy

```
User Request
  └─> orcha-{uuid}                           (Orcha session)
        ├─> {orcha-session-id}               (Passed to MCP Gateway URL)
        ├─> {orcha-session-id}-approval-{uuid} (Approval decision agent)
        └─> (future) Multi-agent spawn tree
```

### Arbor Tree Structure

```
Tree: {tree_id} (metadata: session_id, task, model)
  └─> Node: "🎬 Orcha session started" (metadata: event=session_started)
      └─> Node: "🤖 Claude Code session created" (metadata: event=claude_session_started)
          └─> Node: "📝 Sending task prompt" (metadata: event=prompt_created, retry_count)
              └─> Node: "✅ Claude Code session completed" (metadata: event=claude_session_complete)
                  └─> Node: "🧪 Running validation test" (metadata: event=validation_started)
                      └─> Node: "📊 Validation result: SUCCESS" (metadata: event=validation_result)
                          └─> Node: "🎉 Orcha session completed" (metadata: event=session_complete)
```

## Files Modified

### Created
- `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/context.rs` - OrchaContext abstraction

### Modified
- `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/orchestrator.rs`
  - Added `OrchaContext::new()` at start of orchestration
  - Replaced direct arbor calls with context methods
  - Updated function signature to accept `Arc<ArborStorage>`
- `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/storage.rs`
  - Added `tree_id TEXT` column migration
  - Updated `SessionInfo` struct with `tree_id` field
  - Modified `create_session()` to accept `tree_id` parameter
- `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/types.rs`
  - Added `tree_id: Option<String>` to `SessionInfo` struct
- `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/activation.rs`
  - Updated `run_task()` and `run_task_async()` to pass arbor_storage
  - Updated `create_session()` to pass tree_id
- `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/mod.rs`
  - Added `mod context` and `pub use context::OrchaContext`

## Next Steps

### Immediate (Blocked by Synapse)
1. **Install synapse** - Need working synapse CLI to interact with substrate
   - Try alternative installation method (pre-built binary?)
   - Or write minimal test client using plexus-protocol directly
2. **Test arbor tracking** - Once synapse available:
   ```bash
   synapse substrate orcha run_task_async --request.model haiku --request.task "create /tmp/test.txt"
   synapse substrate orcha list_sessions
   synapse substrate arbor tree_list  # Find the tree_id
   synapse substrate arbor tree_get --tree_id <id>
   ```
3. **Verify tool use flow** - Check that tools trigger loopback.permit

### Follow-up Testing
1. Run task that requires multiple tools
2. Test approval flow (manual approval mode)
3. Test retry loop with failed validation
4. Verify hierarchical session IDs in MCP gateway logs
5. Check arbor tree contains all expected orchestration events

### Documentation
1. Document synapse installation procedure
2. Write guide for debugging Orcha sessions using arbor trees
3. Add examples of querying orchestration events
4. Document approval interface architecture

## Known Issues

1. **JSON-RPC internal errors** - Direct substrate API calls fail, unclear why
2. **MCP handshake required** - Cannot use raw curl for MCP endpoint
3. **Synapse dependency hell** - Cabal cannot resolve websockets dependency
4. **No error visibility** - When orcha_run_task fails, error not surfaced

## Questions

1. Why do JSON-RPC calls return "Internal error" even for simple methods like health_check?
2. Is there a pre-built synapse binary available?
3. Should we add streaming event support to expose tool use events during orchestration?
4. How do we query arbor trees without synapse?

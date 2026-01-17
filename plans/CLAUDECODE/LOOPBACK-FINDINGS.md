# Loopback Implementation Findings

## Summary

The loopback mechanism for routing Claude Code tool permissions through a parent for approval has been implemented and tested. **The core mechanism is working** - approvals are created, can be queried, and can be approved/denied. However, there's a bug in the response relay - after approval, the tool result isn't properly returned to Claude Code.

## Latest Test Results (via Synapse)

Using a subagent to test via synapse CLI, we confirmed:

1. **Approval Created**: When Claude tries to use the `Read` tool, an approval request is created in SQLite with status `pending`
2. **Pending API Works**: `loopback.pending` correctly returns the pending approval with tool_name, input, and ID
3. **Respond API Works**: `loopback.respond` successfully changes status from `pending` to `approved`
4. **Database Updated**: The approval record shows `resolved_at` timestamp after approval

**Bug Found**: After approval, Claude reports "file doesn't exist or there was an issue accessing it" - the allow response isn't being properly relayed back, so the tool doesn't actually execute.

## What Was Implemented

### New Plugin: `claudecode_loopback`

**Files created:**
- `src/activations/claudecode_loopback/mod.rs`
- `src/activations/claudecode_loopback/types.rs`
- `src/activations/claudecode_loopback/storage.rs`
- `src/activations/claudecode_loopback/activation.rs`

**MCP methods exposed:**
- `loopback.permit` - Permission prompt handler that blocks/polls until parent approves/denies
- `loopback.respond` - Parent calls this to approve/deny a pending request
- `loopback.pending` - List pending approval requests
- `loopback.configure` - Generate MCP config for a loopback session

### ClaudeCode Integration

**Modified files:**
- `src/activations/claudecode/executor.rs` - Added `loopback_enabled` and `loopback_session_id` to `LaunchConfig`
- `src/activations/claudecode/types.rs` - Added `loopback_enabled` to session config
- `src/activations/claudecode/storage.rs` - Persist `loopback_enabled` per session
- `src/activations/claudecode/activation.rs` - Accept `loopback_enabled` parameter in `create`
- `src/builder.rs` - Register loopback activation

## How It's Supposed to Work

1. Create a session with `claudecode.create(..., loopback_enabled=true)`
2. When `chat` is called, the executor:
   - Generates MCP config pointing to Plexus (`http://127.0.0.1:4445/mcp`)
   - Sets `--permission-prompt-tool mcp__plexus__loopback_permit`
   - Sets `LOOPBACK_SESSION_ID` env var for correlation
3. When nested Claude wants to use a tool:
   - Claude CLI calls the permission-prompt-tool
   - `loopback.permit` creates an approval request in SQLite and blocks (polls every 1s)
4. Parent calls `loopback.pending()` to see requests
5. Parent calls `loopback.respond(approval_id, approve=true/false)` to resolve
6. `loopback.permit` returns `{behavior: "allow"}` or `{behavior: "deny"}` to Claude Code

## Test Results

### Working Components

1. **Loopback storage** - Approvals are correctly created and persisted in SQLite
   ```
   sqlite3 loopback.db "SELECT * FROM loopback_approvals WHERE status='pending';"
   ```

2. **MCP tool registration** - `mcp__plexus__loopback_permit` appears in Claude's available tools
   ```
   claude --permission-prompt-tool invalid_tool --print -p "hi"
   # Shows loopback_permit in available tools list
   ```

3. **Direct CLI test** - When running Claude CLI directly with the permission-prompt-tool, it:
   - Creates approval requests in the database
   - Blocks waiting for approval (times out after the specified timeout)
   ```bash
   timeout 10 claude --permission-prompt-tool mcp__plexus__loopback_permit --print -p "Read /tmp/test.txt"
   # Creates approval in database, times out
   ```

4. **Approval flow** - `loopback.pending` and `loopback.respond` work correctly via synapse

### Issue: No Approvals When Launched From Executor

When Claude Code is launched through the claudecode plugin (via `claudecode.chat`), the tools execute without creating approval requests.

**Debug output shows correct command:**
```
cmd='/Users/shmendez/.local/bin/claude' '--output-format' 'stream-json'
'--include-partial-messages' '--verbose' '--print' '--model' 'haiku'
'--permission-prompt-tool' 'mcp__plexus__loopback_permit'
'--mcp-config' '/var/folders/.../mcp-config-xxx.json'
'--' 'Read the first line of Cargo.toml'
```

**MCP config file content:**
```json
{"mcpServers":{"plexus":{"type":"http","url":"http://127.0.0.1:4445/mcp"}}}
```

**Observed behavior:**
- Claude reads the file and returns the correct answer
- No approval requests are created
- Tool executes immediately without blocking

## Root Cause: MCP Response Wrapper Format

The bug is the MCP response wrapper format. Looking at HumanLayer's (hlyr) implementation, the response must be wrapped in the MCP content array format:

```javascript
return {
  content: [{
    type: 'text',
    text: JSON.stringify({
      behavior: 'allow',
      updatedInput: input,
    }),
  }],
}
```

Our Rust code returns the raw `PermitResponse` enum which serializes to just `{"behavior": "allow", "updatedInput": {...}}`, but Claude Code expects the MCP tool result format where the permission JSON is **stringified inside `content[0].text`**.

The MCP bridge (`src/mcp_bridge.rs:250-257`) handles this:
```rust
match &buffered_data[0] {
    serde_json::Value::String(s) => s.clone(),  // Returns string directly
    other => serde_json::to_string_pretty(other).unwrap_or_default(),  // Serializes objects
}
```

When we yield a `PermitResponse` struct, it gets serialized to a JSON object, then the MCP bridge serializes it again to a string. But Claude Code expects the inner JSON to already be a string within the response.

**Fix**: Instead of yielding `PermitResponse::Allow { ... }`, we need to yield a `String` containing the JSON. This way the MCP bridge returns it as-is without double-serialization, and Claude Code receives exactly what it expects.

## Previous Hypotheses (Partially Resolved)

### 1. ~~Loopback not triggering from executor~~ - RESOLVED
When using synapse to call `claudecode.chat`, the loopback IS triggered. The approval appears in the database.

### 2. Response relay bug - CURRENT ISSUE
After approval, the tool doesn't execute. The `loopback.permit` returns `Allow` but Claude reports the file doesn't exist.

### 3. MCP tool response format
The MCP response might need to be in a specific format. Need to check what Claude Code expects from `--permission-prompt-tool`.

## Next Steps

1. **Check MCP response format** - Verify the exact JSON structure Claude Code expects from permission-prompt-tool
2. **Add debug logging to permit** - Log exactly what response is being sent
3. **Test with raw MCP call** - Call loopback_permit directly and inspect the response
4. **Check HumanLayer's response format** - Compare with their working implementation

## Code Locations

- Executor launch logic: `src/activations/claudecode/executor.rs:237-358`
- Loopback permit handler: `src/activations/claudecode_loopback/activation.rs:51-134`
- Loopback storage: `src/activations/claudecode_loopback/storage.rs`
- Session config with loopback: `src/activations/claudecode/types.rs:111-137`

## Related Plans

- `plans/CLAUDECODE/LOOPBACK-1.md` - Original implementation plan
- `plans/ARBOR/RENDER-1.md` - Related Arbor HubContext upgrade for handle resolution

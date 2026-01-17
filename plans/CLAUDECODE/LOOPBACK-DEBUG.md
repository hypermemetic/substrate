# Loopback Debug Investigation

## Current Status: `--permission-prompt-tool` not being invoked

### What Works
1. **Direct MCP calls work**: `synapse plexus loopback permit` successfully calls the permit function
2. **Logging works**: When permit is called directly, we see `[LOOPBACK] permit called: tool=X` in substrate logs
3. **Debug files created**: `/tmp/loopback_permit_entry.txt` and `/tmp/loopback_permit_called.txt` are written on direct calls
4. **Blocking/polling works**: The permit function correctly blocks until approval via `loopback.respond`

### What Doesn't Work
1. **Claude Code never calls the permission-prompt-tool**: When Claude is launched with `--permission-prompt-tool mcp__plexus__loopback_permit`, the tool is never invoked
2. **Even when permission prompts appear**: User sees permission prompts (e.g., for `sudo`), but they're handled internally, not routed to loopback

### Evidence

Executor generates correct command:
```
'/Users/shmendez/.local/bin/claude' '--output-format' 'stream-json'
'--include-partial-messages' '--verbose' '--print' '--model' 'haiku'
'--permission-prompt-tool' 'mcp__plexus__loopback_permit'
'--mcp-config' '/var/folders/.../mcp-config-xxx.json'
'--' 'prompt here'
```

MCP config file contains:
```json
{"mcpServers":{"plexus":{"type":"http","url":"http://127.0.0.1:4445/mcp"}}}
```

### Hypotheses

1. **MCP connection not established**: Claude Code may fail silently to connect to the MCP server
2. **Tool name mismatch**: The tool name `mcp__plexus__loopback_permit` may not match what Claude expects
3. **Permission bypass**: Some permission types may not go through the permission-prompt-tool
4. **MCP initialization**: Claude Code may not be initializing the MCP session correctly

### Next Steps to Investigate

1. **Verify MCP tool naming**: Check what tool names plexus actually exposes via MCP `tools/list`
2. **Check Claude Code logs**: Look for MCP connection errors in Claude's verbose output
3. **Test with stdio MCP**: Try using stdio transport instead of HTTP to rule out HTTP issues
4. **Check Claude Code docs**: Verify the exact expected behavior of `--permission-prompt-tool`

### Key Files
- `src/activations/claudecode_loopback/activation.rs` - permit function with debug logging
- `src/activations/claudecode/executor.rs` - builds command with permission-prompt-tool flag
- `plans/CLAUDECODE/LOOPBACK-FINDINGS.md` - earlier findings about response format

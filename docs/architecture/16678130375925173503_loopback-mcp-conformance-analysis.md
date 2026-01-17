#s Loopback MCP Conformance Analysis

> Comparing Plexus loopback implementation against HumanLayer's working approval flow

## Executive Summary

The Plexus `claudecode_loopback` activation replicates HumanLayer's permission-prompt-tool pattern for routing Claude Code tool permissions through a parent for approval.

**Status: RESOLVED** - The root cause was identified and fixed.

### Root Cause

The MCP bridge (`src/mcp_bridge.rs`) was appending a token count suffix to all tool responses:

```rust
// BEFORE (broken):
let content_with_tokens = format!("{}\n\n[~{} tokens]", text_content, approx_tokens);
Ok(CallToolResult::success(vec![Content::text(content_with_tokens)]))
```

This corrupted the JSON response that Claude Code expects from `--permission-prompt-tool`:

```
// What Plexus returned:
{"behavior":"allow","updatedInput":{"command":"curl example.com"}}

[~13 tokens]

// What Claude Code expected (clean JSON):
{"behavior":"allow","updatedInput":{"command":"curl example.com"}}
```

When Claude Code called `JSON.parse()` on the response, it failed with a Zod validation error due to the suffix.

### The Fix

Removed the token count suffix from MCP tool responses:

```rust
// AFTER (working):
Ok(CallToolResult::success(vec![Content::text(text_content)]))
```

This document preserves the full investigation for reference.

## Reference Architecture: HumanLayer

HumanLayer's approval flow (documented in `humanlayer/docs/architecture/16678191215220290559_approval-flow-architecture.md`):

```
Claude Code CLI
    │
    │ --permission-prompt-tool mcp__codelayer__request_permission
    │ --mcp-config <temp_file.json>
    │
    ▼ (MCP Protocol - stdio)
hlyr MCP Server (Node.js)
    │
    │ Unix Socket JSON-RPC
    ▼
hld Daemon
    │
    ▼
Human Approver (WUI/TUI)
```

### Key Implementation Details

1. **Transport: stdio** - MCP server spawned as child process
   ```go
   MCPServers: map[string]claudecode.MCPServerConfig{
       "codelayer": {
           Command: "hlyr",
           Args:    []string{"mcp"},
       },
   }
   ```

2. **Tool naming**: Simple name `request_permission`
   ```typescript
   const tools = [{
       name: 'request_permission',  // No namespace prefix
       ...
   }]
   ```

3. **Response format**: JSON stringified inside MCP content
   ```typescript
   return {
     content: [{
       type: 'text',
       text: JSON.stringify({ behavior: 'allow', updatedInput: input }),
     }],
   }
   ```

4. **Blocking via polling**: MCP handler polls daemon until human decides
   ```typescript
   while (polling) {
     const approval = await daemonClient.getApproval(approvalId)
     if (approval.status !== 'pending') break
     await sleep(1000)
   }
   ```

## Plexus Implementation

```
Claude Code CLI
    │
    │ --permission-prompt-tool mcp__plexus__loopback_permit  (?)
    │ --mcp-config <temp_file.json>
    │
    ▼ (MCP Protocol - HTTP)
Plexus MCP Server (Rust/rmcp)
    │
    │ In-process
    ▼
loopback.permit activation
    │
    ▼
Parent approval (via loopback.respond)
```

### Verified Working Components

1. **MCP Server responds to tools/list**:
   ```
   loopback.permit
   loopback.respond
   loopback.pending
   loopback.configure
   ```

2. **Direct MCP calls work**: `synapse plexus loopback.permit` creates approvals
3. **Polling/blocking works**: `loopback.permit` correctly waits for `loopback.respond`
4. **Debug logging confirms**: When called directly, permit logs to `/tmp/loopback_permit_called.txt`

### Non-Working: Claude Code Integration

When Claude Code is launched via the executor with `--permission-prompt-tool mcp__plexus__loopback_permit`:
- Tools execute immediately without creating approval requests
- No debug files created (permit never called)
- Claude reads files and returns answers as if no permission system exists

## Identified Non-Conformances

### 1. Transport: HTTP vs stdio

| Aspect | HumanLayer | Plexus |
|--------|------------|--------|
| Transport | stdio (child process) | HTTP (`http://127.0.0.1:4445/mcp`) |
| Initialization | Synchronous on spawn | Requires HTTP session handshake |
| Session management | Implicit (single pipe) | Explicit (`mcp-session-id` header) |

**Impact**: Claude Code may fail to establish MCP session over HTTP and silently fall back to internal permission handling.

### 2. MCP Session Initialization Sequence

The MCP Streamable HTTP transport requires a three-step handshake:

```
1. POST /mcp  {"method": "initialize", ...}
   Response: {"result": {...}, "mcp-session-id": "uuid"}

2. POST /mcp  {"method": "notifications/initialized"}
   Header: mcp-session-id: uuid

3. POST /mcp  {"method": "tools/list", ...}
   Header: mcp-session-id: uuid
```

**Verification**:
```bash
# Without session ID, tools/list fails:
curl -X POST http://127.0.0.1:4445/mcp -d '{"method":"tools/list"}'
# Response: "Unexpected message, expect initialize request"

# With proper handshake: works
```

Claude Code may not implement the full Streamable HTTP session protocol for `--permission-prompt-tool`.

### 3. Tool Name Format

| Aspect | HumanLayer | Plexus |
|--------|------------|--------|
| Tool name in MCP | `request_permission` | `loopback.permit` |
| Claude Code reference | `mcp__codelayer__request_permission` | `mcp__plexus__loopback.permit` |
| Separator | underscore | dot |

**Concern**: The dot in `loopback.permit` may cause issues with Claude Code's tool name parsing. The `mcp__<server>__<tool>` format expects the tool name portion to be a simple identifier.

### 4. Response Content Structure

Plexus loopback returns:
```rust
yield json!({ "behavior": "allow", "updatedInput": input }).to_string();
```

This yields a string through the Plexus streaming system, which becomes:
```json
{"content": [{"type": "text", "text": "{\"behavior\": \"allow\", ...}\n\n[~N tokens]"}]}
```

The `[~N tokens]` suffix appended by `mcp_bridge.rs:274-278` may corrupt the JSON that Claude Code expects.

## Diagnostic Evidence

### MCP Server Logging (Added)

Request logging middleware now captures all HTTP requests:
```
▶▶▶ MCP HTTP REQUEST ▶▶▶
  Method: POST
  URI: /mcp
  Headers: ...
◀◀◀ MCP HTTP RESPONSE ◀◀◀
  Status: 200 OK
```

### Observed Behavior During Claude Code Launch

When Claude Code runs with loopback enabled:
- **Expected**: HTTP requests to `/mcp` with `initialize`, then tool calls
- **Actual**: No HTTP requests observed to MCP endpoint during tool execution

This confirms Claude Code is **not connecting to the HTTP MCP server** when using `--permission-prompt-tool`.

## Root Cause Hypothesis

**Claude Code's `--permission-prompt-tool` may only support stdio-based MCP servers.**

Evidence:
1. HumanLayer exclusively uses stdio transport
2. Claude Code spawns MCP servers as child processes for other integrations
3. HTTP MCP (Streamable HTTP) is a newer transport that may not be supported for permission-prompt-tool specifically
4. No HTTP requests observed when permission-prompt-tool should be invoked

## Recommended Solutions

### Option A: stdio Wrapper (Recommended)

Create a small binary that:
1. Speaks stdio MCP protocol to Claude Code
2. Proxies requests to the HTTP Plexus server
3. Handles session management internally

```
Claude Code ←stdio→ loopback-mcp-proxy ←http→ Plexus
```

### Option B: Native stdio MCP in Plexus

Implement stdio MCP transport directly in Plexus:
- Already have `--stdio` mode for JSON-RPC
- Need to add rmcp stdio transport option
- Loopback would use this instead of HTTP

### Option C: Investigate Claude Code Source

If Claude Code is open source, verify:
1. Does `--permission-prompt-tool` support HTTP MCP?
2. What session management does it implement?
3. Are there debug flags to trace MCP connections?

## Minimal Test Environment

To isolate and verify the permission-prompt-tool mechanism, create the smallest possible self-contained test that mirrors HumanLayer's architecture exactly.

### Goal

Prove that Claude Code's `--permission-prompt-tool` works with a stdio MCP server before integrating with Plexus.

### Test Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Minimal Test Setup                        │
│                                                              │
│  Terminal 1:                                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ claude --permission-prompt-tool mcp__test__permit   │   │
│  │         --mcp-config /tmp/test-mcp.json             │   │
│  │         --print "Read /etc/hostname"                │   │
│  └──────────────────────┬──────────────────────────────┘   │
│                         │ stdio                             │
│                         ▼                                   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ test-permit-server (Node.js - single file)          │   │
│  │   - Exposes tool: permit                            │   │
│  │   - Writes approval to /tmp/pending.json            │   │
│  │   - Polls /tmp/decision.json for response           │   │
│  │   - Returns {behavior: "allow"} or {behavior:"deny"}│   │
│  └─────────────────────────────────────────────────────┘   │
│                                                              │
│  Terminal 2 (Human approver):                               │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ cat /tmp/pending.json                               │   │
│  │ echo '{"approved": true}' > /tmp/decision.json      │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Step 1: Create Minimal MCP Server

`/tmp/test-permit-server.js` - A single-file Node.js MCP server modeled on hlyr:

```javascript
#!/usr/bin/env node
// Minimal MCP server for testing --permission-prompt-tool
// Based on HumanLayer's hlyr implementation

const fs = require('fs');
const readline = require('readline');

const PENDING_FILE = '/tmp/pending.json';
const DECISION_FILE = '/tmp/decision.json';

// Clean up on start
try { fs.unlinkSync(PENDING_FILE); } catch {}
try { fs.unlinkSync(DECISION_FILE); } catch {}

const rl = readline.createInterface({ input: process.stdin });

// MCP Server state
let initialized = false;
let requestId = 0;

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function handleRequest(req) {
  const { method, params, id } = req;

  // Initialize handshake
  if (method === 'initialize') {
    initialized = true;
    return send({
      jsonrpc: '2.0',
      id,
      result: {
        protocolVersion: '2024-11-05',
        capabilities: { tools: {} },
        serverInfo: { name: 'test-permit-server', version: '1.0.0' }
      }
    });
  }

  // Initialized notification (no response needed)
  if (method === 'notifications/initialized') {
    return;
  }

  // List tools
  if (method === 'tools/list') {
    return send({
      jsonrpc: '2.0',
      id,
      result: {
        tools: [{
          name: 'permit',
          description: 'Request permission to perform an action',
          inputSchema: {
            type: 'object',
            properties: {
              tool_name: { type: 'string' },
              tool_use_id: { type: 'string' },
              input: { type: 'object' }
            },
            required: ['tool_name', 'input']
          }
        }]
      }
    });
  }

  // Call tool (permit)
  if (method === 'tools/call' && params?.name === 'permit') {
    const { tool_name, tool_use_id, input } = params.arguments || {};

    // Write pending approval
    const approval = {
      id: `approval-${Date.now()}`,
      tool_name,
      tool_use_id,
      input,
      created_at: new Date().toISOString()
    };
    fs.writeFileSync(PENDING_FILE, JSON.stringify(approval, null, 2));
    console.error(`[permit] Waiting for approval: ${approval.id}`);
    console.error(`[permit] Check: cat ${PENDING_FILE}`);
    console.error(`[permit] Approve: echo '{"approved":true}' > ${DECISION_FILE}`);
    console.error(`[permit] Deny: echo '{"approved":false,"message":"reason"}' > ${DECISION_FILE}`);

    // Poll for decision (like hlyr)
    const pollForDecision = () => {
      return new Promise((resolve) => {
        const interval = setInterval(() => {
          try {
            if (fs.existsSync(DECISION_FILE)) {
              const decision = JSON.parse(fs.readFileSync(DECISION_FILE, 'utf8'));
              clearInterval(interval);
              fs.unlinkSync(DECISION_FILE);
              fs.unlinkSync(PENDING_FILE);
              resolve(decision);
            }
          } catch {}
        }, 500);

        // Timeout after 5 minutes
        setTimeout(() => {
          clearInterval(interval);
          resolve({ approved: false, message: 'Timeout' });
        }, 300000);
      });
    };

    pollForDecision().then((decision) => {
      const response = decision.approved
        ? { behavior: 'allow', updatedInput: input }
        : { behavior: 'deny', message: decision.message || 'Denied' };

      send({
        jsonrpc: '2.0',
        id,
        result: {
          content: [{
            type: 'text',
            text: JSON.stringify(response)
          }]
        }
      });
    });
    return;
  }

  // Unknown method
  send({
    jsonrpc: '2.0',
    id,
    error: { code: -32601, message: `Unknown method: ${method}` }
  });
}

rl.on('line', (line) => {
  try {
    const req = JSON.parse(line);
    handleRequest(req);
  } catch (e) {
    console.error('[error]', e.message);
  }
});

console.error('[test-permit-server] Started, waiting for MCP messages...');
```

### Step 2: Create MCP Config

`/tmp/test-mcp.json`:

```json
{
  "mcpServers": {
    "test": {
      "command": "node",
      "args": ["/tmp/test-permit-server.js"]
    }
  }
}
```

### Step 3: Run the Test

**Terminal 1** - Launch Claude with permission-prompt-tool:
```bash
# Make server executable
chmod +x /tmp/test-permit-server.js

# Launch Claude with the test MCP server
claude \
  --permission-prompt-tool mcp__test__permit \
  --mcp-config /tmp/test-mcp.json \
  --print \
  "Read the contents of /etc/hostname"
```

**Terminal 2** - Act as human approver:
```bash
# Watch for pending approvals
watch -n1 cat /tmp/pending.json 2>/dev/null

# When approval appears, approve it:
echo '{"approved": true}' > /tmp/decision.json

# Or deny it:
echo '{"approved": false, "message": "Not allowed"}' > /tmp/decision.json
```

### Expected Results

**If permission-prompt-tool works with stdio:**
1. Claude will call `mcp__test__permit` before executing Read
2. `/tmp/pending.json` will contain the approval request
3. Server blocks until `/tmp/decision.json` is written
4. Claude proceeds or stops based on decision

**If it doesn't work:**
1. Claude executes Read immediately without calling permit
2. No `/tmp/pending.json` created
3. This confirms the issue is NOT specific to Plexus

### Step 4: Variations to Test

Once basic flow works, test edge cases:

1. **Tool naming with dots**:
   ```javascript
   // In tools/list, try:
   name: 'loopback.permit'  // vs 'permit'
   ```

2. **Response format variations**:
   ```javascript
   // Try with/without updatedInput
   { behavior: 'allow' }
   { behavior: 'allow', updatedInput: input }
   ```

3. **Multiple tools**:
   - Add other tools to verify permit is only called for permission checks

### Step 5: Integration Path

Once the minimal test works:

1. **Replace file-based IPC with HTTP calls to Plexus**:
   ```javascript
   // Instead of file polling:
   const response = await fetch('http://127.0.0.1:4445/mcp', {
     method: 'POST',
     body: JSON.stringify({
       jsonrpc: '2.0',
       method: 'tools/call',
       params: { name: 'loopback.permit', arguments: { tool_name, input } }
     })
   });
   ```

2. **Or use the test server as a permanent stdio-to-Plexus bridge**

### Why This Approach

This minimal test environment:

1. **Isolates variables**: Tests ONLY the permission-prompt-tool mechanism
2. **Mirrors HumanLayer exactly**: stdio transport, simple tool name, same response format
3. **Zero dependencies**: Single Node.js file, no build step
4. **Fast iteration**: Modify and re-test in seconds
5. **Clear success criteria**: Either `/tmp/pending.json` appears or it doesn't

## Resolution

**Fixed in commit**: Removed token count suffix from `src/mcp_bridge.rs`

The permission-prompt-tool now works correctly with Claude Code over HTTP MCP:

```bash
claude --permission-prompt-tool "mcp__plexus__loopback_permit" --print "curl example.com"
```

### Key Learnings

1. **HTTP MCP works** - Claude Code's `--permission-prompt-tool` does support HTTP MCP transport (contrary to initial hypothesis)
2. **Response format is critical** - The `content[0].text` field must be valid JSON that Claude Code can parse
3. **No metadata in responses** - Tool responses used by Claude Code infrastructure (like permission-prompt-tool) must not have any appended metadata

## Streaming Output from Headless Claude Code

To get real-time event streams from Claude Code running in headless/print mode:

### Required Flags

```bash
claude \
  --print \
  --output-format stream-json \
  --verbose \
  --model <model> \
  "your prompt"
```

| Flag | Required | Purpose |
|------|----------|---------|
| `--print` | Yes | Non-interactive mode (no TUI) |
| `--output-format stream-json` | Yes | Emit newline-delimited JSON events |
| `--verbose` | Yes | **Required** when using `stream-json` |
| `--include-partial-messages` | No | Include streaming text deltas |

### Event Types

Events are newline-delimited JSON objects with a `type` field:

```json
{"type":"system","subtype":"init","session_id":"uuid","model":"claude-sonnet-4-20250514","cwd":"/path","tools":["Bash","Read",...]}
{"type":"assistant","message":{"id":"msg_xxx","content":[...],"model":"..."}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}}
{"type":"result","subtype":"success","session_id":"uuid","cost_usd":0.01,"duration_ms":1234,"num_turns":1}
```

| Event Type | Description |
|------------|-------------|
| `system` | Session initialization - model, cwd, available tools |
| `assistant` | Complete assistant message with all content blocks |
| `user` | User message (when replaying) |
| `stream_event` | Partial streaming chunks (requires `--include-partial-messages`) |
| `result` | Final result with cost, duration, error status |

### Stream Event Subtypes

When `--include-partial-messages` is enabled, `stream_event` contains granular updates:

| Subtype | Description |
|---------|-------------|
| `message_start` | New message beginning |
| `content_block_start` | New content block (text or tool_use) |
| `content_block_delta` | Incremental text or JSON delta |
| `content_block_stop` | Content block complete |
| `message_delta` | Message-level updates (stop_reason) |
| `message_stop` | Message complete |

### Full Example with Permission Routing

```bash
claude \
  --print \
  --output-format stream-json \
  --verbose \
  --include-partial-messages \
  --model haiku \
  --permission-prompt-tool "mcp__plexus__loopback_permit" \
  --mcp-config '{"mcpServers":{"plexus":{"type":"http","url":"http://127.0.0.1:4445/mcp"}}}' \
  "curl example.com"
```

### Parsing in Code

**Node.js:**
```javascript
const { spawn } = require('child_process');
const claude = spawn('claude', ['--print', '--output-format', 'stream-json', '--verbose', '-p', 'hello']);

claude.stdout.on('data', (chunk) => {
  for (const line of chunk.toString().split('\n').filter(Boolean)) {
    const event = JSON.parse(line);
    console.log(event.type, event);
  }
});
```

**Rust (see `executor.rs`):**
```rust
let mut reader = BufReader::new(stdout).lines();
while let Some(line) = reader.next_line().await? {
    let event: RawClaudeEvent = serde_json::from_str(&line)?;
    // Process event...
}
```

### Input Streaming

Claude Code also supports streaming input with `--input-format stream-json`:

```bash
claude \
  --print \
  --input-format stream-json \
  --output-format stream-json \
  --verbose \
  --replay-user-messages
```

This enables bidirectional streaming for real-time agent orchestration.

## Appendix: Test Commands

### Verify MCP Server Responds
```bash
# Initialize session
curl -i -X POST http://127.0.0.1:4445/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}'

# Note mcp-session-id from response headers, then:
curl -X POST http://127.0.0.1:4445/mcp \
  -H "mcp-session-id: <session-id>" \
  -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'

curl -X POST http://127.0.0.1:4445/mcp \
  -H "mcp-session-id: <session-id>" \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":2}'
```

### Verify Loopback Directly
```bash
synapse plexus loopback.permit tool_name=Read tool_use_id=test-123 input='{"path":"/tmp/test"}'
# In another terminal:
synapse plexus loopback.pending
synapse plexus loopback.respond approval_id=<id> approve=true
```

## Related Documents

- `plans/CLAUDECODE/LOOPBACK-1.md` - Original implementation plan
- `plans/CLAUDECODE/LOOPBACK-FINDINGS.md` - Earlier response format findings
- `plans/CLAUDECODE/LOOPBACK-DEBUG.md` - Debug investigation notes
- `humanlayer/docs/architecture/16678191215220290559_approval-flow-architecture.md` - HumanLayer reference implementation

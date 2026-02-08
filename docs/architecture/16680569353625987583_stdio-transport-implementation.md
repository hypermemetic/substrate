# stdio Transport Implementation

**Status**: ✅ Implemented
**Date**: 2025-12-19 (Updated: 2025-12-19)
**Author**: Claude Sonnet 4.5
**Affects**: All Plexus clients (symbols CLI, humanlayer-wui, external integrations)

## Updates Since Initial Implementation

### Logging Configuration (2025-12-19)
- **sqlx logging disabled**: Statement logging disabled via `SqliteConnectOptions::disable_statement_logging()` in all storage modules (arbor, cone, claudecode)
- **Debug logs by default**: Release builds now default to `substrate=debug,hub_macro=debug`
- **Boot sequence**: Added ASCII art boot sequence showing active log levels
- **Filter fixed**: Removed stale `RUST_LOG=INFO` from `.env` that was overriding configured filters

### Hub-Macro Adoption
All activations except Health now use `#[hub_methods]` macro:
- **Bash**: 1 method (execute)
- **Arbor**: 19 methods (tree/node/context operations)
- **Cone**: 7 methods (create, get, list, delete, chat, set_head, registry)
- **ClaudeCode**: 6 methods (create, chat, get, list, delete, fork)
- **Health**: Manual implementation (reference pattern)

See `16680562178783729663_session-improvements.md` for detailed hub-macro and type system changes.

## Overview

Implemented transport-agnostic JSON-RPC for Substrate Plexus, enabling the same `RpcModule` to serve both WebSocket (existing) and stdio (new) transports. This enables MCP (Model Context Protocol) compatibility without requiring protocol-level changes.

## Motivation

**Problem**: Plexus was WebSocket-only, preventing integration with MCP-based tools (Claude Code, MCP Inspector, mcptools) which communicate over stdio.

**Solution**: Add stdio transport mode that uses jsonrpsee's `raw_json_request()` to handle line-delimited JSON-RPC over stdin/stdout.

**Key Insight**: MCP and Plexus both use JSON-RPC 2.0 - the difference is purely transport (stdio vs WebSocket). No protocol augmentation needed.

## Architecture

### Before (WebSocket-only)

```
┌─────────────────────────────────┐
│      Plexus (RpcModule)         │
└────────────┬────────────────────┘
             │
             ↓ WebSocket
    ┌────────────────────┐
    │ jsonrpsee Server   │
    │ ws://127.0.0.1:4444│
    └────────────────────┘
             ↑
             │
    ┌────────┴────────┐
    │  WUI Client     │
    │  symbols CLI    │
    └─────────────────┘
```

### After (Transport-agnostic)

```
┌──────────────────────────────────────────────┐
│         Plexus (RpcModule<()>)                │
│    (Transport-agnostic core)                  │
└───────────┬──────────────────────────────────┘
            │
            ├─────────────┬────────────────┐
            ↓             ↓                ↓
      WebSocket       stdio            (Future: HTTP)
    ┌──────────┐   ┌──────────┐
    │ WS Server│   │ stdin/   │
    │ :4444    │   │ stdout   │
    └────┬─────┘   └────┬─────┘
         │              │
         ↓              ↓
   WUI Client     MCP Tools
   symbols CLI    Claude Code
                  mcptools
```

## Implementation Details

### 1. Transport Selection (src/main.rs)

Added CLI flag to select transport mode:

```rust
#[derive(Parser)]
struct Args {
    /// Run in stdio mode for MCP compatibility
    #[arg(long)]
    stdio: bool,

    /// Port for WebSocket server (ignored in stdio mode)
    #[arg(short, long, default_value = "4444")]
    port: u16,
}
```

**Usage**:
- WebSocket: `substrate` (default)
- stdio: `substrate --stdio`

### 2. stdio Transport Implementation (src/main.rs:51-106)

```rust
async fn serve_stdio(module: RpcModule<()>) -> anyhow::Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        // Parse line-delimited JSON-RPC request
        let (response, mut sub_receiver) = module
            .raw_json_request(trimmed, 1024)
            .await?;

        // Write initial response
        stdout.write_all(response.get().as_bytes()).await?;
        stdout.flush().await?;

        // Forward subscription notifications in background
        tokio::spawn(async move {
            while let Some(notification) = sub_receiver.recv().await {
                let mut out = tokio::io::stdout();
                out.write_all(notification.get().as_bytes()).await?;
                out.flush().await?;
            }
        });
    }
}
```

**Key Features**:
- Line-delimited JSON-RPC (newline-separated messages)
- Non-blocking subscription notification forwarding
- Logs to stderr to keep stdout clean
- 1024-message buffer for subscription notifications

### 3. jsonrpsee 0.26 Migration

Upgraded from jsonrpsee 0.21 → 0.26 to access `raw_json_request()`.

**API Changes**:

| Component | 0.21 | 0.26 |
|-----------|------|------|
| Subscription closures | 3 params `(params, pending, ctx)` | 4 params `(params, pending, ctx, ext)` |
| Sending to sink | `SubscriptionMessage::from_json()` | `serde_json::value::to_raw_value()` + `sink.send()` |
| Error type | `StringError` | `SubscriptionError` |

**Files Modified**:
- `Cargo.toml`: Version bump
- `src/plugin_system/conversion.rs`: Updated subscription API
- `src/plexus/plexus.rs`: Updated all subscription handlers
- `src/plexus/middleware.rs`: Removed deprecated middleware

### 4. Transport-agnostic RpcModule

The core `RpcModule` requires **zero changes**. It's completely unaware of transport:

```rust
// Build plexus (same for both transports)
let plexus = build_plexus().await;
let module = plexus.into_rpc_module()?;

// Transport selection
if args.stdio {
    serve_stdio(module).await
} else {
    serve_websocket(module).await
}
```

## Protocol Specification

### JSON-RPC 2.0 Format

**Request** (stdin):
```json
{"jsonrpc":"2.0","method":"bash_execute","params":{"command":"pwd"},"id":1}
```

**Response** (stdout):
```json
{"jsonrpc":"2.0","id":1,"result":1234567890}
```

**Subscription Notifications** (stdout):
```json
{"jsonrpc":"2.0","method":"bash_execute","params":{"subscription":1234567890,"result":{...}}}
```

### Message Flow

```
Client                  Substrate (stdio)
  │                           │
  ├──────── Request ─────────>│  (stdin)
  │  {"method":"bash_execute"}│
  │                           │
  │<──── Subscription ID ─────┤  (stdout)
  │  {"id":1,"result":123}    │
  │                           │
  │<──── Notification 1 ──────┤  (stdout)
  │<──── Notification 2 ──────┤  (stdout)
  │<──── Done Event ──────────┤  (stdout)
```

## Client Migration Guide

### Current WebSocket Clients

**symbols CLI** (`/Users/shmendez/dev/controlflow/symbols/`):
- Uses `Plexus.connect` over WebSocket
- Connects to `ws://127.0.0.1:4444`
- **No changes required** - WebSocket still default

**humanlayer-wui** (`humanlayer-wui/src/lib/daemon/http-client.ts`):
- Uses `HLDClient` over HTTP/WebSocket
- Connects to daemon via `getDaemonUrl()`
- **No changes required** - uses HTTP/WS to daemon

### New stdio Clients

For MCP-compatible clients:

```bash
# Direct usage
echo '{"jsonrpc":"2.0","method":"substrate.schema","id":1}' | \
  substrate --stdio

# With mcptools
mcp shell substrate --stdio
```

**TypeScript/Node.js Client**:
```typescript
import { spawn } from 'child_process';

const substrate = spawn('substrate', ['--stdio']);

// Write requests to stdin
substrate.stdin.write(JSON.stringify({
  jsonrpc: '2.0',
  method: 'bash_execute',
  params: { command: 'pwd' },
  id: 1
}) + '\n');

// Read responses from stdout
substrate.stdout.on('data', (data) => {
  const lines = data.toString().split('\n');
  lines.forEach(line => {
    if (line.trim()) {
      const message = JSON.parse(line);
      console.log(message);
    }
  });
});
```

**Python Client**:
```python
import subprocess
import json

proc = subprocess.Popen(
    ['substrate', '--stdio'],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    text=True
)

# Send request
request = json.dumps({
    "jsonrpc": "2.0",
    "method": "health_check",
    "params": {},
    "id": 1
})
proc.stdin.write(request + '\n')
proc.stdin.flush()

# Read response
for line in proc.stdout:
    message = json.loads(line)
    print(message)
```

### symbols CLI - Future stdio Support

To add stdio support to symbols CLI:

1. **Add stdio connection mode** to `Plexus.Client`:
```haskell
data ConnectionMode
  = WebSocket String Int  -- Host, Port
  | Stdio FilePath        -- Path to substrate binary

connectStdio :: FilePath -> IO PlexusConnection
connectStdio binPath = do
  (Just stdin, Just stdout, _, ph) <-
    createProcess (proc binPath ["--stdio"])
      { std_in = CreatePipe
      , std_out = CreatePipe
      , std_err = Inherit
      }
  -- Parse line-delimited JSON from stdout
  -- Send JSON-RPC to stdin
```

2. **Update dynamic CLI** to support `--stdio` flag:
```haskell
data GlobalOpts = GlobalOpts
  { optStdio :: Maybe FilePath  -- Path to substrate binary
  , optHost :: String
  , optPort :: Int
  -- ...
  }
```

3. **Connection logic**:
```haskell
connect :: GlobalOpts -> IO PlexusConnection
connect GlobalOpts{..} = case optStdio of
  Just binPath -> connectStdio binPath
  Nothing -> connectWebSocket optHost optPort
```

## Testing

### Validation Results

✅ **All features working**:
- Method calls (`substrate.schema`, `health_check`)
- Streaming subscriptions (`bash_execute`)
- Multiple concurrent requests
- All 5 activations (34 methods)

### Test Commands

```bash
# Basic method call
echo '{"jsonrpc":"2.0","method":"substrate.hash","id":1}' | \
  substrate --stdio

# Subscription with streaming
(echo '{"jsonrpc":"2.0","method":"bash_execute","params":{"command":"echo test"},"id":2}'; sleep 1) | \
  substrate --stdio

# List available activations
(echo '{"jsonrpc":"2.0","method":"substrate.schema","id":3}'; sleep 1) | \
  substrate --stdio | jq -c 'select(.params.result.data.activations)'
```

### MCP Inspector Testing

```bash
# Install
npx @modelcontextprotocol/inspector

# Configure stdio transport
# Command: /path/to/substrate --stdio
# Access UI at http://localhost:6274
```

### mcptools CLI Testing

```bash
# Install
brew tap f/mcptools && brew install mcp

# Interactive shell
mcp shell ./target/release/substrate --stdio

# List tools
mcp tools ./target/release/substrate --stdio

# Call method
mcp call bash_execute --params '{"command":"pwd"}' \
  ./target/release/substrate --stdio
```

## Performance Considerations

### stdio vs WebSocket

| Aspect | WebSocket | stdio |
|--------|-----------|-------|
| Connection overhead | TCP handshake | Process spawn |
| Latency | Network (local: ~1ms) | IPC (~0.1ms) |
| Throughput | High | High |
| Multiplexing | Single connection | Process per client |
| Use case | Long-lived, multi-client | Single-client, tool integration |

**Recommendation**:
- Use **WebSocket** for web UIs, long-running clients
- Use **stdio** for CLI tools, MCP integration, automation

### Resource Usage

stdio mode spawns one substrate process per client:

```bash
# Multiple concurrent clients
mcp shell substrate --stdio &  # Process 1
mcp shell substrate --stdio &  # Process 2
# Each has isolated state, SQLite connection
```

**Optimization**: For high-concurrency scenarios, consider daemon mode with WebSocket (current humanlayer-wui approach).

## Migration Timeline

### Phase 1: ✅ Complete (2025-12-19)
- [x] stdio transport implementation
- [x] jsonrpsee 0.26 migration
- [x] CLI flag for transport selection
- [x] Testing and validation

### Phase 2: Future
- [ ] Update symbols CLI to support `--stdio` flag
- [ ] Add stdio examples to documentation
- [ ] Create TypeScript/Python client libraries
- [ ] Add transport selection to configuration files

### Phase 3: Future
- [ ] HTTP transport (REST-like single-shot calls)
- [ ] Unix domain socket transport
- [ ] Transport auto-detection (env vars)

## Breaking Changes

**None** - This is a backwards-compatible addition:
- Existing WebSocket clients continue working unchanged
- Default behavior unchanged (WebSocket on port 4444)
- stdio is opt-in via `--stdio` flag

## Security Considerations

### stdio Mode
- **Process isolation**: Each client gets dedicated process
- **No network exposure**: Communication over file descriptors only
- **Same permissions**: Inherits spawning process permissions

### WebSocket Mode
- **Network binding**: Listens on `127.0.0.1:4444` (localhost only)
- **No authentication**: Designed for local-only access
- **Shared state**: Multiple clients share same process/database

**Recommendation**: Use stdio for untrusted contexts, WebSocket for local development.

## Related Documents

- `docs/architecture/16680880693241553663_frontend-guidance-migration.md` - Stream-based guidance
- `docs/architecture/16680881573410764543_guidance-stream-based-errors.md` - Error guidance design
- `docs/architecture/old/2025-12-07_jsonrpc-library-comparison.md` - jsonrpsee selection rationale

## References

- [Model Context Protocol Specification](https://modelcontextprotocol.io/)
- [jsonrpsee Documentation](https://docs.rs/jsonrpsee/)
- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)

## Appendix: Complete Examples

### Example 1: Health Check

**Request**:
```bash
echo '{"jsonrpc":"2.0","method":"health_check","params":{},"id":1}' | substrate --stdio
```

**Response**:
```json
{"jsonrpc":"2.0","id":1,"result":4327288867291854}
{"jsonrpc":"2.0","method":"health_check","params":{"subscription":4327288867291854,"result":{"plexus_hash":"49df07e4f596ea6a","type":"data","provenance":{"segments":["health"]},"content_type":"health.event","data":{"status":"healthy","timestamp":1766173921,"type":"status","uptime_seconds":0}}}}
{"jsonrpc":"2.0","method":"health_check","params":{"subscription":4327288867291854,"result":{"plexus_hash":"49df07e4f596ea6a","type":"done","provenance":{"segments":["health"]}}}}
```

### Example 2: bash_execute with Streaming

**Request**:
```bash
(echo '{"jsonrpc":"2.0","method":"bash_execute","params":{"command":"echo Hello && echo World"},"id":2}'; sleep 2) | substrate --stdio
```

**Response**:
```json
{"jsonrpc":"2.0","id":2,"result":2089191490909612}
{"jsonrpc":"2.0","method":"bash_execute","params":{"subscription":2089191490909612,"result":{"plexus_hash":"49df07e4f596ea6a","type":"data","provenance":{"segments":["bash"]},"content_type":"bash.event","data":{"line":"Hello","type":"stdout"}}}}
{"jsonrpc":"2.0","method":"bash_execute","params":{"subscription":2089191490909612,"result":{"plexus_hash":"49df07e4f596ea6a","type":"data","provenance":{"segments":["bash"]},"content_type":"bash.event","data":{"line":"World","type":"stdout"}}}}
{"jsonrpc":"2.0","method":"bash_execute","params":{"subscription":2089191490909612,"result":{"plexus_hash":"49df07e4f596ea6a","type":"data","provenance":{"segments":["bash"]},"content_type":"bash.event","data":{"code":0,"type":"exit"}}}}
{"jsonrpc":"2.0","method":"bash_execute","params":{"subscription":2089191490909612,"result":{"plexus_hash":"49df07e4f596ea6a","type":"done","provenance":{"segments":["bash"]}}}}}
```

### Example 3: Available Activations

**Request**:
```bash
(echo '{"jsonrpc":"2.0","method":"plexus_schema","id":3}'; sleep 1) | substrate --stdio | jq -c 'select(.params.result.data.activations) | .params.result.data.activations[] | {namespace, version, method_count: (.methods | length)}'
```

**Output**:
```json
{"namespace":"arbor","version":"1.0.0","method_count":19}
{"namespace":"bash","version":"1.0.0","method_count":1}
{"namespace":"claudecode","version":"1.0.0","method_count":6}
{"namespace":"cone","version":"1.0.0","method_count":7}
{"namespace":"health","version":"1.0.0","method_count":1}
```

---

**Implementation complete. Transport-agnostic Plexus ready for production use.**

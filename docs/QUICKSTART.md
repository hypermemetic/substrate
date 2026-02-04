# Plexus RPC Quick Start

Get started with Plexus RPC in 5 minutes. This guide takes you from zero to making your first RPC calls.

## What is Plexus RPC?

Plexus RPC is a protocol where **code IS schema**. Write Rust methods, get type-safe clients and streaming by default. No separate schema files, zero drift.

## Prerequisites

- Rust 1.70+ and Cargo
- Optional: `wscat` for testing (`npm install -g wscat`)
- Optional: Haskell Stack for Synapse CLI

## 1. Clone and Build

```bash
# Clone the repository
git clone https://github.com/your-org/substrate.git
cd substrate

# Build in release mode
cargo build --release

# The binary will be at target/release/substrate
```

## 2. Start the Server

```bash
cargo run --release
```

You should see output like:

```
INFO substrate: Substrate plexus started at ws://127.0.0.1:4444
INFO substrate: Available activations:
    - arbor (Conversation tree storage)
    - cone (LLM orchestration)
    - echo (Echo service)
    - health (Health checks)
```

The server is now running on `ws://127.0.0.1:4444`.

## 3. Make Your First RPC Call

### Option A: Using wscat (WebSocket)

```bash
wscat -c ws://localhost:4444
```

Once connected, send a JSON-RPC request:

```json
{"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"echo.echo","params":{"message":"Hello Plexus!","count":3}}}
```

You'll receive streaming responses:

```json
{"jsonrpc":"2.0","id":1,"result":{"type":"Content","data":{"message":"Hello Plexus!"}}}
{"jsonrpc":"2.0","id":1,"result":{"type":"Content","data":{"message":"Hello Plexus!"}}}
{"jsonrpc":"2.0","id":1,"result":{"type":"Content","data":{"message":"Hello Plexus!"}}}
{"jsonrpc":"2.0","id":1,"result":{"type":"Done"}}
```

### Option B: Using curl (HTTP)

```bash
# Simple echo call
curl -X POST http://localhost:4444/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "plexus_call",
    "params": {
      "method": "echo.echo",
      "params": {
        "message": "Hello Plexus!",
        "count": 1
      }
    }
  }'
```

## 4. Query the Schema

Get the full schema for all activations:

```bash
# Via wscat
{"jsonrpc":"2.0","id":2,"method":"plexus_schema"}

# Via curl
curl -X POST http://localhost:4444/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"plexus_schema"}'
```

Query a specific activation's schema:

```bash
{"jsonrpc":"2.0","id":3,"method":"plexus_call","params":{"method":"arbor.schema"}}
```

The schema includes:
- Method names and descriptions
- Parameter types (JSON Schema)
- Return types (JSON Schema)
- Streaming annotations
- Hash-based versioning

## 5. Use the Synapse CLI

Synapse is a dynamic CLI generated from Plexus RPC schemas. Install it with Haskell Stack:

```bash
# From the synapse directory
cd ../synapse
stack install

# Connect and explore
synapse
```

Synapse provides interactive commands for all Plexus RPC methods:

```bash
# List available activations
synapse help

# Call arbor methods
synapse arbor tree-list
synapse arbor tree-create '{"title":"My Tree"}'

# Call cone methods
synapse cone chat my-session "Tell me about Plexus RPC"

# Get method-specific help
synapse arbor tree-create --help
```

## 6. Generate a TypeScript Client

Generate type-safe TypeScript clients from runtime schemas:

```bash
# Install the codegen tool (requires hub-codegen)
cd ../hub-codegen
cargo install --path .

# Generate TypeScript client
hub-codegen --server ws://localhost:4444 --output ./client

# This creates:
# ./client/index.ts - Type-safe client with all activations
# ./client/types.ts - TypeScript types matching Rust schemas
```

Use the generated client:

```typescript
import { createPlexusClient } from './client';

const client = await createPlexusClient('ws://localhost:4444');

// Type-safe calls with autocomplete
for await (const item of client.echo.echo({ message: "Hello", count: 3 })) {
  console.log(item);
}

// Streaming with progress
for await (const item of client.cone.chat({
  name: "session1",
  prompt: "Explain Plexus RPC"
})) {
  if (item.type === "Progress") {
    console.log(`Progress: ${item.percentage}%`);
  } else if (item.type === "Content") {
    console.log(item.data);
  }
}
```

## 7. Explore the Schema Structure

Plexus RPC uses tree-structured namespaces with dot notation:

```
plexus
├── arbor.tree_create       (Conversation tree storage)
├── arbor.node_create_text
├── cone.create             (LLM orchestration)
├── cone.chat
├── echo.echo               (Echo service)
└── health.check            (Health checks)
```

Every activation can have child activations, creating arbitrary nesting depth.

## Key Concepts

### Streaming by Default

All Plexus RPC methods return streams. Even non-streaming methods emit:
1. `Content` item(s) with the result
2. `Done` item to signal completion

Streaming methods can emit:
- `Progress` - progress updates with percentage
- `Content` - actual data chunks
- `Error` - recoverable errors
- `Done` - stream completion

### Hash-Based Versioning

Every method schema has a content hash. Parent hashes incorporate child hashes. When any method changes, the root hash changes. This enables:
- Automatic cache invalidation
- Client version detection
- Schema drift warnings

### Code IS Schema

The Rust implementation IS the schema. There are no separate `.proto` files, YAML specs, or GraphQL schemas to maintain. The `#[hub_method]` macro extracts schemas at compile time, making drift impossible.

## Next Steps

- **[Examples](../examples/)** - Explore example activations (Echo, Solar, Health)
- **[Comparison](./COMPARISON.md)** - How Plexus RPC compares to gRPC, OpenAPI, tRPC, GraphQL
- **[Architecture](../README.md)** - Deep dive into Plexus RPC design
- **[Plugin Development Guide](./architecture/16678373036159325695_plugin-development-guide.md)** - Create your own activations

## Common Operations

### Check Server Health

```bash
{"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"health.check"}}
```

### Create a Conversation Tree

```bash
{"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"arbor.tree_create","params":{"title":"My Conversation"}}}
```

### Chat with an LLM

```bash
{"jsonrpc":"2.0","id":1,"method":"plexus_call","params":{"method":"cone.chat","params":{"name":"session1","prompt":"Hello!"}}}
```

## Troubleshooting

### Server won't start

- Check if port 4444 is already in use: `lsof -i :4444`
- Set a custom port via environment variable: `SUBSTRATE_PORT=8080 cargo run`

### WebSocket connection fails

- Ensure the server is running: `ps aux | grep substrate`
- Check firewall rules
- Verify the URL: `ws://127.0.0.1:4444` (not `wss://` for local)

### Schema queries return empty

- Wait a few seconds after server startup
- Check server logs for activation registration errors
- Verify activations are properly registered in `main.rs`

## Learn More

Plexus RPC's key value proposition:

> **Write Rust methods. Get type-safe clients, dynamic CLIs, and streaming by default. Zero separate schema files, zero drift.**

For detailed architecture and advanced usage, see the [full documentation](../README.md).

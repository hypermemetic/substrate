# Mobile Client Architecture (Tauri + iOS)

**Status**: Design proposal
**Date**: 2026-01-25
**Audience**: Mobile app developers, client integrators

## Overview

This document describes the architecture for building iOS (and cross-platform mobile) applications using Tauri 2.0+ with remote Substrate backend connectivity. The approach leverages existing `hub-codegen` TypeScript client generation to create type-safe, WebSocket-connected mobile apps.

## Architecture

### High-Level Design

```
┌──────────────────────────────────────┐         ┌─────────────────────┐
│     iOS Device (Tauri App)           │         │  Remote Server      │
│                                      │         │  (Mac/Linux/Cloud)  │
│  ┌────────────────────────────────┐ │  WS     │                     │
│  │  Frontend (React/Vue/Svelte)   │ │ :4444   │  ┌───────────────┐  │
│  │                                 │ │◄───────►│  │  Substrate    │  │
│  │  import { createClient }        │ │         │  │               │  │
│  │    from '@plexus/client'        │ │         │  │  Plexus Hub:  │  │
│  │                                 │ │         │  │  - arbor      │  │
│  │  const client = createClient({  │ │         │  │  - cone       │  │
│  │    url: 'ws://server:4444'      │ │         │  │  - claudecode │  │
│  │  })                             │ │         │  │  - bash       │  │
│  │                                 │ │         │  │  - jsexec     │  │
│  │  // Type-safe API calls         │ │         │  │  - etc.       │  │
│  │  await client.cone.create(...)  │ │         │  └───────────────┘  │
│  │  for await (const msg of        │ │         │                     │
│  │    client.cone.chat(...)) {...} │ │         │                     │
│  └────────────────────────────────┘ │         │                     │
│                                      │         │                     │
│  ┌────────────────────────────────┐ │         │                     │
│  │  Tauri Runtime (Rust)          │ │         │                     │
│  │  - WebView host                │ │         │                     │
│  │  - IPC bridge                  │ │         │                     │
│  │  - Native capabilities         │ │         │                     │
│  └────────────────────────────────┘ │         │                     │
└──────────────────────────────────────┘         └─────────────────────┘
```

### Key Design Decision: Remote Backend

**We do NOT embed the Substrate Rust backend in the iOS app.** Instead:

1. **Backend runs remotely** - Mac workstation, Linux server, or cloud instance
2. **Mobile app is thin client** - Pure UI layer with WebSocket transport
3. **Generated TypeScript client** - Fully type-safe via `hub-codegen`

**Rationale:**

- **No iOS restrictions** - bash, claudecode, process spawning all work server-side
- **Simpler deployment** - Just webview + generated client code
- **Better performance** - Heavy compute stays on server hardware
- **Consistent backend** - Same Substrate instance across desktop CLI, web, and mobile
- **Easier updates** - Update server without App Store review

## Technology Stack

### Mobile App Layer

**Tauri 2.0+**
- Cross-platform (iOS, Android, desktop) from single codebase
- Rust runtime for native capabilities
- Web technologies for UI (React, Vue, Svelte)
- IPC bridge between webview and native code
- iOS support: https://v2.tauri.app/start/migrate/from-tauri-1/#ios-and-android-support

**Frontend Framework** (Choose one)
- React + TypeScript (most ecosystem support)
- Vue 3 + TypeScript (reactive, simpler learning curve)
- Svelte + TypeScript (smallest bundle, fastest)

**Generated Client** (`hub-codegen` output)
- Location: `substrate-sandbox-ts/lib/` (reference implementation)
- Files: `transport.ts`, `rpc.ts`, namespace clients (cone/, arbor/, etc.)
- Transport: WebSocket (`SubstrateClient` class)
- Protocol: JSON-RPC 2.0 with subscription support
- Type safety: Full TypeScript types for all methods and events

### Backend Layer

**Substrate Server** (existing)
- Runs on Mac/Linux/Cloud
- Exposes WebSocket on port 4444 (configurable)
- Full Plexus hub with all activations
- No mobile-specific changes needed

**hub-transport** (existing)
- Generic transport layer for any `Activation`
- WebSocket JSON-RPC server
- MCP HTTP support (optional)
- See `hub-transport/README.md`

## Code Generation Pipeline

```
┌─────────────────┐
│  Rust Plugins   │
│  (substrate)    │
│  #[hub_methods] │
└────────┬────────┘
         │
         ▼
┌─────────────────┐      ┌──────────────┐      ┌─────────────────┐
│  JSON Schema    │─────▶│   Synapse    │─────▶│      IR         │
│  (via schemars) │      │   (Haskell)  │      │    (JSON)       │
└─────────────────┘      └──────────────┘      └────────┬────────┘
                                                         │
                                                         ▼
                                          ┌──────────────────────────┐
                                          │     hub-codegen          │
                                          │     (Rust)               │
                                          │  - transport.ts          │
                                          │  - rpc.ts                │
                                          │  - types.ts              │
                                          │  - cone/client.ts        │
                                          │  - arbor/client.ts       │
                                          │  - etc.                  │
                                          └─────────┬────────────────┘
                                                    │
                                                    ▼
                                          ┌──────────────────────────┐
                                          │  Mobile App Frontend     │
                                          │  (Type-safe imports)     │
                                          └──────────────────────────┘
```

**Regeneration Workflow:**

```bash
# When backend schema changes
cd substrate
synapse plexus -i | hub-codegen -o ../mobile-app/src/plexus-client

# TypeScript types automatically updated
# Compiler catches breaking changes
```

## Client Usage Patterns

### Basic Setup

```typescript
// src/lib/substrate.ts
import { createClient } from './plexus-client';

export const substrate = createClient({
  url: import.meta.env.VITE_SUBSTRATE_URL || 'ws://localhost:4444',
  connectionTimeout: 10000,
  debug: import.meta.env.DEV,
});
```

### Streaming Methods (AsyncGenerator)

```typescript
// Cone chat - streaming LLM responses
async function chatWithCone(coneName: string, prompt: string) {
  for await (const event of substrate.cone.chat({
    identifier: { type: 'by_name', name: coneName },
    prompt,
  })) {
    switch (event.type) {
      case 'chat_start':
        console.log('Chat started:', event.coneId);
        break;
      case 'chat_content':
        // Streaming token
        appendToUI(event.content);
        break;
      case 'chat_complete':
        console.log('Chat complete:', event.usage);
        break;
      case 'error':
        showError(event.message);
        break;
    }
  }
}
```

### Non-Streaming Methods (Promise)

```typescript
// Cone list - single result
const result = await substrate.cone.list();
if (result.type === 'cone_list') {
  displayCones(result.cones);
} else {
  showError(result.message);
}

// Arbor tree creation
const createResult = await substrate.arbor.tree.create({
  ownerId: 'user-123',
  metadata: { name: 'My Conversation' },
});
```

### Error Handling

```typescript
try {
  const result = await substrate.cone.create({
    name: 'my-assistant',
    modelId: 'claude-3-haiku-20240307',
    systemPrompt: 'You are a helpful assistant.',
  });

  if (result.type === 'error') {
    // Graceful error (returned, not thrown)
    console.error('Failed to create cone:', result.message);
  } else {
    console.log('Created cone:', result.coneId);
  }
} catch (err) {
  // Transport error (network, WebSocket)
  console.error('Network error:', err);
}
```

## Implementation Guide

### Step 1: Create Tauri Project

```bash
# Install Tauri CLI
npm install -g @tauri-apps/cli@next

# Create new project
npm create tauri-app@latest substrate-mobile
# Choose: React/Vue/Svelte + TypeScript

cd substrate-mobile
```

### Step 2: Add iOS Support

```bash
# Initialize iOS target
npx tauri ios init

# iOS dependencies (macOS only)
# - Xcode 15+
# - iOS SDK
# - CocoaPods
```

### Step 3: Copy Generated Client

```bash
# From substrate repo
cd substrate
synapse plexus -i | hub-codegen -o ../substrate-mobile/src/plexus-client

# Or copy from substrate-sandbox-ts if already generated
cp -r ../substrate-sandbox-ts/lib ../substrate-mobile/src/plexus-client
```

### Step 4: Configure Environment

```javascript
// .env.development
VITE_SUBSTRATE_URL=ws://localhost:4444

// .env.production
VITE_SUBSTRATE_URL=wss://your-server.com:4444
```

### Step 5: Build UI

```typescript
// src/App.tsx (React example)
import { useState, useEffect } from 'react';
import { substrate } from './lib/substrate';

export default function App() {
  const [cones, setCones] = useState([]);

  useEffect(() => {
    loadCones();
  }, []);

  async function loadCones() {
    const result = await substrate.cone.list();
    if (result.type === 'cone_list') {
      setCones(result.cones);
    }
  }

  return (
    <div>
      <h1>My Cones</h1>
      <ul>
        {cones.map(cone => (
          <li key={cone.id}>{cone.name}</li>
        ))}
      </ul>
    </div>
  );
}
```

### Step 6: Run & Test

```bash
# Development (iOS simulator)
npx tauri ios dev

# Production build
npx tauri ios build
```

## Deployment Strategies

### Development

**Backend:** Local Substrate server on Mac
```bash
cd substrate
cargo run
# Listening on ws://127.0.0.1:4444
```

**Mobile:** iOS Simulator connecting to localhost
```bash
cd substrate-mobile
npx tauri ios dev
# Client connects to ws://localhost:4444
```

### Production

**Option 1: VPS/Cloud Server**
- Deploy Substrate to DigitalOcean/AWS/Hetzner
- Expose WebSocket on public IP with TLS
- Mobile app connects to `wss://your-server.com:4444`
- Requires: SSL certificate, firewall config

**Option 2: User's Mac (personal use)**
- User runs Substrate on their Mac
- ngrok or Tailscale for remote access
- Mobile app connects via tunnel URL
- Best for: Power users, developers

**Option 3: Hybrid (recommended)**
- Public hosted instance for general users
- Optional self-hosted for power users
- App has server URL config screen

## Security Considerations

### Authentication

Current Substrate has **no authentication**. For production mobile apps:

**Option 1: Add Auth to Substrate**
```rust
// New activation: substrate/src/activations/auth/
// JWT-based authentication
// Middleware on WebSocket connection
```

**Option 2: Reverse Proxy**
```nginx
# nginx in front of Substrate
# Handle auth, rate limiting, TLS
upstream substrate {
    server localhost:4444;
}
```

**Option 3: VPN/Tailscale**
- Private network access only
- No internet exposure
- Best for personal use

### Data Privacy

- **LLM API keys** - Stored server-side only (never in mobile app)
- **Conversation trees** - Stored in Substrate's SQLite (server)
- **User data** - Mobile app has no persistent storage
- **Transport** - Use WSS (WebSocket Secure) in production

### iOS App Store Review

**Potential Issues:**
- Apps using AI/LLM features face scrutiny
- Need clear privacy policy
- Explain data usage
- Consider App Store alternatives (TestFlight, direct distribution)

## Performance Considerations

### WebSocket Connection Management

```typescript
// Reconnection logic
class ResilientSubstrateClient {
  private client: SubstrateClient;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;

  async connect() {
    try {
      await this.client.connect();
      this.reconnectAttempts = 0;
    } catch (err) {
      if (this.reconnectAttempts < this.maxReconnectAttempts) {
        this.reconnectAttempts++;
        const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
        await new Promise(resolve => setTimeout(resolve, delay));
        return this.connect();
      }
      throw err;
    }
  }
}
```

### iOS Background Limitations

- **iOS suspends apps** aggressively (after 30 seconds in background)
- **WebSocket connections** will close
- **Strategy:** Reconnect on app foreground event

```typescript
// React Native-style lifecycle (adapt for Tauri)
useEffect(() => {
  const handleAppStateChange = (state: string) => {
    if (state === 'active') {
      substrate.reconnect();
    }
  };
  // Subscribe to app state changes
  return () => {
    // Cleanup
  };
}, []);
```

### Streaming Response Buffering

For long-running streams (large tree renders, long chats):

```typescript
// Buffer and batch UI updates
const BATCH_INTERVAL = 100; // ms
let buffer: string[] = [];
let timer: NodeJS.Timeout | null = null;

for await (const event of substrate.cone.chat(...)) {
  if (event.type === 'chat_content') {
    buffer.push(event.content);

    if (!timer) {
      timer = setTimeout(() => {
        flushToUI(buffer.join(''));
        buffer = [];
        timer = null;
      }, BATCH_INTERVAL);
    }
  }
}
```

## Future Enhancements

### Offline Mode

**Challenge:** Mobile apps should work offline
**Solution:** Local SQLite cache with sync

```typescript
// Hybrid client: local cache + remote sync
class HybridSubstrateClient {
  private remote: SubstrateClient;
  private cache: LocalCache; // IndexedDB/SQLite

  async cone.list() {
    // Try remote first
    try {
      const result = await this.remote.cone.list();
      this.cache.update('cones', result);
      return result;
    } catch (err) {
      // Fallback to cache
      return this.cache.get('cones');
    }
  }
}
```

### Push Notifications

**Use case:** Notify when long-running task completes

```rust
// New activation: substrate/src/activations/notifications/
// WebPush integration
// APNs (Apple Push Notification service) support
```

### Native Swift UI Option

**Alternative to Tauri:** Pure Swift app with FFI bindings

```swift
// Swift client using Rust FFI
import SubstrateFFI

let client = SubstrateClient(url: "ws://localhost:4444")
let cones = try await client.cone.list()
```

**Tradeoff:**
- ✅ Better iOS integration (widgets, shortcuts, etc.)
- ❌ Platform-specific code (lose cross-platform)
- ❌ Need to maintain Swift bindings

## Comparison: Alternative Approaches

### ❌ Embed Backend in iOS App

```
┌──────────────────────────┐
│     iOS App              │
│  ┌────────────────────┐  │
│  │  UI (Swift)        │  │
│  └────────────────────┘  │
│  ┌────────────────────┐  │
│  │  Substrate (Rust)  │  │  ← Runs on device
│  │  - arbor           │  │
│  │  - cone            │  │
│  │  - ❌ bash         │  │  ← Restricted!
│  │  - ❌ claudecode   │  │  ← Process limits
│  └────────────────────┘  │
└──────────────────────────┘
```

**Problems:**
- iOS restricts process spawning (breaks bash, claudecode)
- App sandboxing limits file system access
- Embedded Rust increases app size
- App Store review challenges

### ✅ Remote Backend (Chosen Approach)

```
┌──────────────┐         ┌──────────────┐
│  iOS App     │   WS    │  Server      │
│  (UI only)   │◄───────►│  (Full impl) │
└──────────────┘         └──────────────┘
```

**Benefits:**
- No iOS restrictions
- Lighter app
- Easier to update
- Shared backend across platforms

## Related Documentation

- **hub-codegen** - `../../../hub-codegen/README.md` (TypeScript client generation)
- **hub-transport** - `../../../hub-transport/README.md` (WebSocket server)
- **substrate-sandbox-ts** - `../../../substrate-sandbox-ts/README.md` (Reference client)
- **Tauri 2.0 Docs** - https://v2.tauri.app/

## References

- Tauri iOS Support: https://v2.tauri.app/start/migrate/from-tauri-1/#ios-and-android-support
- JSON-RPC 2.0 Spec: https://www.jsonrpc.org/specification
- WebSocket Protocol: https://datatracker.ietf.org/doc/html/rfc6455
- iOS Background Execution: https://developer.apple.com/documentation/uikit/app_and_environment/scenes/preparing_your_ui_to_run_in_the_background

## Appendix: Quick Start Checklist

- [ ] Install Tauri CLI (`npm install -g @tauri-apps/cli@next`)
- [ ] Create Tauri project (`npm create tauri-app@latest`)
- [ ] Add iOS support (`npx tauri ios init`)
- [ ] Copy generated client from `substrate-sandbox-ts/lib/`
- [ ] Configure WebSocket URL in environment
- [ ] Implement UI (React/Vue/Svelte)
- [ ] Test in iOS Simulator (`npx tauri ios dev`)
- [ ] Deploy backend to VPS/cloud
- [ ] Build production app (`npx tauri ios build`)
- [ ] (Optional) Submit to App Store

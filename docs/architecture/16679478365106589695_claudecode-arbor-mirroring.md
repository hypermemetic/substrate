# ClaudeCode Arbor Mirroring Architecture

## Goal

Make the substrate `claudecode` activation a close mirror of Claude Code's internal state, using Arbor as the primary context store. This enables:

1. **Full conversation reconstruction** from arbor alone
2. **Branching/forking** at any point in conversation history
3. **Rich structured content** (tool calls, thinking, etc.) as first-class nodes
4. **Cross-session context sharing** via arbor tree references

## Current State

The existing activation wraps Claude Code CLI and stores:

| Component | Storage | Limitation |
|-----------|---------|------------|
| Session metadata | SQLite (`claudecode_sessions`) | Separate from arbor |
| Messages (text only) | SQLite (`claudecode_messages`) | Only stores final text content |
| Arbor nodes | External handles pointing to SQLite | Content blocks not structured |
| Unknown events | SQLite (`claudecode_unknown_events`) | Disconnected from tree |

**Current arbor tree structure:**
```
tree_root
└── user_node_1 (handle: msg-{id}:user:user)
    └── assistant_node_1 (handle: msg-{id}:assistant:assistant)
        └── user_node_2
            └── assistant_node_2
                └── ...
```

## Claude Code Internal State

From `claude --help` and stream-json output analysis, Claude Code maintains:

### Session State
```json
{
  "session_id": "sess_abc123",      // For --resume
  "model": "claude-sonnet-4-5-...", // Full model ID
  "cwd": "/path/to/project",        // Working directory
  "tools": ["Bash", "Edit", ...],   // Available tools
  "system_prompt": "...",           // Optional
  "mcp_config": {...}               // Optional
}
```

### Message Structure
Each assistant message contains **content blocks**:
```json
{
  "role": "assistant",
  "content": [
    {"type": "thinking", "thinking": "..."},
    {"type": "text", "text": "..."},
    {"type": "tool_use", "id": "tu_...", "name": "Bash", "input": {...}},
    {"type": "tool_result", "tool_use_id": "tu_...", "content": "..."}
  ]
}
```

### Stream Events
```
system (init) → stream_event (content_block_*) → assistant → result
```

## Proposed Design

### 1. Arbor as Primary Store

Every semantic unit becomes an arbor node. Messages are **composite nodes** with content blocks as children:

```
tree_root
└── turn_1 (external: claudecode@1.0.0::turn)
    ├── user_message (external: claudecode@1.0.0::message:user)
    └── assistant_message (external: claudecode@1.0.0::message:assistant)
        ├── thinking_block (external: claudecode@1.0.0::thinking)
        ├── text_block (external: claudecode@1.0.0::content)
        ├── tool_use (external: claudecode@1.0.0::tool_use:Bash:tu_123)
        └── tool_result (external: claudecode@1.0.0::tool_result:tu_123)
└── turn_2
    └── ...
```

### 2. Handle Schema

Handles encode the type and identity of each node:

| Type | Handle Format | Meta Fields |
|------|---------------|-------------|
| Turn | `claudecode@1.0.0::turn` | `[turn_index]` |
| Message | `claudecode@1.0.0::message` | `[msg_id, role]` |
| Content | `claudecode@1.0.0::content` | `[block_index]` |
| Thinking | `claudecode@1.0.0::thinking` | `[block_index, signature?]` |
| Tool Use | `claudecode@1.0.0::tool_use` | `[tool_use_id, tool_name]` |
| Tool Result | `claudecode@1.0.0::tool_result` | `[tool_use_id, is_error]` |

### 3. Storage Layer Changes

**Keep SQLite for:**
- Session registry (name → session_id mapping)
- Claude session ID tracking (for `--resume`)
- Content blob storage (the actual text/JSON for handles)

**Move to Arbor:**
- Conversation structure (what was `claudecode_messages`)
- Turn/block relationships
- Branching history

### 4. New Database Schema

```sql
-- Lightweight session registry (just identity + arbor binding)
CREATE TABLE claudecode_sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    tree_id TEXT NOT NULL,           -- Arbor tree
    head_node_id TEXT NOT NULL,      -- Current position in tree
    claude_session_id TEXT,          -- For --resume
    working_dir TEXT NOT NULL,
    model TEXT NOT NULL,
    system_prompt TEXT,
    mcp_config TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Content blob storage (handles point here)
CREATE TABLE claudecode_blobs (
    id TEXT PRIMARY KEY,              -- UUID
    blob_type TEXT NOT NULL,          -- 'message', 'content', 'thinking', 'tool_use', 'tool_result'
    data TEXT NOT NULL,               -- JSON content
    tokens_in INTEGER,
    tokens_out INTEGER,
    cost_usd REAL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_blobs_type ON claudecode_blobs(blob_type);
```

### 5. Handle Resolution

When walking an arbor tree, external handles are resolved:

```rust
async fn resolve_handle(&self, handle: &Handle) -> Result<BlobData, Error> {
    // Handle format: claudecode@1.0.0::method:meta0:meta1:...
    match handle.method.as_str() {
        "message" | "content" | "thinking" | "tool_use" | "tool_result" => {
            let blob_id = &handle.meta[0]; // First meta is always blob ID
            self.storage.blob_get(blob_id).await
        }
        "turn" => {
            // Turn is a structural node, no blob needed
            Ok(BlobData::Turn { index: handle.meta[0].parse()? })
        }
        _ => Err(Error::UnknownHandleType)
    }
}
```

### 6. Chat Flow (Revised)

```
1. Receive prompt from caller
2. Create turn node (child of current head)
3. Create user_message blob + node (child of turn)
4. Launch Claude Code subprocess
5. Stream events:
   - ContentBlockStart(thinking) → create thinking blob (partial)
   - ContentBlockDelta → append to thinking blob
   - ContentBlockStop → finalize thinking blob, create thinking node
   - ContentBlockStart(text) → create content blob (partial)
   - ...etc for each block type
   - ContentBlockStart(tool_use) → create tool_use blob + node
   - (tool execution happens in Claude)
   - tool_result events → create tool_result blob + node
6. On assistant message complete:
   - Create assistant_message node (parent of all content block nodes)
   - Update head to assistant_message node
7. On result event:
   - Store cost/tokens in blob metadata
   - Update claude_session_id for future --resume
```

### 7. Streaming Content Accumulation

For partial messages, we need to accumulate content before creating blobs:

```rust
struct StreamAccumulator {
    current_block_type: Option<String>,
    current_block_index: usize,
    accumulated_text: String,
    accumulated_tool_input: String,
    pending_blocks: Vec<ContentBlock>,
}

impl StreamAccumulator {
    fn on_content_block_start(&mut self, block: StreamContentBlock) {
        self.current_block_type = Some(block.type_name());
        self.current_block_index = block.index;
        self.accumulated_text.clear();
        self.accumulated_tool_input.clear();
    }

    fn on_content_block_delta(&mut self, delta: StreamDelta) {
        match delta {
            StreamDelta::TextDelta { text } => self.accumulated_text.push_str(&text),
            StreamDelta::InputJsonDelta { partial_json } => {
                self.accumulated_tool_input.push_str(&partial_json)
            }
        }
    }

    fn on_content_block_stop(&mut self) -> ContentBlock {
        // Finalize and return the complete block
        let block = match self.current_block_type.as_deref() {
            Some("text") => ContentBlock::Text { text: self.accumulated_text.clone() },
            Some("thinking") => ContentBlock::Thinking {
                thinking: self.accumulated_text.clone(),
                signature: None,
            },
            Some("tool_use") => ContentBlock::ToolUse {
                id: self.current_tool_id.clone(),
                name: self.current_tool_name.clone(),
                input: serde_json::from_str(&self.accumulated_tool_input).unwrap_or_default(),
            },
            _ => ContentBlock::Unknown,
        };
        self.pending_blocks.push(block.clone());
        block
    }
}
```

### 8. Fork/Branch Semantics

When forking a session:

```rust
async fn fork(&self, source_session: &str, new_name: &str) -> Result<SessionConfig> {
    let source = self.storage.session_get_by_name(source_session).await?;

    // Create new session pointing to same tree, same head
    // The arbor tree is shared; divergence happens on next chat
    let new_session = self.storage.session_create(
        new_name,
        source.working_dir,
        source.model,
        source.system_prompt,
        source.mcp_config,
    ).await?;

    // Update new session's head to source's current position
    self.storage.session_update_head(
        &new_session.id,
        source.head.node_id,
        None, // New Claude session will be created on first chat
    ).await?;

    Ok(new_session)
}
```

On next chat, the forked session creates a new branch:
```
original_tree:
  └── turn_1
      └── turn_2  ← both sessions point here after fork
          ├── turn_3a  ← original session continues here
          └── turn_3b  ← forked session diverges here
```

### 9. Context Reconstruction

To rebuild full context for `--resume`:

```rust
async fn build_context(&self, session: &SessionConfig) -> Result<Vec<Message>> {
    // Walk arbor path from root to head
    let path = self.arbor.context_get_path(
        &session.head.tree_id,
        &session.head.node_id,
    ).await?;

    let mut messages = Vec::new();

    for node in path {
        if let NodeType::External { handle } = &node.data {
            match self.resolve_handle(handle).await? {
                BlobData::Message { role, content_block_ids } => {
                    // Reconstruct message with all its content blocks
                    let blocks = self.resolve_content_blocks(content_block_ids).await?;
                    messages.push(Message { role, content: blocks });
                }
                _ => {} // Skip structural nodes
            }
        }
    }

    messages
}
```

### 10. Migration Strategy

1. **Add new tables** alongside existing ones
2. **Dual-write** during transition: write to both old and new schemas
3. **Read from new** when available, fall back to old
4. **Backfill** existing sessions on access
5. **Remove old tables** once migration complete

## Event Types (Complete)

From Claude Code `stream-json` output:

| Event Type | Description | Arbor Node? |
|------------|-------------|-------------|
| `system` | Session init (session_id, model, cwd, tools) | No (metadata) |
| `stream_event.message_start` | Message begins | No |
| `stream_event.content_block_start` | Block begins (text/thinking/tool_use) | Creates partial |
| `stream_event.content_block_delta` | Streaming content | Appends to partial |
| `stream_event.content_block_stop` | Block ends | Finalizes blob + node |
| `stream_event.message_delta` | Message metadata (stop_reason) | No |
| `stream_event.message_stop` | Message complete | Creates message node |
| `assistant` | Full assistant message (non-streaming) | Creates all nodes |
| `user` | User message echo | Already created |
| `result` | Session complete (cost, turns, session_id) | Updates metadata |

## API Changes

### New Methods

```rust
// Get full conversation as structured data
async fn get_conversation(&self, name: String) -> Stream<ConversationEvent>;

// Get specific content block
async fn get_block(&self, blob_id: String) -> Stream<BlockResult>;

// Set head to arbitrary position (for navigation)
async fn set_head(&self, name: String, node_id: NodeId) -> Stream<SetHeadResult>;

// List branches from a node
async fn list_branches(&self, name: String, node_id: NodeId) -> Stream<BranchInfo>;
```

### Enhanced Chat Events

```rust
enum ChatEvent {
    Start { id: ClaudeCodeId, turn_position: Position },
    UserMessage { position: Position },

    // Granular streaming
    ThinkingStart { block_index: usize },
    ThinkingDelta { text: String },
    ThinkingEnd { position: Position },

    ContentStart { block_index: usize },
    ContentDelta { text: String },
    ContentEnd { position: Position },

    ToolUseStart { tool_use_id: String, tool_name: String },
    ToolUseDelta { partial_json: String },
    ToolUseEnd { position: Position, input: Value },

    ToolResult { tool_use_id: String, output: String, is_error: bool, position: Position },

    AssistantMessage { position: Position },

    Complete {
        new_head: Position,
        claude_session_id: String,
        usage: ChatUsage,
    },

    Passthrough { event_type: String, handle: String, data: Value },
    Err { message: String },
}
```

## Benefits

1. **Full fidelity**: Every content block is addressable
2. **Branching**: Fork at any point, navigate history
3. **Context sharing**: Trees can be referenced by multiple sessions
4. **Reconstruction**: Rebuild any conversation state from arbor
5. **Debugging**: Inspect individual blocks, tool calls, thinking
6. **Streaming**: Real-time updates at block granularity

## Implementation Phases

### Phase 1: Schema + Blob Storage
- Add `claudecode_blobs` table
- Add blob CRUD to storage layer
- Keep existing activation working

### Phase 2: Structured Nodes
- Create content block nodes during streaming
- Parent them to message nodes
- Update handle resolution

### Phase 3: Turn Grouping
- Add turn nodes as structural containers
- Update fork semantics

### Phase 4: New Methods
- Add `get_conversation`, `get_block`, `set_head`, `list_branches`
- Enhanced chat events with granular positions

### Phase 5: Migration
- Backfill existing sessions
- Remove old message table

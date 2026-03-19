# ClaudeCode

Manages Claude Code CLI sessions with persistent conversation history backed by
Arbor trees. Supports forking (branch from any point in history), streaming chat,
async polling, and import/export of Claude's native JSONL session files.

---

## Model

Each session has:
- A **config** (model, system prompt, working dir, loopback settings)
- An **Arbor tree** — conversation stored as nodes (user messages, assistant
  responses, tool use, tool results)
- A **head** — current position in the tree (tree_id + node_id)

Forking creates a new session from any node in the tree, branching the history.

---

## Hub methods

**Namespace:** `claudecode`

### Session management

| Method | Params | Returns |
|--------|--------|---------|
| `create` | `name, working_dir, model, system_prompt?, loopback_enabled?, metadata?` | `CreateResult { session_id, head }` |
| `get` | `session_id` | `GetResult { config }` |
| `list` | — | `ListResult { sessions }` |
| `delete` | `session_id` | `DeleteResult` |
| `fork` | `session_id, fork_at_node` | `ForkResult { session_id, head }` |

### Chat

| Method | Params | Returns |
|--------|--------|---------|
| `chat` | `session_id, prompt` | `Stream<ChatEvent>` — blocking stream |
| `chat_start` | `session_id, prompt` | `ChatStartResult { stream_id }` — non-blocking |
| `poll_stream` | `stream_id, from_offset` | `PollResult { events, status, has_more }` |
| `list_streams` | — | `StreamListResult` |

### Context

| Method | Params | Returns |
|--------|--------|---------|
| `render_context` | `session_id, node_id` | `RenderResult` — root-to-node as Claude API messages |
| `get_tree` | `session_id` | `GetTreeResult { tree_id, head_node_id }` |

### Session file I/O (Claude's JSONL format)

| Method | Params | Returns |
|--------|--------|---------|
| `sessions_list` | `project_path` | `SessionsListResult` |
| `sessions_get` | `project_path, session_id` | `SessionsGetResult` — raw events |
| `sessions_import` | `project_path, session_id, owner_id` | `SessionsImportResult` — imports to Arbor |
| `sessions_export` | `tree_id, project_path, session_id` | `SessionsExportResult` — Arbor → JSONL |

---

## Chat events

```rust
enum ChatEvent {
    Start,
    Content     { text: String },
    Thinking    { text: String },
    ToolUse     { tool_name, input },
    ToolResult  { content },
    Complete    { usage },
    Passthrough { data },
    Err         { message },
}
```

---

## Loopback

When `loopback_enabled = true`, Claude sessions route tool permission requests
through `claudecode_loopback` for external approval. See
[claudecode_loopback](../claudecode_loopback/README.md).

---

## Storage

SQLite (`claudecode.db`): sessions, messages.
Arbor: conversation tree (one tree per session).
In-memory: active stream buffers for non-blocking chat.

# Cone

Multi-model LLM agent with persistent conversation context in Arbor trees.
Wraps the `cllient` registry to support Claude, OpenAI, and other providers.
Conversation history is stored in a database and referenced via Arbor External
nodes, enabling branching and time-travel.

---

## Model

Each Cone has:
- A **config** (name, model_id, system_prompt)
- An **Arbor tree** — messages stored in the Cone DB, referenced as External
  nodes with handles
- A **head** — current position in the tree

Chat walks the Arbor path from root to head, resolves External handles to
messages, and builds the LLM request context.

---

## Hub methods

**Namespace:** `cone`

| Method | Params | Returns |
|--------|--------|---------|
| `create` | `name, model_id, system_prompt?, metadata?` | `CreateResult { cone_id, head }` |
| `get` | `identifier` | `GetResult { config }` — name or UUID |
| `list` | — | `ListResult { cones }` |
| `delete` | `identifier` | `DeleteResult` |
| `chat` | `identifier, prompt, ephemeral?` | `Stream<ChatEvent>` |
| `set_head` | `identifier, node_id` | `SetHeadResult` — branch / time-travel |
| `registry` | — | `RegistryResult` — available models and providers |

---

## Chat events

```rust
enum ChatEvent {
    Start,
    Content  { text: String },
    Complete { usage: ChatUsage },
    Error    { message: String },
}
```

`ephemeral=true` — don't advance head, mark nodes for deletion. Useful for
one-off queries without affecting the canonical conversation.

---

## Identifiers

`get`, `delete`, `chat`, `set_head` all accept a name or UUID. Partial name
matching is supported (e.g. `"assistant"` matches `"assistant#550e"`).

If a name collision occurs on `create`, a `#<uuid-fragment>` suffix is appended
automatically.

---

## Storage

SQLite (`cones.db`): `cones`, `messages`.
Arbor: conversation tree (one tree per Cone).

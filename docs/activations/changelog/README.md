# Changelog

Tracks Plexus RPC API changes by hash and enforces documentation. On startup,
substrate computes a hash of the current API schema and checks it against the
last recorded hash. If the hash changed and the change is undocumented, it logs
a warning.

Also maintains a queue for planned changes that haven't shipped yet.

---

## Hub methods

**Namespace:** `changelog`

### Entries

| Method | Params | Returns |
|--------|--------|---------|
| `add` | `hash, summary, previous_hash?, details?, author?, queue_id?` | `EntryAdded` |
| `list` | — | `Entries` — newest first |
| `get` | `hash` | `Status` — single entry |
| `check` | `current_hash` | `StartupCheck` — documented? |

### Queue (planned changes)

| Method | Params | Returns |
|--------|--------|---------|
| `queue_add` | `description, tags?` | `QueueAdded` |
| `queue_list` | `tag?` | `QueueEntries` |
| `queue_pending` | `tag?` | `QueueEntries` — pending only |
| `queue_get` | `id` | `QueueItem` |
| `queue_complete` | `id, hash` | `QueueUpdated` — links to implementation hash |

---

## Startup flow

```bash
# Check if current hash is documented
LANG=C.UTF-8 synapse substrate changelog check --current_hash <hash>

# Add an entry when you ship a breaking change
LANG=C.UTF-8 synapse substrate changelog add \
  --hash <new_hash> \
  --previous_hash <old_hash> \
  --summary "Add run_plan method to orcha"
```

If `check` returns `hash_changed=true, is_documented=false`, substrate logs
`UNDOCUMENTED PLEXUS CHANGE` but still starts.

---

## Storage

SQLite: `changelog_entries` (hash as PK), `hash_state` (last known hash),
`queue_entries`.

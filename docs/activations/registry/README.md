# Registry

Backend discovery and registration. Maintains a list of Plexus RPC backends
that can be queried, pinged for health checks, and loaded from a TOML config
file at startup.

Lives in the `plexus-registry` crate (not in `src/activations/`).

---

## Hub methods

**Namespace:** `registry`

| Method | Params | Returns |
|--------|--------|---------|
| `register` | `name, host, port, protocol?, description?, namespace?` | `BackendRegistered` |
| `list` | `active_only?` | `Backends { backends }` — defaults to active only |
| `get` | `name` | `Backend` or `Error` |
| `update` | `name, host?, port?, protocol?, description?, namespace?` | `BackendUpdated` |
| `delete` | `name` | `BackendDeleted` |
| `ping` | `name` | `Ping { success, message }` — updates `last_seen` |
| `reload` | — | `Reloaded { count }` — reloads from config file |

---

## Config file

Backends can be defined in `~/.config/plexus/backends.toml` and loaded at
startup (or on demand via `reload`). Existing entries are not overwritten.

---

## Backend sources

`Auto` (self-registered), `File` (from TOML), `Manual` (via RPC), `Env` (env vars).

---

## Storage

SQLite at `~/.config/plexus/registry.db`.

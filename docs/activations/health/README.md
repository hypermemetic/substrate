# Health

Reports server uptime and health status.

---

## Hub methods

**Namespace:** `health`

| Method | Params | Returns |
|--------|--------|---------|
| `check` | — | `Status { status: "healthy", uptime_seconds, timestamp }` |

---

## Notes

Tracks `start_time` at construction. `uptime_seconds` is elapsed time since
substrate started. Reference implementation of the manual RPC trait pattern
(without using the `#[hub_methods]` macro).

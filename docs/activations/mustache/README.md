# Mustache

Template rendering for activation output. Other activations register named
Mustache templates (keyed by `plugin_id + method + name`) and call `render`
to produce formatted output from structured values.

---

## Hub methods

**Namespace:** `mustache`

| Method | Params | Returns |
|--------|--------|---------|
| `render` | `plugin_id, method, template_name?, value` | `Rendered { output }` or `Error` |
| `register_template` | `plugin_id, method, name, template` | `Registered` or `Error` |
| `list_templates` | `plugin_id` | `Templates { templates }` |
| `get_template` | `plugin_id, method, name` | `Template { template }` or `NotFound` |
| `delete_template` | `plugin_id, method, name` | `Deleted { count }` |

`template_name` defaults to `"default"` if omitted.

---

## Notes

- Templates are validated via `mustache::compile_str()` before storing.
- `register_template` is idempotent (insert-or-update).
- Registrations keyed by `(plugin_id, method, name)` — unique per activation method.

---

## Storage

SQLite (`templates.db`): `templates(id, plugin_id, method, name, template, ...)`.

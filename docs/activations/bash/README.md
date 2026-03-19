# Bash

Executes shell commands and streams stdout, stderr, and exit code in real time.

---

## Hub methods

**Namespace:** `bash`

| Method | Params | Returns |
|--------|--------|---------|
| `execute` | `command: String` | `Stream<BashEvent>` |

---

## Event types

```rust
enum BashEvent {
    Stdout { line: String },
    Stderr { line: String },
    Exit   { code: i32 },
    Error  { message: String },
}
```

Stdout lines are yielded as they arrive. Stderr is buffered in a background task
(to prevent pipe deadlock when stdout is high-volume) and yielded after stdout
completes. Exit code follows.

---

## Notes

- Spawns `bash -c <command>`
- Buffers up to 100 stderr lines per process
- Exit code -1 if the process wait fails
- Non-blocking async I/O throughout — no synchronous blocking

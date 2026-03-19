# Echo

Minimal example activation. Echoes messages back with a configurable repeat
count. Reference implementation for the `#[hub_methods]` macro pattern.

---

## Hub methods

**Namespace:** `echo`

| Method | Params | Returns |
|--------|--------|---------|
| `echo` | `message: String, count: u32` | `Stream<EchoEvent>` — yields Echo for each repetition, 500ms delay |
| `once` | `message: String` | `Stream<EchoEvent>` — single Echo |
| `ping` | — | `Stream<EchoEvent>` — single Pong |

```rust
enum EchoEvent {
    Echo { message: String, count: u32 },
    Pong,
}
```

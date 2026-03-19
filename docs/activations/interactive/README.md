# Interactive

Demonstrates bidirectional communication — server-to-client prompts, selections,
and confirmations mid-stream. Not a production feature; shows the bidirectional
protocol pattern.

The bidirectional protocol is designed but not yet fully shipped. See
`plans/BIDIR/` for the implementation plan.

---

## Hub methods

**Namespace:** `interactive`

| Method | Params | Returns |
|--------|--------|---------|
| `wizard` | — | `Stream<WizardEvent>` — prompts name → selects template → confirms |
| `delete` | `paths: Vec<String>` | `Stream<DeleteEvent>` — confirms then deletes |
| `confirm` | `message: String` | `Stream<ConfirmEvent>` — yes/no |

All methods use `StandardBidirChannel` for interactive requests. On
non-bidirectional transports (e.g. HTTP), `BidirError::NotSupported` is returned
gracefully.

---

## Events

```rust
enum WizardEvent { Started, NameCollected { name }, TemplateSelected { template },
                   Created { name, template }, Cancelled, Error { message }, Done }
enum DeleteEvent  { Deleted { path }, Cancelled, Done }
enum ConfirmEvent { Confirmed, Declined, Error { message } }
```

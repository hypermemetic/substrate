# ClaudeCode Loopback

Permission gating for Claude Code tool calls. When a Claude session runs with
`loopback_enabled`, every tool use is intercepted and held until an external
caller approves or denies it.

Claude calls `permit` (via `--permission-prompt-tool mcp__plexus__loopback_permit`),
which blocks until a parent calls `respond`. The parent can poll `pending` or
use `wait_for_approval` to avoid polling.

---

## Hub methods

**Namespace:** `claudecode_loopback`

| Method | Params | Returns |
|--------|--------|---------|
| `permit` | `tool_name, tool_use_id, input` | `Stream<String>` — blocks until approved/denied, returns JSON for Claude |
| `respond` | `approval_id, approve: bool, message?` | `RespondResult` |
| `pending` | `session_id?` | `PendingResult { approvals }` |
| `wait_for_approval` | `session_id, timeout_secs?` | `WaitForApprovalResult` — blocks until new approval arrives |
| `configure` | `session_id` | `ConfigureResult { mcp_config_json }` |

---

## Flow

```
Claude wants to use a tool
  → calls loopback.permit (via MCP)
  → creates ApprovalRequest, blocks (polls every 1s, timeout 5min)

Parent calls loopback.pending → sees the request
Parent calls loopback.respond(approval_id, approve=true)
  → permit unblocks, returns {"behavior":"allow",...} to Claude
  → Claude executes the tool
```

`wait_for_approval` is more efficient than polling `pending` — it blocks
on a Tokio notification until a new approval arrives.

---

## Approval states

`Pending → Approved | Denied | TimedOut`

---

## Storage

SQLite (`loopback.db`): approval requests.
In-memory: per-session notifiers, session parent/child hierarchy maps.

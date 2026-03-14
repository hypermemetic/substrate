# Orcha Approval Workflow

## Overview

When Orcha runs a task, it spawns a Claude Code session with a `--permission-prompt-tool` hook. Every time Claude wants to execute a tool (Bash, Write, Read, etc.), it calls that hook instead of running directly. The hook routes the request to Orcha, which must either approve or deny it before Claude can proceed.

This gives the orchestration layer — and by extension a human operator or another Claude instance — full control over what the agent is allowed to do.

## Architecture

### Components

```
┌─────────────────────────────────────────────────────────────────┐
│  Orcha Session (orcha-<uuid>)                                   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  ClaudeCode Session (cc-uuid)                           │   │
│  │                                                         │   │
│  │  claude CLI                                             │   │
│  │    --permission-prompt-tool mcp__plexus__loopback_permit│   │
│  │    --mcp-config → loopback MCP server                   │   │
│  │                                                         │   │
│  │  On every tool use:                                     │   │
│  │    1. Claude calls loopback_permit via MCP              │   │
│  │    2. loopback_permit BLOCKS until resolved             │   │
│  │    3. Returns allow/deny JSON to Claude                 │   │
│  └──────────────────────┬──────────────────────────────────┘   │
│                         │ permit call                           │
│  ┌──────────────────────▼──────────────────────────────────┐   │
│  │  LoopbackStorage                                        │   │
│  │                                                         │   │
│  │  loopback_approvals table:                              │   │
│  │    id, session_id, tool_name, tool_use_id,              │   │
│  │    input, status (pending/approved/denied)              │   │
│  │                                                         │   │
│  │  session_parents: cc-uuid → orcha-uuid                  │   │
│  │  session_children: orcha-uuid → [cc-uuid]               │   │
│  │  session_notifiers: session → Arc<Notify>               │   │
│  └──────────────────────┬──────────────────────────────────┘   │
│                         │ notify (propagates to parent)         │
│  ┌──────────────────────▼──────────────────────────────────┐   │
│  │  Approval Handler                                       │   │
│  │  (tokio::spawn per ToolUse event)                       │   │
│  │                                                         │   │
│  │  Waits on Notify for cc-uuid, then:                     │   │
│  │  → Auto mode:   spawns Haiku decision agent             │   │
│  │  → Manual mode: returns immediately (human resolves)    │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### Session ID Topology

There are three distinct IDs in play:

| ID | Format | Purpose |
|----|--------|---------|
| `orcha_session_id` | `orcha-<uuid>` | Orcha session, top-level handle |
| `cc_session_id` | `<uuid>` | ClaudeCode session UUID, returned by `claudecode.create()` |
| `cc_session_name` | `orcha-<uuid>-cc` | Human-readable name for the ClaudeCode session |

The ClaudeCode session's `loopback_session_id` is set to `cc_session_id`. This is what appears in `loopback_approvals.session_id` for all approvals from that session.

The parent relationship (`register_session_parent(cc_session_id, orcha_session_id)`) lets the Orcha session be notified when child approvals arrive, and lets `list_pending(orcha_session_id)` return child approvals via the inverse `session_children` map.

---

## Flow 1: Manual Approval (Default)

`auto_approve = false` (the default). Orcha blocks Claude's tool call and waits for an external actor to resolve it.

```
Orcha                  LoopbackStorage         Approver (human / Claude)
  │                          │                          │
  │── run_task_async ────────►│                          │
  │                          │                          │
  │  [Claude starts, hits Write tool]                   │
  │                          │                          │
  │  loopback_permit called──►│                          │
  │  (Claude is blocked)     │◄── wait_for_approval ───│
  │                          │    (session: orcha-xxx)  │
  │                          │                          │
  │  handle_tool_approval:   │                          │
  │  notifier.notified() ◄───│ notify_session fires     │
  │  auto_approve=false      │ (propagates to parent)   │
  │  → returns immediately   │                          │
  │                          │                          │
  │                          │── list_pending ──────────►│
  │                          │   (session: orcha-xxx)   │
  │                          │   returns cc-uuid approvals│
  │                          │                          │
  │                          │◄── resolve_approval ─────│
  │                          │    (approved=true)       │
  │                          │                          │
  │  loopback_permit unblocks│                          │
  │  returns allow to Claude │                          │
  │                          │                          │
  │  Claude executes tool ───►│                          │
```

**Synapse commands:**

```bash
# 1. Start task
synapse substrate orcha run_task_async \
  --request.task "Create /tmp/foo.txt with: hello" \
  --request.model "claude-sonnet-4-6"
# → session_id: orcha-<uuid>

# 2. Block until approval request arrives
synapse substrate loopback wait_for_approval \
  --session-id "orcha-<uuid>" \
  --timeout-secs 300
# → returns: approval id, tool_name, input

# 3. Approve
synapse substrate orcha approve_request \
  --request.approval-id "<approval-uuid>" \
  --request.message "looks good"
```

---

## Flow 2: Auto-Approval (Decision Agent)

`auto_approve = true`. Orcha spawns an ephemeral Haiku session to evaluate each tool request and resolve it autonomously.

```
Orcha                  LoopbackStorage         Haiku Decision Agent
  │                          │                          │
  │  [Claude hits tool]      │                          │
  │                          │                          │
  │  loopback_permit called──►│                          │
  │  (Claude is blocked)     │                          │
  │                          │                          │
  │  handle_tool_approval:   │                          │
  │  wait notifier ◄─────────│ notify fires             │
  │  auto_approve=true       │                          │
  │  create haiku session ───────────────────────────── ►│
  │                          │                          │
  │  prompt: "APPROVE or DENY│                          │
  │   Tool: Write            │                          │
  │   Input: {file_path:...} │                          │
  │   Task context: ..."     │                          │
  │                          │◄── "APPROVE" ────────────│
  │                          │                          │
  │  resolve_approval(true) ─►│                          │
  │                          │                          │
  │  loopback_permit unblocks│                          │
  │  Claude executes tool    │                          │
```

**Guidelines applied by decision agent:**
- APPROVE: Write, Read, Edit, Bash for build/test/standard dev operations
- DENY: access to sensitive files, dangerous commands, system file modification

---

## Arbor Integration

Every event in the orchestration is written to an Arbor tree for inspection and debugging. The Orcha session tree captures high-level events; the ClaudeCode session tree captures the full Claude conversation including tool uses and launch commands.

```
orcha tree (orcha-<uuid>):
  session_started
  └─ claude_session_created: <cc-uuid>
     └─ prompt_created (retry 0)
        └─ claude_session_complete: <cc-uuid>
           └─ session_complete: success_no_validation

cc tree (<cc-uuid>):
  user_message
  └─ assistant_start
     ├─ content_text: "..."
     ├─ content_tool_use: {name: "Write", input: {...}}
     └─ assistant_complete
  launch_command: "claude --permission-prompt-tool ..."
  (stderr lines if any)
```

---

## Notification Propagation

The `LoopbackStorage` notifier system uses `tokio::sync::Notify` for zero-overhead wakeup:

- Each session has one `Arc<Notify>` created lazily by `get_or_create_notifier(session_id)`
- When a permit call creates an approval, `notify_session(cc_session_id)` fires
- That call also checks `session_parents[cc_session_id]` and fires the parent notifier
- `wait_for_approval(orcha_session_id)` therefore wakes on child approvals
- `list_pending(orcha_session_id)` checks `session_children[orcha_session_id]` to include child records in the query

This means a single `wait_for_approval` call on the top-level Orcha session ID is sufficient to observe all tool requests from all child sessions — including future multi-agent scenarios where multiple ClaudeCode sessions run in parallel under one Orcha session.

---

## Deadline / Timeout

The approval handler waits up to **1 hour** for an approval to arrive via the notifier. If no approval arrives (e.g. approver never responds), it logs a warning and returns, leaving the approval record in `pending` state. Claude's `loopback_permit` call continues blocking independently (it has its own timeout on the permit side).

To deny a stuck approval:

```bash
synapse substrate loopback respond \
  --approval-id "<uuid>" \
  --approved false \
  --message "timed out"
```

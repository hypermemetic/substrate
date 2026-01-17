# LOOPBACK-1: MCP Loopback for Claude Code Tool Forwarding

## Overview

Implement a loopback approval system that routes nested Claude Code CLI tool permissions through Plexus MCP to the parent Claude session. This enables hierarchical agent supervision where a parent session can approve/deny tool use by child sessions.

Based on the HumanLayer pattern which uses:
- `--permission-prompt-tool` CLI flag to route all tool permissions through an MCP tool
- A blocking MCP handler that polls until approval resolves
- Returns `{behavior: "allow/deny"}` to Claude Code CLI

Our implementation routes approvals to the parent Claude session instead of a human UI.

## Architecture

```
Nested Claude Code CLI
    │ --permission-prompt-tool mcp__plexus__loopback.permit
    ▼
Plexus MCP Server (http://127.0.0.1:4445/mcp)
    │ loopback.permit blocks, polls queue
    ▼
Approval Queue in Plexus (SQLite)
    ▲
    │ loopback.respond(request_id, allow/deny)
    │
Parent Claude session (receives ToolRequest event)
```

### Flow

1. Parent creates claudecode session with `loopback_enabled: true`
2. Child CLI launches with `--permission-prompt-tool mcp__plexus__loopback.permit`
3. When child wants to use a tool, Claude Code calls `loopback.permit`
4. `loopback.permit` creates queue entry, emits event to parent, then **blocks polling**
5. Parent receives ToolRequest event, decides to allow/deny
6. Parent calls `loopback.respond(request_id, "allow")`
7. `loopback.permit` sees resolution, returns `{behavior: "allow"}` to CLI
8. Child CLI proceeds (or aborts if denied)

### Key Insight: Blocking Handler

The critical piece is that `loopback.permit` does **not** return until approval resolves. This is how HumanLayer's `hlyr` tool works - it blocks the MCP call while polling an external queue. The Claude Code CLI waits for the MCP response before proceeding.

## Tickets

| Ticket | Description | Blocked By | Unlocks |
|--------|-------------|------------|---------|
| LOOPBACK-2 | Types and Events | - | LOOPBACK-3, LOOPBACK-4, LOOPBACK-5 |
| LOOPBACK-3 | Approval Queue Storage | LOOPBACK-2 | LOOPBACK-4, LOOPBACK-5, LOOPBACK-6 |
| LOOPBACK-4 | loopback.permit MCP tool | LOOPBACK-2, LOOPBACK-3 | LOOPBACK-7 |
| LOOPBACK-5 | loopback.respond method | LOOPBACK-2, LOOPBACK-3 | LOOPBACK-7 |
| LOOPBACK-6 | loopback.pending method | LOOPBACK-3 | LOOPBACK-7 |
| LOOPBACK-7 | claudecode integration | LOOPBACK-4, LOOPBACK-5, LOOPBACK-6 | LOOPBACK-8 |
| LOOPBACK-8 | builder.rs integration | LOOPBACK-7 | - |

## Dependency DAG

```
        LOOPBACK-2 (types/events)
              │
    ┌─────────┴─────────┐
    ▼                   ▼
LOOPBACK-3          (unblocks)
(queue storage)         │
    │                   │
    ├───────┬───────────┤
    ▼       ▼           ▼
LOOPBACK-4 LOOPBACK-5 LOOPBACK-6
(permit)   (respond)   (pending)
    │         │           │
    └─────────┴─────┬─────┘
                    ▼
              LOOPBACK-7
           (claudecode integration)
                    │
                    ▼
              LOOPBACK-8
            (builder.rs)
```

## Critical Path

`LOOPBACK-2 → LOOPBACK-3 → LOOPBACK-4 → LOOPBACK-7 → LOOPBACK-8`

The parallel work (LOOPBACK-5, LOOPBACK-6) can happen alongside LOOPBACK-4 once LOOPBACK-3 completes.

---

## LOOPBACK-2: Types and Events

**blocked_by:** []
**unlocks:** [LOOPBACK-3, LOOPBACK-4, LOOPBACK-5]

### Scope

Define core types for the loopback approval system.

### Types

```rust
/// Request from nested CLI for tool permission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitRequest {
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,  // Claude's tool_use block ID
}

/// Response format expected by Claude Code CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitResponse {
    pub behavior: PermitBehavior,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermitBehavior {
    Allow,
    Deny,
}

/// Status of an approval in the queue
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalStatus {
    Pending,
    Allowed,
    Denied,
}

/// Stored approval record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub id: Uuid,
    pub session_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,
    pub status: ApprovalStatus,
    pub response_message: Option<String>,
    pub updated_input: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}
```

### Event

```rust
/// Event emitted to parent session when tool approval is requested
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequestEvent {
    pub approval_id: Uuid,
    pub session_name: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,
}
```

### Acceptance Criteria

- [ ] Types compile and derive necessary traits
- [ ] PermitResponse serializes to format Claude Code expects
- [ ] Event can be emitted through existing event system

---

## LOOPBACK-3: Approval Queue Storage

**blocked_by:** [LOOPBACK-2]
**unlocks:** [LOOPBACK-4, LOOPBACK-5, LOOPBACK-6]

### Scope

SQLite-backed queue for pending approvals with poll-friendly access.

### Schema

```sql
CREATE TABLE IF NOT EXISTS loopback_approvals (
    id TEXT PRIMARY KEY,           -- UUID
    session_id TEXT NOT NULL,      -- claudecode session name
    tool_name TEXT NOT NULL,
    tool_input TEXT NOT NULL,      -- JSON
    tool_use_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending/allowed/denied
    response_message TEXT,
    updated_input TEXT,            -- JSON, optional modified input
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_loopback_session_status
    ON loopback_approvals(session_id, status);
```

### Methods

```rust
impl ApprovalQueue {
    /// Create new pending approval, returns ID
    pub async fn create_approval(&self, session_id: &str, request: PermitRequest) -> Result<Uuid>;

    /// Get approval by ID (for polling)
    pub async fn get_approval(&self, id: Uuid) -> Result<Option<ApprovalRecord>>;

    /// Resolve approval (allow/deny)
    pub async fn resolve_approval(
        &self,
        id: Uuid,
        status: ApprovalStatus,
        message: Option<String>,
        updated_input: Option<serde_json::Value>,
    ) -> Result<()>;

    /// List pending approvals for session
    pub async fn list_pending(&self, session_id: &str) -> Result<Vec<ApprovalRecord>>;

    /// Cleanup old resolved approvals (background task)
    pub async fn cleanup_old(&self, older_than: Duration) -> Result<u64>;
}
```

### Acceptance Criteria

- [ ] Table created on plugin init
- [ ] create_approval returns UUID immediately
- [ ] get_approval returns current status (for polling)
- [ ] resolve_approval updates status atomically
- [ ] list_pending filters by session

---

## LOOPBACK-4: loopback.permit MCP Tool

**blocked_by:** [LOOPBACK-2, LOOPBACK-3]
**unlocks:** [LOOPBACK-7]

### Scope

The blocking MCP tool that Claude Code calls for permission. This is the critical piece.

### Behavior

1. Receive PermitRequest from Claude Code CLI
2. Create queue entry with `create_approval`
3. Emit ToolRequestEvent to parent session
4. **Poll queue** until status != Pending (with backoff)
5. Return PermitResponse based on resolution

### Implementation Notes

```rust
#[plexus::method]
async fn permit(
    &self,
    ctx: &HubContext,
    session_id: String,  // Which claudecode session
    request: PermitRequest,
) -> Result<PermitResponse> {
    // 1. Create approval record
    let approval_id = self.queue.create_approval(&session_id, request.clone()).await?;

    // 2. Emit event to parent
    ctx.emit(ToolRequestEvent {
        approval_id,
        session_name: session_id.clone(),
        tool_name: request.tool_name.clone(),
        tool_input: request.tool_input.clone(),
        tool_use_id: request.tool_use_id.clone(),
    }).await?;

    // 3. Poll until resolved (this is the blocking part!)
    let poll_interval = Duration::from_millis(500);
    let timeout = Duration::from_secs(300);  // 5 min timeout
    let start = Instant::now();

    loop {
        if start.elapsed() > timeout {
            // Auto-deny on timeout
            self.queue.resolve_approval(
                approval_id,
                ApprovalStatus::Denied,
                Some("Approval timed out".into()),
                None,
            ).await?;
        }

        let record = self.queue.get_approval(approval_id).await?
            .ok_or_else(|| anyhow!("Approval record disappeared"))?;

        match record.status {
            ApprovalStatus::Pending => {
                tokio::time::sleep(poll_interval).await;
                continue;
            }
            ApprovalStatus::Allowed => {
                return Ok(PermitResponse {
                    behavior: PermitBehavior::Allow,
                    updated_input: record.updated_input,
                    message: record.response_message,
                });
            }
            ApprovalStatus::Denied => {
                return Ok(PermitResponse {
                    behavior: PermitBehavior::Deny,
                    updated_input: None,
                    message: record.response_message,
                });
            }
        }
    }
}
```

### Key Design Decision

The poll loop runs **inside** the MCP handler. The handler does not return until approval resolves. This is intentional - Claude Code CLI is waiting for the response.

### Acceptance Criteria

- [ ] Handler blocks until approval resolves
- [ ] Emits event on creation
- [ ] Returns correct PermitResponse format
- [ ] Times out after configurable period
- [ ] Handles queue errors gracefully

---

## LOOPBACK-5: loopback.respond Method

**blocked_by:** [LOOPBACK-2, LOOPBACK-3]
**unlocks:** [LOOPBACK-7]

### Scope

Method for parent session to resolve pending approvals.

### Interface

```rust
#[plexus::method]
async fn respond(
    &self,
    approval_id: Uuid,
    allow: bool,
    message: Option<String>,
    updated_input: Option<serde_json::Value>,
) -> Result<()> {
    let status = if allow {
        ApprovalStatus::Allowed
    } else {
        ApprovalStatus::Denied
    };

    self.queue.resolve_approval(approval_id, status, message, updated_input).await
}
```

### Usage from Parent

When parent receives ToolRequestEvent:

```
// Allow the tool
loopback.respond(approval_id, true, null, null)

// Deny with message
loopback.respond(approval_id, false, "Tool not allowed in this context", null)

// Allow with modified input
loopback.respond(approval_id, true, null, {"path": "/safe/path/only"})
```

### Acceptance Criteria

- [ ] Updates queue status
- [ ] Unblocks waiting permit handler
- [ ] Handles already-resolved approvals gracefully
- [ ] Returns error for unknown approval_id

---

## LOOPBACK-6: loopback.pending Method

**blocked_by:** [LOOPBACK-3]
**unlocks:** [LOOPBACK-7]

### Scope

Method to list pending approvals for a session.

### Interface

```rust
#[plexus::method]
async fn pending(
    &self,
    session_id: Option<String>,  // None = all sessions
) -> Result<Vec<ApprovalRecord>> {
    match session_id {
        Some(id) => self.queue.list_pending(&id).await,
        None => self.queue.list_all_pending().await,
    }
}
```

### Acceptance Criteria

- [ ] Returns pending approvals
- [ ] Filters by session when provided
- [ ] Returns empty vec, not error, when none pending

---

## LOOPBACK-7: claudecode Integration

**blocked_by:** [LOOPBACK-4, LOOPBACK-5, LOOPBACK-6]
**unlocks:** [LOOPBACK-8]

### Scope

Integrate loopback into claudecode plugin launch flow.

### Changes to LaunchConfig

```rust
pub struct LaunchConfig {
    // ... existing fields ...

    /// Enable loopback approval for tool permissions
    pub loopback_enabled: bool,

    /// Custom timeout for approval polling (default 300s)
    pub loopback_timeout_secs: Option<u64>,
}
```

### MCP Config Generation

When `loopback_enabled`, generate MCP config:

```json
{
  "mcpServers": {
    "plexus": {
      "url": "http://127.0.0.1:4445/mcp"
    }
  }
}
```

### CLI Arguments

When `loopback_enabled`, add to spawn:

```
--permission-prompt-tool mcp__plexus__loopback.permit
--mcp-config /tmp/plexus-mcp-{session_id}.json
```

### Acceptance Criteria

- [ ] LaunchConfig accepts loopback_enabled
- [ ] MCP config file generated correctly
- [ ] CLI spawned with permission-prompt-tool flag
- [ ] Session ID passed correctly to permit calls
- [ ] Config file cleaned up on session end

---

## LOOPBACK-8: builder.rs Integration

**blocked_by:** [LOOPBACK-7]
**unlocks:** []

### Scope

Register the loopback activation in the Plexus builder.

### Changes

```rust
// In builder.rs or wherever activations are registered

impl PlexusBuilder {
    pub fn with_claudecode_loopback(mut self) -> Self {
        self.activations.push(ClaudeCodeLoopbackActivation::new());
        self
    }
}
```

### Plugin Structure

```rust
pub struct ClaudeCodeLoopbackPlugin {
    queue: ApprovalQueue,
}

impl ClaudeCodeLoopbackPlugin {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            queue: ApprovalQueue::new(db),
        }
    }
}
```

### Acceptance Criteria

- [ ] Plugin registered in default builder
- [ ] Queue initialized with shared database
- [ ] Methods exposed via MCP

---

## Implementation Notes

### Why Polling Instead of Channels?

The permit handler runs in an MCP request context. Using channels would require:
1. Creating a oneshot channel per request
2. Storing it somewhere the respond method can access
3. Managing channel lifetime

Polling the database is simpler and naturally persists across restarts. The 500ms poll interval is acceptable latency for human-in-the-loop approvals.

### Future Optimization

Could add a notify mechanism (condvar or broadcast channel) to wake pollers immediately when respond is called, reducing latency while keeping database as source of truth.

### Security Considerations

- Session isolation: Only parent session should be able to respond to its children's approvals
- Consider adding session ownership verification in respond method
- Approval timeout prevents indefinite blocking

## References

- HumanLayer implementation: Uses same pattern with `hlyr` MCP tool
- Claude Code `--permission-prompt-tool` documentation
- Existing claudecode plugin in `crates/plexus-claudecode/`

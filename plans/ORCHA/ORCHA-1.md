# ORCHA-1: Approval Interface & Auto-Approval Configuration

## Overview

Orcha currently hardcodes automatic approval via ephemeral Haiku sessions. While this enables autonomous operation, it lacks:
1. **User control** - No way to disable auto-approval for sensitive tasks
2. **Visibility** - Approval happens internally via loopback, not exposed as Orcha feature
3. **Flexibility** - Can't switch between autonomous and interactive modes
4. **Testing** - Difficult to test approval flows without auto-approval interference

This plan adds explicit approval controls to Orcha's API while maintaining the safe default of Haiku-based auto-approval.

## Problem Statement

**Current Architecture:**
```
User → orcha.run_task → orchestrator
                            ↓
                    (creates ClaudeCode session with loopback=true)
                            ↓
                    Agent requests tool → loopback.permit() blocks
                            ↓
                    orchestrator spawns Haiku → decides APPROVE/DENY
                            ↓
                    loopback.storage().resolve_approval()
                            ↓
                    loopback.permit() unblocks → tool executes
```

**Issues:**
- Auto-approval is always on - no opt-out
- To manually approve, must use `loopback.respond` directly (couples to loopback implementation)
- No way to list Orcha-specific pending approvals
- Approval is hidden implementation detail, not first-class feature

## Goals

1. **Expose approval as first-class Orcha API** - Add `orcha.list_pending_approvals`, `orcha.approve_request`, `orcha.deny_request`
2. **Make auto-approval configurable** - Add `auto_approve` flag to `RunTaskRequest` (default: true)
3. **Maintain safe defaults** - Auto-approval via Haiku remains default behavior
4. **Preserve loopback abstraction** - Orcha methods wrap loopback, don't leak implementation
5. **Enable manual approval workflows** - Allow users to explicitly approve/deny during task execution

## Non-Goals

- Changing loopback's internal implementation
- Adding UI for approval management (TUI/web)
- Multi-user approval workflows
- Approval policies/rules engine

---

## Tickets

### ORCHA-1.1: Add `auto_approve` Configuration Field

**Priority:** High
**Estimate:** 1 hour
**Dependencies:** None

**Description:**

Add `auto_approve` field to `RunTaskRequest` to control whether Haiku automatically judges approvals.

**Changes:**

1. Update `RunTaskRequest` in `src/activations/orcha/types.rs`:
```rust
pub struct RunTaskRequest {
    pub task: String,
    pub model: String,
    pub rules: Option<String>,
    pub max_retries: Option<u32>,

    /// Enable automatic approval via Haiku decision agent
    ///
    /// When true (default), spawns ephemeral Haiku session to judge each approval.
    /// When false, approvals must be handled manually via orcha.approve_request.
    #[serde(default = "default_auto_approve")]
    pub auto_approve: Option<bool>,
}

fn default_auto_approve() -> Option<bool> {
    Some(true)
}
```

2. Thread `auto_approve` through orchestrator call chain:
   - `run_orchestration_task` → store in `OrchaSession` or pass to `handle_tool_approval`
   - `handle_tool_approval` → add `auto_approve: bool` parameter
   - Conditional logic: if `auto_approve`, run current Haiku logic; else, skip and wait

**Testing:**
- Run task with `--request.auto_approve true` → should auto-approve (current behavior)
- Run task with `--request.auto_approve false` → should block until manual approval
- Run task with field omitted → should default to `true`

**Success Criteria:**
- Can disable auto-approval via API
- Default behavior unchanged (auto-approval works)
- No breaking changes to existing callers

---

### ORCHA-1.2: Add `list_pending_approvals` Method

**Priority:** High
**Estimate:** 1 hour
**Dependencies:** None

**Description:**

Expose pending approval requests for an Orcha session via dedicated API method.

**Changes:**

1. Add request/response types in `src/activations/orcha/types.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListApprovalsRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalInfo {
    pub approval_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub tool_input: serde_json::Value,
    pub created_at: String, // ISO 8601 timestamp
}
```

2. Add method in `src/activations/orcha/activation.rs`:
```rust
#[hub_methods]
impl<P: HubContext> Orcha<P> {
    /// List pending approval requests for a session
    ///
    /// Returns all approval requests awaiting manual approval.
    /// Only relevant when auto_approve is disabled.
    #[plexus_macros::hub_method]
    async fn list_pending_approvals(
        &self,
        request: ListApprovalsRequest,
    ) -> impl Stream<Item = ApprovalInfo> + Send + 'static {
        let loopback = self.loopback.clone();
        let session_id = request.session_id;

        stream! {
            let approvals = loopback.storage()
                .get_pending_approvals(&session_id)
                .await;

            for approval in approvals {
                yield ApprovalInfo {
                    approval_id: approval.id,
                    session_id: approval.session_id,
                    tool_name: approval.tool_name,
                    tool_use_id: approval.tool_use_id,
                    tool_input: approval.tool_input,
                    created_at: approval.created_at.to_rfc3339(),
                };
            }
        }
    }
}
```

**Testing:**
- Start task with `auto_approve = false`
- Wait for approval request
- Call `orcha.list_pending_approvals` → should see pending approval
- Approve it via loopback
- Call `orcha.list_pending_approvals` again → should be empty

**Success Criteria:**
- Can query pending approvals via Orcha API
- Returns all relevant fields for decision-making
- Works independently of loopback.pending

---

### ORCHA-1.3: Add `approve_request` Method

**Priority:** High
**Estimate:** 1 hour
**Dependencies:** ORCHA-1.2

**Description:**

Add explicit approval method to Orcha API.

**Changes:**

1. Add request/response types in `src/activations/orcha/types.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveRequest {
    pub approval_id: String,

    /// Optional message explaining approval decision
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResult {
    pub success: bool,
    pub approval_id: String,
    pub message: Option<String>,
}
```

2. Add method in `src/activations/orcha/activation.rs`:
```rust
/// Approve a pending request
///
/// Approves a tool use request and unblocks the waiting agent.
/// The approval_id comes from list_pending_approvals.
#[plexus_macros::hub_method]
async fn approve_request(
    &self,
    request: ApproveRequest,
) -> impl Stream<Item = ApprovalResult> + Send + 'static {
    let loopback = self.loopback.clone();
    let approval_id = request.approval_id;
    let message = request.message;

    stream! {
        match loopback.storage()
            .resolve_approval(&approval_id, true, message.clone())
            .await
        {
            Ok(_) => {
                yield ApprovalResult {
                    success: true,
                    approval_id: approval_id.clone(),
                    message: Some("Approved".to_string()),
                };
            }
            Err(e) => {
                yield ApprovalResult {
                    success: false,
                    approval_id: approval_id.clone(),
                    message: Some(format!("Failed to approve: {}", e)),
                };
            }
        }
    }
}
```

**Testing:**
- Start task with `auto_approve = false`
- Wait for tool request → verify agent blocks
- Get approval_id from `list_pending_approvals`
- Call `approve_request` with that ID
- Verify agent unblocks and tool executes

**Success Criteria:**
- Can manually approve via Orcha API
- Agent unblocks upon approval
- Approval is recorded in loopback storage

---

### ORCHA-1.4: Add `deny_request` Method

**Priority:** Medium
**Estimate:** 30 minutes
**Dependencies:** ORCHA-1.3

**Description:**

Add explicit denial method (mirror of approve_request).

**Changes:**

1. Add request type in `src/activations/orcha/types.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenyRequest {
    pub approval_id: String,

    /// Reason for denial (shown to agent)
    pub reason: Option<String>,
}
```

2. Add method in `src/activations/orcha/activation.rs`:
```rust
/// Deny a pending request
///
/// Denies a tool use request. The agent will receive an error
/// and may adapt or fail depending on its error handling.
#[plexus_macros::hub_method]
async fn deny_request(
    &self,
    request: DenyRequest,
) -> impl Stream<Item = ApprovalResult> + Send + 'static {
    let loopback = self.loopback.clone();
    let approval_id = request.approval_id;
    let reason = request.reason;

    stream! {
        match loopback.storage()
            .resolve_approval(&approval_id, false, reason.clone())
            .await
        {
            Ok(_) => {
                yield ApprovalResult {
                    success: true,
                    approval_id: approval_id.clone(),
                    message: reason.or(Some("Denied".to_string())),
                };
            }
            Err(e) => {
                yield ApprovalResult {
                    success: false,
                    approval_id: approval_id.clone(),
                    message: Some(format!("Failed to deny: {}", e)),
                };
            }
        }
    }
}
```

**Testing:**
- Start task with `auto_approve = false`
- Wait for tool request
- Call `deny_request` with approval_id
- Verify agent receives error/denial
- Check agent continues or fails appropriately

**Success Criteria:**
- Can manually deny via Orcha API
- Agent receives clear denial message
- Denial is recorded in loopback storage

---

### ORCHA-1.5: Update Orchestrator Auto-Approval Logic

**Priority:** High
**Estimate:** 2 hours
**Dependencies:** ORCHA-1.1

**Description:**

Modify `handle_tool_approval` to conditionally spawn Haiku based on `auto_approve` setting.

**Changes:**

1. Pass `auto_approve` through call chain in `src/activations/orcha/orchestrator.rs`:
   - `run_orchestration_task` receives it from `RunTaskRequest`
   - Store in local state or pass to `handle_tool_approval`
   - `spawn_tool_approval_handler` gets `auto_approve` parameter

2. Update `handle_tool_approval` signature:
```rust
async fn handle_tool_approval<P: HubContext>(
    loopback: Arc<ClaudeCodeLoopback>,
    claudecode: Arc<ClaudeCode<P>>,
    orcha_session_id: String,
    tool_name: String,
    tool_use_id: String,
    tool_input: serde_json::Value,
    task_context: String,
    auto_approve: bool, // NEW
) {
    // Wait for approval to be created by loopback.permit()
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let approvals = loopback.storage().get_pending_approvals(&orcha_session_id).await;
    let approval_id = match approvals.iter().find(|a| a.tool_use_id == tool_use_id) {
        Some(a) => a.id.clone(),
        None => {
            tracing::warn!("No approval found for tool_use_id: {}", tool_use_id);
            return;
        }
    };

    if !auto_approve {
        // Manual mode: do nothing, wait for external approval
        tracing::info!(
            "Auto-approval disabled for session {}. Waiting for manual approval of tool: {}",
            orcha_session_id,
            tool_name
        );
        return;
    }

    // Auto-approval mode: spawn Haiku decision agent (existing logic)
    // ... rest of current implementation ...
}
```

**Testing:**
- Task with `auto_approve = true` → Haiku judges (existing behavior)
- Task with `auto_approve = false` → approval blocks, manual approval works
- Verify no Haiku session spawned when `auto_approve = false`

**Success Criteria:**
- Auto-approval can be disabled
- Manual approval workflow functional
- No regression in auto-approval behavior

---

### ORCHA-1.6: Add Usage Examples to Documentation

**Priority:** Low
**Estimate:** 30 minutes
**Dependencies:** ORCHA-1.1, ORCHA-1.2, ORCHA-1.3, ORCHA-1.4

**Description:**

Document new approval features with practical examples.

**Changes:**

Create `/workspace/hypermemetic/plexus-substrate/src/activations/orcha/APPROVAL.md`:

```markdown
# Orcha Approval Workflows

## Overview

Orcha supports two approval modes:

1. **Auto-Approval (Default)** - Haiku judges each tool request autonomously
2. **Manual Approval** - Human approves/denies each request explicitly

## Auto-Approval Mode

Default behavior - safe for most tasks:

```bash
synapse substrate orcha run_task \
  --request.task "Refactor the cache module" \
  --request.model sonnet
  # auto_approve defaults to true
```

Haiku evaluates each tool request based on:
- Relevance to task
- Safety (no destructive operations)
- Context appropriateness

## Manual Approval Mode

For sensitive tasks or explicit control:

```bash
# Terminal 1: Start task with manual approval
synapse substrate orcha run_task \
  --request.task "Delete old database files" \
  --request.model sonnet \
  --request.auto_approve false
# Returns: session_id: orcha-abc-123

# Terminal 2: Monitor and approve
synapse substrate orcha list_pending_approvals \
  --request.session_id "orcha-abc-123"
# Shows: approval_id, tool_name, tool_input

synapse substrate orcha approve_request \
  --request.approval_id "approval-xyz-789" \
  --request.message "Confirmed: safe to delete logs"

# Or deny:
synapse substrate orcha deny_request \
  --request.approval_id "approval-xyz-789" \
  --request.reason "Too broad - specify exact files"
```

## Use Cases

**Auto-Approval (Recommended):**
- Development tasks (code review, refactoring)
- Analysis tasks (reading files, searching)
- Standard operations (build, test, lint)

**Manual Approval (When Needed):**
- Production database operations
- File deletion tasks
- External API calls
- Security-sensitive operations
```

**Success Criteria:**
- Documentation is clear and actionable
- Examples work as written
- Covers both approval modes

---

### ORCHA-1.7: Add Integration Tests

**Priority:** Medium
**Estimate:** 2 hours
**Dependencies:** All above tickets

**Description:**

Add comprehensive tests for approval workflows.

**Test Cases:**

1. **Auto-approval enabled (default)**
   - Task with Write tool → auto-approved by Haiku
   - Verify tool executes without blocking
   - Check Haiku decision recorded

2. **Auto-approval disabled**
   - Task with Write tool → blocks on approval
   - Manual approval via `orcha.approve_request` → unblocks
   - Verify tool executes after approval

3. **Manual denial**
   - Task with Bash tool → blocks on approval
   - Manual denial via `orcha.deny_request` → unblocks with error
   - Verify agent receives denial message

4. **List pending approvals**
   - Multiple blocked approvals
   - Verify all returned by `list_pending_approvals`
   - Approve one → verify others still pending

5. **Edge cases**
   - Invalid approval_id → error
   - Approve already-resolved → idempotent or error?
   - Session cleanup with pending approvals

**Location:**
`src/activations/orcha/tests.rs` (expand existing test module)

**Success Criteria:**
- All tests pass
- Coverage for both approval modes
- Edge cases handled gracefully

---

## Implementation Order

1. **ORCHA-1.1** - Add configuration (enables feature flag)
2. **ORCHA-1.5** - Update orchestrator (implements conditional logic)
3. **ORCHA-1.2** - List approvals (visibility)
4. **ORCHA-1.3** - Approve method (manual workflow)
5. **ORCHA-1.4** - Deny method (complete API)
6. **ORCHA-1.7** - Integration tests (validation)
7. **ORCHA-1.6** - Documentation (communication)

## Success Metrics

- **Backward compatibility:** Existing `run_task` calls work unchanged
- **Default behavior:** Auto-approval remains default (safe and autonomous)
- **Manual control:** Can disable auto-approval and manually manage approvals
- **API clarity:** Approval is first-class Orcha feature, not hidden implementation
- **Testing:** Integration tests cover both approval modes

## Future Enhancements (Out of Scope)

- **Approval policies** - Rules engine for auto-deny patterns
- **Multi-user approval** - Require N approvers for sensitive operations
- **Approval audit log** - Historical tracking of all approval decisions
- **Approval UI** - Web/TUI interface for approval management
- **Timeout configuration** - Auto-deny after N seconds
- **Selective auto-approval** - Auto-approve Read/Grep, manual for Write/Bash

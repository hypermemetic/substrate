# Loopback Blocking Approval API

## Problem

Previously, Claude Code had to poll for approvals in a loop:

```bash
# Old approach: Poll every second
while true; do
    result=$(synapse substrate loopback pending --session-id "session-123")
    if [ -n "$result" ]; then
        break
    fi
    sleep 1
done
```

This added latency (average 500ms) and wasted resources.

## Solution: `loopback.wait_for_approval`

The new `wait_for_approval` method **blocks until an approval arrives**, eliminating polling:

```bash
# New approach: Block until approval arrives
synapse substrate loopback wait_for_approval \
  --request.session_id "session-123" \
  --request.timeout_secs 300
```

**Key Benefits:**
- **No polling** - Command blocks until approval arrives or timeout
- **Instant response** - Returns immediately when approval created (< 1ms latency)
- **Efficient** - Uses `tokio::sync::Notify` for event-driven wake-up
- **Timeout support** - Configurable timeout prevents indefinite waits

## API Specification

### Method: `loopback.wait_for_approval`

**Parameters:**
- `session_id` (required): Session ID to wait for approvals
- `timeout_secs` (optional): Timeout in seconds (default: 300 = 5 minutes)

**Returns:**
```typescript
type WaitForApprovalResult =
  | { type: "ok", approvals: ApprovalRequest[] }
  | { type: "timeout", message: string }
  | { type: "error", message: string }
```

**Behavior:**
1. Checks if approvals already exist → returns immediately
2. If none, blocks until new approval arrives
3. Uses `tokio::sync::Notify` for efficient event-driven wake-up
4. Returns on first new approval or timeout

## Usage Examples

### Basic Usage

```bash
# Wait for approval with default 5-minute timeout
synapse substrate loopback wait_for_approval \
  --request.session_id "orcha-abc-123"
```

### Custom Timeout

```bash
# Wait for up to 10 minutes
synapse substrate loopback wait_for_approval \
  --request.session_id "orcha-abc-123" \
  --request.timeout_secs 600
```

### Integration with Orcha Approval Loop

For Claude Code to handle Orcha approvals:

```bash
#!/usr/bin/env bash
# Wait for approval, respond to it

SESSION_ID="orcha-abc-123"

# Block until approval arrives
RESULT=$(synapse substrate loopback wait_for_approval \
  --request.session_id "$SESSION_ID" \
  --raw)

TYPE=$(echo "$RESULT" | jq -r '.type')

if [ "$TYPE" = "ok" ]; then
    # Extract approval details
    APPROVAL_ID=$(echo "$RESULT" | jq -r '.approvals[0].id')
    TOOL_NAME=$(echo "$RESULT" | jq -r '.approvals[0].tool_name')
    TOOL_INPUT=$(echo "$RESULT" | jq -r '.approvals[0].input')

    echo "Approval request received:"
    echo "  Tool: $TOOL_NAME"
    echo "  Input: $TOOL_INPUT"

    # Ask user for decision
    read -p "Approve this tool use? [y/N] " response

    if [ "$response" = "y" ]; then
        synapse substrate loopback respond \
          --request.approval_id "$APPROVAL_ID" \
          --request.approve true \
          --request.message "Approved by user"
    else
        synapse substrate loopback respond \
          --request.approval_id "$APPROVAL_ID" \
          --request.approve false \
          --request.message "Denied by user"
    fi
elif [ "$TYPE" = "timeout" ]; then
    echo "No approval received within timeout"
    exit 1
else
    echo "Error: $(echo "$RESULT" | jq -r '.message')"
    exit 1
fi
```

## Implementation Details

### Event-Driven Wake-Up

When an approval is created, the storage layer notifies all waiters:

```rust
// In storage.rs
pub async fn create_approval(...) -> Result<ApprovalRequest, String> {
    // Insert into database
    sqlx::query("INSERT INTO loopback_approvals ...").await?;

    // Notify waiters (instant wake-up)
    self.notify_session(session_id);

    Ok(approval)
}

fn notify_session(&self, session_id: &str) {
    if let Some(notifier) = self.session_notifiers.get(session_id) {
        notifier.notify_waiters(); // Wake all blocked wait_for_approval calls
    }
}
```

### Blocking Loop

The `wait_for_approval` method uses a tight loop with event-driven wake-up:

```rust
loop {
    // Check for existing approvals first
    if let Ok(approvals) = storage.list_pending(Some(&session_id)).await {
        if !approvals.is_empty() {
            return Ok { approvals };
        }
    }

    // Wait for notification or timeout
    tokio::select! {
        _ = notifier.notified() => continue,  // New approval arrived, check again
        _ = sleep(remaining_timeout) => return Timeout,
    }
}
```

**Why this works:**
- No busy polling - sleeps until notification
- Instant wake-up when approval arrives
- Checks for existing approvals before waiting (prevents race condition)
- Timeout prevents indefinite blocking

## Comparison: Old vs New

| Aspect | Old (`pending` + polling) | New (`wait_for_approval`) |
|--------|---------------------------|---------------------------|
| **Latency** | ~500ms average (0-1s) | < 1ms (instant) |
| **CPU Usage** | Continuous polling | Event-driven (minimal) |
| **Network** | N requests/second | 1 request total |
| **Complexity** | Client implements loop | Server handles blocking |
| **Code** | ~20 lines shell script | 1 command |

## Migration Guide

### Before (Polling Pattern)

```bash
SESSION_ID="orcha-abc-123"
TIMEOUT=300
START=$(date +%s)

while true; do
    ELAPSED=$(($(date +%s) - START))
    if [ $ELAPSED -ge $TIMEOUT ]; then
        echo "Timeout"
        exit 1
    fi

    APPROVALS=$(synapse substrate loopback pending \
        --session-id "$SESSION_ID" \
        --raw)

    if [ "$(echo "$APPROVALS" | jq -r '.approvals | length')" -gt 0 ]; then
        echo "$APPROVALS"
        break
    fi

    sleep 1
done
```

### After (Blocking Pattern)

```bash
SESSION_ID="orcha-abc-123"

synapse substrate loopback wait_for_approval \
    --request.session_id "$SESSION_ID" \
    --request.timeout_secs 300 \
    --raw
```

**Reduction**: 20 lines → 1 line, instant response

## Error Handling

### Timeout

```bash
$ synapse substrate loopback wait_for_approval \
    --request.session_id "session-123" \
    --request.timeout_secs 10

{
  "type": "timeout",
  "message": "No approval received within 10 seconds"
}
```

### Invalid Session

```bash
$ synapse substrate loopback wait_for_approval \
    --request.session_id "invalid-session"

# Returns immediately if no notifier registered
{
  "type": "timeout",
  "message": "No approval received within 300 seconds"
}
```

### Database Error

```bash
{
  "type": "error",
  "message": "Failed to check pending approvals: database error"
}
```

## Performance Characteristics

- **Wake-up latency**: < 1ms (tokio::sync::Notify)
- **Memory overhead**: ~48 bytes per session (Arc<Notify>)
- **Max concurrent waiters**: Unlimited (notify_waiters wakes all)
- **Cleanup**: Notifiers removed when session cleaned up

## Thread Safety

- **Notify storage**: `Arc<RwLock<HashMap<String, Arc<Notify>>>>`
- **Multiple waiters**: Supported via `notify_waiters()`
- **Race conditions**: Prevented by checking pending before waiting

## Future Enhancements

1. **Streaming mode**: Stream all approvals as they arrive
2. **Filtering**: Wait for specific tool names only
3. **Priority**: Return high-priority approvals first
4. **Batch wait**: Wait for N approvals before returning

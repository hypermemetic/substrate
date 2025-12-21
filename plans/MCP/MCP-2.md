# MCP-2: McpState Enum and State Machine

## Metadata
- **blocked_by:** []
- **unlocks:** [MCP-3, MCP-4, MCP-5]
- **priority:** Critical (on critical path)

## Scope

Implement the MCP protocol state machine that guards method access.

## Implementation

```rust
// src/mcp/state.rs

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum McpState {
    /// Initial state - only `initialize` allowed
    Uninitialized,
    /// After `initialize` received, before `initialized` notification
    Initializing,
    /// Fully operational - all methods allowed
    Ready,
    /// Graceful shutdown in progress
    ShuttingDown,
}

pub struct McpStateMachine {
    state: Arc<RwLock<McpState>>,
}

impl McpStateMachine {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(McpState::Uninitialized)),
        }
    }

    pub fn transition(&self, to: McpState) -> Result<(), McpError> {
        let mut state = self.state.write().unwrap();
        match (*state, to) {
            (McpState::Uninitialized, McpState::Initializing) => Ok(()),
            (McpState::Initializing, McpState::Ready) => Ok(()),
            (McpState::Ready, McpState::ShuttingDown) => Ok(()),
            _ => Err(McpError::InvalidStateTransition),
        }?;
        *state = to;
        Ok(())
    }

    pub fn require(&self, required: McpState) -> Result<(), McpError> {
        let state = self.state.read().unwrap();
        if *state != required {
            return Err(McpError::WrongState {
                expected: required,
                actual: *state,
            });
        }
        Ok(())
    }

    pub fn require_ready(&self) -> Result<(), McpError> {
        self.require(McpState::Ready)
    }
}
```

## Files to Create/Modify

- Create `src/mcp/mod.rs`
- Create `src/mcp/state.rs`
- Add `mod mcp;` to `src/lib.rs`

## Acceptance Criteria

- [ ] `McpState` enum with 4 states
- [ ] `McpStateMachine` with `transition()` and `require()` methods
- [ ] Invalid transitions return error
- [ ] Thread-safe (uses `RwLock`)
- [ ] Unit tests for state transitions

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stream events from bash command execution
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BashEvent {
    /// Standard output line
    Stdout { line: String },
    /// Standard error line
    Stderr { line: String },
    /// Exit code when process completes
    Exit { code: i32 },
}

// Keep the old name as an alias for backwards compatibility
pub type BashOutput = BashEvent;

/// Error events from bash execution
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BashError {
    pub message: String,
}

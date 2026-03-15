use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    /// Error from the executor itself (not the command)
    Error { message: String },
}

// Keep the old name as an alias for backwards compatibility
pub type BashOutput = BashEvent;

/// Typed errors from the bash executor
#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("failed to spawn bash (command='{command}'): {source}")]
    SpawnFailed {
        command: String,
        source: std::io::Error,
    },

    #[error("failed to capture {stream} from bash process (command='{command}')")]
    StdioCaptureFailed {
        stream: &'static str,
        command: String,
    },

    #[error("failed to wait for bash process (command='{command}'): {source}")]
    WaitFailed {
        command: String,
        source: std::io::Error,
    },
}

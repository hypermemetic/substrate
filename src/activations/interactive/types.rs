//! Interactive activation event types
//!
//! Demonstrates domain events for bidirectional communication patterns.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted by interactive operations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum InteractiveEvent {
    /// A step in a multi-step operation has started
    StepStarted {
        /// Current step number (1-indexed)
        step: usize,
        /// Total number of steps
        total: usize,
        /// Human-readable description of this step
        description: String,
    },

    /// Result of a confirmation prompt
    ConfirmResult {
        /// The question that was asked
        question: String,
        /// Whether the user confirmed
        confirmed: bool,
    },

    /// Result of a text input prompt
    PromptResult {
        /// The question that was asked
        question: String,
        /// The user's answer
        answer: String,
    },

    /// Result of a selection prompt
    SelectResult {
        /// The question that was asked
        question: String,
        /// The selected option(s)
        selected: Vec<String>,
    },

    /// A checkpoint in an operation requiring approval
    Checkpoint {
        /// Current step number
        step: usize,
        /// Name of this checkpoint
        name: String,
        /// Whether the checkpoint was approved
        approved: bool,
    },

    /// Final result of an operation
    Result {
        /// Whether the operation succeeded
        success: bool,
        /// Summary of the operation
        summary: String,
        /// Additional data from the operation
        data: Value,
    },

    /// An error occurred during the operation
    Error {
        /// Human-readable error message
        message: String,
        /// Whether the operation can be retried
        recoverable: bool,
    },

    /// Operation was cancelled
    Cancelled {
        /// Reason for cancellation
        reason: String,
    },
}

//! Orcha Context - manages arbor tree for orchestration events
//!
//! This provides a clean abstraction over arbor tree operations,
//! tracking all orchestration events in a structured way.

use crate::activations::arbor::{ArborStorage, NodeId, TreeId};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Orcha context for tracking orchestration events in arbor
pub struct OrchaContext {
    arbor: Arc<ArborStorage>,
    tree_id: TreeId,
    session_id: String,
    last_node_id: Mutex<Option<NodeId>>,
}

impl OrchaContext {
    /// Create a new Orcha context with an arbor tree
    pub async fn new(
        arbor: Arc<ArborStorage>,
        session_id: String,
        task: String,
        model: String,
    ) -> Result<Self, String> {
        // Create arbor tree for this orchestration session
        let tree_id = arbor
            .tree_create(
                Some(serde_json::json!({
                    "session_id": session_id.clone(),
                    "task": task.clone(),
                    "model": model.clone(),
                })),
                "orcha",
            )
            .await
            .map_err(|e| format!("Failed to create arbor tree: {}", e))?;

        let ctx = Self {
            arbor,
            tree_id,
            session_id: session_id.clone(),
            last_node_id: Mutex::new(None),
        };

        // Write initial session_started node
        ctx.write_node(
            format!(
                "session_started: {}\ntask: {}\nmodel: {}",
                session_id, task, model
            ),
            serde_json::json!({
                "event": "session_started",
                "session_id": session_id,
            }),
        )
        .await;

        Ok(ctx)
    }

    /// Get the tree_id for this context
    pub fn tree_id(&self) -> String {
        self.tree_id.to_string()
    }

    /// Write a node to the arbor tree, chained to the previous node
    async fn write_node(&self, content: String, metadata: serde_json::Value) {
        let parent = *self.last_node_id.lock().await;
        match self
            .arbor
            .node_create_text(&self.tree_id, parent, content, Some(metadata))
            .await
        {
            Ok(node_id) => {
                *self.last_node_id.lock().await = Some(node_id);
            }
            Err(e) => {
                tracing::warn!("Failed to write arbor node: {}", e);
            }
        }
    }

    /// Record that a Claude Code session was created
    pub async fn claude_session_created(&self, claude_session_id: String, session_name: String) {
        self.write_node(
            format!(
                "claude_session_created: {}\nsession_name: {}",
                claude_session_id, session_name
            ),
            serde_json::json!({
                "event": "claude_session_started",
                "claude_session_id": claude_session_id,
                "session_name": session_name,
            }),
        )
        .await;
    }

    /// Record that a prompt was sent to Claude
    pub async fn prompt_created(&self, prompt: String, retry_count: u32) {
        self.write_node(
            format!("prompt_created (retry {}):\n{}", retry_count, prompt),
            serde_json::json!({
                "event": "prompt_created",
                "prompt": prompt,
                "retry_count": retry_count,
            }),
        )
        .await;
    }

    /// Record that Claude session completed
    pub async fn claude_session_complete(&self, claude_session_id: String) {
        self.write_node(
            format!("claude_session_complete: {}", claude_session_id),
            serde_json::json!({
                "event": "claude_session_complete",
                "claude_session_id": claude_session_id,
            }),
        )
        .await;
    }

    /// Record that validation started
    pub async fn validation_started(&self, test_command: String, cwd: String) {
        self.write_node(
            format!("validation_started:\n{}", test_command),
            serde_json::json!({
                "event": "validation_started",
                "test_command": test_command,
                "cwd": cwd,
            }),
        )
        .await;
    }

    /// Record validation result
    pub async fn validation_result(&self, success: bool, output: String) {
        self.write_node(
            format!(
                "validation_result: {}\noutput:\n{}",
                if success { "SUCCESS" } else { "FAILED" },
                output
            ),
            serde_json::json!({
                "event": "validation_result",
                "success": success,
                "output": output,
            }),
        )
        .await;
    }

    /// Record session completion
    pub async fn session_complete(&self, status: &str) {
        self.write_node(
            format!("session_complete: {} status={}", self.session_id, status),
            serde_json::json!({
                "event": "session_complete",
                "status": status,
            }),
        )
        .await;
    }

    /// Record a tool use event
    pub async fn tool_use(&self, tool_name: String, tool_use_id: String, input: String) {
        self.write_node(
            format!(
                "tool_use: {}\nid: {}\ninput:\n{}",
                tool_name, tool_use_id, input
            ),
            serde_json::json!({
                "event": "tool_use",
                "tool_name": tool_name,
                "tool_use_id": tool_use_id,
                "input": input,
            }),
        )
        .await;
    }

    /// Record a tool result
    pub async fn tool_result(&self, tool_use_id: String, output: String, is_error: bool) {
        self.write_node(
            format!(
                "tool_result for {}: {}\n{}",
                tool_use_id,
                if is_error { "ERROR" } else { "SUCCESS" },
                output
            ),
            serde_json::json!({
                "event": "tool_result",
                "tool_use_id": tool_use_id,
                "output": output,
                "is_error": is_error,
            }),
        )
        .await;
    }

    /// Record Claude's text output
    pub async fn claude_output(&self, text: String) {
        self.write_node(
            format!("claude_output:\n{}", text),
            serde_json::json!({
                "event": "claude_output",
                "text": text,
            }),
        )
        .await;
    }
}

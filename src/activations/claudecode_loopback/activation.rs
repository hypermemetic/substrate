use super::storage::{LoopbackStorage, LoopbackStorageConfig};
use super::types::*;
use async_stream::stream;
use futures::Stream;
use hub_macro::hub_methods;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// ClaudeCode Loopback - routes tool permissions back to parent for approval
#[derive(Clone)]
pub struct ClaudeCodeLoopback {
    storage: Arc<LoopbackStorage>,
    mcp_url: String,
}

impl ClaudeCodeLoopback {
    pub async fn new(config: LoopbackStorageConfig) -> Result<Self, String> {
        let storage = LoopbackStorage::new(config).await?;
        let mcp_url = std::env::var("PLEXUS_MCP_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:4445/mcp".to_string());

        Ok(Self {
            storage: Arc::new(storage),
            mcp_url,
        })
    }

    pub fn with_mcp_url(mut self, url: String) -> Self {
        self.mcp_url = url;
        self
    }

    /// Get the underlying storage (for sharing with ClaudeCode)
    pub fn storage(&self) -> Arc<LoopbackStorage> {
        self.storage.clone()
    }
}

#[hub_methods(
    namespace = "loopback",
    version = "1.0.0",
    description = "Route tool permissions to parent for approval"
)]
impl ClaudeCodeLoopback {
    /// Permission prompt handler - blocks until parent approves/denies
    ///
    /// This is called by Claude Code CLI via --permission-prompt-tool.
    /// It blocks (polls) until the parent calls loopback.respond().
    ///
    /// Returns a JSON string (not object) because Claude Code expects the MCP response
    /// to have the permission JSON already stringified in content[0].text.
    /// See: https://github.com/anthropics/claude-code/blob/main/docs/permission-prompt-tool.md
    #[hub_macro::hub_method(params(
        tool_name = "Name of the tool being requested",
        tool_use_id = "Unique ID for this tool invocation",
        input = "Tool input parameters"
    ))]
    async fn permit(
        &self,
        tool_name: String,
        tool_use_id: String,
        input: Value,
    ) -> impl Stream<Item = String> + Send + 'static {
        // IMMEDIATE DEBUG: Log before stream starts
        eprintln!("[LOOPBACK] permit called: tool={}, tool_use_id={}", tool_name, tool_use_id);

        let storage = self.storage.clone();

        // Look up session ID from pre-registered tool_use_id mapping
        // This mapping was set by run_chat_background when it saw the ToolUse event
        let session_id = storage.lookup_session_by_tool(&tool_use_id)
            .unwrap_or_else(|| "unknown".to_string());

        stream! {
            // DEBUG: Log the lookup result
            eprintln!("[LOOPBACK] permit: tool_use_id={} mapped to session_id={}", tool_use_id, session_id);

            // Create approval request
            let approval = match storage.create_approval(
                &session_id,
                &tool_name,
                &tool_use_id,
                &input,
            ).await {
                Ok(a) => a,
                Err(e) => {
                    // Return deny response as JSON string
                    let response = json!({
                        "behavior": "deny",
                        "message": format!("Failed to create approval: {}", e)
                    });
                    yield response.to_string();
                    return;
                }
            };

            let approval_id = approval.id;
            let timeout_secs = 300u64; // 5 minute timeout
            let poll_interval = Duration::from_secs(1);
            let start = std::time::Instant::now();

            // Blocking poll loop - like HumanLayer's hlyr
            loop {
                // Check timeout
                if start.elapsed().as_secs() > timeout_secs {
                    let _ = storage.resolve_approval(&approval_id, false, Some("Timed out".to_string())).await;
                    let response = json!({
                        "behavior": "deny",
                        "message": "Approval request timed out"
                    });
                    yield response.to_string();
                    return;
                }

                // Poll for resolution
                match storage.get_approval(&approval_id).await {
                    Ok(current) => {
                        match current.status {
                            ApprovalStatus::Approved => {
                                // Return allow response as JSON string
                                // Claude Code expects: {"behavior": "allow", "updatedInput": {...}}
                                let response = json!({
                                    "behavior": "allow",
                                    "updatedInput": input.clone()
                                });
                                yield response.to_string();
                                return;
                            }
                            ApprovalStatus::Denied => {
                                let response = json!({
                                    "behavior": "deny",
                                    "message": current.response_message.unwrap_or_else(|| "Denied by parent".to_string())
                                });
                                yield response.to_string();
                                return;
                            }
                            ApprovalStatus::TimedOut => {
                                let response = json!({
                                    "behavior": "deny",
                                    "message": "Approval timed out"
                                });
                                yield response.to_string();
                                return;
                            }
                            ApprovalStatus::Pending => {
                                // Continue polling
                            }
                        }
                    }
                    Err(e) => {
                        let response = json!({
                            "behavior": "deny",
                            "message": format!("Failed to check approval: {}", e)
                        });
                        yield response.to_string();
                        return;
                    }
                }

                sleep(poll_interval).await;
            }
        }
    }

    /// Respond to a pending approval request
    #[hub_macro::hub_method(params(
        approval_id = "ID of the approval request",
        approve = "Whether to approve (true) or deny (false)",
        message = "Optional message/reason"
    ))]
    async fn respond(
        &self,
        approval_id: ApprovalId,
        approve: bool,
        message: Option<String>,
    ) -> impl Stream<Item = RespondResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.resolve_approval(&approval_id, approve, message).await {
                Ok(()) => {
                    yield RespondResult::Ok { approval_id };
                }
                Err(e) => {
                    yield RespondResult::Err { message: e };
                }
            }
        }
    }

    /// List pending approval requests
    #[hub_macro::hub_method(params(
        session_id = "Optional session ID to filter by"
    ))]
    async fn pending(
        &self,
        session_id: Option<String>,
    ) -> impl Stream<Item = PendingResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.list_pending(session_id.as_deref()).await {
                Ok(approvals) => {
                    yield PendingResult::Ok { approvals };
                }
                Err(e) => {
                    yield PendingResult::Err { message: e };
                }
            }
        }
    }

    /// Generate MCP configuration for a loopback session
    #[hub_macro::hub_method(params(
        session_id = "Session ID for correlation"
    ))]
    async fn configure(
        &self,
        session_id: String,
    ) -> impl Stream<Item = ConfigureResult> + Send + 'static {
        let mcp_url = self.mcp_url.clone();

        stream! {
            // Include session_id in env config for correlation
            let config = json!({
                "mcpServers": {
                    "plexus": {
                        "type": "http",
                        "url": mcp_url
                    }
                },
                "env": {
                    "LOOPBACK_SESSION_ID": session_id
                }
            });

            yield ConfigureResult::Ok { mcp_config: config };
        }
    }
}

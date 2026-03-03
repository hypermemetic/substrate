use super::orchestrator::run_orchestration_task;
use super::storage::OrchaStorage;
use super::types::*;
use crate::activations::claudecode::ClaudeCode;
use crate::activations::claudecode_loopback::ClaudeCodeLoopback;
use crate::plexus::{HubContext, NoParent};
use async_stream::stream;
use futures::Stream;
use futures::StreamExt;
use plexus_macros::hub_methods;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::process::Command;
use uuid::Uuid;

/// Orcha activation - Full orchestration with approval loops and validation
///
/// Provides both full orchestration (run_task) and coordination helpers.
#[derive(Clone)]
pub struct Orcha<P: HubContext = NoParent> {
    storage: Arc<OrchaStorage>,
    claudecode: Arc<ClaudeCode<P>>,
    loopback: Arc<ClaudeCodeLoopback>,
    arbor_storage: Arc<crate::activations::arbor::ArborStorage>,
    _phantom: PhantomData<P>,
}

impl<P: HubContext> Orcha<P> {
    /// Create a new Orcha activation
    pub fn new(
        storage: Arc<OrchaStorage>,
        claudecode: Arc<ClaudeCode<P>>,
        loopback: Arc<ClaudeCodeLoopback>,
        arbor_storage: Arc<crate::activations::arbor::ArborStorage>,
    ) -> Self {
        Self {
            storage,
            claudecode,
            loopback,
            arbor_storage,
            _phantom: PhantomData,
        }
    }
}

#[hub_methods(
    namespace = "orcha",
    version = "1.0.0",
    description = "Full task orchestration with approval loops and validation"
)]
impl<P: HubContext> Orcha<P> {
    /// Run a complete orchestration task
    ///
    /// This is the main entry point for running tasks with the full orcha pattern:
    /// - Creates sessions
    /// - Runs task with approval handling
    /// - Extracts and executes validation
    /// - Auto-retries on failure
    #[plexus_macros::hub_method]
    async fn run_task(
        &self,
        request: RunTaskRequest,
    ) -> impl Stream<Item = OrchaEvent> + Send + 'static {
        run_orchestration_task(
            self.storage.clone(),
            self.claudecode.clone(),
            self.loopback.clone(),
            request,
            None, // Let orchestrator generate session_id
        ).await
    }
    /// Create a new orchestration session
    ///
    /// Creates a session record to track orchestration state. The client should
    /// then create a corresponding claudecode session with loopback enabled.
    #[plexus_macros::hub_method]
    async fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> impl Stream<Item = CreateSessionResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Generate unique session ID
            let session_id = format!("orcha-{}", Uuid::new_v4());

            // Determine agent mode
            let agent_mode = if request.multi_agent {
                AgentMode::Multi
            } else {
                AgentMode::Single
            };

            // Create session in storage
            let session_result = storage.create_session(
                session_id.clone(),
                request.model.clone(),
                request.working_directory.clone(),
                request.rules.clone(),
                request.max_retries,
                agent_mode,
            ).await;

            match session_result {
                Ok(session) => {
                    yield CreateSessionResult::Ok {
                        session_id,
                        created_at: session.created_at,
                    };
                }
                Err(e) => {
                    yield CreateSessionResult::Err {
                        message: format!("Failed to create session: {}", e),
                    };
                }
            }
        }
    }

    /// Update session state
    ///
    /// Called by the client to update the current state of the session
    #[plexus_macros::hub_method]
    async fn update_session_state(
        &self,
        session_id: SessionId,
        state: SessionState,
    ) -> impl Stream<Item = UpdateSessionStateResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.update_state(&session_id, state).await {
                Ok(_) => {
                    yield UpdateSessionStateResult::Ok;
                }
                Err(e) => {
                    yield UpdateSessionStateResult::Err {
                        message: format!("Failed to update state: {}", e),
                    };
                }
            }
        }
    }

    /// Get session information
    #[plexus_macros::hub_method]
    async fn get_session(
        &self,
        request: GetSessionRequest,
    ) -> impl Stream<Item = GetSessionResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.get_session(&request.session_id).await {
                Ok(session) => {
                    yield GetSessionResult::Ok { session };
                }
                Err(e) => {
                    yield GetSessionResult::Err {
                        message: format!("Session not found: {}", e),
                    };
                }
            }
        }
    }

    /// Extract validation artifact from text
    ///
    /// Scans text for {"orcha_validate": {...}} pattern and extracts test command
    #[plexus_macros::hub_method]
    async fn extract_validation(
        &self,
        text: String,
    ) -> impl Stream<Item = ExtractValidationResult> + Send + 'static {
        stream! {
            match extract_validation_artifact(&text) {
                Some(artifact) => {
                    yield ExtractValidationResult::Ok { artifact };
                }
                None => {
                    yield ExtractValidationResult::NotFound;
                }
            }
        }
    }

    /// Run a validation test
    ///
    /// Executes a test command and returns the result
    #[plexus_macros::hub_method]
    async fn run_validation(
        &self,
        artifact: ValidationArtifact,
    ) -> impl Stream<Item = RunValidationResult> + Send + 'static {
        stream! {
            let result = run_validation_test(&artifact).await;

            yield RunValidationResult::Ok { result };
        }
    }

    /// Increment retry counter for a session
    ///
    /// Called when validation fails and the client wants to retry
    #[plexus_macros::hub_method]
    async fn increment_retry(
        &self,
        session_id: SessionId,
    ) -> impl Stream<Item = IncrementRetryResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.increment_retry(&session_id).await {
                Ok(count) => {
                    let session = storage.get_session(&session_id).await.ok();
                    let max_retries = session.map(|s| s.max_retries).unwrap_or(3);

                    yield IncrementRetryResult::Ok {
                        retry_count: count,
                        max_retries,
                        exceeded: count >= max_retries,
                    };
                }
                Err(e) => {
                    yield IncrementRetryResult::Err {
                        message: format!("Failed to increment retry: {}", e),
                    };
                }
            }
        }
    }

    /// List all sessions
    #[plexus_macros::hub_method]
    async fn list_sessions(&self) -> impl Stream<Item = ListSessionsResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            let sessions = storage.list_sessions().await;
            yield ListSessionsResult::Ok { sessions };
        }
    }

    /// Delete a session
    #[plexus_macros::hub_method]
    async fn delete_session(
        &self,
        session_id: SessionId,
    ) -> impl Stream<Item = DeleteSessionResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.delete_session(&session_id).await {
                Ok(_) => {
                    yield DeleteSessionResult::Ok;
                }
                Err(e) => {
                    yield DeleteSessionResult::Err {
                        message: format!("Failed to delete session: {}", e),
                    };
                }
            }
        }
    }

    /// Run a task asynchronously - returns immediately with session_id
    ///
    /// Like run_task but non-blocking. Returns the session_id immediately
    /// and the task runs in the background. Use check_status or get_session
    /// to check on progress.
    #[plexus_macros::hub_method]
    async fn run_task_async(
        &self,
        request: RunTaskRequest,
    ) -> impl Stream<Item = RunTaskAsyncResult> + Send + 'static {
        let storage = self.storage.clone();
        let claudecode = self.claudecode.clone();
        let loopback = self.loopback.clone();

        stream! {
            // Generate session ID that will be used by the orchestrator
            let session_id = format!("orcha-{}", Uuid::new_v4());
            let session_id_for_spawn = session_id.clone();

            // Spawn the orchestration task in the background
            let req = request.clone();
            tokio::spawn(async move {
                let stream = run_orchestration_task(
                    storage,
                    claudecode,
                    loopback,
                    req,
                    Some(session_id_for_spawn), // Pass the session_id to orchestrator
                ).await;
                tokio::pin!(stream);

                // Consume the stream in the background
                while let Some(_event) = stream.next().await {
                    // Events are discarded in async mode
                    // Use get_session or check_status to monitor
                }
            });

            // Return immediately with session_id
            yield RunTaskAsyncResult::Ok { session_id };
        }
    }

    /// List all orcha monitor trees
    ///
    /// Returns all arbor trees created by orcha for monitoring sessions
    #[plexus_macros::hub_method]
    async fn list_monitor_trees(
        &self,
    ) -> impl Stream<Item = ListMonitorTreesResult> + Send + 'static {
        let arbor_storage = self.arbor_storage.clone();

        stream! {
            // Query arbor for trees with metadata type="orcha_monitor"
            let filter = serde_json::json!({"type": "orcha_monitor"});

            match arbor_storage.tree_query_by_metadata(&filter).await {
                Ok(tree_ids) => {
                    let mut trees = Vec::new();

                    // Get metadata for each tree
                    for tree_id in tree_ids {
                        if let Ok(tree) = arbor_storage.tree_get(&tree_id).await {
                            if let Some(metadata) = &tree.metadata {
                                let session_id = metadata.get("session_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let tree_path = metadata.get("tree_path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();

                                trees.push(MonitorTreeInfo {
                                    tree_id: tree.id.to_string(),
                                    session_id,
                                    tree_path,
                                });
                            }
                        }
                    }

                    yield ListMonitorTreesResult::Ok { trees };
                }
                Err(_) => {
                    yield ListMonitorTreesResult::Ok { trees: vec![] };
                }
            }
        }
    }

    /// Check status of a running session by asking Claude to summarize
    ///
    /// Creates an ephemeral forked session to generate a summary of what's happening,
    /// and saves the summary to an arbor monitoring tree for historical tracking.
    #[plexus_macros::hub_method]
    async fn check_status(
        &self,
        request: CheckStatusRequest,
    ) -> impl Stream<Item = CheckStatusResult> + Send + 'static {
        let claudecode = self.claudecode.clone();
        let arbor_storage = self.arbor_storage.clone();
        let storage = self.storage.clone();
        let session_id = request.session_id.clone();

        stream! {
            // First, get the actual session state from storage
            let session_info = match storage.get_session(&session_id).await {
                Ok(info) => info,
                Err(e) => {
                    yield CheckStatusResult::Err {
                        message: format!("Session not found: {}", e),
                    };
                    return;
                }
            };

            // Branch based on agent mode
            if session_info.agent_mode == AgentMode::Multi {
                // Multi-agent status check
                let agents = match storage.list_agents(&session_id).await {
                    Ok(a) => a,
                    Err(e) => {
                        yield CheckStatusResult::Err {
                            message: format!("Failed to list agents: {}", e),
                        };
                        return;
                    }
                };

                if agents.is_empty() {
                    yield CheckStatusResult::Err {
                        message: "No agents found in session".to_string(),
                    };
                    return;
                }

                // Generate summaries for each agent in parallel
                let summary_futures: Vec<_> = agents.iter().map(|agent| {
                    generate_agent_summary(&claudecode, &arbor_storage, agent.clone())
                }).collect();

                let agent_summaries: Vec<AgentSummary> = futures::future::join_all(summary_futures)
                    .await
                    .into_iter()
                    .filter_map(|r| r.ok())
                    .collect();

                // Generate overall meta-summary
                let overall_summary = generate_overall_summary(
                    &claudecode,
                    &session_id,
                    &agent_summaries,
                ).await;

                let summary = overall_summary.unwrap_or_else(|| "Unable to generate summary".to_string());

                // Save to arbor monitoring tree
                match save_status_summary_to_arbor(&arbor_storage, &session_id, &summary).await {
                    Ok(_) => {
                        yield CheckStatusResult::Ok {
                            summary,
                            agent_summaries,
                        };
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save summary to arbor: {}", e);
                        yield CheckStatusResult::Ok {
                            summary,
                            agent_summaries,
                        };
                    }
                }

                return;
            }

            // Single-agent mode (original logic below)


            // Format session state as context for Claude and extract stream_id for arbor lookup
            let (state_description, stream_id_opt) = match &session_info.state {
                SessionState::Idle => ("idle (not currently executing)".to_string(), None),
                SessionState::Running { stream_id, sequence, active_agents, completed_agents, failed_agents } => {
                    let agent_info = if *active_agents > 0 || *completed_agents > 0 || *failed_agents > 0 {
                        format!(" (agents: {} active, {} complete, {} failed)", active_agents, completed_agents, failed_agents)
                    } else {
                        String::new()
                    };
                    (format!("running (stream: {}, sequence: {}{})", stream_id, sequence, agent_info), Some(stream_id.clone()))
                }
                SessionState::WaitingApproval { approval_id } => {
                    (format!("waiting for approval (approval_id: {})", approval_id), None)
                }
                SessionState::Validating { test_command } => {
                    (format!("validating with command: {}", test_command), None)
                }
                SessionState::Complete => ("completed successfully".to_string(), None),
                SessionState::Failed { error } => {
                    (format!("failed with error: {}", error), None)
                }
            };

            // Try to get the conversation tree from claudecode if we have a stream_id
            let conversation_context = if let Some(stream_id) = stream_id_opt {
                // Get the claudecode session to find its arbor tree
                match claudecode.storage.session_get_by_name(&stream_id).await {
                    Ok(cc_session) => {
                        // Get and render the arbor tree as a formatted conversation
                        match arbor_storage.tree_get(&cc_session.head.tree_id).await {
                            Ok(tree) => {
                                let formatted = format_conversation_from_tree(&tree);
                                Some(formatted)
                            }
                            Err(e) => {
                                tracing::warn!("Failed to get arbor tree for claudecode session {}: {}", stream_id, e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to get claudecode session {}: {}", stream_id, e);
                        None
                    }
                }
            } else {
                None
            };

            // Create an ephemeral session to ask for a summary
            let summary_session = format!("orcha-check-{}", Uuid::new_v4());

            // Create the session - using Haiku for fast, cheap summaries
            let mut create_stream = claudecode.create(
                summary_session.clone(),
                "/workspace".to_string(), // Default, doesn't matter for ephemeral
                crate::activations::claudecode::Model::Haiku,
                None,
                Some(false), // No loopback needed for summary
            ).await;
            tokio::pin!(create_stream);

            let mut create_ok = false;
            while let Some(result) = create_stream.next().await {
                if let crate::activations::claudecode::CreateResult::Ok { .. } = result {
                    create_ok = true;
                }
            }

            if !create_ok {
                yield CheckStatusResult::Err {
                    message: "Failed to create summary session".to_string(),
                };
                return;
            }

            // Ask Claude to summarize the session with actual context
            let prompt = if let Some(conversation) = conversation_context {
                format!(
                    "An orcha orchestration session has the following status:\n\n\
                     - Session ID: {}\n\
                     - Model: {}\n\
                     - State: {}\n\
                     - Retry count: {}/{}\n\
                     - Created at: {} (unix timestamp)\n\
                     - Last activity: {} (unix timestamp)\n\n\
                     Here is the actual conversation tree showing what the agent has been doing:\n\n\
                     {}\n\n\
                     Generate a brief, natural language summary (2-3 sentences) of what's happening in this session.\n\
                     Focus on what the agent is currently doing or has accomplished. Be specific about the task.",
                    session_id,
                    session_info.model,
                    state_description,
                    session_info.retry_count,
                    session_info.max_retries,
                    session_info.created_at,
                    session_info.last_activity,
                    conversation
                )
            } else {
                format!(
                    "An orcha orchestration session has the following status:\n\n\
                     - Session ID: {}\n\
                     - Model: {}\n\
                     - State: {}\n\
                     - Retry count: {}/{}\n\
                     - Created at: {} (unix timestamp)\n\
                     - Last activity: {} (unix timestamp)\n\n\
                     Generate a brief, natural language summary (2-3 sentences) of what's happening in this session.\n\
                     Focus on the current state and what the agent is doing or has done.",
                    session_id,
                    session_info.model,
                    state_description,
                    session_info.retry_count,
                    session_info.max_retries,
                    session_info.created_at,
                    session_info.last_activity
                )
            };

            let chat_stream = claudecode.chat(
                summary_session.clone(),
                prompt,
                Some(true), // Ephemeral - don't save to history
            ).await;
            tokio::pin!(chat_stream);

            let mut summary = String::new();
            while let Some(event) = chat_stream.next().await {
                if let crate::activations::claudecode::ChatEvent::Content { text } = event {
                    summary.push_str(&text);
                }
            }

            if summary.is_empty() {
                yield CheckStatusResult::Err {
                    message: "Failed to generate summary".to_string(),
                };
            } else {
                // Save summary to arbor monitoring tree
                match save_status_summary_to_arbor(&arbor_storage, &session_id, &summary).await {
                    Ok(_) => {
                        yield CheckStatusResult::Ok {
                            summary,
                            agent_summaries: vec![],  // Single-agent mode
                        };
                    }
                    Err(e) => {
                        // Still return the summary even if arbor save fails
                        tracing::warn!("Failed to save summary to arbor: {}", e);
                        yield CheckStatusResult::Ok {
                            summary,
                            agent_summaries: vec![],  // Single-agent mode
                        };
                    }
                }
            }
        }
    }

    /// Spawn a new agent in an existing session (multi-agent mode)
    ///
    /// Creates a new ClaudeCode session and tracks it as an agent within the orcha session.
    /// Can be called explicitly via API or by agents themselves requesting helpers.
    #[plexus_macros::hub_method]
    async fn spawn_agent(
        &self,
        request: SpawnAgentRequest,
    ) -> impl Stream<Item = SpawnAgentResult> + Send + 'static {
        let storage = self.storage.clone();
        let claudecode = self.claudecode.clone();
        let loopback = self.loopback.clone();

        stream! {
            // Verify session exists and is in multi-agent mode
            let session = match storage.get_session(&request.session_id).await {
                Ok(s) => s,
                Err(e) => {
                    yield SpawnAgentResult::Err {
                        message: format!("Session not found: {}", e),
                    };
                    return;
                }
            };

            if session.agent_mode != AgentMode::Multi {
                yield SpawnAgentResult::Err {
                    message: "Session is not in multi-agent mode".to_string(),
                };
                return;
            }

            // Parse model
            let model = match session.model.as_str() {
                "opus" => crate::activations::claudecode::Model::Opus,
                "sonnet" => crate::activations::claudecode::Model::Sonnet,
                "haiku" => crate::activations::claudecode::Model::Haiku,
                _ => crate::activations::claudecode::Model::Sonnet,
            };

            // Create ClaudeCode session for this agent
            let cc_session_name = format!("orcha-agent-{}", Uuid::new_v4());

            let create_stream = claudecode.create(
                cc_session_name.clone(),
                "/workspace".to_string(),  // TODO: Get from session
                model.clone(),
                None,
                Some(true), // Loopback enabled
            ).await;
            tokio::pin!(create_stream);

            let mut create_ok = false;
            while let Some(result) = create_stream.next().await {
                if let crate::activations::claudecode::CreateResult::Ok { .. } = result {
                    create_ok = true;
                    break;
                }
            }

            if !create_ok {
                yield SpawnAgentResult::Err {
                    message: "Failed to create ClaudeCode session".to_string(),
                };
                return;
            }

            // Create agent record
            match storage.create_agent(
                &request.session_id,
                cc_session_name.clone(),
                request.subtask.clone(),
                false, // Not primary
                request.parent_agent_id,
            ).await {
                Ok(agent) => {
                    // Spawn background task to run this agent
                    let config = super::orchestrator::AgentConfig {
                        model,
                        working_directory: "/workspace".to_string(),
                        max_retries: session.max_retries,
                        task_context: request.subtask.clone(),
                        auto_approve: true, // TODO: Store in session and retrieve
                    };

                    super::orchestrator::spawn_agent_task(
                        storage.clone(),
                        claudecode.clone(),
                        loopback.clone(),
                        agent.clone(),
                        request.subtask.clone(),
                        config,
                    );

                    yield SpawnAgentResult::Ok {
                        agent_id: agent.agent_id,
                        claudecode_session_id: cc_session_name,
                    };
                }
                Err(e) => {
                    yield SpawnAgentResult::Err {
                        message: format!("Failed to create agent: {}", e),
                    };
                }
            }
        }
    }

    /// List all agents in a session
    #[plexus_macros::hub_method]
    async fn list_agents(
        &self,
        request: ListAgentsRequest,
    ) -> impl Stream<Item = ListAgentsResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.list_agents(&request.session_id).await {
                Ok(agents) => {
                    yield ListAgentsResult::Ok { agents };
                }
                Err(e) => {
                    yield ListAgentsResult::Err {
                        message: format!("Failed to list agents: {}", e),
                    };
                }
            }
        }
    }

    /// Get specific agent info
    #[plexus_macros::hub_method]
    async fn get_agent(
        &self,
        request: GetAgentRequest,
    ) -> impl Stream<Item = GetAgentResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.get_agent(&request.agent_id).await {
                Ok(agent) => {
                    yield GetAgentResult::Ok { agent };
                }
                Err(e) => {
                    yield GetAgentResult::Err {
                        message: format!("Agent not found: {}", e),
                    };
                }
            }
        }
    }

    /// List pending approval requests for a session
    ///
    /// Returns all approval requests awaiting manual approval.
    /// Only relevant when auto_approve is disabled.
    #[plexus_macros::hub_method]
    async fn list_pending_approvals(
        &self,
        request: ListApprovalsRequest,
    ) -> impl Stream<Item = ListApprovalsResult> + Send + 'static {
        let loopback = self.loopback.clone();
        let session_id = request.session_id;

        stream! {
            match loopback.storage().list_pending(Some(&session_id)).await {
                Ok(approvals) => {
                    let approval_infos: Vec<ApprovalInfo> = approvals
                        .into_iter()
                        .map(|approval| ApprovalInfo {
                            approval_id: approval.id.to_string(),
                            session_id: approval.session_id,
                            tool_name: approval.tool_name,
                            tool_use_id: approval.tool_use_id,
                            tool_input: approval.input,
                            created_at: chrono::DateTime::from_timestamp(approval.created_at, 0)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| approval.created_at.to_string()),
                        })
                        .collect();

                    yield ListApprovalsResult::Ok {
                        approvals: approval_infos,
                    };
                }
                Err(e) => {
                    yield ListApprovalsResult::Err {
                        message: format!("Failed to list pending approvals: {}", e),
                    };
                }
            }
        }
    }

    /// Approve a pending request
    ///
    /// Approves a tool use request and unblocks the waiting agent.
    /// The approval_id comes from list_pending_approvals.
    #[plexus_macros::hub_method]
    async fn approve_request(
        &self,
        request: ApproveRequest,
    ) -> impl Stream<Item = ApprovalActionResult> + Send + 'static {
        let loopback = self.loopback.clone();
        let approval_id = request.approval_id.clone();
        let message = request.message.clone();

        stream! {
            match uuid::Uuid::parse_str(&approval_id) {
                Ok(uuid_id) => {
                    match loopback.storage()
                        .resolve_approval(&uuid_id, true, message.clone())
                        .await
                    {
                        Ok(_) => {
                            yield ApprovalActionResult::Ok {
                                approval_id: approval_id.clone(),
                                message: Some("Approved".to_string()),
                            };
                        }
                        Err(e) => {
                            yield ApprovalActionResult::Err {
                                message: format!("Failed to approve: {}", e),
                            };
                        }
                    }
                }
                Err(_) => {
                    yield ApprovalActionResult::Err {
                        message: format!("Invalid approval_id format: {}", approval_id),
                    };
                }
            }
        }
    }

    /// Deny a pending request
    ///
    /// Denies a tool use request. The agent will receive an error
    /// and may adapt or fail depending on its error handling.
    #[plexus_macros::hub_method]
    async fn deny_request(
        &self,
        request: DenyRequest,
    ) -> impl Stream<Item = ApprovalActionResult> + Send + 'static {
        let loopback = self.loopback.clone();
        let approval_id = request.approval_id.clone();
        let reason = request.reason.clone();

        stream! {
            match uuid::Uuid::parse_str(&approval_id) {
                Ok(uuid_id) => {
                    match loopback.storage()
                        .resolve_approval(&uuid_id, false, reason.clone())
                        .await
                    {
                        Ok(_) => {
                            yield ApprovalActionResult::Ok {
                                approval_id: approval_id.clone(),
                                message: reason.or(Some("Denied".to_string())),
                            };
                        }
                        Err(e) => {
                            yield ApprovalActionResult::Err {
                                message: format!("Failed to deny: {}", e),
                            };
                        }
                    }
                }
                Err(_) => {
                    yield ApprovalActionResult::Err {
                        message: format!("Invalid approval_id format: {}", approval_id),
                    };
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Extract validation artifact from accumulated text
fn extract_validation_artifact(text: &str) -> Option<ValidationArtifact> {
    // Look for {"orcha_validate": {...}} pattern
    use regex::Regex;

    let re = Regex::new(r#"\{"orcha_validate"\s*:\s*(\{[^}]+\})\}"#).ok()?;
    let captures = re.captures(text)?;
    let json_str = captures.get(1)?.as_str();

    serde_json::from_str::<ValidationArtifact>(json_str).ok()
}

/// Run a validation test command
async fn run_validation_test(artifact: &ValidationArtifact) -> ValidationResult {
    let output = Command::new("sh")
        .arg("-c")
        .arg(&artifact.test_command)
        .current_dir(&artifact.cwd)
        .output()
        .await;

    match output {
        Ok(output) => ValidationResult {
            success: output.status.success(),
            output: String::from_utf8_lossy(&output.stdout).to_string()
                + &String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        },
        Err(e) => ValidationResult {
            success: false,
            output: format!("Failed to execute command: {}", e),
            exit_code: None,
        },
    }
}

/// Format an arbor tree into a readable conversation
///
/// Converts the JSON-based arbor tree structure into a human-readable conversation format
fn format_conversation_from_tree(tree: &crate::activations::arbor::Tree) -> String {
    use crate::activations::arbor::NodeType;

    let mut output = String::new();
    let mut current_role = String::new();
    let mut message_text = String::new();
    let mut tool_uses = Vec::new();

    // Walk the tree in order
    fn walk_nodes(
        tree: &crate::activations::arbor::Tree,
        node_id: &crate::activations::arbor::NodeId,
        output: &mut String,
        current_role: &mut String,
        message_text: &mut String,
        tool_uses: &mut Vec<String>,
    ) {
        if let Some(node) = tree.nodes.get(node_id) {
            if let NodeType::Text { content } = &node.data {
                // Try to parse as JSON to extract event type
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(content) {
                    if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                        match event_type {
                            "user_message" => {
                                // Flush previous message
                                flush_message(output, current_role, message_text, tool_uses);

                                *current_role = "User".to_string();
                                if let Some(content) = event.get("content").and_then(|v| v.as_str()) {
                                    *message_text = content.to_string();
                                }
                            }
                            "assistant_start" => {
                                // Flush previous message
                                flush_message(output, current_role, message_text, tool_uses);

                                *current_role = "Assistant".to_string();
                                *message_text = String::new();
                            }
                            "content_text" => {
                                if let Some(text) = event.get("text").and_then(|v| v.as_str()) {
                                    message_text.push_str(text);
                                }
                            }
                            "content_tool_use" => {
                                if let Some(name) = event.get("name").and_then(|v| v.as_str()) {
                                    let mut tool_str = format!("[Tool: {}]", name);
                                    if let Some(input) = event.get("input") {
                                        if let Ok(input_str) = serde_json::to_string_pretty(input) {
                                            // Limit tool input to 200 chars
                                            let trimmed = if input_str.len() > 200 {
                                                format!("{}...", &input_str[..200])
                                            } else {
                                                input_str
                                            };
                                            tool_str.push_str(&format!(" {}", trimmed));
                                        }
                                    }
                                    tool_uses.push(tool_str);
                                }
                            }
                            _ => {} // Ignore other event types
                        }
                    }
                }
            }

            // Recursively walk children
            for child_id in &node.children {
                walk_nodes(tree, child_id, output, current_role, message_text, tool_uses);
            }
        }
    }

    fn flush_message(
        output: &mut String,
        current_role: &str,
        message_text: &str,
        tool_uses: &mut Vec<String>,
    ) {
        if !current_role.is_empty() && (!message_text.is_empty() || !tool_uses.is_empty()) {
            output.push_str(&format!("{}:\n", current_role));
            if !message_text.is_empty() {
                output.push_str(message_text);
                output.push_str("\n");
            }
            for tool in tool_uses.drain(..) {
                output.push_str(&format!("  {}\n", tool));
            }
            output.push_str("\n");
        }
    }

    // Start walking from root
    walk_nodes(tree, &tree.root, &mut output, &mut current_role, &mut message_text, &mut tool_uses);

    // Flush any remaining message
    flush_message(&mut output, &current_role, &message_text, &mut tool_uses);

    output
}

/// Save a status summary to the arbor monitoring tree
///
/// Creates a monitoring tree for the session (if it doesn't exist) and appends
/// the summary as a new text node with timestamp.
async fn save_status_summary_to_arbor(
    arbor_storage: &crate::activations::arbor::ArborStorage,
    session_id: &str,
    summary: &str,
) -> Result<(), String> {
    use crate::activations::arbor::TreeId;

    // Generate deterministic tree ID from path: orcha.<session-id>.monitor
    let tree_path = format!("orcha.{}.monitor", session_id);
    let tree_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, tree_path.as_bytes());
    let monitor_tree_id = TreeId::from(tree_uuid);

    // Try to get existing tree, create if it doesn't exist
    let tree = match arbor_storage.tree_get(&monitor_tree_id).await {
        Ok(tree) => tree,
        Err(_) => {
            // Tree doesn't exist, create it with our deterministic ID
            let metadata = serde_json::json!({
                "type": "orcha_monitor",
                "session_id": session_id,
                "tree_path": tree_path
            });

            let created_tree_id = arbor_storage.tree_create_with_id(
                Some(monitor_tree_id),
                Some(metadata),
                "orcha",
            ).await.map_err(|e| e.to_string())?;

            arbor_storage.tree_get(&created_tree_id).await
                .map_err(|e| e.to_string())?
        }
    };

    // Find the latest summary node to append to, or use root
    let parent = tree.nodes.values()
        .filter(|n| matches!(n.data, crate::activations::arbor::NodeType::Text { .. }))
        .max_by_key(|n| n.created_at)
        .map(|n| n.id)
        .unwrap_or(tree.root);

    // Append summary as a text node with timestamp
    let timestamp = chrono::Utc::now().to_rfc3339();
    let summary_content = format!(
        "[{}] {}\n",
        timestamp,
        summary.trim()
    );

    arbor_storage.node_create_text(
        &tree.id,
        Some(parent),
        summary_content,
        None,
    ).await.map_err(|e| e.to_string())?;

    Ok(())
}

/// Generate summary for a single agent
async fn generate_agent_summary<P: HubContext>(
    claudecode: &ClaudeCode<P>,
    arbor_storage: &crate::activations::arbor::ArborStorage,
    agent: AgentInfo,
) -> Result<AgentSummary, String> {
    use futures::StreamExt;

    // Get conversation tree for this agent's ClaudeCode session
    let cc_session = claudecode.storage.session_get_by_name(&agent.claudecode_session_id).await
        .map_err(|e| format!("Failed to get CC session: {}", e))?;

    let tree = arbor_storage.tree_get(&cc_session.head.tree_id).await
        .map_err(|e| format!("Failed to get tree: {}", e))?;

    let conversation = format_conversation_from_tree(&tree);

    // Create ephemeral session to generate summary
    let summary_session = format!("orcha-agent-summary-{}", Uuid::new_v4());

    let mut create_stream = claudecode.create(
        summary_session.clone(),
        "/workspace".to_string(),
        crate::activations::claudecode::Model::Haiku,
        None,
        Some(false),
    ).await;
    tokio::pin!(create_stream);

    // Wait for creation
    let mut created = false;
    while let Some(result) = create_stream.next().await {
        if let crate::activations::claudecode::CreateResult::Ok { .. } = result {
            created = true;
            break;
        }
    }

    if !created {
        return Err("Failed to create summary session".to_string());
    }

    // Ask for summary
    let prompt = format!(
        "Summarize this agent's work in 2-3 sentences:\n\n\
         Subtask: {}\n\
         State: {:?}\n\n\
         Conversation:\n{}\n\n\
         Be concise and focus on what was accomplished or is in progress.",
        agent.subtask,
        agent.state,
        conversation
    );

    let chat_stream = claudecode.chat(summary_session, prompt, Some(true)).await;
    tokio::pin!(chat_stream);

    let mut summary = String::new();
    while let Some(event) = chat_stream.next().await {
        if let crate::activations::claudecode::ChatEvent::Content { text } = event {
            summary.push_str(&text);
        }
    }

    Ok(AgentSummary {
        agent_id: agent.agent_id,
        subtask: agent.subtask,
        state: agent.state,
        summary,
    })
}

/// Generate overall meta-summary combining all agent work
async fn generate_overall_summary<P: HubContext>(
    claudecode: &ClaudeCode<P>,
    session_id: &SessionId,
    agent_summaries: &[AgentSummary],
) -> Option<String> {
    use futures::StreamExt;

    let summary_session = format!("orcha-meta-summary-{}", Uuid::new_v4());

    // Create session
    let mut create_stream = claudecode.create(
        summary_session.clone(),
        "/workspace".to_string(),
        crate::activations::claudecode::Model::Haiku,
        None,
        Some(false),
    ).await;
    tokio::pin!(create_stream);

    let mut created = false;
    while let Some(result) = create_stream.next().await {
        if let crate::activations::claudecode::CreateResult::Ok { .. } = result {
            created = true;
            break;
        }
    }

    if !created {
        return None;
    }

    // Build prompt with all agent summaries
    let mut agent_list = String::new();
    for (i, summary) in agent_summaries.iter().enumerate() {
        agent_list.push_str(&format!(
            "{}. {} ({:?})\n   {}\n\n",
            i + 1,
            summary.subtask,
            summary.state,
            summary.summary
        ));
    }

    let prompt = format!(
        "This is a multi-agent orchestration session with {} agents working on different subtasks.\n\n\
         Agent summaries:\n{}\n\
         Provide a 2-4 sentence overall summary of the session's progress and coordination.\n\
         Focus on: what's the big picture? What's been accomplished? What's still in progress?",
        agent_summaries.len(),
        agent_list
    );

    let chat_stream = claudecode.chat(summary_session, prompt, Some(true)).await;
    tokio::pin!(chat_stream);

    let mut summary = String::new();
    while let Some(event) = chat_stream.next().await {
        if let crate::activations::claudecode::ChatEvent::Content { text } = event {
            summary.push_str(&text);
        }
    }

    Some(summary)
}

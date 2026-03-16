use super::{
    executor::{ClaudeCodeExecutor, LaunchConfig},
    sessions,
    storage::ClaudeCodeStorage,
    types::*,
};
use crate::activations::arbor::{NodeId, TreeId};
use crate::plexus::{HubContext, NoParent};
use async_stream::stream;
use futures::{Stream, StreamExt};
use plexus_macros::hub_methods;
use serde_json::Value;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};
use tracing::Instrument;

/// ClaudeCode activation - manages Claude Code sessions with Arbor-backed history
///
/// Generic over `P: HubContext` to allow different parent contexts:
/// - `Weak<DynamicHub>` when registered with a DynamicHub
/// - Custom context types for sub-hubs
/// - `NoParent` for standalone testing
#[derive(Clone)]
pub struct ClaudeCode<P: HubContext = NoParent> {
    pub storage: Arc<ClaudeCodeStorage>,
    executor: ClaudeCodeExecutor,
    /// Hub reference for resolving foreign handles when walking arbor trees
    hub: Arc<OnceLock<P>>,
    _phantom: PhantomData<P>,
}

impl<P: HubContext> ClaudeCode<P> {
    /// Create a new ClaudeCode with a specific parent context type
    pub fn with_context_type(storage: Arc<ClaudeCodeStorage>) -> Self {
        Self {
            storage,
            executor: ClaudeCodeExecutor::new(),
            hub: Arc::new(OnceLock::new()),
            _phantom: PhantomData,
        }
    }

    /// Create with custom executor and parent context type
    pub fn with_executor_and_context(storage: Arc<ClaudeCodeStorage>, executor: ClaudeCodeExecutor) -> Self {
        Self {
            storage,
            executor,
            hub: Arc::new(OnceLock::new()),
            _phantom: PhantomData,
        }
    }

    /// Inject parent context for resolving foreign handles
    ///
    /// Called during hub construction (e.g., via Arc::new_cyclic for DynamicHub).
    /// This allows ClaudeCode to resolve handles from other activations when walking arbor trees.
    pub fn inject_parent(&self, parent: P) {
        let _ = self.hub.set(parent);
    }

    /// Check if parent context has been injected
    pub fn has_parent(&self) -> bool {
        self.hub.get().is_some()
    }

    /// Get a reference to the parent context
    ///
    /// Returns None if inject_parent hasn't been called yet.
    pub fn parent(&self) -> Option<&P> {
        self.hub.get()
    }

    /// Resolve a claudecode handle to its message content
    ///
    /// Called by the macro-generated resolve_handle method.
    /// Handle format: {plugin_id}@1.0.0::chat:msg-{uuid}:{role}:{name}
    pub async fn resolve_handle_impl(
        &self,
        handle: &crate::types::Handle,
    ) -> Result<crate::plexus::PlexusStream, crate::plexus::PlexusError> {
        use crate::plexus::{PlexusError, wrap_stream};
        use async_stream::stream;

        let storage = self.storage.clone();

        // Join meta parts into colon-separated identifier
        // Format: "msg-{uuid}:{role}:{name}"
        if handle.meta.is_empty() {
            return Err(PlexusError::ExecutionError(
                "ClaudeCode handle missing message ID in meta".to_string()
            ));
        }
        let identifier = handle.meta.join(":");

        // Extract name from meta if present (for response)
        let name = handle.meta.get(2).cloned();

        let result_stream = stream! {
            match storage.resolve_message_handle(&identifier).await {
                Ok(message) => {
                    yield ResolveResult::Message {
                        id: message.id.to_string(),
                        role: message.role.as_str().to_string(),
                        content: message.content,
                        model: message.model_id,
                        name: name.unwrap_or_else(|| message.role.as_str().to_string()),
                    };
                }
                Err(e) => {
                    yield ResolveResult::Error {
                        message: format!("Failed to resolve handle: {}", e),
                    };
                }
            }
        };

        Ok(wrap_stream(result_stream, "claudecode.resolve_handle", vec!["claudecode".into()]))
    }
}

/// Convenience constructors for ClaudeCode with NoParent (standalone/testing)
impl ClaudeCode<NoParent> {
    pub fn new(storage: Arc<ClaudeCodeStorage>) -> Self {
        Self::with_context_type(storage)
    }

    pub fn with_executor(storage: Arc<ClaudeCodeStorage>, executor: ClaudeCodeExecutor) -> Self {
        Self::with_executor_and_context(storage, executor)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ARBOR EVENT CAPTURE HELPERS (Milestone 2)
// ═══════════════════════════════════════════════════════════════════════════

/// Create an arbor text node for a chat event
async fn create_event_node(
    arbor: &crate::activations::arbor::ArborStorage,
    tree_id: &crate::activations::arbor::TreeId,
    parent_id: &crate::activations::arbor::NodeId,
    event: &NodeEvent,
) -> Result<crate::activations::arbor::NodeId, String> {
    let json = serde_json::to_string(event)
        .map_err(|e| format!("Failed to serialize event: {}", e))?;

    arbor.node_create_text(tree_id, Some(*parent_id), json, None)
        .await
        .map_err(|e| e.to_string())
}

#[hub_methods(
    namespace = "claudecode",
    version = "1.0.0",
    description = "Manage Claude Code sessions with Arbor-backed conversation history",
    resolve_handle
)]
impl<P: HubContext> ClaudeCode<P> {
    /// Create a new Claude Code session
    #[plexus_macros::hub_method(params(
        name = "Human-readable name for the session",
        working_dir = "Working directory for Claude Code",
        model = "Model to use (opus, sonnet, haiku)",
        system_prompt = "Optional system prompt / instructions",
        loopback_enabled = "Enable loopback mode - routes tool permissions through parent for approval",
        loopback_session_id = "Session ID for loopback MCP URL correlation (e.g., orcha-xxx-claude-yyy)"
    ))]
    pub async fn create(
        &self,
        name: String,
        working_dir: String,
        model: Model,
        system_prompt: Option<String>,
        loopback_enabled: Option<bool>,
        loopback_session_id: Option<String>,
    ) -> impl Stream<Item = CreateResult> + Send + 'static {
        let storage = self.storage.clone();
        let loopback = loopback_enabled.unwrap_or(false);

        stream! {
            // Resolve relative paths to absolute before storing
            let working_dir = match std::fs::canonicalize(&working_dir) {
                Ok(p) => p.to_string_lossy().into_owned(),
                Err(e) => {
                    yield CreateResult::Err {
                        message: ClaudeCodeError::PathResolution {
                            path: working_dir,
                            source: e,
                        }.to_string(),
                    };
                    return;
                }
            };

            // Fail fast: if loopback is requested, the MCP server must be reachable.
            // Without it Claude cannot resolve the permission-prompt tool and will
            // return empty output instead of an error.
            if loopback {
                if let Err(e) = super::executor::check_mcp_reachable().await {
                    yield CreateResult::Err { message: e };
                    return;
                }
            }

            // claude_session_id is None initially; populated after first chat with real Claude UUID
            match storage.session_create(name, working_dir, model, system_prompt, None, loopback, None, loopback_session_id, None).await {
                Ok(config) => {
                    yield CreateResult::Ok {
                        id: config.id,
                        head: config.head,
                    };
                }
                Err(e) => {
                    yield CreateResult::Err { message: e.to_string() };
                }
            }
        }
    }

    /// Chat with a session, streaming tokens like Cone
    #[plexus_macros::hub_method(
        streaming,
        params(
            name = "Session name to chat with",
            prompt = "User message / prompt to send",
            ephemeral = "If true, creates nodes but doesn't advance head and marks for deletion",
            allowed_tools = "Optional list of tools to allow (e.g. [\"WebSearch\", \"Read\"])"
        )
    )]
    pub async fn chat(
        &self,
        name: String,
        prompt: String,
        ephemeral: Option<bool>,
        allowed_tools: Option<Vec<String>>,
    ) -> impl Stream<Item = ChatEvent> + Send + 'static {
        let storage = self.storage.clone();
        let executor = self.executor.clone();

        // Resolve before entering stream to avoid lifetime issues
        let resolve_result = storage.session_get_by_name(&name).await;

        stream! {
            let is_ephemeral = ephemeral.unwrap_or(false);

            // 1. Resolve and load session
            let config = match resolve_result {
                Ok(c) => c,
                Err(e) => {
                    yield ChatEvent::Err { message: e.to_string() };
                    return;
                }
            };

            let session_id = config.id;

            // 2. Store user message in our database (ephemeral if requested)
            let user_msg = if is_ephemeral {
                match storage.message_create_ephemeral(
                    &session_id,
                    MessageRole::User,
                    prompt.clone(),
                    None, None, None, None,
                ).await {
                    Ok(m) => m,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            } else {
                match storage.message_create(
                    &session_id,
                    MessageRole::User,
                    prompt.clone(),
                    None, None, None, None,
                ).await {
                    Ok(m) => m,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            };

            // 3. Create user message node in Arbor (ephemeral if requested)
            let user_handle = ClaudeCodeStorage::message_to_handle(&user_msg, "user");
            let user_node_id = if is_ephemeral {
                match storage.arbor().node_create_external_ephemeral(
                    &config.head.tree_id,
                    Some(config.head.node_id),
                    user_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            } else {
                match storage.arbor().node_create_external(
                    &config.head.tree_id,
                    Some(config.head.node_id),
                    user_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            };

            let user_position = Position::new(config.head.tree_id, user_node_id);

            // Track current parent for event node chain (Milestone 2)
            let mut current_parent = user_node_id;

            // Create UserMessage event node (Milestone 2)
            let user_event = NodeEvent::UserMessage { content: prompt.clone() };
            if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &user_event).await {
                current_parent = node_id;
            }

            // Create AssistantStart event node (Milestone 2)
            let start_event = NodeEvent::AssistantStart;
            if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &start_event).await {
                current_parent = node_id;
            }

            // 4. Emit Start
            yield ChatEvent::Start {
                id: session_id,
                user_position,
            };

            // 5. Build launch config
            let launch_config = LaunchConfig {
                query: prompt,
                // Use stored Claude UUID for --resume (None on first call = new session)
                session_id: config.claude_session_id.clone(),
                fork_session: false,
                model: config.model,
                working_dir: config.working_dir.clone(),
                system_prompt: config.system_prompt.clone(),
                mcp_config: config.mcp_config.clone(),
                loopback_enabled: config.loopback_enabled,
                // Use loopback_session_id for MCP URL correlation (e.g., orcha-xxx-claude-yyy)
                loopback_session_id: if config.loopback_enabled {
                    config.loopback_session_id.clone()
                } else {
                    None
                },
                allowed_tools: allowed_tools.unwrap_or_default(),
                ..Default::default()
            };

            // 6. Launch Claude and stream events
            let prev_claude_session_id = config.claude_session_id.clone();
            let mut response_content = String::new();
            let mut claude_session_id = config.claude_session_id.clone();
            let mut cost_usd = None;
            let mut num_turns = None;

            let mut raw_stream = executor.launch(launch_config).await;

            // Track current tool use for streaming tool input
            let mut current_tool_id: Option<String> = None;
            let mut current_tool_name: Option<String> = None;
            let mut current_tool_input = String::new();

            while let Some(event) = raw_stream.next().await {
                match event {
                    RawClaudeEvent::System { session_id: sid, .. } => {
                        if let Some(id) = sid {
                            claude_session_id = Some(id);
                        }
                    }
                    RawClaudeEvent::StreamEvent { event: inner, session_id: sid } => {
                        if let Some(id) = sid {
                            claude_session_id = Some(id);
                        }
                        match inner {
                            StreamEventInner::ContentBlockDelta { delta, .. } => {
                                match delta {
                                    StreamDelta::TextDelta { text } => {
                                        response_content.push_str(&text);

                                        // Create arbor node for text content (Milestone 2)
                                        let event = NodeEvent::ContentText { text: text.clone() };
                                        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                            current_parent = node_id;
                                        }

                                        yield ChatEvent::Content { text };
                                    }
                                    StreamDelta::InputJsonDelta { partial_json } => {
                                        current_tool_input.push_str(&partial_json);
                                    }
                                }
                            }
                            StreamEventInner::ContentBlockStart { content_block, .. } => {
                                if let Some(StreamContentBlock::ToolUse { id, name, .. }) = content_block {
                                    current_tool_id = Some(id);
                                    current_tool_name = Some(name);
                                    current_tool_input.clear();
                                }
                            }
                            StreamEventInner::ContentBlockStop { .. } => {
                                // Emit tool use if we were building one
                                if let (Some(id), Some(name)) = (current_tool_id.take(), current_tool_name.take()) {
                                    let input: Value = serde_json::from_str(&current_tool_input)
                                        .unwrap_or(Value::Object(serde_json::Map::new()));

                                    // Create arbor node for tool use (Milestone 2)
                                    let event = NodeEvent::ContentToolUse {
                                        id: id.clone(),
                                        name: name.clone(),
                                        input: input.clone(),
                                    };
                                    if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                        current_parent = node_id;
                                    }

                                    yield ChatEvent::ToolUse {
                                        tool_name: name,
                                        tool_use_id: id,
                                        input,
                                    };
                                    current_tool_input.clear();
                                }
                            }
                            _ => {}
                        }
                    }
                    RawClaudeEvent::Assistant { message } => {
                        // Still handle non-streaming assistant messages (tool results, etc.)
                        if let Some(msg) = message {
                            if let Some(content) = msg.content {
                                for block in content {
                                    match block {
                                        RawContentBlock::Text { text } => {
                                            // Only emit if we haven't already streamed this
                                            if response_content.is_empty() {
                                                response_content.push_str(&text);

                                                // Create arbor node for text content (Milestone 2)
                                                let event = NodeEvent::ContentText { text: text.clone() };
                                                if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                                    current_parent = node_id;
                                                }

                                                yield ChatEvent::Content { text };
                                            }
                                        }
                                        RawContentBlock::ToolUse { id, name, input } => {
                                            // Create arbor node for tool use (Milestone 2)
                                            let event = NodeEvent::ContentToolUse {
                                                id: id.clone(),
                                                name: name.clone(),
                                                input: input.clone(),
                                            };
                                            if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                                current_parent = node_id;
                                            }

                                            yield ChatEvent::ToolUse {
                                                tool_name: name,
                                                tool_use_id: id,
                                                input,
                                            };
                                        }
                                        RawContentBlock::ToolResult { tool_use_id, content, is_error } => {
                                            // Create arbor node for tool result (Milestone 2)
                                            let event = NodeEvent::UserToolResult {
                                                tool_use_id: tool_use_id.clone(),
                                                content: content.clone().unwrap_or_default(),
                                                is_error: is_error.unwrap_or(false),
                                            };
                                            if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                                current_parent = node_id;
                                            }

                                            yield ChatEvent::ToolResult {
                                                tool_use_id,
                                                output: content.unwrap_or_default(),
                                                is_error: is_error.unwrap_or(false),
                                            };
                                        }
                                        RawContentBlock::Thinking { thinking, .. } => {
                                            // Create arbor node for thinking (Milestone 2)
                                            let event = NodeEvent::ContentThinking { thinking: thinking.clone() };
                                            if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                                current_parent = node_id;
                                            }

                                            yield ChatEvent::Thinking { thinking };
                                        }
                                    }
                                }
                            }
                        }
                    }
                    RawClaudeEvent::Result {
                        session_id: sid,
                        cost_usd: cost,
                        num_turns: turns,
                        is_error,
                        error,
                        ..
                    } => {
                        if let Some(id) = sid {
                            claude_session_id = Some(id);
                        }
                        cost_usd = cost;
                        num_turns = turns;

                        // Check for error
                        if is_error == Some(true) {
                            if let Some(err_msg) = error {
                                yield ChatEvent::Err { message: err_msg };
                                return;
                            }
                        }
                    }
                    RawClaudeEvent::Unknown { event_type, data } => {
                        // Store unknown event and get handle
                        match storage.unknown_event_store(Some(&session_id), &event_type, &data).await {
                            Ok(handle) => {
                                tracing::debug!(event_type = %event_type, handle = %handle, "Unknown Claude event stored");
                                yield ChatEvent::Passthrough { event_type, handle, data };
                            }
                            Err(e) => {
                                tracing::warn!(event_type = %event_type, error = %e, "Failed to store unknown event");
                                // Still forward the event even if storage fails
                                yield ChatEvent::Passthrough {
                                    event_type,
                                    handle: "storage-failed".to_string(),
                                    data,
                                };
                            }
                        }
                    }
                    RawClaudeEvent::User { .. } => {
                        // User events are echoed back but we don't need to process them
                    }
                    RawClaudeEvent::LaunchCommand { command } => {
                        let event = NodeEvent::LaunchCommand { command };
                        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                            current_parent = node_id;
                        }
                    }
                    RawClaudeEvent::Stderr { text } => {
                        tracing::warn!(stderr = %text, "Claude stderr");
                        let event = NodeEvent::ClaudeStderr { text };
                        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                            current_parent = node_id;
                        }
                    }
                }
            }

            // 6b. If we captured a new Claude session UUID, persist it for future --resume
            if let Some(ref new_id) = claude_session_id {
                if prev_claude_session_id.as_deref() != Some(new_id.as_str()) {
                    let _ = storage.session_update_claude_id(&session_id, new_id.clone()).await;
                }
            }

            // Guard: if stream produced nothing, emit error instead of ghost Complete
            if response_content.is_empty() && claude_session_id.is_none() {
                yield ChatEvent::Err {
                    message: "Claude process produced no response. Check substrate logs for details.".to_string(),
                };
                return;
            }

            // 7. Store assistant response (ephemeral if requested)
            let model_id = format!("claude-code-{}", config.model.as_str());
            let assistant_msg = if is_ephemeral {
                match storage.message_create_ephemeral(
                    &session_id,
                    MessageRole::Assistant,
                    response_content,
                    Some(model_id),
                    None,
                    None,
                    cost_usd,
                ).await {
                    Ok(m) => m,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            } else {
                match storage.message_create(
                    &session_id,
                    MessageRole::Assistant,
                    response_content,
                    Some(model_id),
                    None,
                    None,
                    cost_usd,
                ).await {
                    Ok(m) => m,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            };

            // Create AssistantComplete event node (Milestone 2)
            let complete_event = NodeEvent::AssistantComplete { usage: None };
            if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &complete_event).await {
                current_parent = node_id;
            }

            // 8. Create assistant node in Arbor (ephemeral if requested)
            let assistant_handle = ClaudeCodeStorage::message_to_handle(&assistant_msg, "assistant");
            let assistant_node_id = if is_ephemeral {
                match storage.arbor().node_create_external_ephemeral(
                    &config.head.tree_id,
                    Some(current_parent),
                    assistant_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            } else {
                match storage.arbor().node_create_external(
                    &config.head.tree_id,
                    Some(current_parent),
                    assistant_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Err { message: e.to_string() };
                        return;
                    }
                }
            };

            let new_head = Position::new(config.head.tree_id, assistant_node_id);

            // 9. Update session head and Claude session ID (skip for ephemeral)
            if !is_ephemeral {
                if let Err(e) = storage.session_update_head(&session_id, assistant_node_id, claude_session_id.clone()).await {
                    yield ChatEvent::Err { message: e.to_string() };
                    return;
                }
            }

            // 10. Emit Complete
            // For ephemeral, new_head points to the ephemeral node (not the session's actual head)
            yield ChatEvent::Complete {
                new_head: if is_ephemeral { config.head } else { new_head },
                claude_session_id: claude_session_id.unwrap_or_default(),
                usage: Some(ChatUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cost_usd,
                    num_turns,
                }),
            };
        }
    }

    /// Get session configuration details
    #[plexus_macros::hub_method]
    async fn get(&self, name: String) -> impl Stream<Item = GetResult> + Send + 'static {
        let result = self.storage.session_get_by_name(&name).await;

        stream! {
            match result {
                Ok(config) => {
                    yield GetResult::Ok { config };
                }
                Err(e) => {
                    yield GetResult::Err { message: e.to_string() };
                }
            }
        }
    }

    /// List all Claude Code sessions
    #[plexus_macros::hub_method]
    async fn list(&self) -> impl Stream<Item = ListResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.session_list().await {
                Ok(sessions) => {
                    yield ListResult::Ok { sessions };
                }
                Err(e) => {
                    yield ListResult::Err { message: e.to_string() };
                }
            }
        }
    }

    /// Delete a session
    #[plexus_macros::hub_method]
    async fn delete(&self, name: String) -> impl Stream<Item = DeleteResult> + Send + 'static {
        let storage = self.storage.clone();
        let resolve_result = storage.session_get_by_name(&name).await;

        stream! {
            let config = match resolve_result {
                Ok(c) => c,
                Err(e) => {
                    yield DeleteResult::Err { message: e.to_string() };
                    return;
                }
            };

            match storage.session_delete(&config.id).await {
                Ok(_) => {
                    yield DeleteResult::Ok { id: config.id };
                }
                Err(e) => {
                    yield DeleteResult::Err { message: e.to_string() };
                }
            }
        }
    }

    /// Fork a session to create a branch point
    #[plexus_macros::hub_method]
    async fn fork(
        &self,
        name: String,
        new_name: String,
    ) -> impl Stream<Item = ForkResult> + Send + 'static {
        let storage = self.storage.clone();
        let resolve_result = storage.session_get_by_name(&name).await;

        stream! {
            // Get parent session
            let parent = match resolve_result {
                Ok(c) => c,
                Err(e) => {
                    yield ForkResult::Err { message: e.to_string() };
                    return;
                }
            };

            // Create new session starting at parent's head position
            // The new session will fork Claude's session on first chat
            let new_config = match storage.session_create(
                new_name,
                parent.working_dir.clone(),
                parent.model,
                parent.system_prompt.clone(),
                parent.mcp_config.clone(),
                parent.loopback_enabled,
                None, // claude_session_id - will be set on first chat with fork_session=true
                None, // loopback_session_id
                None, // metadata
            ).await {
                Ok(mut c) => {
                    // Update head to parent's position (share the same tree point)
                    // This creates a branch - the new session diverges from here
                    if let Err(e) = storage.session_update_head(&c.id, parent.head.node_id, None).await {
                        yield ForkResult::Err { message: e.to_string() };
                        return;
                    }
                    c.head = parent.head;
                    c
                }
                Err(e) => {
                    yield ForkResult::Err { message: e.to_string() };
                    return;
                }
            };

            yield ForkResult::Ok {
                id: new_config.id,
                head: new_config.head,
            };
        }
    }

    /// Start an async chat - returns immediately with stream_id for polling
    ///
    /// This is the non-blocking version of chat, designed for loopback scenarios
    /// where the parent needs to poll for events and handle tool approvals.
    #[plexus_macros::hub_method(
        params(
            name = "Session name to chat with",
            prompt = "User message / prompt to send",
            ephemeral = "If true, creates nodes but doesn't advance head and marks for deletion"
        )
    )]
    async fn chat_async(
        &self,
        name: String,
        prompt: String,
        ephemeral: Option<bool>,
    ) -> impl Stream<Item = ChatStartResult> + Send + 'static {
        let storage = self.storage.clone();
        let executor = self.executor.clone();

        // Resolve session before entering stream
        let resolve_result = storage.session_get_by_name(&name).await;

        stream! {
            let is_ephemeral = ephemeral.unwrap_or(false);

            // 1. Resolve session
            let config = match resolve_result {
                Ok(c) => c,
                Err(e) => {
                    yield ChatStartResult::Err { message: e.to_string() };
                    return;
                }
            };

            let session_id = config.id;

            // 2. Create stream buffer
            let stream_id = match storage.stream_create(session_id).await {
                Ok(id) => id,
                Err(e) => {
                    yield ChatStartResult::Err { message: e.to_string() };
                    return;
                }
            };

            // 3. Spawn background task to run the chat
            let storage_bg = storage.clone();
            let executor_bg = executor.clone();
            let prompt_bg = prompt.clone();
            let config_bg = config.clone();
            let stream_id_bg = stream_id;

            tokio::spawn(async move {
                Self::run_chat_background(
                    storage_bg,
                    executor_bg,
                    config_bg,
                    prompt_bg,
                    is_ephemeral,
                    stream_id_bg,
                ).await;
            }.instrument(tracing::info_span!("chat_async_bg", stream_id = %stream_id)));

            // 4. Return immediately with stream_id
            yield ChatStartResult::Ok {
                stream_id,
                session_id,
            };
        }
    }

    /// Poll a stream for new events
    ///
    /// Returns events since the last poll (or from the specified offset).
    /// Use this to read events from an async chat started with chat_async.
    #[plexus_macros::hub_method(
        params(
            stream_id = "Stream ID returned from chat_async",
            from_seq = "Optional: start reading from this sequence number",
            limit = "Optional: max events to return (default 100)"
        )
    )]
    async fn poll(
        &self,
        stream_id: StreamId,
        from_seq: Option<u64>,
        limit: Option<u64>,
    ) -> impl Stream<Item = PollResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            let limit_usize = limit.map(|l| l as usize);

            match storage.stream_poll(&stream_id, from_seq, limit_usize).await {
                Ok((info, events)) => {
                    let has_more = info.read_position < info.event_count;
                    yield PollResult::Ok {
                        status: info.status,
                        events,
                        read_position: info.read_position,
                        total_events: info.event_count,
                        has_more,
                    };
                }
                Err(e) => {
                    yield PollResult::Err { message: e.to_string() };
                }
            }
        }
    }

    /// List active streams
    ///
    /// Returns all active streams, optionally filtered by session.
    #[plexus_macros::hub_method(
        params(
            session_id = "Optional: filter by session ID"
        )
    )]
    async fn streams(
        &self,
        session_id: Option<ClaudeCodeId>,
    ) -> impl Stream<Item = StreamListResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            let streams = if let Some(sid) = session_id {
                storage.stream_list_for_session(&sid).await
            } else {
                storage.stream_list().await
            };

            yield StreamListResult::Ok { streams };
        }
    }

    /// Get arbor tree information for a session
    #[plexus_macros::hub_method(params(
        name = "Session name"
    ))]
    async fn get_tree(
        &self,
        name: String,
    ) -> impl Stream<Item = GetTreeResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            let config = match storage.session_get_by_name(&name).await {
                Ok(c) => c,
                Err(e) => {
                    yield GetTreeResult::Err { message: e.to_string() };
                    return;
                }
            };

            yield GetTreeResult::Ok {
                tree_id: config.head.tree_id,
                head: config.head.node_id,
            };
        }
    }

    /// Render arbor tree as Claude API messages
    #[plexus_macros::hub_method(params(
        name = "Session name",
        start = "Optional start node (default: root)",
        end = "Optional end node (default: head)"
    ))]
    async fn render_context(
        &self,
        name: String,
        start: Option<NodeId>,
        end: Option<NodeId>,
    ) -> impl Stream<Item = RenderResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Get session config
            let config = match storage.session_get_by_name(&name).await {
                Ok(c) => c,
                Err(e) => {
                    yield RenderResult::Err { message: e.to_string() };
                    return;
                }
            };

            let tree_id = config.head.tree_id;
            let end_node = end.unwrap_or(config.head.node_id);

            // Get root if start not specified
            let start_node = if let Some(s) = start {
                s
            } else {
                match storage.arbor().tree_get(&tree_id).await {
                    Ok(tree) => tree.root,
                    Err(e) => {
                        yield RenderResult::Err { message: e.to_string() };
                        return;
                    }
                }
            };

            // Render messages
            let messages = match storage.render_messages(&tree_id, &start_node, &end_node).await {
                Ok(m) => m,
                Err(e) => {
                    yield RenderResult::Err { message: e.to_string() };
                    return;
                }
            };

            yield RenderResult::Ok { messages };
        }
    }

    /// List all session files for a project
    #[plexus_macros::hub_method(params(
        project_path = "Project path (e.g., '-workspace-hypermemetic-hub-codegen')"
    ))]
    async fn sessions_list(
        &self,
        project_path: String,
    ) -> impl Stream<Item = SessionsListResult> + Send + 'static {
        stream! {
            match sessions::list_sessions(&project_path).await {
                Ok(sessions) => {
                    yield SessionsListResult::Ok { sessions };
                }
                Err(e) => {
                    yield SessionsListResult::Err { message: e };
                }
            }
        }
    }

    /// Get events from a session file
    #[plexus_macros::hub_method(params(
        project_path = "Project path",
        session_id = "Session ID (UUID)"
    ))]
    async fn sessions_get(
        &self,
        project_path: String,
        session_id: String,
    ) -> impl Stream<Item = SessionsGetResult> + Send + 'static {
        stream! {
            match sessions::read_session(&project_path, &session_id).await {
                Ok(events) => {
                    let event_count = events.len();
                    // Convert to JSON values for transport
                    let events_json: Vec<serde_json::Value> = events.into_iter()
                        .filter_map(|e| serde_json::to_value(e).ok())
                        .collect();

                    yield SessionsGetResult::Ok {
                        session_id: session_id.clone(),
                        event_count,
                        events: events_json,
                    };
                }
                Err(e) => {
                    yield SessionsGetResult::Err { message: e };
                }
            }
        }
    }

    /// Import a session file into arbor
    #[plexus_macros::hub_method(params(
        project_path = "Project path",
        session_id = "Session ID to import",
        owner_id = "Owner ID for the new tree (default: 'claudecode')"
    ))]
    async fn sessions_import(
        &self,
        project_path: String,
        session_id: String,
        owner_id: Option<String>,
    ) -> impl Stream<Item = SessionsImportResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            let owner = owner_id.unwrap_or_else(|| "claudecode".to_string());

            match sessions::import_to_arbor(storage.arbor(), &project_path, &session_id, &owner).await {
                Ok(tree_id) => {
                    yield SessionsImportResult::Ok {
                        tree_id,
                        session_id,
                    };
                }
                Err(e) => {
                    yield SessionsImportResult::Err { message: e };
                }
            }
        }
    }

    /// Export an arbor tree to a session file
    #[plexus_macros::hub_method(params(
        tree_id = "Arbor tree ID to export",
        project_path = "Project path",
        session_id = "Session ID for the exported file"
    ))]
    async fn sessions_export(
        &self,
        tree_id: TreeId,
        project_path: String,
        session_id: String,
    ) -> impl Stream<Item = SessionsExportResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match sessions::export_from_arbor(storage.arbor(), &tree_id, &project_path, &session_id).await {
                Ok(()) => {
                    yield SessionsExportResult::Ok {
                        tree_id,
                        session_id,
                    };
                }
                Err(e) => {
                    yield SessionsExportResult::Err { message: e };
                }
            }
        }
    }

    /// Delete a session file
    #[plexus_macros::hub_method(params(
        project_path = "Project path",
        session_id = "Session ID to delete"
    ))]
    async fn sessions_delete(
        &self,
        project_path: String,
        session_id: String,
    ) -> impl Stream<Item = SessionsDeleteResult> + Send + 'static {
        stream! {
            match sessions::delete_session(&project_path, &session_id).await {
                Ok(()) => {
                    yield SessionsDeleteResult::Ok {
                        session_id,
                        deleted: true,
                    };
                }
                Err(e) => {
                    yield SessionsDeleteResult::Err { message: e };
                }
            }
        }
    }
}

// Background task implementation (outside the hub_methods block)
impl<P: HubContext> ClaudeCode<P> {
    /// Run chat in background, pushing events to stream buffer
    async fn run_chat_background(
        storage: Arc<ClaudeCodeStorage>,
        executor: ClaudeCodeExecutor,
        config: ClaudeCodeConfig,
        prompt: String,
        is_ephemeral: bool,
        stream_id: StreamId,
    ) {
        let session_id = config.id;

        // 1. Store user message
        let user_msg = if is_ephemeral {
            match storage.message_create_ephemeral(
                &session_id,
                MessageRole::User,
                prompt.clone(),
                None, None, None, None,
            ).await {
                Ok(m) => m,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        } else {
            match storage.message_create(
                &session_id,
                MessageRole::User,
                prompt.clone(),
                None, None, None, None,
            ).await {
                Ok(m) => m,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        };

        // 2. Create user node in Arbor
        let user_handle = ClaudeCodeStorage::message_to_handle(&user_msg, "user");
        let user_node_id = if is_ephemeral {
            match storage.arbor().node_create_external_ephemeral(
                &config.head.tree_id,
                Some(config.head.node_id),
                user_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        } else {
            match storage.arbor().node_create_external(
                &config.head.tree_id,
                Some(config.head.node_id),
                user_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        };

        let user_position = Position::new(config.head.tree_id, user_node_id);

        // Track current parent for event node chain (Milestone 2)
        let mut current_parent = user_node_id;

        // Create UserMessage event node (Milestone 2)
        let user_event = NodeEvent::UserMessage { content: prompt.clone() };
        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &user_event).await {
            current_parent = node_id;
        }

        // Create AssistantStart event node (Milestone 2)
        let start_event = NodeEvent::AssistantStart;
        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &start_event).await {
            current_parent = node_id;
        }

        // Update stream with user position
        if let Err(e) = storage.stream_set_user_position(&stream_id, user_position).await {
            tracing::error!(stream_id = %stream_id, error = %e, "Failed to set user position on stream");
        }

        // 3. Push Start event
        if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Start {
            id: session_id,
            user_position,
        }).await {
            tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
        }

        // 4. Build launch config
        let launch_config = LaunchConfig {
            query: prompt,
            session_id: config.claude_session_id.clone(),
            fork_session: false,
            model: config.model,
            working_dir: config.working_dir.clone(),
            system_prompt: config.system_prompt.clone(),
            mcp_config: config.mcp_config.clone(),
            loopback_enabled: config.loopback_enabled,
            // Use claude_session_id for MCP URL transparency (e.g., orcha-xxx)
            loopback_session_id: if config.loopback_enabled {
                config.claude_session_id.clone()
            } else {
                None
            },
            ..Default::default()
        };

        // 5. Launch Claude and stream events to buffer
        let mut response_content = String::new();
        let mut claude_session_id = config.claude_session_id.clone();
        let mut cost_usd = None;
        let mut num_turns = None;

        let mut raw_stream = executor.launch(launch_config).await;

        // Track current tool use for streaming
        let mut current_tool_id: Option<String> = None;
        let mut current_tool_name: Option<String> = None;
        let mut current_tool_input = String::new();

        while let Some(event) = raw_stream.next().await {
            match event {
                RawClaudeEvent::System { session_id: sid, .. } => {
                    if let Some(id) = sid {
                        claude_session_id = Some(id);
                    }
                }
                RawClaudeEvent::StreamEvent { event: inner, session_id: sid } => {
                    if let Some(id) = sid {
                        claude_session_id = Some(id);
                    }
                    match inner {
                        StreamEventInner::ContentBlockDelta { delta, .. } => {
                            match delta {
                                StreamDelta::TextDelta { text } => {
                                    response_content.push_str(&text);

                                    // Create arbor node for text content (Milestone 2)
                                    let event = NodeEvent::ContentText { text: text.clone() };
                                    if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                        current_parent = node_id;
                                    }

                                    if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Content { text }).await {
                                        tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                                    }
                                }
                                StreamDelta::InputJsonDelta { partial_json } => {
                                    current_tool_input.push_str(&partial_json);
                                }
                            }
                        }
                        StreamEventInner::ContentBlockStart { content_block, .. } => {
                            if let Some(StreamContentBlock::ToolUse { id, name, .. }) = content_block {
                                current_tool_id = Some(id);
                                current_tool_name = Some(name);
                                current_tool_input.clear();
                            }
                        }
                        StreamEventInner::ContentBlockStop { .. } => {
                            if let (Some(id), Some(name)) = (current_tool_id.take(), current_tool_name.take()) {
                                let input: Value = serde_json::from_str(&current_tool_input)
                                    .unwrap_or(Value::Object(serde_json::Map::new()));

                                // Check if this is a loopback_permit call (tool waiting for approval)
                                if name == "mcp__plexus__loopback_permit" {
                                    if let Err(e) = storage.stream_set_status(&stream_id, StreamStatus::AwaitingPermission, None).await {
                                        tracing::error!(stream_id = %stream_id, error = %e, "Failed to update stream status");
                                    }
                                }

                                // Create arbor node for tool use (Milestone 2)
                                let event = NodeEvent::ContentToolUse {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                };
                                if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                    current_parent = node_id;
                                }

                                if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::ToolUse {
                                    tool_name: name,
                                    tool_use_id: id,
                                    input,
                                }).await {
                                    tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                                }
                                current_tool_input.clear();
                            }
                        }
                        StreamEventInner::MessageDelta { delta } => {
                            // If stop_reason is tool_use with loopback, mark as awaiting
                            if delta.stop_reason == Some("tool_use".to_string()) {
                                // Check if we're in loopback mode (already marked above)
                            }
                        }
                        _ => {}
                    }
                }
                RawClaudeEvent::Assistant { message } => {
                    if let Some(msg) = message {
                        if let Some(content) = msg.content {
                            for block in content {
                                match block {
                                    RawContentBlock::Text { text } => {
                                        if response_content.is_empty() {
                                            response_content.push_str(&text);

                                            // Create arbor node for text content (Milestone 2)
                                            let event = NodeEvent::ContentText { text: text.clone() };
                                            if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                                current_parent = node_id;
                                            }

                                            if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Content { text }).await {
                                                tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                                            }
                                        }
                                    }
                                    RawContentBlock::ToolUse { id, name, input } => {
                                        // Create arbor node for tool use (Milestone 2)
                                        let event = NodeEvent::ContentToolUse {
                                            id: id.clone(),
                                            name: name.clone(),
                                            input: input.clone(),
                                        };
                                        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                            current_parent = node_id;
                                        }

                                        if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::ToolUse {
                                            tool_name: name,
                                            tool_use_id: id,
                                            input,
                                        }).await {
                                            tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                                        }
                                    }
                                    RawContentBlock::ToolResult { tool_use_id, content, is_error } => {
                                        // Tool completed - back to running if was awaiting
                                        if let Err(e) = storage.stream_set_status(&stream_id, StreamStatus::Running, None).await {
                                            tracing::error!(stream_id = %stream_id, error = %e, "Failed to update stream status");
                                        }

                                        // Create arbor node for tool result (Milestone 2)
                                        let event = NodeEvent::UserToolResult {
                                            tool_use_id: tool_use_id.clone(),
                                            content: content.clone().unwrap_or_default(),
                                            is_error: is_error.unwrap_or(false),
                                        };
                                        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                            current_parent = node_id;
                                        }

                                        if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::ToolResult {
                                            tool_use_id,
                                            output: content.unwrap_or_default(),
                                            is_error: is_error.unwrap_or(false),
                                        }).await {
                                            tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                                        }
                                    }
                                    RawContentBlock::Thinking { thinking, .. } => {
                                        // Create arbor node for thinking (Milestone 2)
                                        let event = NodeEvent::ContentThinking { thinking: thinking.clone() };
                                        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                                            current_parent = node_id;
                                        }

                                        if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Thinking { thinking }).await {
                                            tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                RawClaudeEvent::Result {
                    session_id: sid,
                    cost_usd: cost,
                    num_turns: turns,
                    is_error,
                    error,
                    ..
                } => {
                    if let Some(id) = sid {
                        claude_session_id = Some(id);
                    }
                    cost_usd = cost;
                    num_turns = turns;

                    if is_error == Some(true) {
                        if let Some(err_msg) = error {
                            if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: err_msg.clone() }).await {
                                tracing::error!(stream_id = %stream_id, error = %e, "Failed to push error event to stream");
                            }
                            if let Err(e) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(err_msg)).await {
                                tracing::error!(stream_id = %stream_id, error = %e, "Failed to update stream status to Failed");
                            }
                            return;
                        }
                    }
                }
                RawClaudeEvent::Unknown { event_type, data } => {
                    match storage.unknown_event_store(Some(&session_id), &event_type, &data).await {
                        Ok(handle) => {
                            if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Passthrough { event_type, handle, data }).await {
                                tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                            }
                        }
                        Err(_) => {
                            if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Passthrough {
                                event_type,
                                handle: "storage-failed".to_string(),
                                data,
                            }).await {
                                tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
                            }
                        }
                    }
                }
                RawClaudeEvent::User { .. } => {}
                RawClaudeEvent::LaunchCommand { command } => {
                    let event = NodeEvent::LaunchCommand { command };
                    if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                        current_parent = node_id;
                    }
                }
                RawClaudeEvent::Stderr { text } => {
                    tracing::warn!(stderr = %text, "Claude stderr");
                    let event = NodeEvent::ClaudeStderr { text };
                    if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &event).await {
                        current_parent = node_id;
                    }
                }
            }
        }

        // 6. Store assistant response
        let model_id = format!("claude-code-{}", config.model.as_str());
        let assistant_msg = if is_ephemeral {
            match storage.message_create_ephemeral(
                &session_id,
                MessageRole::Assistant,
                response_content,
                Some(model_id),
                None,
                None,
                cost_usd,
            ).await {
                Ok(m) => m,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        } else {
            match storage.message_create(
                &session_id,
                MessageRole::Assistant,
                response_content,
                Some(model_id),
                None,
                None,
                cost_usd,
            ).await {
                Ok(m) => m,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        };

        // Create AssistantComplete event node (Milestone 2)
        let complete_event = NodeEvent::AssistantComplete { usage: None };
        if let Ok(node_id) = create_event_node(storage.arbor(), &config.head.tree_id, &current_parent, &complete_event).await {
            current_parent = node_id;
        }

        // 7. Create assistant node in Arbor
        let assistant_handle = ClaudeCodeStorage::message_to_handle(&assistant_msg, "assistant");
        let assistant_node_id = if is_ephemeral {
            match storage.arbor().node_create_external_ephemeral(
                &config.head.tree_id,
                Some(current_parent),
                assistant_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        } else {
            match storage.arbor().node_create_external(
                &config.head.tree_id,
                Some(current_parent),
                assistant_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                    }
                    if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                        tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                    }
                    return;
                }
            }
        };

        let new_head = Position::new(config.head.tree_id, assistant_node_id);

        // 8. Update session head (skip for ephemeral)
        if !is_ephemeral {
            if let Err(e) = storage.session_update_head(&session_id, assistant_node_id, claude_session_id.clone()).await {
                if let Err(e2) = storage.stream_push_event(&stream_id, ChatEvent::Err { message: e.to_string() }).await {
                    tracing::error!(stream_id = %stream_id, error = %e2, "Failed to push error event to stream");
                }
                if let Err(e2) = storage.stream_set_status(&stream_id, StreamStatus::Failed, Some(e.to_string())).await {
                    tracing::error!(stream_id = %stream_id, error = %e2, "Failed to update stream status to Failed");
                }
                return;
            }
        }

        // 9. Push Complete event and mark stream as complete
        if let Err(e) = storage.stream_push_event(&stream_id, ChatEvent::Complete {
            new_head: if is_ephemeral { config.head } else { new_head },
            claude_session_id: claude_session_id.unwrap_or_default(),
            usage: Some(ChatUsage {
                input_tokens: None,
                output_tokens: None,
                cost_usd,
                num_turns,
            }),
        }).await {
            tracing::error!(stream_id = %stream_id, error = %e, "Failed to push event to stream");
        }

        if let Err(e) = storage.stream_set_status(&stream_id, StreamStatus::Complete, None).await {
            tracing::error!(stream_id = %stream_id, error = %e, "Failed to update stream status");
        }
    }
}

use super::{
    executor::{ClaudeCodeExecutor, LaunchConfig},
    storage::ClaudeCodeStorage,
    types::*,
};
use crate::plexus::{HubContext, NoParent};
use async_stream::stream;
use futures::{Stream, StreamExt};
use hub_macro::hub_methods;
use serde_json::Value;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

/// ClaudeCode activation - manages Claude Code sessions with Arbor-backed history
///
/// Generic over `P: HubContext` to allow different parent contexts:
/// - `Weak<Plexus>` when registered with a Plexus hub
/// - Custom context types for sub-hubs
/// - `NoParent` for standalone testing
#[derive(Clone)]
pub struct ClaudeCode<P: HubContext = NoParent> {
    storage: Arc<ClaudeCodeStorage>,
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
    /// Called during hub construction (e.g., via Arc::new_cyclic for Plexus).
    /// This allows ClaudeCode to resolve handles from other plugins when walking arbor trees.
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

#[hub_methods(
    namespace = "claudecode",
    version = "1.0.0",
    description = "Manage Claude Code sessions with Arbor-backed conversation history"
)]
impl<P: HubContext> ClaudeCode<P> {
    /// Create a new Claude Code session
    #[hub_macro::hub_method]
    async fn create(
        &self,
        name: String,
        working_dir: String,
        model: Model,
        system_prompt: Option<String>,
    ) -> impl Stream<Item = CreateResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.session_create(name, working_dir, model, system_prompt, None, None).await {
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
    #[hub_macro::hub_method(
        streaming,
        params(
            name = "Session name to chat with",
            prompt = "User message / prompt to send",
            ephemeral = "If true, creates nodes but doesn't advance head and marks for deletion"
        )
    )]
    async fn chat(
        &self,
        name: String,
        prompt: String,
        ephemeral: Option<bool>,
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

            // 4. Emit Start
            yield ChatEvent::Start {
                id: session_id,
                user_position,
            };

            // 5. Build launch config
            let launch_config = LaunchConfig {
                query: prompt,
                session_id: config.claude_session_id.clone(),
                fork_session: false,
                model: config.model,
                working_dir: config.working_dir.clone(),
                system_prompt: config.system_prompt.clone(),
                mcp_config: config.mcp_config.clone(),
                ..Default::default()
            };

            // 6. Launch Claude and stream events
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
                                                yield ChatEvent::Content { text };
                                            }
                                        }
                                        RawContentBlock::ToolUse { id, name, input } => {
                                            yield ChatEvent::ToolUse {
                                                tool_name: name,
                                                tool_use_id: id,
                                                input,
                                            };
                                        }
                                        RawContentBlock::ToolResult { tool_use_id, content, is_error } => {
                                            yield ChatEvent::ToolResult {
                                                tool_use_id,
                                                output: content.unwrap_or_default(),
                                                is_error: is_error.unwrap_or(false),
                                            };
                                        }
                                        RawContentBlock::Thinking { thinking, .. } => {
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
                }
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

            // 8. Create assistant node in Arbor (ephemeral if requested)
            let assistant_handle = ClaudeCodeStorage::message_to_handle(&assistant_msg, "assistant");
            let assistant_node_id = if is_ephemeral {
                match storage.arbor().node_create_external_ephemeral(
                    &config.head.tree_id,
                    Some(user_node_id),
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
                    Some(user_node_id),
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
    #[hub_macro::hub_method]
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
    #[hub_macro::hub_method]
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
    #[hub_macro::hub_method]
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
    #[hub_macro::hub_method]
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
                None,
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
}

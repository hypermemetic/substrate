use super::{
    executor::{ClaudeCodeExecutor, LaunchConfig},
    storage::ClaudeCodeStorage,
    types::*,
};
use async_stream::stream;
use futures::{Stream, StreamExt};
use hub_macro::hub_methods;
use serde_json::Value;
use std::sync::Arc;

/// ClaudeCode activation - manages Claude Code sessions with Arbor-backed history
#[derive(Clone)]
pub struct ClaudeCode {
    storage: Arc<ClaudeCodeStorage>,
    executor: ClaudeCodeExecutor,
}

impl ClaudeCode {
    pub fn new(storage: Arc<ClaudeCodeStorage>) -> Self {
        Self {
            storage,
            executor: ClaudeCodeExecutor::new(),
        }
    }

    pub fn with_executor(storage: Arc<ClaudeCodeStorage>, executor: ClaudeCodeExecutor) -> Self {
        Self { storage, executor }
    }
}

#[hub_methods(
    namespace = "claudecode",
    version = "1.0.0",
    description = "Manage Claude Code sessions with Arbor-backed conversation history"
)]
impl ClaudeCode {
    /// Create a new Claude Code session
    #[hub_macro::hub_method]
    async fn create(
        &self,
        name: String,
        working_dir: String,
        model: Model,
        system_prompt: Option<String>,
    ) -> impl Stream<Item = ClaudeCodeEvent> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.session_create(name, working_dir, model, system_prompt, None, None).await {
                Ok(config) => {
                    yield ClaudeCodeEvent::Created {
                        claudecode_id: config.id,
                        head: config.head,
                        claude_session_id: None,
                    };
                }
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                }
            }
        }
    }

    /// Chat with a session, streaming tokens like Cone
    #[hub_macro::hub_method]
    async fn chat(
        &self,
        name: String,
        prompt: String,
    ) -> impl Stream<Item = ClaudeCodeEvent> + Send + 'static {
        let storage = self.storage.clone();
        let executor = self.executor.clone();

        // Resolve before entering stream to avoid lifetime issues
        let resolve_result = storage.session_get_by_name(&name).await;

        stream! {
            // 1. Resolve and load session
            let config = match resolve_result {
                Ok(c) => c,
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                    return;
                }
            };

            let session_id = config.id;

            // 2. Store user message in our database
            let user_msg = match storage.message_create(
                &session_id,
                MessageRole::User,
                prompt.clone(),
                None, None, None, None,
            ).await {
                Ok(m) => m,
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                    return;
                }
            };

            // 3. Create user message node in Arbor
            let user_handle = ClaudeCodeStorage::message_to_handle(&user_msg, "user");
            let user_node_id = match storage.arbor().node_create_external(
                &config.head.tree_id,
                Some(config.head.node_id),
                user_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                    return;
                }
            };

            let user_position = Position::new(config.head.tree_id, user_node_id);

            // 4. Emit ChatStart
            yield ClaudeCodeEvent::ChatStart {
                claudecode_id: session_id,
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
                                        yield ClaudeCodeEvent::ChatContent {
                                            claudecode_id: session_id,
                                            content: text,
                                        };
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
                                    yield ClaudeCodeEvent::ChatToolUse {
                                        claudecode_id: session_id,
                                        tool_name: name,
                                        tool_use_id: id,
                                        tool_input: input,
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
                                                yield ClaudeCodeEvent::ChatContent {
                                                    claudecode_id: session_id,
                                                    content: text,
                                                };
                                            }
                                        }
                                        RawContentBlock::ToolUse { id, name, input } => {
                                            yield ClaudeCodeEvent::ChatToolUse {
                                                claudecode_id: session_id,
                                                tool_name: name,
                                                tool_use_id: id,
                                                tool_input: input,
                                            };
                                        }
                                        RawContentBlock::ToolResult { tool_use_id, content, is_error } => {
                                            yield ClaudeCodeEvent::ChatToolResult {
                                                claudecode_id: session_id,
                                                tool_use_id,
                                                output: content.unwrap_or_default(),
                                                is_error: is_error.unwrap_or(false),
                                            };
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
                                yield ClaudeCodeEvent::Error { message: err_msg };
                                return;
                            }
                        }
                    }
                    _ => {}
                }
            }

            // 7. Store assistant response
            let model_id = format!("claude-code-{}", config.model.as_str());
            let assistant_msg = match storage.message_create(
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
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                    return;
                }
            };

            // 8. Create assistant node in Arbor
            let assistant_handle = ClaudeCodeStorage::message_to_handle(&assistant_msg, "assistant");
            let assistant_node_id = match storage.arbor().node_create_external(
                &config.head.tree_id,
                Some(user_node_id),
                assistant_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                    return;
                }
            };

            let new_head = Position::new(config.head.tree_id, assistant_node_id);

            // 9. Update session head and Claude session ID
            if let Err(e) = storage.session_update_head(&session_id, assistant_node_id, claude_session_id.clone()).await {
                yield ClaudeCodeEvent::Error { message: e.to_string() };
                return;
            }

            // 10. Emit ChatComplete
            yield ClaudeCodeEvent::ChatComplete {
                claudecode_id: session_id,
                new_head,
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
    async fn get(&self, name: String) -> impl Stream<Item = ClaudeCodeEvent> + Send + 'static {
        let result = self.storage.session_get_by_name(&name).await;

        stream! {
            match result {
                Ok(config) => {
                    yield ClaudeCodeEvent::Data { config };
                }
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                }
            }
        }
    }

    /// List all Claude Code sessions
    #[hub_macro::hub_method]
    async fn list(&self) -> impl Stream<Item = ClaudeCodeEvent> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.session_list().await {
                Ok(sessions) => {
                    yield ClaudeCodeEvent::List { sessions };
                }
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                }
            }
        }
    }

    /// Delete a session
    #[hub_macro::hub_method]
    async fn delete(&self, name: String) -> impl Stream<Item = ClaudeCodeEvent> + Send + 'static {
        let storage = self.storage.clone();
        let resolve_result = storage.session_get_by_name(&name).await;

        stream! {
            let config = match resolve_result {
                Ok(c) => c,
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                    return;
                }
            };

            match storage.session_delete(&config.id).await {
                Ok(_) => {
                    yield ClaudeCodeEvent::Deleted { claudecode_id: config.id };
                }
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
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
    ) -> impl Stream<Item = ClaudeCodeEvent> + Send + 'static {
        let storage = self.storage.clone();
        let resolve_result = storage.session_get_by_name(&name).await;

        stream! {
            // Get parent session
            let parent = match resolve_result {
                Ok(c) => c,
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
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
                        yield ClaudeCodeEvent::Error { message: e.to_string() };
                        return;
                    }
                    c.head = parent.head;
                    c
                }
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                    return;
                }
            };

            yield ClaudeCodeEvent::Created {
                claudecode_id: new_config.id,
                head: new_config.head,
                claude_session_id: None,  // Will fork Claude session on first chat
            };
        }
    }
}

use super::{
    executor::{ClaudeCodeExecutor, LaunchConfig},
    storage::ClaudeCodeStorage,
    types::*,
};
use crate::plexus::{into_plexus_stream, Activation, PlexusError, PlexusStream, Provenance};
use crate::plugin_system::conversion::{IntoSubscription, SubscriptionResult};
use async_stream::stream;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use jsonrpsee::{core::server::Methods, proc_macros::rpc, PendingSubscriptionSink};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;

/// RPC interface for ClaudeCode activation
#[rpc(server, namespace = "claudecode")]
pub trait ClaudeCodeRpc {
    /// Create a new Claude Code session
    #[subscription(
        name = "create",
        unsubscribe = "unsubscribe_create",
        item = serde_json::Value
    )]
    async fn create(
        &self,
        name: String,
        working_dir: String,
        model: String,
        system_prompt: Option<String>,
    ) -> SubscriptionResult;

    /// Chat with a session (streams tokens)
    #[subscription(
        name = "chat",
        unsubscribe = "unsubscribe_chat",
        item = serde_json::Value
    )]
    async fn chat(&self, name: String, prompt: String) -> SubscriptionResult;

    /// Get session details
    #[subscription(
        name = "get",
        unsubscribe = "unsubscribe_get",
        item = serde_json::Value
    )]
    async fn get(&self, name: String) -> SubscriptionResult;

    /// List all sessions
    #[subscription(
        name = "list",
        unsubscribe = "unsubscribe_list",
        item = serde_json::Value
    )]
    async fn list(&self) -> SubscriptionResult;

    /// Delete a session
    #[subscription(
        name = "delete",
        unsubscribe = "unsubscribe_delete",
        item = serde_json::Value
    )]
    async fn delete(&self, name: String) -> SubscriptionResult;

    /// Fork a session (create branch point)
    #[subscription(
        name = "fork",
        unsubscribe = "unsubscribe_fork",
        item = serde_json::Value
    )]
    async fn fork(&self, name: String, new_name: String) -> SubscriptionResult;
}

/// Method enum for schema generation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ClaudeCodeMethod {
    /// Create a new Claude Code session
    Create {
        name: String,
        working_dir: String,
        model: Model,
        system_prompt: Option<String>,
    },
    /// Chat with a session (streams tokens like Cone)
    Chat {
        identifier: SessionIdentifier,
        prompt: String,
    },
    /// Get session details
    Get {
        identifier: SessionIdentifier,
    },
    /// List all sessions
    List,
    /// Delete a session
    Delete {
        identifier: SessionIdentifier,
    },
    /// Fork a session (create branch point)
    Fork {
        identifier: SessionIdentifier,
        new_name: String,
    },
}

/// Identifier for a session (by ID or name)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionIdentifier {
    ById { id: ClaudeCodeId },
    ByName { name: String },
}

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

    /// Resolve a session identifier to a config
    async fn resolve_session(
        &self,
        identifier: &SessionIdentifier,
    ) -> Result<ClaudeCodeConfig, ClaudeCodeError> {
        match identifier {
            SessionIdentifier::ById { id } => self.storage.session_get(id).await,
            SessionIdentifier::ByName { name } => self.storage.session_get_by_name(name).await,
        }
    }

    /// Create a new session
    async fn create_impl(
        &self,
        name: String,
        working_dir: String,
        model: Model,
        system_prompt: Option<String>,
    ) -> Pin<Box<dyn Stream<Item = ClaudeCodeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        Box::pin(stream! {
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
        })
    }

    /// Chat with a session - the main operation that streams tokens
    async fn chat_impl(
        &self,
        identifier: SessionIdentifier,
        prompt: String,
    ) -> Pin<Box<dyn Stream<Item = ClaudeCodeEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        let executor = self.executor.clone();

        // Resolve before entering stream to avoid lifetime issues
        let resolve_result = self.resolve_session(&identifier).await;

        Box::pin(stream! {
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

            while let Some(event) = raw_stream.next().await {
                match event {
                    RawClaudeEvent::System { session_id: sid, .. } => {
                        if let Some(id) = sid {
                            claude_session_id = Some(id);
                        }
                    }
                    RawClaudeEvent::Assistant { message } => {
                        if let Some(msg) = message {
                            if let Some(content) = msg.content {
                                for block in content {
                                    match block {
                                        RawContentBlock::Text { text } => {
                                            response_content.push_str(&text);
                                            yield ClaudeCodeEvent::ChatContent {
                                                claudecode_id: session_id,
                                                content: text,
                                            };
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
        })
    }

    /// Get session details
    async fn get_impl(
        &self,
        identifier: SessionIdentifier,
    ) -> Pin<Box<dyn Stream<Item = ClaudeCodeEvent> + Send + 'static>> {
        // Resolve before entering stream to avoid lifetime issues
        let result = self.resolve_session(&identifier).await;

        Box::pin(stream! {
            match result {
                Ok(config) => {
                    yield ClaudeCodeEvent::Data { config };
                }
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                }
            }
        })
    }

    /// List all sessions
    async fn list_impl(&self) -> Pin<Box<dyn Stream<Item = ClaudeCodeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        Box::pin(stream! {
            match storage.session_list().await {
                Ok(sessions) => {
                    yield ClaudeCodeEvent::List { sessions };
                }
                Err(e) => {
                    yield ClaudeCodeEvent::Error { message: e.to_string() };
                }
            }
        })
    }

    /// Delete a session
    async fn delete_impl(
        &self,
        identifier: SessionIdentifier,
    ) -> Pin<Box<dyn Stream<Item = ClaudeCodeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        // Resolve before entering stream to avoid lifetime issues
        let resolve_result = self.resolve_session(&identifier).await;

        Box::pin(stream! {
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
        })
    }

    /// Fork a session (create a branch point)
    async fn fork_impl(
        &self,
        identifier: SessionIdentifier,
        new_name: String,
    ) -> Pin<Box<dyn Stream<Item = ClaudeCodeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        // Resolve before entering stream to avoid lifetime issues
        let resolve_result = self.resolve_session(&identifier).await;

        Box::pin(stream! {
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
        })
    }
}

#[async_trait]
impl Activation for ClaudeCode {
    type Methods = ClaudeCodeMethod;

    fn namespace(&self) -> &str {
        "claudecode"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Manage Claude Code sessions with Arbor-backed conversation history"
    }

    fn methods(&self) -> Vec<&str> {
        vec!["create", "chat", "get", "list", "delete", "fork"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        match method {
            "create" => Some("Create a new Claude Code session".to_string()),
            "chat" => Some("Chat with a session, streaming tokens like Cone".to_string()),
            "get" => Some("Get session configuration details".to_string()),
            "list" => Some("List all Claude Code sessions".to_string()),
            "delete" => Some("Delete a session".to_string()),
            "fork" => Some("Fork a session to create a branch point".to_string()),
            _ => None,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        let provenance = Provenance::root("claudecode");

        let stream: Pin<Box<dyn Stream<Item = ClaudeCodeEvent> + Send + 'static>> = match method {
            "create" => {
                let name: String = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("name required".to_string()))?
                    .to_string();
                let working_dir: String = params
                    .get("working_dir")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".")
                    .to_string();
                let model: Model = params
                    .get("model")
                    .and_then(|v| v.as_str())
                    .and_then(Model::from_str)
                    .unwrap_or(Model::Sonnet);
                let system_prompt: Option<String> = params
                    .get("system_prompt")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                self.create_impl(name, working_dir, model, system_prompt).await
            }
            "chat" => {
                let identifier = if let Some(id) = params.get("id").and_then(|v| v.as_str()) {
                    SessionIdentifier::ById {
                        id: uuid::Uuid::parse_str(id)
                            .map_err(|e| PlexusError::InvalidParams(format!("Invalid ID: {}", e)))?,
                    }
                } else if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    SessionIdentifier::ByName { name: name.to_string() }
                } else {
                    return Err(PlexusError::InvalidParams("id or name required".to_string()));
                };

                let prompt: String = params
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("prompt required".to_string()))?
                    .to_string();

                self.chat_impl(identifier, prompt).await
            }
            "get" => {
                let identifier = if let Some(id) = params.get("id").and_then(|v| v.as_str()) {
                    SessionIdentifier::ById {
                        id: uuid::Uuid::parse_str(id)
                            .map_err(|e| PlexusError::InvalidParams(format!("Invalid ID: {}", e)))?,
                    }
                } else if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    SessionIdentifier::ByName { name: name.to_string() }
                } else {
                    return Err(PlexusError::InvalidParams("id or name required".to_string()));
                };

                self.get_impl(identifier).await
            }
            "list" => self.list_impl().await,
            "delete" => {
                let identifier = if let Some(id) = params.get("id").and_then(|v| v.as_str()) {
                    SessionIdentifier::ById {
                        id: uuid::Uuid::parse_str(id)
                            .map_err(|e| PlexusError::InvalidParams(format!("Invalid ID: {}", e)))?,
                    }
                } else if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    SessionIdentifier::ByName { name: name.to_string() }
                } else {
                    return Err(PlexusError::InvalidParams("id or name required".to_string()));
                };

                self.delete_impl(identifier).await
            }
            "fork" => {
                let identifier = if let Some(id) = params.get("id").and_then(|v| v.as_str()) {
                    SessionIdentifier::ById {
                        id: uuid::Uuid::parse_str(id)
                            .map_err(|e| PlexusError::InvalidParams(format!("Invalid ID: {}", e)))?,
                    }
                } else if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    SessionIdentifier::ByName { name: name.to_string() }
                } else {
                    return Err(PlexusError::InvalidParams("id or name required".to_string()));
                };

                let new_name: String = params
                    .get("new_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("new_name required".to_string()))?
                    .to_string();

                self.fork_impl(identifier, new_name).await
            }
            _ => {
                return Err(PlexusError::MethodNotFound {
                    activation: "claudecode".to_string(),
                    method: method.to_string(),
                });
            }
        };

        Ok(into_plexus_stream(stream, provenance))
    }

    fn into_rpc_methods(self) -> Methods {
        self.into_rpc().into()
    }
}

// Implement the RPC server trait generated by the #[rpc] macro
#[async_trait]
impl ClaudeCodeRpcServer for ClaudeCode {
    async fn create(
        &self,
        pending: PendingSubscriptionSink,
        name: String,
        working_dir: String,
        model: String,
        system_prompt: Option<String>,
    ) -> SubscriptionResult {
        let model = Model::from_str(&model).unwrap_or(Model::Sonnet);
        let stream = self.create_impl(name, working_dir, model, system_prompt).await;
        stream
            .into_subscription(pending, Provenance::root("claudecode"))
            .await
    }

    async fn chat(
        &self,
        pending: PendingSubscriptionSink,
        name: String,
        prompt: String,
    ) -> SubscriptionResult {
        let identifier = SessionIdentifier::ByName { name };
        let stream = self.chat_impl(identifier, prompt).await;
        stream
            .into_subscription(pending, Provenance::root("claudecode"))
            .await
    }

    async fn get(&self, pending: PendingSubscriptionSink, name: String) -> SubscriptionResult {
        let identifier = SessionIdentifier::ByName { name };
        let stream = self.get_impl(identifier).await;
        stream
            .into_subscription(pending, Provenance::root("claudecode"))
            .await
    }

    async fn list(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.list_impl().await;
        stream
            .into_subscription(pending, Provenance::root("claudecode"))
            .await
    }

    async fn delete(&self, pending: PendingSubscriptionSink, name: String) -> SubscriptionResult {
        let identifier = SessionIdentifier::ByName { name };
        let stream = self.delete_impl(identifier).await;
        stream
            .into_subscription(pending, Provenance::root("claudecode"))
            .await
    }

    async fn fork(
        &self,
        pending: PendingSubscriptionSink,
        name: String,
        new_name: String,
    ) -> SubscriptionResult {
        let identifier = SessionIdentifier::ByName { name };
        let stream = self.fork_impl(identifier, new_name).await;
        stream
            .into_subscription(pending, Provenance::root("claudecode"))
            .await
    }
}

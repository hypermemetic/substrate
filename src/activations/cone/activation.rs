use super::methods::ConeIdentifier;
use super::storage::{ConeStorage, ConeStorageConfig};
use super::types::{
    ChatEvent, ChatUsage, CreateResult, DeleteResult, GetResult,
    ListResult, MessageRole, RegistryResult, ResolveResult, SetHeadResult,
};
use crate::activations::arbor::{Node, NodeId, NodeType};
use crate::activations::bash::Bash;
use crate::plexus::Plexus;
use async_stream::stream;
use cllient::{Message, ModelRegistry};
use futures::Stream;
use hub_macro::hub_methods;
use std::sync::{Arc, OnceLock, Weak};

/// Cone plugin - orchestrates LLM conversations with Arbor context
#[derive(Clone)]
pub struct Cone {
    storage: Arc<ConeStorage>,
    llm_registry: Arc<ModelRegistry>,
    /// Hub reference for resolving foreign handles when walking arbor trees
    hub: Arc<OnceLock<Weak<Plexus>>>,
}

impl Cone {
    pub async fn new(
        config: ConeStorageConfig,
        arbor: Arc<crate::activations::arbor::ArborStorage>,
    ) -> Result<Self, String> {
        let storage = ConeStorage::new(config, arbor)
            .await
            .map_err(|e| format!("Failed to initialize cone storage: {}", e.message))?;

        let llm_registry = ModelRegistry::new()
            .map_err(|e| format!("Failed to initialize LLM registry: {}", e))?;

        Ok(Self {
            storage: Arc::new(storage),
            llm_registry: Arc::new(llm_registry),
            hub: Arc::new(OnceLock::new()),
        })
    }

    /// Inject hub reference for resolving foreign handles
    ///
    /// Called during Plexus construction via Arc::new_cyclic.
    /// This allows Cone to resolve handles from other plugins when walking arbor trees.
    pub fn inject_hub(&self, hub: Weak<Plexus>) {
        let _ = self.hub.set(hub);
    }

    /// Register default templates with the mustache plugin
    ///
    /// Call this during initialization to register Cone's default templates
    /// for rendering resolved messages and events.
    pub async fn register_default_templates(
        &self,
        mustache: &crate::activations::mustache::Mustache,
    ) -> Result<(), String> {
        let plugin_id = Self::PLUGIN_ID;

        mustache.register_templates(plugin_id, &[
            // Chat method - resolved message template
            ("chat", "default", "[{{role}}] {{#name}}({{name}}) {{/name}}{{content}}"),
            ("chat", "markdown", "**{{role}}**{{#name}} ({{name}}){{/name}}\n\n{{content}}"),
            ("chat", "json", r#"{"role":"{{role}}","content":"{{content}}","name":"{{name}}"}"#),

            // Create method - cone created event
            ("create", "default", "Cone created: {{cone_id}} (head: {{head.tree_id}}/{{head.node_id}})"),

            // List method - cone list event
            ("list", "default", "{{#cones}}{{name}} ({{id}}) - {{model_id}}\n{{/cones}}"),
        ]).await
    }

    /// Get the hub reference
    ///
    /// Panics if called before inject_hub.
    #[allow(dead_code)]
    pub fn hub(&self) -> Arc<Plexus> {
        self.hub
            .get()
            .expect("hub not initialized - inject_hub must be called first")
            .upgrade()
            .expect("hub has been dropped")
    }

    /// Resolve a cone handle to its message content
    ///
    /// Called by the macro-generated resolve_handle method.
    /// Handle format: cone@1.0.0::chat:msg-{uuid}:{role}:{name}
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
                "Cone handle missing message ID in meta".to_string()
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
                        message: format!("Failed to resolve handle: {}", e.message),
                    };
                }
            }
        };

        Ok(wrap_stream(result_stream, "cone.resolve_handle", vec!["cone".into()]))
    }
}

#[hub_methods(
    namespace = "cone",
    version = "1.0.0",
    description = "LLM cone with persistent conversation context",
    resolve_handle
)]
impl Cone {
    /// Create a new cone (LLM agent with persistent conversation context)
    #[hub_macro::hub_method(
        params(
            name = "Human-readable name for the cone",
            model_id = "LLM model ID (e.g., 'gpt-4o-mini', 'claude-3-haiku-20240307')",
            system_prompt = "Optional system prompt / instructions",
            metadata = "Optional configuration metadata"
        )
    )]
    async fn create(
        &self,
        name: String,
        model_id: String,
        system_prompt: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> impl Stream<Item = CreateResult> + Send + 'static {
        let storage = self.storage.clone();
        let llm_registry = self.llm_registry.clone();

        stream! {
            // Validate model exists before creating cone
            if let Err(e) = llm_registry.from_id(&model_id) {
                yield CreateResult::Error {
                    message: format!("Invalid model_id '{}': {}", model_id, e)
                };
                return;
            }

            match storage.cone_create(name, model_id, system_prompt, metadata).await {
                Ok(cone) => {
                    yield CreateResult::Created {
                        cone_id: cone.id,
                        head: cone.head,
                    };
                }
                Err(e) => {
                    yield CreateResult::Error { message: e.message };
                }
            }
        }
    }

    /// Get cone configuration by name or ID
    #[hub_macro::hub_method(
        params(identifier = "Cone name or UUID (e.g., 'my-assistant' or '550e8400-e29b-...')")
    )]
    async fn get(
        &self,
        identifier: ConeIdentifier,
    ) -> impl Stream<Item = GetResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield GetResult::Error { message: e.message };
                    return;
                }
            };

            match storage.cone_get(&cone_id).await {
                Ok(cone) => {
                    yield GetResult::Data { cone };
                }
                Err(e) => {
                    yield GetResult::Error { message: e.message };
                }
            }
        }
    }

    /// List all cones
    #[hub_macro::hub_method]
    async fn list(&self) -> impl Stream<Item = ListResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.cone_list().await {
                Ok(cones) => {
                    yield ListResult::List { cones };
                }
                Err(e) => {
                    yield ListResult::Error { message: e.message };
                }
            }
        }
    }

    /// Delete a cone (associated tree is preserved)
    #[hub_macro::hub_method(
        params(identifier = "Cone name or UUID (e.g., 'my-assistant' or '550e8400-e29b-...')")
    )]
    async fn delete(
        &self,
        identifier: ConeIdentifier,
    ) -> impl Stream<Item = DeleteResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield DeleteResult::Error { message: e.message };
                    return;
                }
            };

            match storage.cone_delete(&cone_id).await {
                Ok(()) => {
                    yield DeleteResult::Deleted { cone_id };
                }
                Err(e) => {
                    yield DeleteResult::Error { message: e.message };
                }
            }
        }
    }

    /// Chat with a cone - appends prompt to context, calls LLM, advances head
    #[hub_macro::hub_method(
        streaming,
        params(
            identifier = "Cone name or UUID (e.g., 'my-assistant' or '550e8400-e29b-...')",
            prompt = "User message / prompt to send to the LLM",
            ephemeral = "If true, creates nodes but doesn't advance head and marks for deletion"
        )
    )]
    async fn chat(
        &self,
        identifier: ConeIdentifier,
        prompt: String,
        ephemeral: Option<bool>,
    ) -> impl Stream<Item = ChatEvent> + Send + 'static {
        let storage = self.storage.clone();
        let llm_registry = self.llm_registry.clone();

        stream! {
            let is_ephemeral = ephemeral.unwrap_or(false);

            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield ChatEvent::Error { message: e.message };
                    return;
                }
            };

            // 1. Load cone config
            let cone = match storage.cone_get(&cone_id).await {
                Ok(a) => a,
                Err(e) => {
                    yield ChatEvent::Error { message: format!("Failed to get cone: {}", e.message) };
                    return;
                }
            };

            // 2. Build context from arbor path (handles only)
            let context_nodes = match storage.arbor().context_get_path(&cone.head.tree_id, &cone.head.node_id).await {
                Ok(nodes) => nodes,
                Err(e) => {
                    yield ChatEvent::Error { message: format!("Failed to get context path: {}", e) };
                    return;
                }
            };

            // Resolve handles to messages
            let messages = match resolve_context_to_messages(&storage, &context_nodes, &cone.system_prompt).await {
                Ok(msgs) => msgs,
                Err(e) => {
                    yield ChatEvent::Error { message: format!("Failed to resolve context: {}", e) };
                    return;
                }
            };

            // 3. Store user message in cone database (ephemeral if requested)
            let user_message = if is_ephemeral {
                match storage.message_create_ephemeral(
                    &cone_id,
                    MessageRole::User,
                    prompt.clone(),
                    None,
                    None,
                    None,
                ).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to store user message: {}", e.message) };
                        return;
                    }
                }
            } else {
                match storage.message_create(
                    &cone_id,
                    MessageRole::User,
                    prompt.clone(),
                    None,
                    None,
                    None,
                ).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to store user message: {}", e.message) };
                        return;
                    }
                }
            };

            // Create external node with handle pointing to user message (ephemeral if requested)
            let user_handle = ConeStorage::message_to_handle(&user_message, "user");
            let user_node_id = if is_ephemeral {
                match storage.arbor().node_create_external_ephemeral(
                    &cone.head.tree_id,
                    Some(cone.head.node_id),
                    user_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to create user node: {}", e) };
                        return;
                    }
                }
            } else {
                match storage.arbor().node_create_external(
                    &cone.head.tree_id,
                    Some(cone.head.node_id),
                    user_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to create user node: {}", e) };
                        return;
                    }
                }
            };

            let user_position = cone.head.advance(user_node_id);

            // Signal chat start
            yield ChatEvent::Start {
                cone_id,
                user_position,
            };

            // 4. Build LLM request with resolved messages + new user prompt
            let mut llm_messages = messages;
            llm_messages.push(Message::user(&prompt));

            let request_builder = match llm_registry.from_id(&cone.model_id) {
                Ok(rb) => rb,
                Err(e) => {
                    yield ChatEvent::Error { message: format!("Failed to create request builder: {}", e) };
                    return;
                }
            };

            let mut builder = request_builder;
            if let Some(ref sys) = cone.system_prompt {
                builder = builder.system(sys);
            }
            builder = builder.messages(llm_messages);

            // Stream the response
            let mut stream_result = match builder.stream().await {
                Ok(s) => s,
                Err(e) => {
                    yield ChatEvent::Error { message: format!("Failed to start LLM stream: {}", e) };
                    return;
                }
            };

            let mut full_response = String::new();
            let mut input_tokens: Option<i64> = None;
            let mut output_tokens: Option<i64> = None;

            use futures::StreamExt;
            while let Some(event) = stream_result.next().await {
                match event {
                    Ok(cllient::streaming::StreamEvent::Content(text)) => {
                        full_response.push_str(&text);
                        yield ChatEvent::Content {
                            cone_id,
                            content: text,
                        };
                    }
                    Ok(cllient::streaming::StreamEvent::Usage { input_tokens: inp, output_tokens: out, .. }) => {
                        input_tokens = inp.map(|t| t as i64);
                        output_tokens = out.map(|t| t as i64);
                    }
                    Ok(cllient::streaming::StreamEvent::Error(e)) => {
                        yield ChatEvent::Error { message: format!("LLM error: {}", e) };
                        return;
                    }
                    Ok(_) => {
                        // Ignore other events (Start, Finish, Role, Raw)
                    }
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Stream error: {}", e) };
                        return;
                    }
                }
            }

            // 5. Store assistant response in cone database (ephemeral if requested)
            let assistant_message = if is_ephemeral {
                match storage.message_create_ephemeral(
                    &cone_id,
                    MessageRole::Assistant,
                    full_response,
                    Some(cone.model_id.clone()),
                    input_tokens,
                    output_tokens,
                ).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to store assistant message: {}", e.message) };
                        return;
                    }
                }
            } else {
                match storage.message_create(
                    &cone_id,
                    MessageRole::Assistant,
                    full_response,
                    Some(cone.model_id.clone()),
                    input_tokens,
                    output_tokens,
                ).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to store assistant message: {}", e.message) };
                        return;
                    }
                }
            };

            // Create external node with handle pointing to assistant message (ephemeral if requested)
            let assistant_handle = ConeStorage::message_to_handle(&assistant_message, &cone.name);
            let response_node_id = if is_ephemeral {
                match storage.arbor().node_create_external_ephemeral(
                    &cone.head.tree_id,
                    Some(user_node_id),
                    assistant_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to create response node: {}", e) };
                        return;
                    }
                }
            } else {
                match storage.arbor().node_create_external(
                    &cone.head.tree_id,
                    Some(user_node_id),
                    assistant_handle,
                    None,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        yield ChatEvent::Error { message: format!("Failed to create response node: {}", e) };
                        return;
                    }
                }
            };

            let new_head = user_position.advance(response_node_id);

            // 6. Update canonical_head (skip for ephemeral)
            if !is_ephemeral {
                if let Err(e) = storage.cone_update_head(&cone_id, response_node_id).await {
                    yield ChatEvent::Error { message: format!("Failed to update head: {}", e.message) };
                    return;
                }
            }

            let usage_info = if input_tokens.is_some() || output_tokens.is_some() {
                Some(ChatUsage {
                    input_tokens: input_tokens.map(|t| t as u64),
                    output_tokens: output_tokens.map(|t| t as u64),
                    total_tokens: input_tokens.and_then(|i| output_tokens.map(|o| (i + o) as u64)),
                })
            } else {
                None
            };

            // For ephemeral, return original head (not the ephemeral node)
            yield ChatEvent::Complete {
                cone_id,
                new_head: if is_ephemeral { cone.head } else { new_head },
                usage: usage_info,
            };
        }
    }

    /// Move cone's canonical head to a different node in the tree
    #[hub_macro::hub_method(
        params(
            identifier = "Cone name or UUID (e.g., 'my-assistant' or '550e8400-e29b-...')",
            node_id = "UUID of the target node to set as the new head"
        )
    )]
    async fn set_head(
        &self,
        identifier: ConeIdentifier,
        node_id: NodeId,
    ) -> impl Stream<Item = SetHeadResult> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield SetHeadResult::Error { message: e.message };
                    return;
                }
            };

            // Get current head first
            let old_head = match storage.cone_get(&cone_id).await {
                Ok(cone) => cone.head,
                Err(e) => {
                    yield SetHeadResult::Error { message: e.message };
                    return;
                }
            };

            // Advance to new node in same tree
            let new_head = old_head.advance(node_id);

            match storage.cone_update_head(&cone_id, node_id).await {
                Ok(()) => {
                    yield SetHeadResult::Updated {
                        cone_id,
                        old_head,
                        new_head,
                    };
                }
                Err(e) => {
                    yield SetHeadResult::Error { message: e.message };
                }
            }
        }
    }

    /// Get available LLM services and models
    #[hub_macro::hub_method]
    async fn registry(&self) -> impl Stream<Item = RegistryResult> + Send + 'static {
        let llm_registry = self.llm_registry.clone();

        stream! {
            let export = llm_registry.export();
            yield RegistryResult::Registry(export);
        }
    }
}

/// Resolve arbor context path to cllient messages by resolving handles
async fn resolve_context_to_messages(
    storage: &ConeStorage,
    nodes: &[Node],
    _system_prompt: &Option<String>,
) -> Result<Vec<Message>, String> {
    let mut messages = Vec::new();

    for node in nodes {
        match &node.data {
            NodeType::Text { content } => {
                // Text nodes shouldn't exist in the new design, but handle gracefully
                // Skip empty root nodes
                if !content.is_empty() {
                    messages.push(Message::user(content));
                }
            }
            NodeType::External { handle } => {
                // Resolve handle based on plugin_id
                if handle.plugin_id == Cone::PLUGIN_ID {
                    // Resolve cone message handle - message UUID is in meta[0]
                    let msg_id = handle.meta.first()
                        .ok_or_else(|| "Cone handle missing message ID in meta".to_string())?;
                    let msg = storage
                        .resolve_message_handle(msg_id)
                        .await
                        .map_err(|e| format!("Failed to resolve message handle: {}", e.message))?;

                    let cllient_msg = match msg.role {
                        MessageRole::User => Message::user(&msg.content),
                        MessageRole::Assistant => Message::assistant(&msg.content),
                        MessageRole::System => Message::system(&msg.content),
                    };
                    messages.push(cllient_msg);
                } else if handle.plugin_id == Bash::PLUGIN_ID {
                    // TODO: Resolve bash output when bash plugin integration is added
                    let cmd_id = handle.meta.first().map(|s| s.as_str()).unwrap_or("unknown");
                    messages.push(Message::user(&format!(
                        "[Tool output from bash: {}]",
                        cmd_id
                    )));
                } else {
                    // Unknown handle plugin - include as reference using Display
                    messages.push(Message::user(&format!(
                        "[External reference: {}]",
                        handle
                    )));
                }
            }
        }
    }

    Ok(messages)
}

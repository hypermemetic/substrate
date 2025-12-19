use super::methods::ConeIdentifier;
use super::storage::{ConeStorage, ConeStorageConfig};
use super::types::{ConeEvent, ChatUsage, MessageRole};
use crate::activations::arbor::{Node, NodeId, NodeType};
use async_stream::stream;
use cllient::{Message, ModelRegistry};
use futures::Stream;
use hub_macro::hub_methods;
use std::sync::Arc;

/// Cone plugin - orchestrates LLM conversations with Arbor context
#[derive(Clone)]
pub struct Cone {
    storage: Arc<ConeStorage>,
    llm_registry: Arc<ModelRegistry>,
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
        })
    }
}

#[hub_methods(
    namespace = "cone",
    version = "1.0.0",
    description = "LLM cone with persistent conversation context"
)]
impl Cone {
    /// Create a new cone (LLM agent with persistent conversation context)
    #[hub_macro::hub_method(params(
        name = "Human-readable name for the cone",
        model_id = "LLM model ID (e.g., 'gpt-4o-mini', 'claude-3-haiku-20240307')",
        system_prompt = "Optional system prompt / instructions",
        metadata = "Optional configuration metadata"
    ))]
    async fn create(
        &self,
        name: String,
        model_id: String,
        system_prompt: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> impl Stream<Item = ConeEvent> + Send + 'static {
        let storage = self.storage.clone();
        let llm_registry = self.llm_registry.clone();

        stream! {
            // Validate model exists before creating cone
            if let Err(e) = llm_registry.from_id(&model_id) {
                yield ConeEvent::Error {
                    message: format!("Invalid model_id '{}': {}", model_id, e)
                };
                return;
            }

            match storage.cone_create(name, model_id, system_prompt, metadata).await {
                Ok(cone) => {
                    yield ConeEvent::ConeCreated {
                        cone_id: cone.id,
                        head: cone.head,
                    };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        }
    }

    /// Get cone configuration by name or ID
    #[hub_macro::hub_method(params(
        identifier = "Cone identifier - use {by_name: {name: '...'}} or {by_id: {id: '...'}}"
    ))]
    async fn get(
        &self,
        identifier: ConeIdentifier,
    ) -> impl Stream<Item = ConeEvent> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                    return;
                }
            };

            match storage.cone_get(&cone_id).await {
                Ok(cone) => {
                    yield ConeEvent::ConeData { cone };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        }
    }

    /// List all cones
    #[hub_macro::hub_method]
    async fn list(&self) -> impl Stream<Item = ConeEvent> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            match storage.cone_list().await {
                Ok(cones) => {
                    yield ConeEvent::ConeList { cones };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        }
    }

    /// Delete a cone (associated tree is preserved)
    #[hub_macro::hub_method(params(
        identifier = "Cone identifier - use {by_name: {name: '...'}} or {by_id: {id: '...'}}"
    ))]
    async fn delete(
        &self,
        identifier: ConeIdentifier,
    ) -> impl Stream<Item = ConeEvent> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                    return;
                }
            };

            match storage.cone_delete(&cone_id).await {
                Ok(()) => {
                    yield ConeEvent::ConeDeleted { cone_id };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        }
    }

    /// Chat with a cone - appends prompt to context, calls LLM, advances head
    #[hub_macro::hub_method(params(
        identifier = "Cone identifier - use {by_name: {name: '...'}} or {by_id: {id: '...'}}",
        prompt = "User message / prompt to send to the LLM"
    ))]
    async fn chat(
        &self,
        identifier: ConeIdentifier,
        prompt: String,
    ) -> impl Stream<Item = ConeEvent> + Send + 'static {
        let storage = self.storage.clone();
        let llm_registry = self.llm_registry.clone();

        stream! {
            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                    return;
                }
            };

            // 1. Load cone config
            let cone = match storage.cone_get(&cone_id).await {
                Ok(a) => a,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to get cone: {}", e.message) };
                    return;
                }
            };

            // 2. Build context from arbor path (handles only)
            let context_nodes = match storage.arbor().context_get_path(&cone.head.tree_id, &cone.head.node_id).await {
                Ok(nodes) => nodes,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to get context path: {}", e) };
                    return;
                }
            };

            // Resolve handles to messages
            let messages = match resolve_context_to_messages(&storage, &context_nodes, &cone.system_prompt).await {
                Ok(msgs) => msgs,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to resolve context: {}", e) };
                    return;
                }
            };

            // 3. Store user message in cone database
            let user_message = match storage.message_create(
                &cone_id,
                MessageRole::User,
                prompt.clone(),
                None,
                None,
                None,
            ).await {
                Ok(msg) => msg,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to store user message: {}", e.message) };
                    return;
                }
            };

            // Create external node with handle pointing to user message
            let user_handle = ConeStorage::message_to_handle(&user_message, "user");
            let user_node_id = match storage.arbor().node_create_external(
                &cone.head.tree_id,
                Some(cone.head.node_id),
                user_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to create user node: {}", e) };
                    return;
                }
            };

            let user_position = cone.head.advance(user_node_id);

            // Signal chat start
            yield ConeEvent::ChatStart {
                cone_id,
                user_position,
            };

            // 4. Build LLM request with resolved messages + new user prompt
            let mut llm_messages = messages;
            llm_messages.push(Message::user(&prompt));

            let request_builder = match llm_registry.from_id(&cone.model_id) {
                Ok(rb) => rb,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to create request builder: {}", e) };
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
                    yield ConeEvent::Error { message: format!("Failed to start LLM stream: {}", e) };
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
                        yield ConeEvent::ChatContent {
                            cone_id,
                            content: text,
                        };
                    }
                    Ok(cllient::streaming::StreamEvent::Usage { input_tokens: inp, output_tokens: out, .. }) => {
                        input_tokens = inp.map(|t| t as i64);
                        output_tokens = out.map(|t| t as i64);
                    }
                    Ok(cllient::streaming::StreamEvent::Error(e)) => {
                        yield ConeEvent::Error { message: format!("LLM error: {}", e) };
                        return;
                    }
                    Ok(_) => {
                        // Ignore other events (Start, Finish, Role, Raw)
                    }
                    Err(e) => {
                        yield ConeEvent::Error { message: format!("Stream error: {}", e) };
                        return;
                    }
                }
            }

            // 5. Store assistant response in cone database
            let assistant_message = match storage.message_create(
                &cone_id,
                MessageRole::Assistant,
                full_response,
                Some(cone.model_id.clone()),
                input_tokens,
                output_tokens,
            ).await {
                Ok(msg) => msg,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to store assistant message: {}", e.message) };
                    return;
                }
            };

            // Create external node with handle pointing to assistant message
            let assistant_handle = ConeStorage::message_to_handle(&assistant_message, &cone.name);
            let response_node_id = match storage.arbor().node_create_external(
                &cone.head.tree_id,
                Some(user_node_id),
                assistant_handle,
                None,
            ).await {
                Ok(id) => id,
                Err(e) => {
                    yield ConeEvent::Error { message: format!("Failed to create response node: {}", e) };
                    return;
                }
            };

            let new_head = user_position.advance(response_node_id);

            // 6. Update canonical_head
            if let Err(e) = storage.cone_update_head(&cone_id, response_node_id).await {
                yield ConeEvent::Error { message: format!("Failed to update head: {}", e.message) };
                return;
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

            yield ConeEvent::ChatComplete {
                cone_id,
                new_head,
                usage: usage_info,
            };
        }
    }

    /// Move cone's canonical head to a different node in the tree
    #[hub_macro::hub_method(params(
        identifier = "Cone identifier - use {by_name: {name: '...'}} or {by_id: {id: '...'}}",
        node_id = "UUID of the target node to set as the new head"
    ))]
    async fn set_head(
        &self,
        identifier: ConeIdentifier,
        node_id: NodeId,
    ) -> impl Stream<Item = ConeEvent> + Send + 'static {
        let storage = self.storage.clone();

        stream! {
            // Resolve identifier to ConeId
            let cone_id = match storage.resolve_cone_identifier(&identifier).await {
                Ok(id) => id,
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                    return;
                }
            };

            // Get current head first
            let old_head = match storage.cone_get(&cone_id).await {
                Ok(cone) => cone.head,
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                    return;
                }
            };

            // Advance to new node in same tree
            let new_head = old_head.advance(node_id);

            match storage.cone_update_head(&cone_id, node_id).await {
                Ok(()) => {
                    yield ConeEvent::HeadUpdated {
                        cone_id,
                        old_head,
                        new_head,
                    };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        }
    }

    /// Get available LLM services and models
    #[hub_macro::hub_method]
    async fn registry(&self) -> impl Stream<Item = ConeEvent> + Send + 'static {
        let llm_registry = self.llm_registry.clone();

        stream! {
            let export = llm_registry.export();
            yield ConeEvent::Registry(export);
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
                // Resolve handle based on source
                match handle.source.as_str() {
                    "cone" => {
                        // Resolve cone message handle
                        let msg = storage
                            .resolve_message_handle(&handle.identifier)
                            .await
                            .map_err(|e| format!("Failed to resolve message handle: {}", e.message))?;

                        let cllient_msg = match msg.role {
                            MessageRole::User => Message::user(&msg.content),
                            MessageRole::Assistant => Message::assistant(&msg.content),
                            MessageRole::System => Message::system(&msg.content),
                        };
                        messages.push(cllient_msg);
                    }
                    "bash" => {
                        // TODO: Resolve bash output when bash plugin integration is added
                        messages.push(Message::user(&format!(
                            "[Tool output from bash: {}]",
                            handle.identifier
                        )));
                    }
                    _ => {
                        // Unknown handle source - include as reference
                        messages.push(Message::user(&format!(
                            "[External reference: {}:{}]",
                            handle.source, handle.identifier
                        )));
                    }
                }
            }
        }
    }

    Ok(messages)
}

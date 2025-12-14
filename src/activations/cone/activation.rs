use super::storage::{ConeStorage, ConeStorageConfig};
use super::types::{ConeEvent, ConeId, ChatUsage, MessageRole};
use crate::{
    plexus::{into_plexus_stream, Provenance, PlexusError, PlexusStream, Activation},
    plugin_system::conversion::{IntoSubscription, SubscriptionResult},
    activations::arbor::{Node, NodeId, NodeType},
};
use async_stream::stream;
use async_trait::async_trait;
use cllient::{Message, ModelRegistry};
use futures::{Stream, StreamExt};
use jsonrpsee::{core::server::Methods, proc_macros::rpc, PendingSubscriptionSink};
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;

/// RPC interface for Cone plugin
#[rpc(server, namespace = "cone")]
pub trait ConeRpc {
    /// Create a new cone
    #[subscription(
        name = "create",
        unsubscribe = "unsubscribe_create",
        item = serde_json::Value
    )]
    async fn create(
        &self,
        name: String,
        model_id: String,
        system_prompt: Option<String>,
        metadata: Option<Value>,
    ) -> SubscriptionResult;

    /// Get an cone by ID
    #[subscription(
        name = "get",
        unsubscribe = "unsubscribe_get",
        item = serde_json::Value
    )]
    async fn get(&self, cone_id: String) -> SubscriptionResult;

    /// List all cones
    #[subscription(
        name = "list",
        unsubscribe = "unsubscribe_list",
        item = serde_json::Value
    )]
    async fn list(&self) -> SubscriptionResult;

    /// Delete an cone
    #[subscription(
        name = "delete",
        unsubscribe = "unsubscribe_delete",
        item = serde_json::Value
    )]
    async fn delete(&self, cone_id: String) -> SubscriptionResult;

    /// Chat with an cone - appends prompt to context, calls LLM, advances head
    #[subscription(
        name = "chat",
        unsubscribe = "unsubscribe_chat",
        item = serde_json::Value
    )]
    async fn chat(&self, cone_id: String, prompt: String) -> SubscriptionResult;

    /// Move cone's canonical head to a different node
    #[subscription(
        name = "set_head",
        unsubscribe = "unsubscribe_set_head",
        item = serde_json::Value
    )]
    async fn set_head(&self, cone_id: String, node_id: String) -> SubscriptionResult;

    /// Get available LLM services and models
    #[subscription(
        name = "registry",
        unsubscribe = "unsubscribe_registry",
        item = serde_json::Value
    )]
    async fn registry(&self) -> SubscriptionResult;
}

/// Cone plugin - orchestrates LLM conversations with Arbor context
#[derive(Clone)]
pub struct Cone {
    storage: Arc<ConeStorage>,
    llm_registry: Arc<ModelRegistry>,
}

impl Cone {
    pub async fn new(config: ConeStorageConfig) -> Result<Self, String> {
        let storage = ConeStorage::new(config)
            .await
            .map_err(|e| format!("Failed to initialize cone storage: {}", e.message))?;

        let llm_registry = ModelRegistry::new()
            .map_err(|e| format!("Failed to initialize LLM registry: {}", e))?;

        Ok(Self {
            storage: Arc::new(storage),
            llm_registry: Arc::new(llm_registry),
        })
    }

    // ========================================================================
    // Stream implementations
    // ========================================================================

    async fn create_stream(
        &self,
        name: String,
        model_id: String,
        system_prompt: Option<String>,
        metadata: Option<Value>,
    ) -> Pin<Box<dyn Stream<Item = ConeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        Box::pin(stream! {
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
        })
    }

    async fn get_stream(
        &self,
        cone_id: ConeId,
    ) -> Pin<Box<dyn Stream<Item = ConeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        Box::pin(stream! {
            match storage.cone_get(&cone_id).await {
                Ok(cone) => {
                    yield ConeEvent::ConeData { cone };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        })
    }

    async fn list_stream(&self) -> Pin<Box<dyn Stream<Item = ConeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        Box::pin(stream! {
            match storage.cone_list().await {
                Ok(cones) => {
                    yield ConeEvent::ConeList { cones };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        })
    }

    async fn delete_stream(
        &self,
        cone_id: ConeId,
    ) -> Pin<Box<dyn Stream<Item = ConeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        Box::pin(stream! {
            match storage.cone_delete(&cone_id).await {
                Ok(()) => {
                    yield ConeEvent::ConeDeleted { cone_id };
                }
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                }
            }
        })
    }

    async fn set_head_stream(
        &self,
        cone_id: ConeId,
        new_node_id: NodeId,
    ) -> Pin<Box<dyn Stream<Item = ConeEvent> + Send + 'static>> {
        let storage = self.storage.clone();

        Box::pin(stream! {
            // Get current head first
            let old_head = match storage.cone_get(&cone_id).await {
                Ok(cone) => cone.head,
                Err(e) => {
                    yield ConeEvent::Error { message: e.message };
                    return;
                }
            };

            // Advance to new node in same tree
            let new_head = old_head.advance(new_node_id);

            match storage.cone_update_head(&cone_id, new_node_id).await {
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
        })
    }

    fn registry_stream(&self) -> Pin<Box<dyn Stream<Item = ConeEvent> + Send + 'static>> {
        let llm_registry = self.llm_registry.clone();

        Box::pin(stream! {
            let export = llm_registry.export();
            yield ConeEvent::Registry(export);
        })
    }

    /// Core chat implementation:
    /// 1. Load cone config (get canonical_head)
    /// 2. Build context from arbor path (resolve handles to messages)
    /// 3. Store user message, create external node with handle
    /// 4. Call LLM with full context
    /// 5. Stream response, store assistant message, create external node
    /// 6. Update canonical_head to the new response node
    async fn chat_stream(
        &self,
        cone_id: ConeId,
        prompt: String,
    ) -> Pin<Box<dyn Stream<Item = ConeEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        let llm_registry = self.llm_registry.clone();

        Box::pin(stream! {
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
            // TODO: Track actual user name in cone config or chat params
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
        })
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
                        // For now, include as a system message indicating tool output
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

#[async_trait]
impl ConeRpcServer for Cone {
    async fn create(
        &self,
        pending: PendingSubscriptionSink,
        name: String,
        model_id: String,
        system_prompt: Option<String>,
        metadata: Option<Value>,
    ) -> SubscriptionResult {
        let stream = self.create_stream(name, model_id, system_prompt, metadata).await;
        stream
            .into_subscription(pending, Provenance::root("cone"))
            .await
    }

    async fn get(&self, pending: PendingSubscriptionSink, cone_id: String) -> SubscriptionResult {
        let cone_id = match uuid::Uuid::parse_str(&cone_id) {
            Ok(id) => id,
            Err(e) => {
                let stream: Pin<Box<dyn Stream<Item = ConeEvent> + Send>> = Box::pin(stream! {
                    yield ConeEvent::Error { message: format!("Invalid cone ID: {}", e) };
                });
                return stream.into_subscription(pending, Provenance::root("cone")).await;
            }
        };

        let stream = self.get_stream(cone_id).await;
        stream
            .into_subscription(pending, Provenance::root("cone"))
            .await
    }

    async fn list(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.list_stream().await;
        stream
            .into_subscription(pending, Provenance::root("cone"))
            .await
    }

    async fn delete(
        &self,
        pending: PendingSubscriptionSink,
        cone_id: String,
    ) -> SubscriptionResult {
        let cone_id = match uuid::Uuid::parse_str(&cone_id) {
            Ok(id) => id,
            Err(e) => {
                let stream: Pin<Box<dyn Stream<Item = ConeEvent> + Send>> = Box::pin(stream! {
                    yield ConeEvent::Error { message: format!("Invalid cone ID: {}", e) };
                });
                return stream.into_subscription(pending, Provenance::root("cone")).await;
            }
        };

        let stream = self.delete_stream(cone_id).await;
        stream
            .into_subscription(pending, Provenance::root("cone"))
            .await
    }

    async fn chat(
        &self,
        pending: PendingSubscriptionSink,
        cone_id: String,
        prompt: String,
    ) -> SubscriptionResult {
        let cone_id = match uuid::Uuid::parse_str(&cone_id) {
            Ok(id) => id,
            Err(e) => {
                let stream: Pin<Box<dyn Stream<Item = ConeEvent> + Send>> = Box::pin(stream! {
                    yield ConeEvent::Error { message: format!("Invalid cone ID: {}", e) };
                });
                return stream.into_subscription(pending, Provenance::root("cone")).await;
            }
        };

        let stream = self.chat_stream(cone_id, prompt).await;
        stream
            .into_subscription(pending, Provenance::root("cone"))
            .await
    }

    async fn set_head(
        &self,
        pending: PendingSubscriptionSink,
        cone_id: String,
        node_id: String,
    ) -> SubscriptionResult {
        let cone_id = match uuid::Uuid::parse_str(&cone_id) {
            Ok(id) => id,
            Err(e) => {
                let stream: Pin<Box<dyn Stream<Item = ConeEvent> + Send>> = Box::pin(stream! {
                    yield ConeEvent::Error { message: format!("Invalid cone ID: {}", e) };
                });
                return stream.into_subscription(pending, Provenance::root("cone")).await;
            }
        };

        let node_id = match NodeId::parse_str(&node_id) {
            Ok(id) => id,
            Err(e) => {
                let stream: Pin<Box<dyn Stream<Item = ConeEvent> + Send>> = Box::pin(stream! {
                    yield ConeEvent::Error { message: format!("Invalid node ID: {}", e) };
                });
                return stream.into_subscription(pending, Provenance::root("cone")).await;
            }
        };

        let stream = self.set_head_stream(cone_id, node_id).await;
        stream
            .into_subscription(pending, Provenance::root("cone"))
            .await
    }

    async fn registry(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.registry_stream();
        stream
            .into_subscription(pending, Provenance::root("cone"))
            .await
    }
}

#[async_trait]
impl Activation for Cone {
    fn namespace(&self) -> &str {
        "cone"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "LLM cone with persistent conversation context"
    }

    fn methods(&self) -> Vec<&str> {
        vec!["create", "get", "list", "delete", "chat", "set_head", "registry"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        match method {
            "create" => Some(
                "Create a new cone.\n\
                Parameters:\n\
                  name: string - Human-readable name\n\
                  model_id: string - LLM model ID (e.g., 'gpt-4o-mini')\n\
                  system_prompt: string? - Optional system instructions\n\
                  metadata: object? - Optional metadata\n\
                Returns: { type: 'cone_created', cone_id, tree_id }"
                    .to_string(),
            ),
            "get" => Some(
                "Get cone configuration.\n\
                Parameters:\n\
                  cone_id: string - UUID of the cone\n\
                Returns: { type: 'cone_data', cone: ConeConfig }"
                    .to_string(),
            ),
            "list" => Some(
                "List all cones.\n\
                Parameters: none\n\
                Returns: { type: 'cone_list', cones: [ConeInfo] }"
                    .to_string(),
            ),
            "delete" => Some(
                "Delete an cone (tree is preserved).\n\
                Parameters:\n\
                  cone_id: string - UUID of the cone\n\
                Returns: { type: 'cone_deleted', cone_id }"
                    .to_string(),
            ),
            "chat" => Some(
                "Chat with an cone. Streams LLM response and advances context.\n\
                Parameters:\n\
                  cone_id: string - UUID of the cone\n\
                  prompt: string - User message\n\
                Returns (streaming):\n\
                  { type: 'chat_start', cone_id, node_id }\n\
                  { type: 'chat_content', cone_id, content }  (multiple)\n\
                  { type: 'chat_complete', cone_id, response_node_id, new_head, usage? }"
                    .to_string(),
            ),
            "set_head" => Some(
                "Move cone's context head to a different node.\n\
                Parameters:\n\
                  cone_id: string - UUID of the cone\n\
                  node_id: string - UUID of the target node\n\
                Returns: { type: 'head_updated', cone_id, old_head, new_head }"
                    .to_string(),
            ),
            "registry" => Some(
                "Get available LLM services and models.\n\
                Parameters: none\n\
                Returns: { type: 'registry', services: [...], families: [...], models: [...] }"
                    .to_string(),
            ),
            _ => None,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        let path = Provenance::root("cone");

        match method {
            "create" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'name'".into()))?
                    .to_string();
                let model_id = params
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'model_id'".into()))?
                    .to_string();
                let system_prompt = params
                    .get("system_prompt")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let metadata = params.get("metadata").cloned();

                let stream = self.create_stream(name, model_id, system_prompt, metadata).await;
                Ok(into_plexus_stream(stream, path))
            }
            "get" => {
                let cone_id_str = params
                    .get("cone_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'cone_id'".into()))?;
                let cone_id = uuid::Uuid::parse_str(cone_id_str)
                    .map_err(|e| PlexusError::InvalidParams(format!("invalid cone_id: {}", e)))?;

                let stream = self.get_stream(cone_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "list" => {
                let stream = self.list_stream().await;
                Ok(into_plexus_stream(stream, path))
            }
            "delete" => {
                let cone_id_str = params
                    .get("cone_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'cone_id'".into()))?;
                let cone_id = uuid::Uuid::parse_str(cone_id_str)
                    .map_err(|e| PlexusError::InvalidParams(format!("invalid cone_id: {}", e)))?;

                let stream = self.delete_stream(cone_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "chat" => {
                let cone_id_str = params
                    .get("cone_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'cone_id'".into()))?;
                let cone_id = uuid::Uuid::parse_str(cone_id_str)
                    .map_err(|e| PlexusError::InvalidParams(format!("invalid cone_id: {}", e)))?;
                let prompt = params
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'prompt'".into()))?
                    .to_string();

                let stream = self.chat_stream(cone_id, prompt).await;
                Ok(into_plexus_stream(stream, path))
            }
            "set_head" => {
                let cone_id_str = params
                    .get("cone_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'cone_id'".into()))?;
                let cone_id = uuid::Uuid::parse_str(cone_id_str)
                    .map_err(|e| PlexusError::InvalidParams(format!("invalid cone_id: {}", e)))?;
                let node_id_str = params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'node_id'".into()))?;
                let node_id = NodeId::parse_str(node_id_str)
                    .map_err(|e| PlexusError::InvalidParams(format!("invalid node_id: {}", e)))?;

                let stream = self.set_head_stream(cone_id, node_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "registry" => {
                let stream = self.registry_stream();
                Ok(into_plexus_stream(stream, path))
            }
            _ => Err(PlexusError::MethodNotFound {
                activation: "cone".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn into_rpc_methods(self) -> Methods {
        self.into_rpc().into()
    }
}

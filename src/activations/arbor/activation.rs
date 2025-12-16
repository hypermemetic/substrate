use super::methods::ArborMethod;
use super::storage::{ArborConfig, ArborStorage};
use super::types::{ArborEvent, NodeId, TreeId, TreeSkeleton};
use crate::{
    plexus::{into_plexus_stream, Provenance, PlexusError, PlexusStream, InnerActivation},
    plugin_system::conversion::{IntoSubscription, SubscriptionResult},
};
use async_trait::async_trait;
use futures::{stream, Stream};
use jsonrpsee::{core::server::Methods, proc_macros::rpc, PendingSubscriptionSink};
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;

/// RPC adapter trait - defines the JSON-RPC interface for Arbor
#[rpc(server, namespace = "arbor")]
pub trait ArborRpc {
    // ========================================================================
    // Tree Operations
    // ========================================================================

    /// Create a new tree
    #[subscription(
        name = "tree_create",
        unsubscribe = "unsubscribe_tree_create",
        item = serde_json::Value
    )]
    async fn tree_create(&self, metadata: Option<Value>, owner_id: String) -> SubscriptionResult;

    /// Get a tree
    #[subscription(
        name = "tree_get",
        unsubscribe = "unsubscribe_tree_get",
        item = serde_json::Value
    )]
    async fn tree_get(&self, tree_id: TreeId) -> SubscriptionResult;

    /// Get tree skeleton (lightweight structure)
    #[subscription(
        name = "tree_get_skeleton",
        unsubscribe = "unsubscribe_tree_get_skeleton",
        item = serde_json::Value
    )]
    async fn tree_get_skeleton(&self, tree_id: TreeId) -> SubscriptionResult;

    /// List all trees
    #[subscription(
        name = "tree_list",
        unsubscribe = "unsubscribe_tree_list",
        item = serde_json::Value
    )]
    async fn tree_list(&self) -> SubscriptionResult;

    /// Update tree metadata
    #[subscription(
        name = "tree_update_metadata",
        unsubscribe = "unsubscribe_tree_update_metadata",
        item = serde_json::Value
    )]
    async fn tree_update_metadata(&self, tree_id: TreeId, metadata: Value) -> SubscriptionResult;

    /// Claim ownership of a tree
    #[subscription(
        name = "tree_claim",
        unsubscribe = "unsubscribe_tree_claim",
        item = serde_json::Value
    )]
    async fn tree_claim(&self, tree_id: TreeId, owner_id: String, count: i64) -> SubscriptionResult;

    /// Release ownership of a tree
    #[subscription(
        name = "tree_release",
        unsubscribe = "unsubscribe_tree_release",
        item = serde_json::Value
    )]
    async fn tree_release(&self, tree_id: TreeId, owner_id: String, count: i64) -> SubscriptionResult;

    /// List trees scheduled for deletion
    #[subscription(
        name = "tree_list_scheduled",
        unsubscribe = "unsubscribe_tree_list_scheduled",
        item = serde_json::Value
    )]
    async fn tree_list_scheduled(&self) -> SubscriptionResult;

    /// List archived trees
    #[subscription(
        name = "tree_list_archived",
        unsubscribe = "unsubscribe_tree_list_archived",
        item = serde_json::Value
    )]
    async fn tree_list_archived(&self) -> SubscriptionResult;

    // ========================================================================
    // Node Operations
    // ========================================================================

    /// Create a text node
    #[subscription(
        name = "node_create_text",
        unsubscribe = "unsubscribe_node_create_text",
        item = serde_json::Value
    )]
    async fn node_create_text(&self, tree_id: TreeId, parent: Option<NodeId>, content: String, metadata: Option<Value>) -> SubscriptionResult;

    /// Create an external node
    #[subscription(
        name = "node_create_external",
        unsubscribe = "unsubscribe_node_create_external",
        item = serde_json::Value
    )]
    async fn node_create_external(&self, tree_id: TreeId, parent: Option<NodeId>, handle: super::types::Handle, metadata: Option<Value>) -> SubscriptionResult;

    /// Get a node
    #[subscription(
        name = "node_get",
        unsubscribe = "unsubscribe_node_get",
        item = serde_json::Value
    )]
    async fn node_get(&self, tree_id: TreeId, node_id: NodeId) -> SubscriptionResult;

    /// Get node children
    #[subscription(
        name = "node_get_children",
        unsubscribe = "unsubscribe_node_get_children",
        item = serde_json::Value
    )]
    async fn node_get_children(&self, tree_id: TreeId, node_id: NodeId) -> SubscriptionResult;

    /// Get node parent
    #[subscription(
        name = "node_get_parent",
        unsubscribe = "unsubscribe_node_get_parent",
        item = serde_json::Value
    )]
    async fn node_get_parent(&self, tree_id: TreeId, node_id: NodeId) -> SubscriptionResult;

    /// Get path from root to node
    #[subscription(
        name = "node_get_path",
        unsubscribe = "unsubscribe_node_get_path",
        item = serde_json::Value
    )]
    async fn node_get_path(&self, tree_id: TreeId, node_id: NodeId) -> SubscriptionResult;

    // ========================================================================
    // Context Operations
    // ========================================================================

    /// List leaf nodes
    #[subscription(
        name = "context_list_leaves",
        unsubscribe = "unsubscribe_context_list_leaves",
        item = serde_json::Value
    )]
    async fn context_list_leaves(&self, tree_id: TreeId) -> SubscriptionResult;

    /// Get full path data to a node
    #[subscription(
        name = "context_get_path",
        unsubscribe = "unsubscribe_context_get_path",
        item = serde_json::Value
    )]
    async fn context_get_path(&self, tree_id: TreeId, node_id: NodeId) -> SubscriptionResult;

    /// Get external handles in path to node
    #[subscription(
        name = "context_get_handles",
        unsubscribe = "unsubscribe_context_get_handles",
        item = serde_json::Value
    )]
    async fn context_get_handles(&self, tree_id: TreeId, node_id: NodeId) -> SubscriptionResult;

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render tree as text
    #[subscription(
        name = "tree_render",
        unsubscribe = "unsubscribe_tree_render",
        item = serde_json::Value
    )]
    async fn tree_render(&self, tree_id: TreeId) -> SubscriptionResult;
}

/// Arbor plugin - manages conversation trees
#[derive(Clone)]
pub struct Arbor {
    storage: Arc<ArborStorage>,
}

impl Arbor {
    /// Create a new Arbor activation with its own storage
    pub async fn new(config: ArborConfig) -> Result<Self, String> {
        let storage = ArborStorage::new(config)
            .await
            .map_err(|e| format!("Failed to initialize Arbor storage: {}", e.message))?;

        Ok(Self {
            storage: Arc::new(storage),
        })
    }

    /// Create an Arbor activation with a shared storage instance
    pub fn with_storage(storage: Arc<ArborStorage>) -> Self {
        Self { storage }
    }

    /// Get the underlying storage (for sharing with other activations)
    pub fn storage(&self) -> Arc<ArborStorage> {
        self.storage.clone()
    }

    // ========================================================================
    // Implementation methods (return streams)
    // ========================================================================

    async fn tree_create_stream(
        &self,
        metadata: Option<Value>,
        owner_id: String,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_create(metadata, &owner_id).await {
                Ok(tree_id) => ArborEvent::TreeCreated { tree_id },
                Err(e) => {
                    // In case of error, we can't yield ArborError since stream type is ArborEvent
                    // Log and return a TreeCreated with nil UUID as a signal
                    eprintln!("Error creating tree: {}", e.message);
                    ArborEvent::TreeCreated {
                        tree_id: TreeId::nil(),
                    }
                }
            }
        }))
    }

    async fn tree_get_stream(
        &self,
        tree_id: TreeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_get(&tree_id).await {
                Ok(tree) => ArborEvent::TreeData { tree },
                Err(e) => {
                    eprintln!("Error getting tree: {}", e.message);
                    // Return error event - we need to handle this better
                    // For now, create an empty tree structure
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_get_skeleton_stream(
        &self,
        tree_id: TreeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_get(&tree_id).await {
                Ok(tree) => ArborEvent::TreeSkeleton {
                    skeleton: TreeSkeleton::from(&tree),
                },
                Err(e) => {
                    eprintln!("Error getting tree skeleton: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_list_stream(&self) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_list(false).await {
                Ok(tree_ids) => ArborEvent::TreeList { tree_ids },
                Err(e) => {
                    eprintln!("Error listing trees: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_update_metadata_stream(
        &self,
        tree_id: TreeId,
        metadata: Value,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_update_metadata(&tree_id, metadata).await {
                Ok(_) => ArborEvent::TreeUpdated { tree_id },
                Err(e) => {
                    eprintln!("Error updating tree metadata: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_claim_stream(
        &self,
        tree_id: TreeId,
        owner_id: String,
        count: i64,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_claim(&tree_id, &owner_id, count).await {
                Ok(new_count) => ArborEvent::TreeClaimed {
                    tree_id,
                    owner_id,
                    new_count,
                },
                Err(e) => {
                    eprintln!("Error claiming tree: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_release_stream(
        &self,
        tree_id: TreeId,
        owner_id: String,
        count: i64,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_release(&tree_id, &owner_id, count).await {
                Ok(new_count) => ArborEvent::TreeReleased {
                    tree_id,
                    owner_id,
                    new_count,
                },
                Err(e) => {
                    eprintln!("Error releasing tree: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_list_scheduled_stream(&self) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_list(true).await {
                Ok(tree_ids) => ArborEvent::TreesScheduled { tree_ids },
                Err(e) => {
                    eprintln!("Error listing scheduled trees: {}", e.message);
                    ArborEvent::TreesScheduled { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_list_archived_stream(&self) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            // Use tree_list with include_scheduled=true to get archived trees
            // TODO: Add dedicated tree_list_archived method to storage if needed
            match storage.tree_list(true).await {
                Ok(tree_ids) => ArborEvent::TreesArchived { tree_ids },
                Err(e) => {
                    eprintln!("Error listing archived trees: {}", e.message);
                    ArborEvent::TreesArchived { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn node_create_text_stream(
        &self,
        tree_id: TreeId,
        parent: Option<NodeId>,
        content: String,
        metadata: Option<Value>,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.node_create_text(&tree_id, parent, content, metadata).await {
                Ok(node_id) => ArborEvent::NodeCreated {
                    tree_id,
                    node_id,
                    parent,
                },
                Err(e) => {
                    eprintln!("Error creating text node: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn node_create_external_stream(
        &self,
        tree_id: TreeId,
        parent: Option<NodeId>,
        handle: super::types::Handle,
        metadata: Option<Value>,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.node_create_external(&tree_id, parent, handle, metadata).await {
                Ok(node_id) => ArborEvent::NodeCreated {
                    tree_id,
                    node_id,
                    parent,
                },
                Err(e) => {
                    eprintln!("Error creating external node: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn node_get_stream(
        &self,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.node_get(&tree_id, &node_id).await {
                Ok(node) => ArborEvent::NodeData { tree_id, node },
                Err(e) => {
                    eprintln!("Error getting node: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn node_get_children_stream(
        &self,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.node_get_children(&tree_id, &node_id).await {
                Ok(children) => ArborEvent::NodeChildren {
                    tree_id,
                    node_id,
                    children,
                },
                Err(e) => {
                    eprintln!("Error getting node children: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn node_get_parent_stream(
        &self,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.node_get_parent(&tree_id, &node_id).await {
                Ok(parent) => ArborEvent::NodeParent {
                    tree_id,
                    node_id,
                    parent,
                },
                Err(e) => {
                    eprintln!("Error getting node parent: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn node_get_path_stream(
        &self,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.node_get_path(&tree_id, &node_id).await {
                Ok(path) => ArborEvent::ContextPath { tree_id, path },
                Err(e) => {
                    eprintln!("Error getting node path: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn context_list_leaves_stream(
        &self,
        tree_id: TreeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.context_list_leaves(&tree_id).await {
                Ok(leaves) => ArborEvent::ContextLeaves { tree_id, leaves },
                Err(e) => {
                    eprintln!("Error listing leaf nodes: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn context_get_path_stream(
        &self,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.context_get_path(&tree_id, &node_id).await {
                Ok(nodes) => ArborEvent::ContextPathData { tree_id, nodes },
                Err(e) => {
                    eprintln!("Error getting context path: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn context_get_handles_stream(
        &self,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.context_get_handles(&tree_id, &node_id).await {
                Ok(handles) => ArborEvent::ContextHandles { tree_id, handles },
                Err(e) => {
                    eprintln!("Error getting context handles: {}", e.message);
                    ArborEvent::TreeList { tree_ids: vec![] }
                }
            }
        }))
    }

    async fn tree_render_stream(
        &self,
        tree_id: TreeId,
    ) -> Pin<Box<dyn Stream<Item = ArborEvent> + Send + 'static>> {
        let storage = self.storage.clone();
        Box::pin(stream::once(async move {
            let storage = storage.as_ref();
            match storage.tree_get(&tree_id).await {
                Ok(tree) => ArborEvent::TreeRender {
                    tree_id,
                    render: tree.render(),
                },
                Err(e) => {
                    eprintln!("Error rendering tree: {}", e.message);
                    ArborEvent::TreeRender {
                        tree_id,
                        render: format!("Error: {}", e.message),
                    }
                }
            }
        }))
    }
}

/// RPC adapter implementation - bridges core system to RPC
#[async_trait]
impl ArborRpcServer for Arbor {
    async fn tree_create(
        &self,
        pending: PendingSubscriptionSink,
        metadata: Option<Value>,
        owner_id: String,
    ) -> SubscriptionResult {
        let stream = self.tree_create_stream(metadata, owner_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_get(&self, pending: PendingSubscriptionSink, tree_id: TreeId) -> SubscriptionResult {
        let stream = self.tree_get_stream(tree_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_get_skeleton(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
    ) -> SubscriptionResult {
        let stream = self.tree_get_skeleton_stream(tree_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_list(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.tree_list_stream().await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_update_metadata(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        metadata: Value,
    ) -> SubscriptionResult {
        let stream = self.tree_update_metadata_stream(tree_id, metadata).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_claim(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        owner_id: String,
        count: i64,
    ) -> SubscriptionResult {
        let stream = self.tree_claim_stream(tree_id, owner_id, count).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_release(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        owner_id: String,
        count: i64,
    ) -> SubscriptionResult {
        let stream = self.tree_release_stream(tree_id, owner_id, count).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_list_scheduled(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.tree_list_scheduled_stream().await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_list_archived(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        let stream = self.tree_list_archived_stream().await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn node_create_text(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        parent: Option<NodeId>,
        content: String,
        metadata: Option<Value>,
    ) -> SubscriptionResult {
        let stream = self.node_create_text_stream(tree_id, parent, content, metadata).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn node_create_external(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        parent: Option<NodeId>,
        handle: super::types::Handle,
        metadata: Option<Value>,
    ) -> SubscriptionResult {
        let stream = self.node_create_external_stream(tree_id, parent, handle, metadata).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn node_get(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> SubscriptionResult {
        let stream = self.node_get_stream(tree_id, node_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn node_get_children(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> SubscriptionResult {
        let stream = self.node_get_children_stream(tree_id, node_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn node_get_parent(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> SubscriptionResult {
        let stream = self.node_get_parent_stream(tree_id, node_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn node_get_path(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> SubscriptionResult {
        let stream = self.node_get_path_stream(tree_id, node_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn context_list_leaves(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
    ) -> SubscriptionResult {
        let stream = self.context_list_leaves_stream(tree_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn context_get_path(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> SubscriptionResult {
        let stream = self.context_get_path_stream(tree_id, node_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn context_get_handles(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
        node_id: NodeId,
    ) -> SubscriptionResult {
        let stream = self.context_get_handles_stream(tree_id, node_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }

    async fn tree_render(
        &self,
        pending: PendingSubscriptionSink,
        tree_id: TreeId,
    ) -> SubscriptionResult {
        let stream = self.tree_render_stream(tree_id).await;
        stream
            .into_subscription(pending, Provenance::root("arbor"))
            .await
    }
}

/// Plugin trait implementation - unified interface for hub
#[async_trait]
impl InnerActivation for Arbor {
    type Methods = ArborMethod;

    fn namespace(&self) -> &str {
        "arbor"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Manage conversation trees with context tracking"
    }

    fn methods(&self) -> Vec<&str> {
        ArborMethod::all_names()
    }

    fn method_help(&self, method: &str) -> Option<String> {
        ArborMethod::description(method).map(|s| s.to_string())
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        let path = Provenance::root("arbor");

        match method {
            "tree_create" => {
                let metadata = params.get("metadata").cloned();
                let owner_id = params
                    .get("owner_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("system")
                    .to_string();
                let stream = self.tree_create_stream(metadata, owner_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_get" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let stream = self.tree_get_stream(tree_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_get_skeleton" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let stream = self.tree_get_skeleton_stream(tree_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_list" => {
                let stream = self.tree_list_stream().await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_update_metadata" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let metadata = params
                    .get("metadata")
                    .cloned()
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'metadata'".into()))?;
                let stream = self.tree_update_metadata_stream(tree_id, metadata).await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_claim" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let owner_id = params
                    .get("owner_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'owner_id'".into()))?
                    .to_string();
                let count = params
                    .get("count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let stream = self.tree_claim_stream(tree_id, owner_id, count).await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_release" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let owner_id = params
                    .get("owner_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'owner_id'".into()))?
                    .to_string();
                let count = params
                    .get("count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let stream = self.tree_release_stream(tree_id, owner_id, count).await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_list_scheduled" => {
                let stream = self.tree_list_scheduled_stream().await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_list_archived" => {
                let stream = self.tree_list_archived_stream().await;
                Ok(into_plexus_stream(stream, path))
            }
            "node_create_text" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let parent = params
                    .get("parent")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok());
                let content = params
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'content'".into()))?
                    .to_string();
                let metadata = params.get("metadata").cloned();
                let stream = self.node_create_text_stream(tree_id, parent, content, metadata).await;
                Ok(into_plexus_stream(stream, path))
            }
            "node_create_external" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let parent = params
                    .get("parent")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok());
                let handle: super::types::Handle = params
                    .get("handle")
                    .cloned()
                    .ok_or_else(|| PlexusError::InvalidParams("missing 'handle'".into()))
                    .and_then(|v| serde_json::from_value(v).map_err(|e| PlexusError::InvalidParams(format!("invalid 'handle': {}", e))))?;
                let metadata = params.get("metadata").cloned();
                let stream = self.node_create_external_stream(tree_id, parent, handle, metadata).await;
                Ok(into_plexus_stream(stream, path))
            }
            "node_get" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let node_id = params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'node_id'".into()))?;
                let stream = self.node_get_stream(tree_id, node_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "node_get_children" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let node_id = params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'node_id'".into()))?;
                let stream = self.node_get_children_stream(tree_id, node_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "node_get_parent" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let node_id = params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'node_id'".into()))?;
                let stream = self.node_get_parent_stream(tree_id, node_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "node_get_path" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let node_id = params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'node_id'".into()))?;
                let stream = self.node_get_path_stream(tree_id, node_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "context_list_leaves" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let stream = self.context_list_leaves_stream(tree_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "context_get_path" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let node_id = params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'node_id'".into()))?;
                let stream = self.context_get_path_stream(tree_id, node_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "context_get_handles" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let node_id = params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NodeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'node_id'".into()))?;
                let stream = self.context_get_handles_stream(tree_id, node_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            "tree_render" => {
                let tree_id = params
                    .get("tree_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| TreeId::parse_str(s).ok())
                    .ok_or_else(|| PlexusError::InvalidParams("missing or invalid 'tree_id'".into()))?;
                let stream = self.tree_render_stream(tree_id).await;
                Ok(into_plexus_stream(stream, path))
            }
            _ => Err(PlexusError::MethodNotFound {
                activation: "arbor".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn into_rpc_methods(self) -> Methods {
        self.into_rpc().into()
    }
}

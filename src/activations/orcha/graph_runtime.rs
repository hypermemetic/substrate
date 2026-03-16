use crate::activations::arbor::ArborStorage;
use crate::activations::lattice::{
    GatherStrategy, LatticeEvent, LatticeEventEnvelope, LatticeStorage, NodeOutput, NodeSpec,
    NodeStatus, ResolvedToken, Token, TokenPayload,
};
use futures::Stream;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use super::types::{OrchaEdgeDef, OrchaNodeDef, OrchaNodeKind, OrchaNodeSpec};

// ─── GraphRuntime (factory) ───────────────────────────────────────────────────

/// Orcha's interface to the graph execution engine.
///
/// Serves as the factory for `OrchaGraph` handles.  The lattice backend is
/// an implementation detail — callers only ever see `OrchaGraph`.
#[derive(Clone)]
pub struct GraphRuntime {
    storage: Arc<LatticeStorage>,
}

impl GraphRuntime {
    pub fn new(storage: Arc<LatticeStorage>) -> Self {
        Self { storage }
    }

    /// Expose the underlying lattice storage.
    pub fn storage(&self) -> Arc<LatticeStorage> {
        self.storage.clone()
    }

    /// Create a new execution graph.
    pub async fn create_graph(&self, metadata: Value) -> Result<OrchaGraph, String> {
        let graph_id = self.storage.create_graph(metadata).await?;
        Ok(OrchaGraph {
            graph_id,
            storage: self.storage.clone(),
        })
    }

    /// Create a new execution graph as a child of an existing graph.
    pub async fn create_child_graph(
        &self,
        parent_id: &str,
        metadata: Value,
    ) -> Result<OrchaGraph, String> {
        let graph_id = self.storage.create_child_graph(parent_id, metadata).await?;
        Ok(OrchaGraph {
            graph_id,
            storage: self.storage.clone(),
        })
    }

    /// Build a child graph from node+edge definitions.
    ///
    /// Creates the child graph under `parent_id`, adds all nodes, and wires all edges.
    /// Returns `(child_graph_id, ticket_id→lattice_node_id map)` on success.
    pub async fn build_child_graph(
        &self,
        parent_id: &str,
        metadata: Value,
        nodes: Vec<OrchaNodeDef>,
        edges: Vec<OrchaEdgeDef>,
    ) -> Result<(String, HashMap<String, String>), String> {
        let graph = self.create_child_graph(parent_id, metadata).await?;
        let graph_id = graph.graph_id.clone();

        let mut id_map: HashMap<String, String> = HashMap::new();
        for OrchaNodeDef { id, spec } in nodes {
            let result = match spec {
                OrchaNodeSpec::Task { task, max_retries } => graph.add_task(task, max_retries).await,
                OrchaNodeSpec::Synthesize { task, max_retries } => graph.add_synthesize(task, max_retries).await,
                OrchaNodeSpec::Validate { command, cwd, max_retries } => graph.add_validate(command, cwd, max_retries).await,
                OrchaNodeSpec::Gather { strategy } => graph.add_gather(strategy).await,
                OrchaNodeSpec::Review { prompt } => graph.add_review(prompt).await,
                OrchaNodeSpec::Plan { task } => graph.add_plan(task).await,
            };
            let lattice_id = result.map_err(|e| format!("Failed to add node '{}': {}", id, e))?;
            id_map.insert(id, lattice_id);
        }

        for OrchaEdgeDef { from, to } in edges {
            let dep_id = id_map
                .get(&from)
                .ok_or_else(|| format!("Unknown node id in edge.from: '{}'", from))?
                .clone();
            let node_id = id_map
                .get(&to)
                .ok_or_else(|| format!("Unknown node id in edge.to: '{}'", to))?
                .clone();
            graph
                .depends_on(&node_id, &dep_id)
                .await
                .map_err(|e| format!("Failed to add edge {} → {}: {}", from, to, e))?;
        }

        Ok((graph_id, id_map))
    }

    /// Open an existing graph by ID.
    pub fn open_graph(&self, graph_id: String) -> OrchaGraph {
        OrchaGraph {
            graph_id,
            storage: self.storage.clone(),
        }
    }
}

// ─── OrchaGraph ───────────────────────────────────────────────────────────────

/// A handle to a single execution graph.
///
/// Provides Orcha's typed node-building API and graph-scoped execution control.
/// No lattice types leak past this boundary — callers work entirely in Orcha
/// concepts (tasks, validate steps, synthesize steps, review gates).
#[derive(Clone)]
pub struct OrchaGraph {
    /// The lattice graph ID — exposed for logging / monitoring.
    pub graph_id: String,
    storage: Arc<LatticeStorage>,
}

impl OrchaGraph {
    // ─── Node builders ───────────────────────────────────────────────────────

    /// Add a task node.
    pub async fn add_task(&self, task: impl Into<String>, max_retries: Option<u8>) -> Result<String, String> {
        let kind = OrchaNodeKind::Task { task: task.into(), max_retries };
        self.add_spec(NodeSpec::Task {
            data: serde_json::to_value(&kind).map_err(|e| e.to_string())?,
            handle: None,
        })
        .await
    }

    /// Add a synthesize node.
    ///
    /// Like task, but graph_runner prepends resolved input tokens as `<prior_work>` context.
    pub async fn add_synthesize(&self, task: impl Into<String>, max_retries: Option<u8>) -> Result<String, String> {
        let kind = OrchaNodeKind::Synthesize { task: task.into(), max_retries };
        self.add_spec(NodeSpec::Task {
            data: serde_json::to_value(&kind).map_err(|e| e.to_string())?,
            handle: None,
        })
        .await
    }

    /// Add a validate node.
    ///
    /// Orcha runs `command` in a shell inside `cwd` (default `/workspace`).
    pub async fn add_validate(
        &self,
        command: impl Into<String>,
        cwd: Option<impl Into<String>>,
        max_retries: Option<u8>,
    ) -> Result<String, String> {
        let kind = OrchaNodeKind::Validate {
            command: command.into(),
            cwd: cwd.map(|d| d.into()),
            max_retries,
        };
        self.add_spec(NodeSpec::Task {
            data: serde_json::to_value(&kind).map_err(|e| e.to_string())?,
            handle: None,
        })
        .await
    }

    /// Add a gather node — engine-executed, auto-fires when all inbound tokens arrive.
    pub async fn add_gather(&self, strategy: GatherStrategy) -> Result<String, String> {
        self.add_spec(NodeSpec::Gather { strategy }).await
    }

    /// Add a SubGraph node — when ready, runs the child graph to completion.
    pub async fn add_subgraph(&self, child_graph_id: impl Into<String>) -> Result<String, String> {
        self.add_spec(NodeSpec::SubGraph { graph_id: child_graph_id.into() }).await
    }

    /// Open a sibling graph by ID sharing the same LatticeStorage.
    pub fn open_child_graph(&self, graph_id: impl Into<String>) -> OrchaGraph {
        OrchaGraph { graph_id: graph_id.into(), storage: self.storage.clone() }
    }

    /// Add a plan node.
    ///
    /// When dispatched, runs Claude to produce a ticket file, compiles it into
    /// a child graph, and executes the child graph inline.
    pub async fn add_plan(&self, task: impl Into<String>) -> Result<String, String> {
        let kind = OrchaNodeKind::Plan { task: task.into() };
        self.add_spec(NodeSpec::Task {
            data: serde_json::to_value(&kind).map_err(|e| e.to_string())?,
            handle: None,
        })
        .await
    }

    /// Add a review node.
    pub async fn add_review(&self, prompt: impl Into<String>) -> Result<String, String> {
        let kind = OrchaNodeKind::Review { prompt: prompt.into() };
        self.add_spec(NodeSpec::Task {
            data: serde_json::to_value(&kind).map_err(|e| e.to_string())?,
            handle: None,
        })
        .await
    }

    /// Declare that `dependent` waits for `dependency` to complete first.
    ///
    /// ```text
    /// graph.depends_on(&validate_node, &task_node)
    /// // validate_node will not start until task_node is complete
    /// ```
    pub async fn depends_on(
        &self,
        dependent: &str,
        dependency: &str,
    ) -> Result<(), String> {
        self.storage
            .add_edge(
                &self.graph_id,
                &dependency.to_string(),
                &dependent.to_string(),
                None,
            )
            .await
    }

    // ─── Execution control ───────────────────────────────────────────────────

    /// Watch this graph's event stream.
    pub fn watch(
        &self,
        after_seq: Option<u64>,
    ) -> impl Stream<Item = LatticeEventEnvelope> + Send + 'static {
        LatticeStorage::execute_stream(self.storage.clone(), self.graph_id.clone(), after_seq)
    }

    /// Signal that a node started executing.
    pub async fn start_node(&self, node_id: &str) -> Result<(), String> {
        self.storage
            .set_node_status(&node_id.to_string(), NodeStatus::Running, None, None)
            .await?;
        self.storage
            .persist_event(&self.graph_id, &LatticeEvent::NodeStarted {
                node_id: node_id.to_string(),
            })
            .await?;
        self.storage.notify_graph(&self.graph_id);
        Ok(())
    }

    /// Signal that a node completed successfully, optionally carrying output.
    pub async fn complete_node(
        &self,
        node_id: &str,
        output: Option<NodeOutput>,
    ) -> Result<(), String> {
        self.storage
            .advance_graph(&self.graph_id, &node_id.to_string(), output, None)
            .await
    }

    /// Signal that a node failed.
    pub async fn fail_node(&self, node_id: &str, error: String) -> Result<(), String> {
        self.storage
            .advance_graph(&self.graph_id, &node_id.to_string(), None, Some(error))
            .await
    }

    /// Get IDs of nodes that have an edge pointing into `node_id`.
    pub async fn get_inbound_node_ids(&self, node_id: &str) -> Result<Vec<String>, String> {
        self.storage.get_inbound_edges(&node_id.to_string()).await
    }

    /// Get the spec of an existing node.
    pub async fn get_node_spec(&self, node_id: &str) -> Result<NodeSpec, String> {
        self.storage.get_node(&node_id.to_string()).await.map(|n| n.spec)
    }

    /// Get the stored output of a completed node.
    pub async fn get_node_output(&self, node_id: &str) -> Result<Option<NodeOutput>, String> {
        self.storage.get_node(&node_id.to_string()).await.map(|n| n.output)
    }

    /// Count the total number of nodes in this graph.
    pub async fn count_nodes(&self) -> Result<usize, String> {
        self.storage.count_nodes(&self.graph_id).await
    }

    /// Get the IDs of nodes that have already reached a terminal state (Complete or Failed).
    /// Used by run_graph_execution to pre-populate the dispatched set on reconnect.
    pub async fn get_terminal_node_ids(&self) -> Result<Vec<String>, String> {
        let nodes = self.storage.get_nodes(&self.graph_id).await?;
        Ok(nodes.into_iter()
            .filter(|n| n.status == NodeStatus::Complete || n.status == NodeStatus::Failed)
            .map(|n| n.id)
            .collect())
    }

    /// Get raw input tokens for a node (what arrived on all inbound edges).
    pub async fn get_node_inputs(&self, node_id: &str) -> Result<Vec<Token>, String> {
        self.storage.get_node_inputs(&node_id.to_string()).await
    }

    /// Get input tokens with Handle payloads resolved to inline Values.
    ///
    /// Lattice resolves handles server-side via Arbor.
    /// Returns ResolvedToken { color, data: Option<Value> }.
    pub async fn get_resolved_inputs(
        &self,
        node_id: &str,
        arbor: &ArborStorage,
    ) -> Result<Vec<ResolvedToken>, String> {
        let tokens = self.storage.get_node_inputs(&node_id.to_string()).await?;
        let mut resolved = Vec::new();
        for token in tokens {
            let data = match token.payload {
                None => None,
                Some(TokenPayload::Data { value }) => Some(value),
                Some(TokenPayload::Handle(handle)) => {
                    let text = resolve_handle(arbor, &handle).await?;
                    Some(serde_json::json!({ "text": text }))
                }
            };
            resolved.push(ResolvedToken { color: token.color, data });
        }
        Ok(resolved)
    }

    // ─── Internal ────────────────────────────────────────────────────────────

    async fn add_spec(&self, spec: NodeSpec) -> Result<String, String> {
        self.storage.add_node(&self.graph_id, None, &spec).await
    }
}

// ─── Handle Resolution ────────────────────────────────────────────────────────

use crate::activations::arbor::{ArborId, NodeType};

/// Resolve an arbor_tree Handle into a context string.
pub(crate) async fn resolve_handle(
    arbor: &ArborStorage,
    handle: &plexus_core::types::Handle,
) -> Result<String, String> {
    match handle.method.as_str() {
        "arbor_tree" => {
            let tree_id_str = handle
                .meta
                .first()
                .ok_or("arbor_tree handle missing tree_id in meta[0]")?;
            let node_id_str = handle
                .meta
                .get(1)
                .ok_or("arbor_tree handle missing node_id in meta[1]")?;

            let tree_id = ArborId::parse_str(tree_id_str)
                .map_err(|e| format!("Invalid tree_id in handle meta[0]: {}", e))?;
            let node_id = ArborId::parse_str(node_id_str)
                .map_err(|e| format!("Invalid node_id in handle meta[1]: {}", e))?;

            let path = arbor
                .context_get_path(&tree_id, &node_id)
                .await
                .map_err(|e| format!("Failed to resolve arbor_tree handle: {}", e))?;

            let context = path
                .iter()
                .filter_map(|node| match &node.data {
                    NodeType::Text { content } => Some(content.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            Ok(context)
        }
        other => Err(format!("Unknown handle method: {}", other)),
    }
}

use crate::activations::lattice::{LatticeStorage, NodeSpec, NodeStatus};
use crate::activations::orcha::OrchaNodeKind;
use crate::plexus::{Activation, ChildRouter, PlexusError, PlexusStream};
use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use plexus_macros::hub_methods;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use super::storage::PmStorage;

// ─── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PmTicketStatus {
    pub ticket_id: String,
    pub node_id: String,
    pub status: String,
    pub kind: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PmGraphStatusResult {
    Ok {
        graph_id: String,
        graph_status: String,
        tickets: Vec<PmTicketStatus>,
    },
    Err {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PmWhatNextResult {
    Ok {
        graph_id: String,
        tickets: Vec<PmTicketStatus>,
    },
    Err {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PmInspectResult {
    Ok {
        ticket_id: String,
        node_id: String,
        status: String,
        kind: String,
        task: Option<String>,
        command: Option<String>,
        output: Option<Value>,
        error: Option<String>,
    },
    NotFound {
        ticket_id: String,
    },
    Err {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PmWhyBlockedResult {
    Ok {
        ticket_id: String,
        blocked_by: Vec<PmTicketStatus>,
    },
    NotBlocked {
        ticket_id: String,
    },
    Err {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PmGraphSummary {
    pub graph_id: String,
    pub status: String,
    pub metadata: Value,
    pub ticket_count: usize,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PmListGraphsResult {
    Ok {
        graphs: Vec<PmGraphSummary>,
    },
    Err {
        message: String,
    },
}

// ─── Pm activation ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Pm {
    pm_storage: Arc<PmStorage>,
    lattice_storage: Arc<LatticeStorage>,
}

impl Pm {
    pub fn new(pm_storage: Arc<PmStorage>, lattice_storage: Arc<LatticeStorage>) -> Self {
        Self { pm_storage, lattice_storage }
    }

    /// Save ticket→node mappings for a graph (called by Orcha after build).
    pub async fn save_ticket_map(
        &self,
        graph_id: &str,
        map: &HashMap<String, String>,
    ) -> Result<(), String> {
        self.pm_storage.save_ticket_map(graph_id, map).await
    }

    /// Fetch the ticket_id→node_id map for a graph.
    pub async fn get_ticket_map(&self, graph_id: &str) -> Result<HashMap<String, String>, String> {
        self.pm_storage.get_ticket_map(graph_id).await
    }

    /// Return all graph IDs known to PM (regardless of status), most-recent first.
    ///
    /// Used by the startup recovery pass to find graphs that should be re-watched.
    pub async fn list_all_graph_ids(&self) -> Result<Vec<String>, String> {
        let entries = self.pm_storage.list_ticket_maps(usize::MAX).await?;
        Ok(entries.into_iter().map(|(id, _)| id).collect())
    }

    /// Save the raw ticket source for a graph (called by run_tickets / run_tickets_async).
    pub async fn save_ticket_source(&self, graph_id: &str, source: &str) -> Result<(), String> {
        self.pm_storage.save_ticket_source(graph_id, source).await
    }

    /// Fetch the raw ticket source for a graph.
    pub async fn get_ticket_source_raw(&self, graph_id: &str) -> Result<Option<String>, String> {
        self.pm_storage.get_ticket_source(graph_id).await
    }
}

#[async_trait]
impl ChildRouter for Pm {
    fn router_namespace(&self) -> &str {
        "pm"
    }

    async fn router_call(
        &self,
        method: &str,
        params: Value,
    ) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, _name: &str) -> Option<Box<dyn ChildRouter>> {
        None
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn node_status_str(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Pending => "pending",
        NodeStatus::Ready => "ready",
        NodeStatus::Running => "running",
        NodeStatus::Complete => "complete",
        NodeStatus::Failed => "failed",
    }
}

fn extract_kind_and_label(spec: &NodeSpec) -> (String, Option<String>) {
    match spec {
        NodeSpec::Task { data, .. } => {
            match serde_json::from_value::<OrchaNodeKind>(data.clone()) {
                Ok(OrchaNodeKind::Task { task }) => {
                    let label = task.chars().take(80).collect::<String>();
                    ("task".to_string(), Some(label))
                }
                Ok(OrchaNodeKind::Synthesize { task }) => {
                    let label = task.chars().take(80).collect::<String>();
                    ("synthesize".to_string(), Some(label))
                }
                Ok(OrchaNodeKind::Validate { command, .. }) => {
                    let label = command.chars().take(80).collect::<String>();
                    ("validate".to_string(), Some(label))
                }
                Ok(OrchaNodeKind::Review { prompt }) => {
                    let label = prompt.chars().take(80).collect::<String>();
                    ("review".to_string(), Some(label))
                }
                Ok(OrchaNodeKind::Plan { task }) => {
                    let label = task.chars().take(80).collect::<String>();
                    ("plan".to_string(), Some(label))
                }
                Err(_) => ("task".to_string(), None),
            }
        }
        NodeSpec::Gather { .. } => ("gather".to_string(), None),
        NodeSpec::Scatter { .. } => ("scatter".to_string(), None),
        NodeSpec::SubGraph { .. } => ("subgraph".to_string(), None),
    }
}

// ─── Hub methods ─────────────────────────────────────────────────────────────

#[hub_methods(
    namespace = "pm",
    version = "1.0.0",
    description = "Project management view of orcha graph execution in ticket vocabulary"
)]
impl Pm {
    /// Get the status of all tickets in a graph.
    #[plexus_macros::hub_method(params(
        graph_id = "The lattice graph ID returned by build_tickets or run_tickets"
    ))]
    async fn graph_status(
        &self,
        graph_id: String,
    ) -> impl Stream<Item = PmGraphStatusResult> + Send + 'static {
        let pm_storage = self.pm_storage.clone();
        let lattice_storage = self.lattice_storage.clone();

        stream! {
            let ticket_map = match pm_storage.get_ticket_map(&graph_id).await {
                Ok(m) => m,
                Err(e) => { yield PmGraphStatusResult::Err { message: e }; return; }
            };

            let mut tickets = Vec::new();
            let mut has_pending = false;
            let mut has_ready = false;
            let mut has_running = false;
            let mut has_failed = false;
            let mut all_complete = true;

            for (ticket_id, node_id) in &ticket_map {
                match lattice_storage.get_node(node_id).await {
                    Ok(node) => {
                        match node.status {
                            NodeStatus::Pending  => { has_pending  = true; all_complete = false; }
                            NodeStatus::Ready    => { has_ready    = true; all_complete = false; }
                            NodeStatus::Running  => { has_running  = true; all_complete = false; }
                            NodeStatus::Failed   => { has_failed   = true; all_complete = false; }
                            NodeStatus::Complete => {}
                        }
                        let (kind, label) = extract_kind_and_label(&node.spec);
                        tickets.push(PmTicketStatus {
                            ticket_id: ticket_id.clone(),
                            node_id: node_id.clone(),
                            status: node_status_str(&node.status).to_string(),
                            kind,
                            label,
                        });
                    }
                    Err(e) => {
                        yield PmGraphStatusResult::Err {
                            message: format!("Failed to get node {}: {}", node_id, e),
                        };
                        return;
                    }
                }
            }

            let graph_status = if has_failed {
                "failed"
            } else if has_running || has_ready {
                "running"
            } else if has_pending {
                "pending"
            } else if all_complete && !ticket_map.is_empty() {
                "complete"
            } else {
                "pending"
            };

            yield PmGraphStatusResult::Ok {
                graph_id,
                graph_status: graph_status.to_string(),
                tickets,
            };
        }
    }

    /// Get tickets that are ready or running (next actionable items).
    #[plexus_macros::hub_method(params(
        graph_id = "The lattice graph ID returned by build_tickets or run_tickets"
    ))]
    async fn what_next(
        &self,
        graph_id: String,
    ) -> impl Stream<Item = PmWhatNextResult> + Send + 'static {
        let pm_storage = self.pm_storage.clone();
        let lattice_storage = self.lattice_storage.clone();

        stream! {
            let ticket_map = match pm_storage.get_ticket_map(&graph_id).await {
                Ok(m) => m,
                Err(e) => { yield PmWhatNextResult::Err { message: e }; return; }
            };

            let mut tickets = Vec::new();
            for (ticket_id, node_id) in &ticket_map {
                match lattice_storage.get_node(node_id).await {
                    Ok(node) => {
                        if matches!(node.status, NodeStatus::Ready | NodeStatus::Running) {
                            let (kind, label) = extract_kind_and_label(&node.spec);
                            tickets.push(PmTicketStatus {
                                ticket_id: ticket_id.clone(),
                                node_id: node_id.clone(),
                                status: node_status_str(&node.status).to_string(),
                                kind,
                                label,
                            });
                        }
                    }
                    Err(e) => {
                        yield PmWhatNextResult::Err {
                            message: format!("Failed to get node {}: {}", node_id, e),
                        };
                        return;
                    }
                }
            }

            yield PmWhatNextResult::Ok { graph_id, tickets };
        }
    }

    /// Inspect a single ticket in detail.
    #[plexus_macros::hub_method(params(
        graph_id = "The lattice graph ID returned by build_tickets or run_tickets",
        ticket_id = "The ticket ID (as used in the ticket file)"
    ))]
    async fn inspect_ticket(
        &self,
        graph_id: String,
        ticket_id: String,
    ) -> impl Stream<Item = PmInspectResult> + Send + 'static {
        let pm_storage = self.pm_storage.clone();
        let lattice_storage = self.lattice_storage.clone();

        stream! {
            let ticket_map = match pm_storage.get_ticket_map(&graph_id).await {
                Ok(m) => m,
                Err(e) => { yield PmInspectResult::Err { message: e }; return; }
            };

            let node_id = match ticket_map.get(&ticket_id) {
                Some(id) => id.clone(),
                None => { yield PmInspectResult::NotFound { ticket_id }; return; }
            };

            let node = match lattice_storage.get_node(&node_id).await {
                Ok(n) => n,
                Err(e) => {
                    yield PmInspectResult::Err {
                        message: format!("Failed to get node: {}", e),
                    };
                    return;
                }
            };

            let status = node_status_str(&node.status).to_string();
            let output = node.output.as_ref()
                .map(|o| serde_json::to_value(o).unwrap_or(Value::Null));
            let error = node.error.clone();

            match &node.spec {
                NodeSpec::Task { data, .. } => {
                    match serde_json::from_value::<OrchaNodeKind>(data.clone()) {
                        Ok(OrchaNodeKind::Task { task }) => {
                            yield PmInspectResult::Ok {
                                ticket_id, node_id, status,
                                kind: "task".to_string(),
                                task: Some(task), command: None, output, error,
                            };
                        }
                        Ok(OrchaNodeKind::Synthesize { task }) => {
                            yield PmInspectResult::Ok {
                                ticket_id, node_id, status,
                                kind: "synthesize".to_string(),
                                task: Some(task), command: None, output, error,
                            };
                        }
                        Ok(OrchaNodeKind::Validate { command, .. }) => {
                            yield PmInspectResult::Ok {
                                ticket_id, node_id, status,
                                kind: "validate".to_string(),
                                task: None, command: Some(command), output, error,
                            };
                        }
                        Ok(OrchaNodeKind::Review { prompt }) => {
                            yield PmInspectResult::Ok {
                                ticket_id, node_id, status,
                                kind: "review".to_string(),
                                task: Some(prompt), command: None, output, error,
                            };
                        }
                        Ok(OrchaNodeKind::Plan { task }) => {
                            yield PmInspectResult::Ok {
                                ticket_id, node_id, status,
                                kind: "plan".to_string(),
                                task: Some(task), command: None, output, error,
                            };
                        }
                        Err(_) => {
                            yield PmInspectResult::Ok {
                                ticket_id, node_id, status,
                                kind: "task".to_string(),
                                task: None, command: None, output, error,
                            };
                        }
                    }
                }
                NodeSpec::Gather { .. } => {
                    yield PmInspectResult::Ok {
                        ticket_id, node_id, status,
                        kind: "gather".to_string(),
                        task: None, command: None, output, error,
                    };
                }
                _ => {
                    yield PmInspectResult::Ok {
                        ticket_id, node_id, status,
                        kind: "other".to_string(),
                        task: None, command: None, output, error,
                    };
                }
            }
        }
    }

    /// Explain why a ticket is blocked.
    #[plexus_macros::hub_method(params(
        graph_id = "The lattice graph ID returned by build_tickets or run_tickets",
        ticket_id = "The ticket ID to investigate"
    ))]
    async fn why_blocked(
        &self,
        graph_id: String,
        ticket_id: String,
    ) -> impl Stream<Item = PmWhyBlockedResult> + Send + 'static {
        let pm_storage = self.pm_storage.clone();
        let lattice_storage = self.lattice_storage.clone();

        stream! {
            let ticket_map = match pm_storage.get_ticket_map(&graph_id).await {
                Ok(m) => m,
                Err(e) => { yield PmWhyBlockedResult::Err { message: e }; return; }
            };

            let node_id = match ticket_map.get(&ticket_id) {
                Some(id) => id.clone(),
                None => {
                    yield PmWhyBlockedResult::Err {
                        message: format!("Ticket not found: {}", ticket_id),
                    };
                    return;
                }
            };

            let predecessors = match lattice_storage.get_inbound_edges(&node_id).await {
                Ok(p) => p,
                Err(e) => {
                    yield PmWhyBlockedResult::Err {
                        message: format!("Failed to get predecessors: {}", e),
                    };
                    return;
                }
            };

            let mut blocked_by = Vec::new();
            for pred_id in predecessors {
                let pred_node = match lattice_storage.get_node(&pred_id).await {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                if pred_node.status == NodeStatus::Complete {
                    continue;
                }

                let pred_ticket_id = pm_storage
                    .get_ticket_for_node(&graph_id, &pred_id)
                    .await
                    .unwrap_or(None)
                    .unwrap_or_else(|| pred_id.clone());

                let (kind, label) = extract_kind_and_label(&pred_node.spec);
                blocked_by.push(PmTicketStatus {
                    ticket_id: pred_ticket_id,
                    node_id: pred_id,
                    status: node_status_str(&pred_node.status).to_string(),
                    kind,
                    label,
                });
            }

            if blocked_by.is_empty() {
                yield PmWhyBlockedResult::NotBlocked { ticket_id };
            } else {
                yield PmWhyBlockedResult::Ok { ticket_id, blocked_by };
            }
        }
    }

    /// Get the raw ticket source for a graph.
    #[plexus_macros::hub_method(params(
        graph_id = "The lattice graph ID"
    ))]
    async fn get_ticket_source(
        &self,
        graph_id: String,
    ) -> impl Stream<Item = Value> + Send + 'static {
        let pm_storage = self.pm_storage.clone();
        stream! {
            match pm_storage.get_ticket_source(&graph_id).await {
                Ok(Some(source)) => yield serde_json::json!({ "type": "ok", "source": source }),
                Ok(None) => yield serde_json::json!({ "type": "not_found", "graph_id": graph_id }),
                Err(e) => yield serde_json::json!({ "type": "err", "message": e }),
            }
        }
    }

    /// List graphs tracked by the pm layer, optionally filtered by project metadata.
    #[plexus_macros::hub_method(params(
        project = "Optional: filter by metadata.project string",
        limit   = "Optional: max results (default 20)"
    ))]
    async fn list_graphs(
        &self,
        project: Option<String>,
        limit: Option<usize>,
    ) -> impl Stream<Item = PmListGraphsResult> + Send + 'static {
        let pm_storage = self.pm_storage.clone();
        let lattice_storage = self.lattice_storage.clone();

        stream! {
            let limit = limit.unwrap_or(20);

            let entries = match pm_storage.list_ticket_maps(limit).await {
                Ok(v) => v,
                Err(e) => {
                    yield PmListGraphsResult::Err { message: e };
                    return;
                }
            };

            let mut graphs = Vec::new();

            for (graph_id, created_at) in entries {
                let lattice_graph = match lattice_storage.get_graph(&graph_id).await {
                    Ok(g) => g,
                    Err(_) => continue,
                };

                // Apply optional project filter.
                if let Some(ref project_filter) = project {
                    let graph_project = lattice_graph.metadata.get("project")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if graph_project != project_filter.as_str() {
                        continue;
                    }
                }

                let ticket_map = match pm_storage.get_ticket_map(&graph_id).await {
                    Ok(m) => m,
                    Err(_) => HashMap::new(),
                };

                let status = lattice_graph.status.to_string();

                graphs.push(PmGraphSummary {
                    graph_id,
                    status,
                    metadata: lattice_graph.metadata,
                    ticket_count: ticket_map.len(),
                    created_at,
                });
            }

            yield PmListGraphsResult::Ok { graphs };
        }
    }
}

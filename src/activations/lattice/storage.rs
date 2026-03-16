use super::types::{
    EdgeCondition, GatherStrategy, GraphId, GraphStatus, JoinType, LatticeEvent,
    LatticeEventEnvelope, LatticeGraph, LatticeNode, NodeId, NodeOutput, NodeSpec, NodeStatus,
    Token, TokenPayload,
};
use crate::activation_db_path_from_module;
use crate::activations::storage::init_sqlite_pool;
use async_stream::stream;
use futures::Stream;
use serde_json::Value;
use sqlx::{sqlite::SqlitePool, Row};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LatticeStorageConfig {
    pub db_path: PathBuf,
}

impl Default for LatticeStorageConfig {
    fn default() -> Self {
        Self {
            db_path: activation_db_path_from_module!("lattice.db"),
        }
    }
}

pub struct LatticeStorage {
    pool: SqlitePool,
    /// Per-graph Notify — wakes the execute() stream when new events are persisted.
    graph_notifiers: Arc<RwLock<HashMap<GraphId, Arc<Notify>>>>,
}

impl LatticeStorage {
    pub async fn new(config: LatticeStorageConfig) -> Result<Self, String> {
        let pool = init_sqlite_pool(config.db_path).await?;
        let storage = Self {
            pool,
            graph_notifiers: Arc::new(RwLock::new(HashMap::new())),
        };
        storage.run_migrations().await?;
        Ok(storage)
    }

    async fn run_migrations(&self) -> Result<(), String> {
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS lattice_graphs (
                id          TEXT PRIMARY KEY,
                metadata    TEXT NOT NULL DEFAULT '{}',
                status      TEXT NOT NULL DEFAULT 'pending',
                created_at  INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS lattice_nodes (
                id           TEXT PRIMARY KEY,
                graph_id     TEXT NOT NULL,
                spec         TEXT NOT NULL,
                status       TEXT NOT NULL DEFAULT 'pending',
                output       TEXT,
                error        TEXT,
                created_at   INTEGER NOT NULL,
                completed_at INTEGER,
                FOREIGN KEY (graph_id) REFERENCES lattice_graphs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_lattice_nodes_graph ON lattice_nodes(graph_id);

            CREATE TABLE IF NOT EXISTS lattice_edges (
                id           TEXT PRIMARY KEY,
                graph_id     TEXT NOT NULL,
                from_node_id TEXT NOT NULL,
                to_node_id   TEXT NOT NULL,
                FOREIGN KEY (graph_id) REFERENCES lattice_graphs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_lattice_edges_graph ON lattice_edges(graph_id);
            CREATE INDEX IF NOT EXISTS idx_lattice_edges_to    ON lattice_edges(to_node_id);

            -- Durable event log: every LatticeEvent is appended here.
            -- seq is the cursor callers use for reconnect replay.
            CREATE TABLE IF NOT EXISTS lattice_events (
                seq        INTEGER PRIMARY KEY AUTOINCREMENT,
                graph_id   TEXT NOT NULL,
                event      TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_lattice_events_graph ON lattice_events(graph_id, seq);

            -- Token delivery tracking (Petri net marking)
            CREATE TABLE IF NOT EXISTS lattice_edge_tokens (
                edge_id    TEXT NOT NULL,
                graph_id   TEXT NOT NULL,
                token      TEXT NOT NULL,
                seq        INTEGER NOT NULL,
                PRIMARY KEY (edge_id, seq)
            );
            CREATE INDEX IF NOT EXISTS idx_lattice_edge_tokens_graph
                ON lattice_edge_tokens(graph_id);
        "#)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Migration failed: {}", e))?;

        // Add columns to existing tables (ignore if already exists)
        let _ = sqlx::query("ALTER TABLE lattice_edges ADD COLUMN condition TEXT")
            .execute(&self.pool).await;
        let _ = sqlx::query("ALTER TABLE lattice_nodes ADD COLUMN join_type TEXT NOT NULL DEFAULT 'all'")
            .execute(&self.pool).await;
        let _ = sqlx::query(
            "ALTER TABLE lattice_graphs ADD COLUMN parent_graph_id TEXT NULL REFERENCES lattice_graphs(id)"
        ).execute(&self.pool).await;
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_lattice_graphs_parent ON lattice_graphs(parent_graph_id)"
        ).execute(&self.pool).await;

        Ok(())
    }

    // ─── Graph / Node / Edge CRUD ────────────────────────────────────────────

    pub async fn create_graph(&self, metadata: Value) -> Result<GraphId, String> {
        let id = format!("lattice-{}", Uuid::new_v4());
        let now = current_timestamp();
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        sqlx::query(
            "INSERT INTO lattice_graphs (id, metadata, status, created_at) VALUES (?, ?, 'pending', ?)"
        )
        .bind(&id)
        .bind(&metadata_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create graph: {}", e))?;

        Ok(id)
    }

    pub async fn create_child_graph(
        &self,
        parent_id: &str,
        metadata: Value,
    ) -> Result<String, String> {
        let id = format!("lattice-{}", Uuid::new_v4());
        let now = current_timestamp();
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        sqlx::query(
            "INSERT INTO lattice_graphs (id, metadata, status, created_at, parent_graph_id) VALUES (?, ?, 'pending', ?, ?)"
        )
        .bind(&id)
        .bind(&metadata_json)
        .bind(now)
        .bind(parent_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create child graph: {}", e))?;

        Ok(id)
    }

    pub async fn get_child_graphs(&self, parent_id: &str) -> Result<Vec<LatticeGraph>, String> {
        let rows = sqlx::query(
            "SELECT id, metadata, status, created_at, parent_graph_id FROM lattice_graphs WHERE parent_graph_id = ? ORDER BY created_at"
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch child graphs: {}", e))?;

        let mut graphs = Vec::new();
        for row in rows {
            let graph_id: String = row.get("id");
            let node_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM lattice_nodes WHERE graph_id = ?"
            )
            .bind(&graph_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("Failed to count nodes: {}", e))?;

            let edge_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM lattice_edges WHERE graph_id = ?"
            )
            .bind(&graph_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("Failed to count edges: {}", e))?;

            graphs.push(self.row_to_graph(row, node_count as usize, edge_count as usize)?);
        }
        Ok(graphs)
    }

    pub async fn add_node(
        &self,
        graph_id: &GraphId,
        node_id_hint: Option<NodeId>,
        spec: &NodeSpec,
    ) -> Result<NodeId, String> {
        let id = node_id_hint.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = current_timestamp();
        let spec_json = serde_json::to_string(spec)
            .map_err(|e| format!("Failed to serialize spec: {}", e))?;

        sqlx::query(
            "INSERT INTO lattice_nodes (id, graph_id, spec, status, created_at) VALUES (?, ?, ?, 'pending', ?)"
        )
        .bind(&id)
        .bind(graph_id)
        .bind(&spec_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to add node: {}", e))?;

        if let Ok(graph_status) = self.get_graph_status(graph_id).await {
            if graph_status == GraphStatus::Running {
                let _ = self.check_and_ready(graph_id, &id).await;
            }
        }

        Ok(id)
    }

    pub async fn add_edge(
        &self,
        graph_id: &GraphId,
        from_node_id: &NodeId,
        to_node_id: &NodeId,
        condition: Option<&EdgeCondition>,
    ) -> Result<(), String> {
        let from_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM lattice_nodes WHERE id = ? AND graph_id = ?"
        )
        .bind(from_node_id)
        .bind(graph_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to validate from_node: {}", e))?;

        if !from_exists {
            return Err(format!("Node {} not found in graph {}", from_node_id, graph_id));
        }

        let to_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM lattice_nodes WHERE id = ? AND graph_id = ?"
        )
        .bind(to_node_id)
        .bind(graph_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to validate to_node: {}", e))?;

        if !to_exists {
            return Err(format!("Node {} not found in graph {}", to_node_id, graph_id));
        }

        let edge_id = Uuid::new_v4().to_string();
        let condition_json = condition
            .map(|c| serde_json::to_string(c).map_err(|e| format!("Failed to serialize condition: {}", e)))
            .transpose()?;

        sqlx::query(
            "INSERT INTO lattice_edges (id, graph_id, from_node_id, to_node_id, condition) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&edge_id)
        .bind(graph_id)
        .bind(from_node_id)
        .bind(to_node_id)
        .bind(condition_json.as_deref())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to add edge: {}", e))?;

        if let Ok(graph_status) = self.get_graph_status(graph_id).await {
            if graph_status == GraphStatus::Running {
                if let Ok(src_node) = self.get_node(from_node_id).await {
                    if src_node.status == NodeStatus::Complete {
                        let tokens: Vec<Token> = src_node.output.as_ref()
                            .map(|o| o.tokens().into_iter().cloned().collect())
                            .unwrap_or_else(|| vec![Token::ok()]);
                        for token in &tokens {
                            let matches = condition
                                .map(|c| c.matches(&token.color))
                                .unwrap_or(true);
                            if matches {
                                let seq = self.count_tokens_on_edge(&edge_id).await? + 1;
                                self.deliver_token(&edge_id, graph_id, token, seq).await?;
                            }
                        }
                        let _ = self.check_and_ready(graph_id, to_node_id).await;
                        self.notify_graph(graph_id);
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn get_graph(&self, graph_id: &GraphId) -> Result<LatticeGraph, String> {
        let row = sqlx::query(
            "SELECT id, metadata, status, created_at, parent_graph_id FROM lattice_graphs WHERE id = ?"
        )
        .bind(graph_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch graph: {}", e))?
        .ok_or_else(|| format!("Graph not found: {}", graph_id))?;

        let node_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lattice_nodes WHERE graph_id = ?"
        )
        .bind(graph_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to count nodes: {}", e))?;

        let edge_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lattice_edges WHERE graph_id = ?"
        )
        .bind(graph_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to count edges: {}", e))?;

        self.row_to_graph(row, node_count as usize, edge_count as usize)
    }

    /// Count the total number of nodes in a graph.
    pub async fn count_nodes(&self, graph_id: &GraphId) -> Result<usize, String> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lattice_nodes WHERE graph_id = ?"
        )
        .bind(graph_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to count nodes: {}", e))?;

        Ok(count as usize)
    }

    pub async fn get_nodes(&self, graph_id: &GraphId) -> Result<Vec<LatticeNode>, String> {
        let rows = sqlx::query(
            "SELECT id, graph_id, spec, status, output, error, created_at, completed_at
             FROM lattice_nodes WHERE graph_id = ? ORDER BY created_at"
        )
        .bind(graph_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch nodes: {}", e))?;

        rows.into_iter().map(|row| self.row_to_node(row)).collect()
    }

    pub async fn get_node(&self, node_id: &NodeId) -> Result<LatticeNode, String> {
        let row = sqlx::query(
            "SELECT id, graph_id, spec, status, output, error, created_at, completed_at
             FROM lattice_nodes WHERE id = ?"
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch node: {}", e))?
        .ok_or_else(|| format!("Node not found: {}", node_id))?;

        self.row_to_node(row)
    }

    pub async fn get_inbound_edges(&self, node_id: &NodeId) -> Result<Vec<NodeId>, String> {
        let rows = sqlx::query(
            "SELECT from_node_id FROM lattice_edges WHERE to_node_id = ?"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch inbound edges: {}", e))?;

        Ok(rows.into_iter().map(|r| r.get::<String, _>("from_node_id")).collect())
    }

    pub async fn get_outbound_edges(&self, node_id: &NodeId) -> Result<Vec<NodeId>, String> {
        let rows = sqlx::query(
            "SELECT to_node_id FROM lattice_edges WHERE from_node_id = ?"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch outbound edges: {}", e))?;

        Ok(rows.into_iter().map(|r| r.get::<String, _>("to_node_id")).collect())
    }

    /// Returns (edge_id, to_node_id, condition) for each outbound edge.
    async fn get_outbound_edges_with_conditions(
        &self,
        node_id: &NodeId,
    ) -> Result<Vec<(String, NodeId, Option<EdgeCondition>)>, String> {
        let rows = sqlx::query(
            "SELECT id, to_node_id, condition FROM lattice_edges WHERE from_node_id = ?"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch outbound edges: {}", e))?;

        rows.into_iter()
            .map(|row| {
                let edge_id: String = row.get("id");
                let to_node_id: String = row.get("to_node_id");
                let condition_json: Option<String> = row.get("condition");
                let condition = condition_json
                    .as_deref()
                    .map(|s| {
                        serde_json::from_str::<EdgeCondition>(s)
                            .map_err(|e| format!("Failed to deserialize edge condition: {}", e))
                    })
                    .transpose()?;
                Ok((edge_id, to_node_id, condition))
            })
            .collect()
    }

    pub async fn get_nodes_with_no_predecessors(
        &self,
        graph_id: &GraphId,
    ) -> Result<Vec<LatticeNode>, String> {
        // Exclude self-loop edges when determining root nodes
        let rows = sqlx::query(
            "SELECT id, graph_id, spec, status, output, error, created_at, completed_at
             FROM lattice_nodes
             WHERE graph_id = ?
               AND id NOT IN (
                   SELECT to_node_id FROM lattice_edges
                   WHERE graph_id = ? AND from_node_id != to_node_id
               )
             ORDER BY created_at"
        )
        .bind(graph_id)
        .bind(graph_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch root nodes: {}", e))?;

        rows.into_iter().map(|row| self.row_to_node(row)).collect()
    }

    pub async fn set_node_status(
        &self,
        node_id: &NodeId,
        status: NodeStatus,
        output: Option<&NodeOutput>,
        error: Option<&str>,
    ) -> Result<(), String> {
        let now = current_timestamp();
        let completed_at = match status {
            NodeStatus::Complete | NodeStatus::Failed => Some(now),
            _ => None,
        };
        let output_json = output
            .map(|o| serde_json::to_string(o).map_err(|e| format!("Failed to serialize output: {}", e)))
            .transpose()?;

        sqlx::query(
            "UPDATE lattice_nodes SET status = ?, output = ?, error = ?, completed_at = ? WHERE id = ?"
        )
        .bind(status.to_string())
        .bind(output_json.as_deref())
        .bind(error)
        .bind(completed_at)
        .bind(node_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to update node status: {}", e))?;

        Ok(())
    }

    pub async fn update_graph_status(
        &self,
        graph_id: &GraphId,
        status: GraphStatus,
    ) -> Result<(), String> {
        sqlx::query("UPDATE lattice_graphs SET status = ? WHERE id = ?")
            .bind(status.to_string())
            .bind(graph_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to update graph status: {}", e))?;
        Ok(())
    }

    pub async fn list_graphs(&self) -> Result<Vec<LatticeGraph>, String> {
        let rows = sqlx::query(
            "SELECT id, metadata, status, created_at, parent_graph_id FROM lattice_graphs ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list graphs: {}", e))?;

        let mut graphs = Vec::new();
        for row in rows {
            let graph_id: String = row.get("id");
            let node_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM lattice_nodes WHERE graph_id = ?"
            )
            .bind(&graph_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("Failed to count nodes: {}", e))?;

            let edge_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM lattice_edges WHERE graph_id = ?"
            )
            .bind(&graph_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("Failed to count edges: {}", e))?;

            graphs.push(self.row_to_graph(row, node_count as usize, edge_count as usize)?);
        }
        Ok(graphs)
    }

    /// Return graph IDs whose status is 'running'.
    ///
    /// Used by the startup recovery pass to find graphs that were mid-execution
    /// when the substrate last shut down.
    pub async fn get_running_graph_ids(&self) -> Result<Vec<GraphId>, String> {
        let rows = sqlx::query(
            "SELECT id FROM lattice_graphs WHERE status = 'running' ORDER BY created_at"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch running graphs: {}", e))?;

        Ok(rows.into_iter().map(|r| r.get::<String, _>("id")).collect())
    }

    /// Reset a single node to 'pending', clearing output/error/completed_at.
    ///
    /// Used by the startup recovery pass to un-stick nodes that were left in
    /// 'ready' state when the substrate last shut down.
    pub async fn reset_node_to_pending(&self, node_id: &NodeId) -> Result<(), String> {
        sqlx::query(
            "UPDATE lattice_nodes SET status = 'pending', output = NULL, error = NULL, completed_at = NULL WHERE id = ?"
        )
        .bind(node_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to reset node to pending: {}", e))?;
        Ok(())
    }

    // ─── Token Delivery ──────────────────────────────────────────────────────

    /// Append a token delivery to an edge.
    async fn deliver_token(
        &self,
        edge_id: &str,
        graph_id: &GraphId,
        token: &Token,
        seq: i64,
    ) -> Result<(), String> {
        let token_json = serde_json::to_string(token)
            .map_err(|e| format!("Failed to serialize token: {}", e))?;

        sqlx::query(
            "INSERT INTO lattice_edge_tokens (edge_id, graph_id, token, seq) VALUES (?, ?, ?, ?)"
        )
        .bind(edge_id)
        .bind(graph_id)
        .bind(&token_json)
        .bind(seq)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to deliver token: {}", e))?;

        Ok(())
    }

    /// Count tokens already delivered to a specific edge.
    async fn count_tokens_on_edge(&self, edge_id: &str) -> Result<i64, String> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lattice_edge_tokens WHERE edge_id = ?"
        )
        .bind(edge_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to count edge tokens: {}", e))?;
        Ok(count)
    }

    /// How many distinct inbound edges have at least one token, vs total inbound.
    async fn count_edges_with_tokens(
        &self,
        node_id: &NodeId,
    ) -> Result<(usize, usize), String> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lattice_edges WHERE to_node_id = ? AND from_node_id != to_node_id"
        )
        .bind(node_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to count inbound edges: {}", e))?;

        let delivered: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT edge_id) FROM lattice_edge_tokens
             WHERE edge_id IN (
                 SELECT id FROM lattice_edges WHERE to_node_id = ? AND from_node_id != to_node_id
             )"
        )
        .bind(node_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to count delivered edges: {}", e))?;

        Ok((delivered as usize, total as usize))
    }

    /// Collect all tokens from all inbound edges of a node (for Gather).
    pub async fn get_node_inputs(&self, node_id: &NodeId) -> Result<Vec<Token>, String> {
        let rows = sqlx::query(
            "SELECT et.token FROM lattice_edge_tokens et
             JOIN lattice_edges e ON et.edge_id = e.id
             WHERE e.to_node_id = ?
             ORDER BY et.seq"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch node inputs: {}", e))?;

        rows.into_iter()
            .map(|row| {
                let token_json: String = row.get("token");
                serde_json::from_str::<Token>(&token_json)
                    .map_err(|e| format!("Failed to deserialize token: {}", e))
            })
            .collect()
    }

    // ─── Event Log ───────────────────────────────────────────────────────────

    /// Append an event to the durable log and return its sequence number.
    pub async fn persist_event(
        &self,
        graph_id: &GraphId,
        event: &LatticeEvent,
    ) -> Result<u64, String> {
        let event_json = serde_json::to_string(event)
            .map_err(|e| format!("Failed to serialize event: {}", e))?;
        let now = current_timestamp();

        let result = sqlx::query(
            "INSERT INTO lattice_events (graph_id, event, created_at) VALUES (?, ?, ?)"
        )
        .bind(graph_id)
        .bind(&event_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to persist event: {}", e))?;

        Ok(result.last_insert_rowid() as u64)
    }

    /// Read all events for a graph with seq > after_seq, in order.
    pub async fn get_events_after(
        &self,
        graph_id: &GraphId,
        after_seq: u64,
    ) -> Result<Vec<(u64, LatticeEvent)>, String> {
        let rows = sqlx::query(
            "SELECT seq, event FROM lattice_events WHERE graph_id = ? AND seq > ? ORDER BY seq"
        )
        .bind(graph_id)
        .bind(after_seq as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch events: {}", e))?;

        rows.into_iter()
            .map(|row| {
                let seq: i64 = row.get("seq");
                let event_json: String = row.get("event");
                let event: LatticeEvent = serde_json::from_str(&event_json)
                    .map_err(|e| format!("Failed to deserialize event: {}", e))?;
                Ok((seq as u64, event))
            })
            .collect()
    }

    /// Wake the execute() stream for a graph.
    pub fn notify_graph(&self, graph_id: &GraphId) {
        if let Ok(notifiers) = self.graph_notifiers.read() {
            if let Some(notifier) = notifiers.get(graph_id) {
                notifier.notify_one();
            }
        }
    }

    pub fn get_or_create_notifier(&self, graph_id: &GraphId) -> Arc<Notify> {
        let mut notifiers = self.graph_notifiers.write().unwrap();
        notifiers
            .entry(graph_id.clone())
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone()
    }

    // ─── Graph Lifecycle ─────────────────────────────────────────────────────

    /// Atomically transition a Pending graph to Running, seed root nodes as Ready,
    /// and persist the initial NodeReady events.
    ///
    /// Idempotent: if another caller already started the graph, this is a no-op.
    pub async fn start_graph(&self, graph_id: &GraphId) -> Result<(), String> {
        // CAS: only transition if still Pending
        let result = sqlx::query(
            "UPDATE lattice_graphs SET status = 'running' WHERE id = ? AND status = 'pending'"
        )
        .bind(graph_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to start graph: {}", e))?;

        if result.rows_affected() == 0 {
            // Already started by a concurrent caller — that's fine
            return Ok(());
        }

        let root_nodes = self.get_nodes_with_no_predecessors(graph_id).await?;

        if root_nodes.is_empty() {
            self.update_graph_status(graph_id, GraphStatus::Complete).await?;
            self.persist_event(graph_id, &LatticeEvent::GraphDone {
                graph_id: graph_id.clone(),
            }).await?;
        } else {
            for node in root_nodes {
                match &node.spec {
                    NodeSpec::Gather { .. } => {
                        // Root Gather node with no predecessors — skip NodeReady.
                        // It will stay Pending (degenerate: no tokens will ever arrive).
                    }
                    _ => {
                        self.set_node_status(&node.id, NodeStatus::Ready, None, None).await?;
                        self.persist_event(graph_id, &LatticeEvent::NodeReady {
                            node_id: node.id,
                            spec: node.spec,
                        }).await?;
                    }
                }
            }
        }

        self.notify_graph(graph_id);
        Ok(())
    }

    /// Reset a zombie Running node back to Ready for crash recovery.
    ///
    /// Inbound edge tokens are never deleted, so the node's join condition
    /// is still satisfied and it can be re-dispatched immediately.
    /// Emits a fresh NodeReady event so the `run_graph_execution` watcher
    /// picks it up.  Idempotent: no-op if node is not Running.
    pub async fn reset_running_to_ready(
        &self,
        graph_id: &GraphId,
        node_id: &NodeId,
    ) -> Result<(), String> {
        let node = self.get_node(node_id).await?;
        if node.status != NodeStatus::Running {
            return Ok(());
        }
        self.set_node_status(node_id, NodeStatus::Ready, None, None).await?;
        self.persist_event(graph_id, &LatticeEvent::NodeReady {
            node_id: node_id.clone(),
            spec: node.spec,
        }).await?;
        self.notify_graph(graph_id);
        Ok(())
    }

    /// Re-emit NodeReady events for all nodes currently in the 'ready' state.
    ///
    /// Called during startup recovery after stuck nodes have been reset.
    /// The graph must already be in 'running' status.  This re-seeds the
    /// execute() stream so that `run_graph_execution` watchers can pick up
    /// and dispatch the pending work.
    pub async fn reemit_ready_nodes(&self, graph_id: &GraphId) -> Result<(), String> {
        let nodes = self.get_nodes(graph_id).await?;
        for node in nodes {
            if node.status == NodeStatus::Ready {
                self.persist_event(graph_id, &LatticeEvent::NodeReady {
                    node_id: node.id,
                    spec: node.spec,
                }).await?;
            }
        }
        self.notify_graph(graph_id);
        Ok(())
    }

    // ─── Transition Logic ────────────────────────────────────────────────────

    /// Called by node_complete / node_failed.
    ///
    /// Uses an iterative queue to handle Gather auto-execution without async recursion.
    /// Implements the colored token model:
    ///   1. Produce tokens from output/error
    ///   2. Route tokens on outbound edges (filtered by edge condition)
    ///   3. For each downstream node: check_and_ready (enables Gather auto-execution)
    ///   4. Error fallback if no tokens delivered
    ///   5. Check graph completion
    pub async fn advance_graph(
        &self,
        graph_id: &GraphId,
        completed_node_id: &NodeId,
        output: Option<NodeOutput>,
        error: Option<String>,
    ) -> Result<(), String> {
        // Queue: (node_id, output, error) — Gather nodes are auto-executed via queue
        let mut queue: Vec<(NodeId, Option<NodeOutput>, Option<String>)> =
            vec![(completed_node_id.clone(), output, error)];

        while let Some((nid, out, err)) = queue.pop() {
            // IDEMPOTENCY: skip if already terminal
            let node = match self.get_node(&nid).await {
                Ok(n) => n,
                Err(_) => continue,
            };
            if node.status == NodeStatus::Complete || node.status == NodeStatus::Failed {
                continue;
            }

            let failed = err.is_some();

            // PRODUCE TOKENS
            let tokens: Vec<Token>;
            if failed {
                let err_msg = err.unwrap_or_default();
                self.set_node_status(&nid, NodeStatus::Failed, None, Some(&err_msg)).await?;
                self.persist_event(graph_id, &LatticeEvent::NodeFailed {
                    node_id: nid.clone(),
                    error: err_msg.clone(),
                }).await?;
                tokens = vec![Token::error(err_msg)];
            } else {
                self.set_node_status(&nid, NodeStatus::Complete, out.as_ref(), None).await?;
                self.persist_event(graph_id, &LatticeEvent::NodeDone {
                    node_id: nid.clone(),
                    output: out.clone(),
                }).await?;
                tokens = out.as_ref()
                    .map(|o| o.tokens().into_iter().cloned().collect())
                    .unwrap_or_else(|| vec![Token::ok()]);
            }

            // ROUTE TOKENS ON OUTBOUND EDGES
            let outbound = self.get_outbound_edges_with_conditions(&nid).await?;
            let mut any_delivered = false;

            for token in &tokens {
                for (edge_id, to_node_id, condition) in &outbound {
                    let matches = condition.as_ref()
                        .map(|c| c.matches(&token.color))
                        .unwrap_or(true);

                    if matches {
                        let seq = self.count_tokens_on_edge(edge_id).await? + 1;
                        self.deliver_token(edge_id, graph_id, token, seq).await?;
                        any_delivered = true;

                        // Check if downstream node is now enabled
                        if let Some(gather_output) = self.check_and_ready(graph_id, to_node_id).await? {
                            queue.push((to_node_id.clone(), Some(gather_output), None));
                        }
                    }
                }
            }

            // ERROR FALLBACK: no handler found for error token
            if failed && !any_delivered {
                let err_str = tokens.first()
                    .and_then(|t| match &t.payload {
                        Some(TokenPayload::Data { value }) => {
                            value.get("message").and_then(|v| v.as_str()).map(|s| s.to_string())
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                self.update_graph_status(graph_id, GraphStatus::Failed).await?;
                self.persist_event(graph_id, &LatticeEvent::GraphFailed {
                    graph_id: graph_id.clone(),
                    node_id: nid.clone(),
                    error: err_str,
                }).await?;
                self.notify_graph(graph_id);
                return Ok(());
            }
        }

        // GRAPH COMPLETION: check if all nodes are complete
        let all_done = self.get_nodes(graph_id).await?
            .iter()
            .all(|n| n.status == NodeStatus::Complete);
        if all_done {
            self.update_graph_status(graph_id, GraphStatus::Complete).await?;
            self.persist_event(graph_id, &LatticeEvent::GraphDone {
                graph_id: graph_id.clone(),
            }).await?;
        }

        self.notify_graph(graph_id);
        Ok(())
    }

    /// Check if a node is enabled (all/any inbound edges delivered tokens).
    /// Returns Some(output) for Gather nodes (caller should queue for processing).
    /// Returns None for Task/Scatter (sets Ready and persists NodeReady).
    async fn check_and_ready(
        &self,
        graph_id: &GraphId,
        node_id: &NodeId,
    ) -> Result<Option<NodeOutput>, String> {
        let node = self.get_node(node_id).await?;

        // Only transition from Pending or Complete (Complete → Ready allows self-loops)
        if node.status != NodeStatus::Pending && node.status != NodeStatus::Complete {
            return Ok(None);
        }

        let (delivered, total) = self.count_edges_with_tokens(node_id).await?;
        let enabled = match &node.join_type {
            JoinType::All => delivered == total && total > 0,
            JoinType::Any => delivered >= 1,
        };

        if !enabled {
            return Ok(None);
        }

        match &node.spec {
            NodeSpec::Gather { strategy } => {
                let tokens = self.get_node_inputs(node_id).await?;
                let output = match strategy {
                    GatherStrategy::All => NodeOutput::Many { tokens },
                    GatherStrategy::First { n } => {
                        let take = (*n).min(tokens.len());
                        NodeOutput::Many { tokens: tokens[..take].to_vec() }
                    }
                };
                self.set_node_status(node_id, NodeStatus::Running, None, None).await?;
                Ok(Some(output))
            }
            _ => {
                // Task / Scatter / SubGraph — emit NodeReady for caller to handle
                self.set_node_status(node_id, NodeStatus::Ready, None, None).await?;
                self.persist_event(graph_id, &LatticeEvent::NodeReady {
                    node_id: node_id.clone(),
                    spec: node.spec.clone(),
                }).await?;
                Ok(None)
            }
        }
    }

    async fn get_graph_status(&self, graph_id: &str) -> Result<GraphStatus, String> {
        let row = sqlx::query("SELECT status FROM lattice_graphs WHERE id = ?")
            .bind(graph_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        let s: String = row.try_get("status").map_err(|e| e.to_string())?;
        s.parse::<GraphStatus>()
    }

    // ─── Execute Stream ──────────────────────────────────────────────────────

    /// Long-lived stream of sequenced events for a graph.
    ///
    /// This is the canonical implementation — both the lattice activation and
    /// Orcha (as a library consumer) call this directly.
    pub fn execute_stream(
        storage: Arc<LatticeStorage>,
        graph_id: GraphId,
        after_seq: Option<u64>,
    ) -> impl Stream<Item = LatticeEventEnvelope> + Send + 'static {
        stream! {
            let graph = match storage.get_graph(&graph_id).await {
                Ok(g) => g,
                Err(e) => {
                    yield LatticeEventEnvelope {
                        seq: 0,
                        event: LatticeEvent::GraphFailed {
                            graph_id: graph_id.clone(),
                            node_id: "".to_string(),
                            error: format!("Graph not found: {}", e),
                        },
                    };
                    return;
                }
            };

            if after_seq.is_none() && graph.status == GraphStatus::Pending {
                if let Err(e) = storage.start_graph(&graph_id).await {
                    yield LatticeEventEnvelope {
                        seq: 0,
                        event: LatticeEvent::GraphFailed {
                            graph_id: graph_id.clone(),
                            node_id: "".to_string(),
                            error: format!("Failed to start graph: {}", e),
                        },
                    };
                    return;
                }
            }

            let notifier = storage.get_or_create_notifier(&graph_id);
            let mut cursor = after_seq.unwrap_or(0);

            loop {
                let events = match storage.get_events_after(&graph_id, cursor).await {
                    Ok(evs) => evs,
                    Err(e) => {
                        yield LatticeEventEnvelope {
                            seq: cursor,
                            event: LatticeEvent::GraphFailed {
                                graph_id: graph_id.clone(),
                                node_id: "".to_string(),
                                error: format!("Event read error: {}", e),
                            },
                        };
                        return;
                    }
                };

                for (seq, event) in events {
                    let done = matches!(
                        event,
                        LatticeEvent::GraphDone { .. } | LatticeEvent::GraphFailed { .. }
                    );
                    cursor = seq;
                    yield LatticeEventEnvelope { seq, event };
                    if done { return; }
                }

                tokio::select! {
                    _ = notifier.notified() => continue,
                    _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                        let event = LatticeEvent::GraphFailed {
                            graph_id: graph_id.clone(),
                            node_id: "timeout".to_string(),
                            error: "Execution timed out".to_string(),
                        };
                        let seq = storage.persist_event(&graph_id, &event).await.unwrap_or(cursor + 1);
                        yield LatticeEventEnvelope { seq, event };
                        return;
                    }
                }
            }
        }
    }

    // ─── Row Helpers ─────────────────────────────────────────────────────────

    fn row_to_graph(
        &self,
        row: sqlx::sqlite::SqliteRow,
        node_count: usize,
        edge_count: usize,
    ) -> Result<LatticeGraph, String> {
        let metadata_json: String = row.get("metadata");
        let status_str: String = row.get("status");

        Ok(LatticeGraph {
            id: row.get("id"),
            metadata: serde_json::from_str(&metadata_json).unwrap_or(serde_json::json!({})),
            status: GraphStatus::from_str(&status_str).unwrap_or(GraphStatus::Pending),
            created_at: row.get("created_at"),
            node_count,
            edge_count,
            parent_graph_id: row.try_get::<Option<String>, _>("parent_graph_id").unwrap_or(None),
        })
    }

    fn row_to_node(&self, row: sqlx::sqlite::SqliteRow) -> Result<LatticeNode, String> {
        let spec_json: String = row.get("spec");
        let status_str: String = row.get("status");
        let output_json: Option<String> = row.get("output");

        let spec: NodeSpec = serde_json::from_str(&spec_json)
            .map_err(|e| format!("Failed to deserialize spec: {}", e))?;
        let status = NodeStatus::from_str(&status_str).unwrap_or(NodeStatus::Pending);
        let output = output_json
            .as_deref()
            .map(|s| serde_json::from_str::<NodeOutput>(s))
            .transpose()
            .map_err(|e| format!("Failed to deserialize output: {}", e))?;

        // join_type column may not exist on older databases
        let join_type_str: Option<String> = row.try_get("join_type").ok().flatten();
        let join_type = match join_type_str.as_deref().unwrap_or("all") {
            "any" => JoinType::Any,
            _ => JoinType::All,
        };

        Ok(LatticeNode {
            id: row.get("id"),
            graph_id: row.get("graph_id"),
            spec,
            status,
            join_type,
            output,
            error: row.get("error"),
            created_at: row.get("created_at"),
            completed_at: row.get("completed_at"),
        })
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

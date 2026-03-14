use crate::activations::storage::{activation_db_path, init_sqlite_pool};
use sqlx::{sqlite::SqlitePool, Row};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PmStorageConfig {
    pub db_path: PathBuf,
}

impl Default for PmStorageConfig {
    fn default() -> Self {
        Self {
            db_path: activation_db_path("pm", "pm.db"),
        }
    }
}

pub struct PmStorage {
    pool: SqlitePool,
}

impl PmStorage {
    pub async fn new(config: PmStorageConfig) -> Result<Self, String> {
        let pool = init_sqlite_pool(config.db_path).await?;
        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    async fn init_schema(&self) -> Result<(), String> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orcha_ticket_maps (
                graph_id   TEXT NOT NULL,
                ticket_id  TEXT NOT NULL,
                node_id    TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                PRIMARY KEY (graph_id, ticket_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create orcha_ticket_maps table: {}", e))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orcha_ticket_sources (
                graph_id   TEXT PRIMARY KEY,
                source     TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create orcha_ticket_sources table: {}", e))?;

        // Migrate: add created_at column if it doesn't exist yet (idempotent).
        let _ = sqlx::query(
            "ALTER TABLE orcha_ticket_maps ADD COLUMN created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))",
        )
        .execute(&self.pool)
        .await;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_ticket_maps_graph ON orcha_ticket_maps(graph_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create graph index: {}", e))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_ticket_maps_node ON orcha_ticket_maps(graph_id, node_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create node index: {}", e))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orcha_node_logs (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                graph_id   TEXT NOT NULL,
                node_id    TEXT NOT NULL,
                ticket_id  TEXT,
                seq        INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                event_data TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create orcha_node_logs table: {}", e))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_node_logs ON orcha_node_logs(graph_id, node_id, seq)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create node logs index: {}", e))?;

        Ok(())
    }

    /// Insert or replace all ticket→node mappings for a graph.
    pub async fn save_ticket_map(
        &self,
        graph_id: &str,
        map: &HashMap<String, String>,
    ) -> Result<(), String> {
        for (ticket_id, node_id) in map {
            sqlx::query(
                "INSERT OR REPLACE INTO orcha_ticket_maps (graph_id, ticket_id, node_id) VALUES (?, ?, ?)",
            )
            .bind(graph_id)
            .bind(ticket_id)
            .bind(node_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to save ticket map entry: {}", e))?;
        }
        Ok(())
    }

    /// Fetch the ticket_id→node_id map for a graph.
    pub async fn get_ticket_map(&self, graph_id: &str) -> Result<HashMap<String, String>, String> {
        let rows = sqlx::query(
            "SELECT ticket_id, node_id FROM orcha_ticket_maps WHERE graph_id = ?",
        )
        .bind(graph_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch ticket map: {}", e))?;

        let mut map = HashMap::new();
        for row in rows {
            let ticket_id: String = row.get("ticket_id");
            let node_id: String = row.get("node_id");
            map.insert(ticket_id, node_id);
        }
        Ok(map)
    }

    /// List all known graph IDs ordered by first insertion time descending.
    /// Returns `Vec<(graph_id, created_at)>`.
    pub async fn list_ticket_maps(&self, limit: usize) -> Result<Vec<(String, i64)>, String> {
        let rows = sqlx::query(
            "SELECT graph_id, MIN(created_at) AS created_at \
             FROM orcha_ticket_maps \
             GROUP BY graph_id \
             ORDER BY created_at DESC \
             LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list ticket maps: {}", e))?;

        let result = rows
            .into_iter()
            .map(|row| {
                let graph_id: String = row.get("graph_id");
                let created_at: i64 = row.get("created_at");
                (graph_id, created_at)
            })
            .collect();

        Ok(result)
    }

    /// Save the raw ticket source for a graph.
    pub async fn save_ticket_source(&self, graph_id: &str, source: &str) -> Result<(), String> {
        sqlx::query(
            "INSERT OR REPLACE INTO orcha_ticket_sources (graph_id, source) VALUES (?, ?)",
        )
        .bind(graph_id)
        .bind(source)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to save ticket source: {}", e))?;
        Ok(())
    }

    /// Fetch the raw ticket source for a graph.
    pub async fn get_ticket_source(&self, graph_id: &str) -> Result<Option<String>, String> {
        let row = sqlx::query(
            "SELECT source FROM orcha_ticket_sources WHERE graph_id = ?",
        )
        .bind(graph_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch ticket source: {}", e))?;
        Ok(row.map(|r| r.get("source")))
    }

    /// Reverse lookup: node_id → ticket_id.
    pub async fn get_ticket_for_node(
        &self,
        graph_id: &str,
        node_id: &str,
    ) -> Result<Option<String>, String> {
        let row = sqlx::query(
            "SELECT ticket_id FROM orcha_ticket_maps WHERE graph_id = ? AND node_id = ?",
        )
        .bind(graph_id)
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch ticket for node: {}", e))?;

        Ok(row.map(|r| r.get("ticket_id")))
    }

    /// Append a single log entry for a node execution event.
    ///
    /// `event_type` is one of: "prompt", "start", "tool_use", "tool_result",
    /// "complete", "error", "passthrough", "outcome".
    /// `event_data` is a JSON string.
    pub async fn append_node_log(
        &self,
        graph_id: &str,
        node_id: &str,
        ticket_id: Option<&str>,
        seq: i64,
        event_type: &str,
        event_data: &str,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO orcha_node_logs (graph_id, node_id, ticket_id, seq, event_type, event_data) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(graph_id)
        .bind(node_id)
        .bind(ticket_id)
        .bind(seq)
        .bind(event_type)
        .bind(event_data)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to append node log: {}", e))?;
        Ok(())
    }

    /// Fetch all log entries for a (graph_id, node_id) pair, ordered by seq.
    pub async fn get_node_log(
        &self,
        graph_id: &str,
        node_id: &str,
    ) -> Result<Vec<NodeLogEntry>, String> {
        let rows = sqlx::query(
            "SELECT seq, event_type, event_data, created_at \
             FROM orcha_node_logs \
             WHERE graph_id = ? AND node_id = ? \
             ORDER BY seq ASC",
        )
        .bind(graph_id)
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch node log: {}", e))?;

        let entries = rows
            .into_iter()
            .map(|row| NodeLogEntry {
                seq: row.get("seq"),
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                created_at: row.get("created_at"),
            })
            .collect();
        Ok(entries)
    }
}

/// A single entry in the node execution log.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeLogEntry {
    pub seq: i64,
    pub event_type: String,
    /// Raw JSON string for the event payload.
    pub event_data: String,
    pub created_at: i64,
}

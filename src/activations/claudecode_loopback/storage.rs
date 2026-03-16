use super::types::{ApprovalId, ApprovalRequest, ApprovalStatus, LoopbackError};
use crate::activations::storage::init_sqlite_pool;
use crate::activation_db_path_from_module;
use serde_json::Value;
use sqlx::{sqlite::SqlitePool, Row};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LoopbackStorageConfig {
    pub db_path: PathBuf,
}

impl Default for LoopbackStorageConfig {
    fn default() -> Self {
        Self {
            db_path: activation_db_path_from_module!("loopback.db"),
        }
    }
}

pub struct LoopbackStorage {
    pool: SqlitePool,
    /// Maps tool_use_id -> session_id for correlation
    /// This allows loopback_permit to find the session_id when called via MCP
    tool_session_map: RwLock<HashMap<String, String>>,
    /// Maps session_id -> Notify for blocking wait on new approvals
    /// Allows wait_for_approval to block until an approval arrives for that session
    session_notifiers: Arc<RwLock<HashMap<String, Arc<Notify>>>>,
    /// Maps child_session_id -> parent_session_id
    /// When a child session gets an approval, the parent is also notified
    session_parents: RwLock<HashMap<String, String>>,
    /// Maps parent_session_id -> [child_session_id]
    /// Allows list_pending to include child session approvals when querying by parent
    session_children: RwLock<HashMap<String, Vec<String>>>,
}

impl LoopbackStorage {
    pub async fn new(config: LoopbackStorageConfig) -> Result<Self, String> {
        let pool = init_sqlite_pool(config.db_path).await?;

        let storage = Self {
            pool,
            tool_session_map: RwLock::new(HashMap::new()),
            session_notifiers: Arc::new(RwLock::new(HashMap::new())),
            session_parents: RwLock::new(HashMap::new()),
            session_children: RwLock::new(HashMap::new()),
        };
        storage.run_migrations().await?;
        Ok(storage)
    }

    /// Register a tool_use_id -> session_id mapping
    /// Called by the background task when it sees a ToolUse event
    pub fn register_tool_session(&self, tool_use_id: &str, session_id: &str) {
        if let Ok(mut map) = self.tool_session_map.write() {
            map.insert(tool_use_id.to_string(), session_id.to_string());
        }
    }

    /// Lookup session_id by tool_use_id
    /// Called by loopback_permit to find the correct session_id
    pub fn lookup_session_by_tool(&self, tool_use_id: &str) -> Option<String> {
        self.tool_session_map.read().ok()?.get(tool_use_id).cloned()
    }

    /// Remove a tool_use_id mapping (called after approval is resolved)
    pub fn remove_tool_mapping(&self, tool_use_id: &str) {
        if let Ok(mut map) = self.tool_session_map.write() {
            map.remove(tool_use_id);
        }
    }

    async fn run_migrations(&self) -> Result<(), LoopbackError> {
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS loopback_approvals (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                tool_use_id TEXT NOT NULL,
                input TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                response_message TEXT,
                created_at INTEGER NOT NULL,
                resolved_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_loopback_session ON loopback_approvals(session_id);
            CREATE INDEX IF NOT EXISTS idx_loopback_status ON loopback_approvals(status);
        "#)
        .execute(&self.pool)
        .await
        .map_err(|e| LoopbackError::Storage { operation: "migration", detail: e.to_string() })?;
        Ok(())
    }

    pub async fn create_approval(
        &self,
        session_id: &str,
        tool_name: &str,
        tool_use_id: &str,
        input: &Value,
    ) -> Result<ApprovalRequest, LoopbackError> {
        let id = Uuid::new_v4();
        let now = current_timestamp();
        let input_json = serde_json::to_string(input)
            .map_err(|e| LoopbackError::Serialization { detail: e.to_string() })?;

        sqlx::query(
            "INSERT INTO loopback_approvals (id, session_id, tool_name, tool_use_id, input, status, created_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?)"
        )
        .bind(id.to_string())
        .bind(session_id)
        .bind(tool_name)
        .bind(tool_use_id)
        .bind(&input_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| LoopbackError::Storage { operation: "create_approval", detail: e.to_string() })?;

        // Notify any waiters that a new approval has arrived
        self.notify_session(session_id);

        Ok(ApprovalRequest {
            id,
            session_id: session_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_use_id: tool_use_id.to_string(),
            input: input.clone(),
            status: ApprovalStatus::Pending,
            response_message: None,
            created_at: now,
            resolved_at: None,
        })
    }

    pub async fn get_approval(&self, id: &ApprovalId) -> Result<ApprovalRequest, LoopbackError> {
        let row = sqlx::query(
            "SELECT id, session_id, tool_name, tool_use_id, input, status, response_message, created_at, resolved_at
             FROM loopback_approvals WHERE id = ?"
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| LoopbackError::Storage { operation: "get_approval", detail: e.to_string() })?
        .ok_or_else(|| LoopbackError::ApprovalNotFound { id: id.to_string() })?;

        self.row_to_approval(row)
    }

    pub async fn resolve_approval(
        &self,
        id: &ApprovalId,
        approved: bool,
        message: Option<String>,
    ) -> Result<(), LoopbackError> {
        let now = current_timestamp();
        let status = if approved { "approved" } else { "denied" };

        let result = sqlx::query(
            "UPDATE loopback_approvals SET status = ?, response_message = ?, resolved_at = ? WHERE id = ?"
        )
        .bind(status)
        .bind(&message)
        .bind(now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| LoopbackError::Storage { operation: "resolve_approval", detail: e.to_string() })?;

        if result.rows_affected() == 0 {
            return Err(LoopbackError::ApprovalNotFound { id: id.to_string() });
        }
        Ok(())
    }

    /// Get all pending approvals for a session
    pub async fn get_pending_approvals(&self, session_id: &str) -> Vec<ApprovalRequest> {
        let rows = sqlx::query(
            "SELECT * FROM loopback_approvals WHERE session_id = ? AND status = 'pending'"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await;

        match rows {
            Ok(rows) => rows.into_iter().filter_map(|row| self.row_to_approval(row).ok()).collect(),
            Err(_) => vec![],
        }
    }

    pub async fn list_pending(&self, session_id: Option<&str>) -> Result<Vec<ApprovalRequest>, LoopbackError> {
        let rows = if let Some(sid) = session_id {
            // Collect all session IDs to query: the given one plus any registered children
            let mut session_ids = vec![sid.to_string()];
            if let Ok(children) = self.session_children.read() {
                if let Some(child_ids) = children.get(sid) {
                    session_ids.extend(child_ids.iter().cloned());
                }
            }

            if session_ids.len() == 1 {
                sqlx::query(
                    "SELECT id, session_id, tool_name, tool_use_id, input, status, response_message, created_at, resolved_at
                     FROM loopback_approvals WHERE session_id = ? AND status = 'pending' ORDER BY created_at"
                )
                .bind(&session_ids[0])
                .fetch_all(&self.pool)
                .await
            } else {
                // Build IN clause for multiple session IDs
                let placeholders = session_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let query_str = format!(
                    "SELECT id, session_id, tool_name, tool_use_id, input, status, response_message, created_at, resolved_at
                     FROM loopback_approvals WHERE session_id IN ({}) AND status = 'pending' ORDER BY created_at",
                    placeholders
                );
                let mut q = sqlx::query(&query_str);
                for sid in &session_ids {
                    q = q.bind(sid);
                }
                q.fetch_all(&self.pool).await
            }
        } else {
            sqlx::query(
                "SELECT id, session_id, tool_name, tool_use_id, input, status, response_message, created_at, resolved_at
                 FROM loopback_approvals WHERE status = 'pending' ORDER BY created_at"
            )
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| LoopbackError::Storage { operation: "list_pending", detail: e.to_string() })?;

        rows.into_iter().map(|r| self.row_to_approval(r)).collect()
    }

    fn row_to_approval(&self, row: sqlx::sqlite::SqliteRow) -> Result<ApprovalRequest, LoopbackError> {
        let id_str: String = row.get("id");
        let input_json: String = row.get("input");
        let status_str: String = row.get("status");

        let status = match status_str.as_str() {
            "pending" => ApprovalStatus::Pending,
            "approved" => ApprovalStatus::Approved,
            "denied" => ApprovalStatus::Denied,
            "timed_out" => ApprovalStatus::TimedOut,
            _ => ApprovalStatus::Pending,
        };

        Ok(ApprovalRequest {
            id: Uuid::parse_str(&id_str).map_err(|e| LoopbackError::InvalidData { detail: format!("Invalid UUID '{}': {}", id_str, e) })?,
            session_id: row.get("session_id"),
            tool_name: row.get("tool_name"),
            tool_use_id: row.get("tool_use_id"),
            input: serde_json::from_str(&input_json).unwrap_or(Value::Null),
            status,
            response_message: row.get("response_message"),
            created_at: row.get("created_at"),
            resolved_at: row.get("resolved_at"),
        })
    }

    /// Get or create a notifier for a session
    /// This allows multiple wait_for_approval calls to wait on the same session
    pub fn get_or_create_notifier(&self, session_id: &str) -> Arc<Notify> {
        let mut notifiers = self.session_notifiers.write().unwrap();
        notifiers
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone()
    }

    /// Register a parent session for a child session.
    /// When the child gets an approval, the parent notifier is also woken.
    /// Also registers the inverse mapping so list_pending can find child approvals.
    pub fn register_session_parent(&self, child_session_id: &str, parent_session_id: &str) {
        if let Ok(mut map) = self.session_parents.write() {
            map.insert(child_session_id.to_string(), parent_session_id.to_string());
        }
        if let Ok(mut map) = self.session_children.write() {
            map.entry(parent_session_id.to_string())
                .or_default()
                .push(child_session_id.to_string());
        }
    }

    /// Notify waiters on a session that a new approval has arrived.
    /// Uses notify_one() so the permit is stored even if no task is currently
    /// suspended in notified() — preventing lost wakeups when the auto-approver
    /// is busy processing a previous batch.
    /// Also notifies the parent session if one is registered.
    fn notify_session(&self, session_id: &str) {
        if let Ok(notifiers) = self.session_notifiers.read() {
            if let Some(notifier) = notifiers.get(session_id) {
                notifier.notify_one();
            }
        }
        // Propagate to parent (e.g., Orcha session waiting on any child approval)
        if let Ok(parents) = self.session_parents.read() {
            if let Some(parent_id) = parents.get(session_id) {
                if let Ok(notifiers) = self.session_notifiers.read() {
                    if let Some(notifier) = notifiers.get(parent_id.as_str()) {
                        notifier.notify_one();
                    }
                }
            }
        }
    }

    /// Clean up notifier for a session (optional, for resource cleanup)
    pub fn remove_notifier(&self, session_id: &str) {
        if let Ok(mut notifiers) = self.session_notifiers.write() {
            notifiers.remove(session_id);
        }
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

use super::types::{ApprovalId, ApprovalRequest, ApprovalStatus};
use serde_json::Value;
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePool}, ConnectOptions, Row};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LoopbackStorageConfig {
    pub db_path: PathBuf,
}

impl Default for LoopbackStorageConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("loopback.db"),
        }
    }
}

pub struct LoopbackStorage {
    pool: SqlitePool,
    /// Maps tool_use_id -> session_id for correlation
    /// This allows loopback_permit to find the session_id when called via MCP
    tool_session_map: RwLock<HashMap<String, String>>,
}

impl LoopbackStorage {
    pub async fn new(config: LoopbackStorageConfig) -> Result<Self, String> {
        let db_url = format!("sqlite:{}?mode=rwc", config.db_path.display());
        let mut options: SqliteConnectOptions = db_url.parse()
            .map_err(|e| format!("Failed to parse DB URL: {}", e))?;
        options.disable_statement_logging();

        let pool = SqlitePool::connect_with(options)
            .await
            .map_err(|e| format!("Failed to connect: {}", e))?;

        let storage = Self {
            pool,
            tool_session_map: RwLock::new(HashMap::new()),
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

    async fn run_migrations(&self) -> Result<(), String> {
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
        .map_err(|e| format!("Migration failed: {}", e))?;
        Ok(())
    }

    pub async fn create_approval(
        &self,
        session_id: &str,
        tool_name: &str,
        tool_use_id: &str,
        input: &Value,
    ) -> Result<ApprovalRequest, String> {
        let id = Uuid::new_v4();
        let now = current_timestamp();
        let input_json = serde_json::to_string(input)
            .map_err(|e| format!("Failed to serialize input: {}", e))?;

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
        .map_err(|e| format!("Failed to create approval: {}", e))?;

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

    pub async fn get_approval(&self, id: &ApprovalId) -> Result<ApprovalRequest, String> {
        let row = sqlx::query(
            "SELECT id, session_id, tool_name, tool_use_id, input, status, response_message, created_at, resolved_at
             FROM loopback_approvals WHERE id = ?"
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch approval: {}", e))?
        .ok_or_else(|| format!("Approval not found: {}", id))?;

        self.row_to_approval(row)
    }

    pub async fn resolve_approval(
        &self,
        id: &ApprovalId,
        approved: bool,
        message: Option<String>,
    ) -> Result<(), String> {
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
        .map_err(|e| format!("Failed to resolve approval: {}", e))?;

        if result.rows_affected() == 0 {
            return Err(format!("Approval not found: {}", id));
        }
        Ok(())
    }

    pub async fn list_pending(&self, session_id: Option<&str>) -> Result<Vec<ApprovalRequest>, String> {
        let rows = if let Some(sid) = session_id {
            sqlx::query(
                "SELECT id, session_id, tool_name, tool_use_id, input, status, response_message, created_at, resolved_at
                 FROM loopback_approvals WHERE session_id = ? AND status = 'pending' ORDER BY created_at"
            )
            .bind(sid)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, session_id, tool_name, tool_use_id, input, status, response_message, created_at, resolved_at
                 FROM loopback_approvals WHERE status = 'pending' ORDER BY created_at"
            )
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| format!("Failed to list pending: {}", e))?;

        rows.into_iter().map(|r| self.row_to_approval(r)).collect()
    }

    fn row_to_approval(&self, row: sqlx::sqlite::SqliteRow) -> Result<ApprovalRequest, String> {
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
            id: Uuid::parse_str(&id_str).map_err(|e| format!("Invalid UUID: {}", e))?,
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
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

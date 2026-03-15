use super::types::{SessionId, SessionInfo, SessionState};
use crate::activations::storage::init_sqlite_pool;
use crate::activation_db_path_from_module;
use sqlx::{sqlite::SqlitePool, Row};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for Orcha storage
#[derive(Debug, Clone)]
pub struct OrchaStorageConfig {
    pub db_path: PathBuf,
}

impl Default for OrchaStorageConfig {
    fn default() -> Self {
        Self {
            db_path: activation_db_path_from_module!("orcha.db"),
        }
    }
}

/// Storage for orcha sessions backed by SQLite
pub struct OrchaStorage {
    pool: SqlitePool,
    /// In-memory cache of active sessions
    sessions: Arc<RwLock<HashMap<SessionId, SessionInfo>>>,
}

impl OrchaStorage {
    /// Create new storage with the given configuration
    pub async fn new(config: OrchaStorageConfig) -> Result<Self, String> {
        let pool = init_sqlite_pool(config.db_path).await?;

        let storage = Self {
            pool,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        };

        storage.init_schema().await?;
        storage.load_sessions().await?;

        Ok(storage)
    }

    /// Initialize database schema
    async fn init_schema(&self) -> Result<(), String> {
        // Create orcha_sessions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orcha_sessions (
                session_id TEXT PRIMARY KEY,
                model TEXT NOT NULL,
                working_directory TEXT NOT NULL,
                rules TEXT,
                max_retries INTEGER NOT NULL DEFAULT 3,
                retry_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_activity INTEGER NOT NULL,
                state_type TEXT NOT NULL,
                state_data TEXT,
                UNIQUE(session_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create orcha_sessions table: {}", e))?;

        // Migrate orcha_sessions: add agent_mode column if not exists
        // SQLite doesn't have a nice IF NOT EXISTS for ALTER TABLE, so we use PRAGMA
        // PRAGMA table_info returns: (cid, name, type, notnull, dflt_value, pk)
        let rows = sqlx::query("PRAGMA table_info(orcha_sessions)")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("Failed to get table info: {}", e))?;

        let column_names: Vec<String> = rows.iter()
            .filter_map(|row| match row.try_get::<String, _>("name") {
                Ok(name) => Some(name),
                Err(e) => {
                    tracing::warn!("Failed to read column name from PRAGMA table_info: {}", e);
                    None
                }
            })
            .collect();

        let has_agent_mode = column_names.iter().any(|name| name == "agent_mode");
        if !has_agent_mode {
            sqlx::query("ALTER TABLE orcha_sessions ADD COLUMN agent_mode TEXT NOT NULL DEFAULT 'single'")
                .execute(&self.pool)
                .await
                .map_err(|e| format!("Failed to add agent_mode column: {}", e))?;
        }

        let has_primary_agent_id = column_names.iter().any(|name| name == "primary_agent_id");
        if !has_primary_agent_id {
            sqlx::query("ALTER TABLE orcha_sessions ADD COLUMN primary_agent_id TEXT")
                .execute(&self.pool)
                .await
                .map_err(|e| format!("Failed to add primary_agent_id column: {}", e))?;
        }

        let has_tree_id = column_names.iter().any(|name| name == "tree_id");
        if !has_tree_id {
            sqlx::query("ALTER TABLE orcha_sessions ADD COLUMN tree_id TEXT")
                .execute(&self.pool)
                .await
                .map_err(|e| format!("Failed to add tree_id column: {}", e))?;
        }

        // Create orcha_agents table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orcha_agents (
                agent_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                claudecode_session_id TEXT NOT NULL,
                subtask TEXT NOT NULL,
                state_type TEXT NOT NULL,
                state_data TEXT,
                is_primary INTEGER NOT NULL DEFAULT 0,
                parent_agent_id TEXT,
                created_at INTEGER NOT NULL,
                last_activity INTEGER NOT NULL,
                completed_at INTEGER,
                error_message TEXT,
                FOREIGN KEY (session_id) REFERENCES orcha_sessions(session_id) ON DELETE CASCADE,
                FOREIGN KEY (parent_agent_id) REFERENCES orcha_agents(agent_id) ON DELETE SET NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create orcha_agents table: {}", e))?;

        // Create indexes for orcha_agents
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_agents_session ON orcha_agents(session_id)")
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to create session index: {}", e))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_agents_state ON orcha_agents(state_type)")
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to create state index: {}", e))?;

        Ok(())
    }

    /// Load all sessions from database into memory cache
    async fn load_sessions(&self) -> Result<(), String> {
        let rows = sqlx::query("SELECT * FROM orcha_sessions")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("Failed to load sessions: {}", e))?;

        let mut sessions = self.sessions.write().await;

        for row in rows {
            let session_id: String = row.get("session_id");
            let model: String = row.get("model");
            let created_at: i64 = row.get("created_at");
            let last_activity: i64 = row.get("last_activity");
            let retry_count: i64 = row.get("retry_count");
            let max_retries: i64 = row.get("max_retries");
            let state_type: String = row.get("state_type");
            let state_data: Option<String> = row.get("state_data");

            // Try to get new fields, default if not present (for backward compat)
            let agent_mode_str: Option<String> = row.try_get("agent_mode").ok();
            let agent_mode = agent_mode_str
                .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                .unwrap_or(super::types::AgentMode::Single);

            let primary_agent_id: Option<String> = row.try_get("primary_agent_id").ok().flatten();
            let tree_id: Option<String> = row.try_get("tree_id").ok().flatten();

            let state = self.deserialize_state(&state_type, state_data.as_deref())?;

            let info = SessionInfo {
                session_id: session_id.clone(),
                model,
                created_at,
                last_activity,
                state,
                retry_count: retry_count as u32,
                max_retries: max_retries as u32,
                agent_mode,
                primary_agent_id,
                tree_id,
            };

            sessions.insert(session_id, info);
        }

        Ok(())
    }

    /// Create a new session
    pub async fn create_session(
        &self,
        session_id: SessionId,
        model: String,
        working_directory: String,
        rules: Option<String>,
        max_retries: u32,
        agent_mode: super::types::AgentMode,
        tree_id: Option<String>,
    ) -> Result<SessionInfo, String> {
        let now = chrono::Utc::now().timestamp();

        let agent_mode_str = match agent_mode {
            super::types::AgentMode::Single => "single",
            super::types::AgentMode::Multi => "multi",
        };

        let info = SessionInfo {
            session_id: session_id.clone(),
            model: model.clone(),
            created_at: now,
            last_activity: now,
            state: SessionState::Idle,
            retry_count: 0,
            max_retries,
            agent_mode,
            primary_agent_id: None,
            tree_id: tree_id.clone(),
        };

        // Insert into database
        sqlx::query(
            r#"
            INSERT INTO orcha_sessions (
                session_id, model, working_directory, rules, max_retries,
                retry_count, created_at, last_activity, state_type, state_data,
                agent_mode, primary_agent_id, tree_id
            ) VALUES (?, ?, ?, ?, ?, 0, ?, ?, 'idle', NULL, ?, NULL, ?)
            "#,
        )
        .bind(&session_id)
        .bind(&model)
        .bind(&working_directory)
        .bind(&rules)
        .bind(max_retries as i64)
        .bind(now)
        .bind(now)
        .bind(agent_mode_str)
        .bind(&tree_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create session: {}", e))?;

        // Add to cache
        self.sessions.write().await.insert(session_id.clone(), info.clone());

        Ok(info)
    }

    /// Get session info
    pub async fn get_session(&self, session_id: &SessionId) -> Result<SessionInfo, String> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| format!("Session not found: {}", session_id))
    }

    /// Update session state
    pub async fn update_state(
        &self,
        session_id: &SessionId,
        state: SessionState,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        let (state_type, state_data) = self.serialize_state(&state);

        sqlx::query(
            r#"
            UPDATE orcha_sessions
            SET state_type = ?, state_data = ?, last_activity = ?
            WHERE session_id = ?
            "#,
        )
        .bind(&state_type)
        .bind(&state_data)
        .bind(now)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to update state: {}", e))?;

        // Update cache
        if let Some(info) = self.sessions.write().await.get_mut(session_id) {
            info.state = state;
            info.last_activity = now;
        }

        Ok(())
    }

    /// Increment retry count
    pub async fn increment_retry(&self, session_id: &SessionId) -> Result<u32, String> {
        sqlx::query(
            r#"
            UPDATE orcha_sessions
            SET retry_count = retry_count + 1
            WHERE session_id = ?
            "#,
        )
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to increment retry count: {}", e))?;

        // Update cache and return new count
        if let Some(info) = self.sessions.write().await.get_mut(session_id) {
            info.retry_count += 1;
            Ok(info.retry_count)
        } else {
            Err(format!("Session not found: {}", session_id))
        }
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions.read().await.values().cloned().collect()
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &SessionId) -> Result<(), String> {
        sqlx::query("DELETE FROM orcha_sessions WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to delete session: {}", e))?;

        self.sessions.write().await.remove(session_id);

        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Agent Management (Multi-Agent Orchestration)
    // ═══════════════════════════════════════════════════════════════════════

    /// Create a new agent for a session
    pub async fn create_agent(
        &self,
        session_id: &SessionId,
        claudecode_session_id: String,
        subtask: String,
        is_primary: bool,
        parent_agent_id: Option<super::types::AgentId>,
    ) -> Result<super::types::AgentInfo, String> {
        let agent_id = format!("agent-{}", uuid::Uuid::new_v4());
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO orcha_agents (
                agent_id, session_id, claudecode_session_id, subtask,
                state_type, state_data, is_primary, parent_agent_id,
                created_at, last_activity
            ) VALUES (?, ?, ?, ?, 'idle', NULL, ?, ?, ?, ?)
            "#,
        )
        .bind(&agent_id)
        .bind(session_id)
        .bind(&claudecode_session_id)
        .bind(&subtask)
        .bind(if is_primary { 1 } else { 0 })
        .bind(&parent_agent_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create agent: {}", e))?;

        Ok(super::types::AgentInfo {
            agent_id,
            session_id: session_id.clone(),
            claudecode_session_id,
            subtask,
            state: super::types::AgentState::Idle,
            is_primary,
            parent_agent_id,
            created_at: now,
            last_activity: now,
            completed_at: None,
            error_message: None,
        })
    }

    /// Get agent by ID
    pub async fn get_agent(&self, agent_id: &super::types::AgentId) -> Result<super::types::AgentInfo, String> {
        let row = sqlx::query("SELECT * FROM orcha_agents WHERE agent_id = ?")
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| format!("Failed to fetch agent: {}", e))?
            .ok_or_else(|| format!("Agent not found: {}", agent_id))?;

        self.row_to_agent(row)
    }

    /// List all agents for a session
    pub async fn list_agents(&self, session_id: &SessionId) -> Result<Vec<super::types::AgentInfo>, String> {
        let rows = sqlx::query(
            "SELECT * FROM orcha_agents WHERE session_id = ? ORDER BY created_at ASC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list agents: {}", e))?;

        rows.into_iter()
            .map(|row| self.row_to_agent(row))
            .collect()
    }

    /// Update agent state
    pub async fn update_agent_state(
        &self,
        agent_id: &super::types::AgentId,
        state: super::types::AgentState,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        let (state_type, state_data) = self.serialize_agent_state(&state);

        // Also update completed_at if state is Complete or Failed
        let completed_at = match state {
            super::types::AgentState::Complete | super::types::AgentState::Failed { .. } => Some(now),
            _ => None,
        };

        // Extract error message if failed
        let error_message = match &state {
            super::types::AgentState::Failed { error } => Some(error.clone()),
            _ => None,
        };

        if completed_at.is_some() {
            sqlx::query(
                r#"
                UPDATE orcha_agents
                SET state_type = ?, state_data = ?, last_activity = ?, completed_at = ?, error_message = ?
                WHERE agent_id = ?
                "#,
            )
            .bind(&state_type)
            .bind(&state_data)
            .bind(now)
            .bind(completed_at)
            .bind(&error_message)
            .bind(agent_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to update agent state: {}", e))?;
        } else {
            sqlx::query(
                r#"
                UPDATE orcha_agents
                SET state_type = ?, state_data = ?, last_activity = ?
                WHERE agent_id = ?
                "#,
            )
            .bind(&state_type)
            .bind(&state_data)
            .bind(now)
            .bind(agent_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to update agent state: {}", e))?;
        }

        Ok(())
    }

    /// Get session agent counts (active, completed, failed)
    pub async fn get_agent_counts(&self, session_id: &SessionId) -> Result<(u32, u32, u32), String> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(CASE WHEN state_type IN ('idle', 'running', 'waiting_approval', 'validating') THEN 1 END) as active,
                COUNT(CASE WHEN state_type = 'complete' THEN 1 END) as completed,
                COUNT(CASE WHEN state_type = 'failed' THEN 1 END) as failed
            FROM orcha_agents WHERE session_id = ?
            "#
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to get agent counts: {}", e))?;

        let active: i64 = row.get("active");
        let completed: i64 = row.get("completed");
        let failed: i64 = row.get("failed");

        Ok((active as u32, completed as u32, failed as u32))
    }

    /// Helper: Convert row to AgentInfo
    fn row_to_agent(&self, row: sqlx::sqlite::SqliteRow) -> Result<super::types::AgentInfo, String> {
        let state_type: String = row.get("state_type");
        let state_data: Option<String> = row.get("state_data");
        let state = self.deserialize_agent_state(&state_type, state_data.as_deref())?;

        Ok(super::types::AgentInfo {
            agent_id: row.get("agent_id"),
            session_id: row.get("session_id"),
            claudecode_session_id: row.get("claudecode_session_id"),
            subtask: row.get("subtask"),
            state,
            is_primary: row.get::<i64, _>("is_primary") == 1,
            parent_agent_id: row.get("parent_agent_id"),
            created_at: row.get("created_at"),
            last_activity: row.get("last_activity"),
            completed_at: row.get("completed_at"),
            error_message: row.get("error_message"),
        })
    }

    /// Helper: Serialize agent state
    fn serialize_agent_state(&self, state: &super::types::AgentState) -> (String, Option<String>) {
        match state {
            super::types::AgentState::Idle => ("idle".to_string(), None),
            super::types::AgentState::Running { sequence } => (
                "running".to_string(),
                Some(serde_json::json!({ "sequence": sequence }).to_string()),
            ),
            super::types::AgentState::WaitingApproval { approval_id } => (
                "waiting_approval".to_string(),
                Some(serde_json::json!({ "approval_id": approval_id }).to_string()),
            ),
            super::types::AgentState::Validating { test_command } => (
                "validating".to_string(),
                Some(serde_json::json!({ "test_command": test_command }).to_string()),
            ),
            super::types::AgentState::Complete => ("complete".to_string(), None),
            super::types::AgentState::Failed { error } => (
                "failed".to_string(),
                Some(serde_json::json!({ "error": error }).to_string()),
            ),
        }
    }

    /// Helper: Deserialize agent state
    fn deserialize_agent_state(&self, state_type: &str, state_data: Option<&str>) -> Result<super::types::AgentState, String> {
        match state_type {
            "idle" => Ok(super::types::AgentState::Idle),
            "running" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse running state: {}", e))?;
                Ok(super::types::AgentState::Running {
                    sequence: data["sequence"].as_u64().unwrap_or(0),
                })
            }
            "waiting_approval" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse waiting_approval state: {}", e))?;
                Ok(super::types::AgentState::WaitingApproval {
                    approval_id: data["approval_id"].as_str().unwrap_or("").to_string(),
                })
            }
            "validating" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse validating state: {}", e))?;
                Ok(super::types::AgentState::Validating {
                    test_command: data["test_command"].as_str().unwrap_or("").to_string(),
                })
            }
            "complete" => Ok(super::types::AgentState::Complete),
            "failed" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse failed state: {}", e))?;
                Ok(super::types::AgentState::Failed {
                    error: data["error"].as_str().unwrap_or("Unknown error").to_string(),
                })
            }
            _ => Err(format!("Unknown agent state type: {}", state_type)),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // State Serialization Helpers
    // ═══════════════════════════════════════════════════════════════════════

    fn serialize_state(&self, state: &SessionState) -> (String, Option<String>) {
        match state {
            SessionState::Idle => ("idle".to_string(), None),
            SessionState::Running { stream_id, sequence, active_agents, completed_agents, failed_agents } => (
                "running".to_string(),
                Some(serde_json::json!({
                    "stream_id": stream_id,
                    "sequence": sequence,
                    "active_agents": active_agents,
                    "completed_agents": completed_agents,
                    "failed_agents": failed_agents,
                }).to_string()),
            ),
            SessionState::WaitingApproval { approval_id } => (
                "waiting_approval".to_string(),
                Some(serde_json::json!({
                    "approval_id": approval_id,
                }).to_string()),
            ),
            SessionState::Validating { test_command } => (
                "validating".to_string(),
                Some(serde_json::json!({
                    "test_command": test_command,
                }).to_string()),
            ),
            SessionState::Complete => ("complete".to_string(), None),
            SessionState::Failed { error } => (
                "failed".to_string(),
                Some(serde_json::json!({
                    "error": error,
                }).to_string()),
            ),
        }
    }

    fn deserialize_state(&self, state_type: &str, state_data: Option<&str>) -> Result<SessionState, String> {
        match state_type {
            "idle" => Ok(SessionState::Idle),
            "running" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse running state: {}", e))?;
                Ok(SessionState::Running {
                    stream_id: data["stream_id"].as_str().unwrap_or("").to_string(),
                    sequence: data["sequence"].as_u64().unwrap_or(0),
                    active_agents: data["active_agents"].as_u64().unwrap_or(0) as u32,
                    completed_agents: data["completed_agents"].as_u64().unwrap_or(0) as u32,
                    failed_agents: data["failed_agents"].as_u64().unwrap_or(0) as u32,
                })
            }
            "waiting_approval" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse waiting_approval state: {}", e))?;
                Ok(SessionState::WaitingApproval {
                    approval_id: data["approval_id"].as_str().unwrap_or("").to_string(),
                })
            }
            "validating" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse validating state: {}", e))?;
                Ok(SessionState::Validating {
                    test_command: data["test_command"].as_str().unwrap_or("").to_string(),
                })
            }
            "complete" => Ok(SessionState::Complete),
            "failed" => {
                let data: serde_json::Value = serde_json::from_str(state_data.unwrap_or("{}"))
                    .map_err(|e| format!("Failed to parse failed state: {}", e))?;
                Ok(SessionState::Failed {
                    error: data["error"].as_str().unwrap_or("Unknown error").to_string(),
                })
            }
            _ => Err(format!("Unknown state type: {}", state_type)),
        }
    }
}

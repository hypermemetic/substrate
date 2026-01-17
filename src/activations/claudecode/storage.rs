use super::activation::ClaudeCode;
use super::types::{
    ClaudeCodeConfig, ClaudeCodeError, ClaudeCodeId, ClaudeCodeInfo,
    Message, MessageId, MessageRole, Model, Position,
};
use crate::activations::arbor::{ArborStorage, NodeId, TreeId};
use crate::types::Handle;
use serde_json::Value;
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePool}, ConnectOptions, Row};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Configuration for ClaudeCode storage
#[derive(Debug, Clone)]
pub struct ClaudeCodeStorageConfig {
    /// Path to SQLite database for ClaudeCode sessions
    pub db_path: PathBuf,
}

impl Default for ClaudeCodeStorageConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("claudecode.db"),
        }
    }
}

/// Storage layer for ClaudeCode sessions
pub struct ClaudeCodeStorage {
    pool: SqlitePool,
    arbor: Arc<ArborStorage>,
}

impl ClaudeCodeStorage {
    /// Create a new ClaudeCode storage instance with a shared Arbor storage
    pub async fn new(
        config: ClaudeCodeStorageConfig,
        arbor: Arc<ArborStorage>,
    ) -> Result<Self, ClaudeCodeError> {
        let db_url = format!("sqlite:{}?mode=rwc", config.db_path.display());
        let mut connect_options: SqliteConnectOptions = db_url.parse()
            .map_err(|e| format!("Failed to parse database URL: {}", e))?;
        connect_options.disable_statement_logging();
        let pool = SqlitePool::connect_with(connect_options.clone())
            .await
            .map_err(|e| format!("Failed to connect to claudecode database: {}", e))?;

        let storage = Self { pool, arbor };
        storage.run_migrations().await?;

        Ok(storage)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<(), ClaudeCodeError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS claudecode_sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                claude_session_id TEXT,
                tree_id TEXT NOT NULL,
                canonical_head TEXT NOT NULL,
                working_dir TEXT NOT NULL,
                model TEXT NOT NULL,
                system_prompt TEXT,
                mcp_config TEXT,
                loopback_enabled INTEGER NOT NULL DEFAULT 0,
                metadata TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS claudecode_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                model_id TEXT,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cost_usd REAL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES claudecode_sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_claudecode_sessions_name ON claudecode_sessions(name);
            CREATE INDEX IF NOT EXISTS idx_claudecode_sessions_tree ON claudecode_sessions(tree_id);
            CREATE INDEX IF NOT EXISTS idx_claudecode_messages_session ON claudecode_messages(session_id);

            CREATE TABLE IF NOT EXISTS claudecode_unknown_events (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                event_type TEXT NOT NULL,
                data TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES claudecode_sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_claudecode_unknown_events_session ON claudecode_unknown_events(session_id);
            CREATE INDEX IF NOT EXISTS idx_claudecode_unknown_events_type ON claudecode_unknown_events(event_type);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to run claudecode migrations: {}", e))?;

        Ok(())
    }

    /// Get access to the underlying arbor storage
    pub fn arbor(&self) -> &ArborStorage {
        &self.arbor
    }

    // ========================================================================
    // Session CRUD Operations
    // ========================================================================

    /// Create a new ClaudeCode session with a new conversation tree
    pub async fn session_create(
        &self,
        name: String,
        working_dir: String,
        model: Model,
        system_prompt: Option<String>,
        mcp_config: Option<Value>,
        loopback_enabled: bool,
        metadata: Option<Value>,
    ) -> Result<ClaudeCodeConfig, ClaudeCodeError> {
        let session_id = ClaudeCodeId::new_v4();
        let now = current_timestamp();

        // Create a new tree for this session
        let tree_id = self
            .arbor
            .tree_create(metadata.clone(), &session_id.to_string())
            .await
            .map_err(|e| format!("Failed to create tree for session: {}", e))?;

        // Get the root node as initial position
        let tree = self
            .arbor
            .tree_get(&tree_id)
            .await
            .map_err(|e| format!("Failed to get tree: {}", e))?;
        let head = Position::new(tree_id, tree.root);

        let metadata_json = metadata.as_ref().map(|m| serde_json::to_string(m).unwrap());
        let mcp_config_json = mcp_config.as_ref().map(|m| serde_json::to_string(m).unwrap());

        // Try inserting with the original name first
        let final_name = match sqlx::query(
            "INSERT INTO claudecode_sessions (id, name, claude_session_id, tree_id, canonical_head, working_dir, model, system_prompt, mcp_config, loopback_enabled, metadata, created_at, updated_at)
             VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id.to_string())
        .bind(&name)
        .bind(head.tree_id.to_string())
        .bind(head.node_id.to_string())
        .bind(&working_dir)
        .bind(model.as_str())
        .bind(&system_prompt)
        .bind(mcp_config_json.clone())
        .bind(loopback_enabled)
        .bind(metadata_json.clone())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        {
            Ok(_) => name,
            Err(e) if e.to_string().contains("UNIQUE constraint failed") => {
                // Name collision - append #uuid to make it unique
                let unique_name = format!("{}#{}", name, session_id);

                sqlx::query(
                    "INSERT INTO claudecode_sessions (id, name, claude_session_id, tree_id, canonical_head, working_dir, model, system_prompt, mcp_config, loopback_enabled, metadata, created_at, updated_at)
                     VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(session_id.to_string())
                .bind(&unique_name)
                .bind(head.tree_id.to_string())
                .bind(head.node_id.to_string())
                .bind(&working_dir)
                .bind(model.as_str())
                .bind(&system_prompt)
                .bind(mcp_config_json)
                .bind(loopback_enabled)
                .bind(metadata_json)
                .bind(now)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(|e| format!("Failed to create session with unique name: {}", e))?;

                unique_name
            }
            Err(e) => return Err(ClaudeCodeError::from(format!("Failed to create session: {}", e))),
        };

        Ok(ClaudeCodeConfig {
            id: session_id,
            name: final_name,
            claude_session_id: None,
            head,
            working_dir,
            model,
            system_prompt,
            mcp_config,
            loopback_enabled,
            metadata,
            created_at: now,
            updated_at: now,
        })
    }

    /// Get a session by ID
    pub async fn session_get(&self, session_id: &ClaudeCodeId) -> Result<ClaudeCodeConfig, ClaudeCodeError> {
        let row = sqlx::query(
            "SELECT id, name, claude_session_id, tree_id, canonical_head, working_dir, model, system_prompt, mcp_config, loopback_enabled, metadata, created_at, updated_at
             FROM claudecode_sessions WHERE id = ?",
        )
        .bind(session_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch session: {}", e))?
        .ok_or_else(|| format!("Session not found: {}", session_id))?;

        self.row_to_config(row)
    }

    /// Get a session by name (supports partial matching)
    pub async fn session_get_by_name(&self, name: &str) -> Result<ClaudeCodeConfig, ClaudeCodeError> {
        // Try exact match first
        if let Some(row) = sqlx::query(
            "SELECT id, name, claude_session_id, tree_id, canonical_head, working_dir, model, system_prompt, mcp_config, loopback_enabled, metadata, created_at, updated_at
             FROM claudecode_sessions WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch session by name: {}", e))?
        {
            return self.row_to_config(row);
        }

        // Try partial match
        let pattern = format!("{}%", name);
        let rows = sqlx::query(
            "SELECT id, name, claude_session_id, tree_id, canonical_head, working_dir, model, system_prompt, mcp_config, loopback_enabled, metadata, created_at, updated_at
             FROM claudecode_sessions WHERE name LIKE ?",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch session by pattern: {}", e))?;

        match rows.len() {
            0 => Err(ClaudeCodeError::from(format!("Session not found with name: {}", name))),
            1 => self.row_to_config(rows.into_iter().next().unwrap()),
            _ => {
                let matches: Vec<String> = rows.iter().map(|r| r.get("name")).collect();
                Err(ClaudeCodeError::from(format!(
                    "Ambiguous name '{}' matches multiple sessions: {}",
                    name,
                    matches.join(", ")
                )))
            }
        }
    }

    /// List all sessions
    pub async fn session_list(&self) -> Result<Vec<ClaudeCodeInfo>, ClaudeCodeError> {
        let rows = sqlx::query(
            "SELECT id, name, claude_session_id, tree_id, canonical_head, working_dir, model, loopback_enabled, created_at
             FROM claudecode_sessions ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list sessions: {}", e))?;

        let sessions: Result<Vec<ClaudeCodeInfo>, ClaudeCodeError> = rows
            .iter()
            .map(|row| {
                let id_str: String = row.get("id");
                let tree_id_str: String = row.get("tree_id");
                let head_str: String = row.get("canonical_head");
                let model_str: String = row.get("model");
                let loopback: i32 = row.get("loopback_enabled");

                let tree_id = TreeId::parse_str(&tree_id_str)
                    .map_err(|e| format!("Invalid tree ID: {}", e))?;
                let node_id = NodeId::parse_str(&head_str)
                    .map_err(|e| format!("Invalid node ID: {}", e))?;
                let model = Model::from_str(&model_str)
                    .ok_or_else(|| format!("Invalid model: {}", model_str))?;

                Ok(ClaudeCodeInfo {
                    id: Uuid::parse_str(&id_str).map_err(|e| format!("Invalid session ID: {}", e))?,
                    name: row.get("name"),
                    model,
                    head: Position::new(tree_id, node_id),
                    claude_session_id: row.get("claude_session_id"),
                    working_dir: row.get("working_dir"),
                    loopback_enabled: loopback != 0,
                    created_at: row.get("created_at"),
                })
            })
            .collect();

        sessions
    }

    /// Update session's canonical head and optionally the Claude session ID
    pub async fn session_update_head(
        &self,
        session_id: &ClaudeCodeId,
        new_head: NodeId,
        claude_session_id: Option<String>,
    ) -> Result<(), ClaudeCodeError> {
        let now = current_timestamp();

        let result = if let Some(claude_id) = claude_session_id {
            sqlx::query(
                "UPDATE claudecode_sessions SET canonical_head = ?, claude_session_id = ?, updated_at = ? WHERE id = ?",
            )
            .bind(new_head.to_string())
            .bind(claude_id)
            .bind(now)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
        } else {
            sqlx::query(
                "UPDATE claudecode_sessions SET canonical_head = ?, updated_at = ? WHERE id = ?",
            )
            .bind(new_head.to_string())
            .bind(now)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
        }
        .map_err(|e| format!("Failed to update session head: {}", e))?;

        if result.rows_affected() == 0 {
            return Err(format!("Session not found: {}", session_id).into());
        }

        Ok(())
    }

    /// Update session configuration
    pub async fn session_update(
        &self,
        session_id: &ClaudeCodeId,
        name: Option<String>,
        model: Option<Model>,
        system_prompt: Option<Option<String>>,
        mcp_config: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<(), ClaudeCodeError> {
        let now = current_timestamp();
        let current = self.session_get(session_id).await?;

        let new_name = name.unwrap_or(current.name);
        let new_model = model.unwrap_or(current.model);
        let new_prompt = system_prompt.unwrap_or(current.system_prompt);
        let new_mcp = mcp_config.or(current.mcp_config);
        let new_metadata = metadata.or(current.metadata);

        let mcp_json = new_mcp.as_ref().map(|m| serde_json::to_string(m).unwrap());
        let metadata_json = new_metadata.as_ref().map(|m| serde_json::to_string(m).unwrap());

        sqlx::query(
            "UPDATE claudecode_sessions SET name = ?, model = ?, system_prompt = ?, mcp_config = ?, metadata = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&new_name)
        .bind(new_model.as_str())
        .bind(&new_prompt)
        .bind(mcp_json)
        .bind(metadata_json)
        .bind(now)
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to update session: {}", e))?;

        Ok(())
    }

    /// Delete a session (does not delete the arbor tree)
    pub async fn session_delete(&self, session_id: &ClaudeCodeId) -> Result<(), ClaudeCodeError> {
        let result = sqlx::query("DELETE FROM claudecode_sessions WHERE id = ?")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to delete session: {}", e))?;

        if result.rows_affected() == 0 {
            return Err(format!("Session not found: {}", session_id).into());
        }

        Ok(())
    }

    // ========================================================================
    // Message Operations
    // ========================================================================

    /// Create a message and return it
    pub async fn message_create(
        &self,
        session_id: &ClaudeCodeId,
        role: MessageRole,
        content: String,
        model_id: Option<String>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cost_usd: Option<f64>,
    ) -> Result<Message, ClaudeCodeError> {
        let message_id = MessageId::new_v4();
        let now = current_timestamp();

        sqlx::query(
            "INSERT INTO claudecode_messages (id, session_id, role, content, model_id, input_tokens, output_tokens, cost_usd, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(message_id.to_string())
        .bind(session_id.to_string())
        .bind(role.as_str())
        .bind(&content)
        .bind(&model_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cost_usd)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create message: {}", e))?;

        Ok(Message {
            id: message_id,
            session_id: *session_id,
            role,
            content,
            created_at: now,
            model_id,
            input_tokens,
            output_tokens,
            cost_usd,
        })
    }

    /// Create an ephemeral message (marked for deletion) and return it
    pub async fn message_create_ephemeral(
        &self,
        session_id: &ClaudeCodeId,
        role: MessageRole,
        content: String,
        model_id: Option<String>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cost_usd: Option<f64>,
    ) -> Result<Message, ClaudeCodeError> {
        let message_id = MessageId::new_v4();
        let now = current_timestamp();

        // Insert with a special marker in metadata or a separate flag
        // For now, we'll use a negative timestamp as a deletion marker
        // Messages with negative created_at are ephemeral and should be cleaned up
        let ephemeral_marker = -now;

        sqlx::query(
            "INSERT INTO claudecode_messages (id, session_id, role, content, model_id, input_tokens, output_tokens, cost_usd, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(message_id.to_string())
        .bind(session_id.to_string())
        .bind(role.as_str())
        .bind(&content)
        .bind(&model_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cost_usd)
        .bind(ephemeral_marker)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create ephemeral message: {}", e))?;

        Ok(Message {
            id: message_id,
            session_id: *session_id,
            role,
            content,
            created_at: ephemeral_marker,
            model_id,
            input_tokens,
            output_tokens,
            cost_usd,
        })
    }

    /// Get a message by ID
    pub async fn message_get(&self, message_id: &MessageId) -> Result<Message, ClaudeCodeError> {
        let row = sqlx::query(
            "SELECT id, session_id, role, content, model_id, input_tokens, output_tokens, cost_usd, created_at
             FROM claudecode_messages WHERE id = ?",
        )
        .bind(message_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch message: {}", e))?
        .ok_or_else(|| format!("Message not found: {}", message_id))?;

        self.row_to_message(row)
    }

    /// Resolve a message handle identifier to a Message
    /// Handle format: "msg-{message_id}:{role}:{name}"
    pub async fn resolve_message_handle(&self, identifier: &str) -> Result<Message, ClaudeCodeError> {
        let parts: Vec<&str> = identifier.splitn(3, ':').collect();
        if parts.len() < 2 {
            return Err(format!("Invalid message handle format: {}", identifier).into());
        }

        let msg_part = parts[0];
        if !msg_part.starts_with("msg-") {
            return Err(format!("Invalid message handle format: {}", identifier).into());
        }

        let message_id_str = &msg_part[4..];
        let message_id = Uuid::parse_str(message_id_str)
            .map_err(|e| format!("Invalid message ID in handle: {}", e))?;

        self.message_get(&message_id).await
    }

    /// Create a handle for a message
    ///
    /// Format: `{plugin_id}@1.0.0::chat:msg-{id}:{role}:{name}`
    /// meta[0] = message ID (with msg- prefix for resolve_message_handle compatibility)
    /// meta[1] = role
    /// meta[2] = name
    pub fn message_to_handle(message: &Message, name: &str) -> Handle {
        Handle::new(ClaudeCode::PLUGIN_ID, "1.0.0", "chat")
            .with_meta(vec![
                format!("msg-{}", message.id),
                message.role.as_str().to_string(),
                name.to_string(),
            ])
    }

    // ========================================================================
    // Unknown Event Operations
    // ========================================================================

    /// Store an unknown event and return its ID (handle)
    pub async fn unknown_event_store(
        &self,
        session_id: Option<&ClaudeCodeId>,
        event_type: &str,
        data: &Value,
    ) -> Result<String, ClaudeCodeError> {
        let id = Uuid::new_v4().to_string();
        let now = current_timestamp();
        let data_json = serde_json::to_string(data)
            .map_err(|e| format!("Failed to serialize unknown event data: {}", e))?;

        sqlx::query(
            "INSERT INTO claudecode_unknown_events (id, session_id, event_type, data, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(session_id.map(|s| s.to_string()))
        .bind(event_type)
        .bind(&data_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to store unknown event: {}", e))?;

        Ok(id)
    }

    /// Retrieve an unknown event by ID
    pub async fn unknown_event_get(&self, id: &str) -> Result<(String, Value), ClaudeCodeError> {
        let row = sqlx::query(
            "SELECT event_type, data FROM claudecode_unknown_events WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch unknown event: {}", e))?
        .ok_or_else(|| format!("Unknown event not found: {}", id))?;

        let event_type: String = row.get("event_type");
        let data_json: String = row.get("data");
        let data: Value = serde_json::from_str(&data_json)
            .map_err(|e| format!("Failed to parse unknown event data: {}", e))?;

        Ok((event_type, data))
    }

    /// List unknown events by type (for analysis/debugging)
    pub async fn unknown_events_by_type(&self, event_type: &str) -> Result<Vec<(String, Value)>, ClaudeCodeError> {
        let rows = sqlx::query(
            "SELECT id, data FROM claudecode_unknown_events WHERE event_type = ? ORDER BY created_at DESC",
        )
        .bind(event_type)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list unknown events: {}", e))?;

        rows.iter()
            .map(|row| {
                let id: String = row.get("id");
                let data_json: String = row.get("data");
                let data: Value = serde_json::from_str(&data_json)
                    .map_err(|e| format!("Failed to parse unknown event data: {}", e))?;
                Ok((id, data))
            })
            .collect()
    }

    // ========================================================================
    // Helper methods
    // ========================================================================

    fn row_to_message(&self, row: sqlx::sqlite::SqliteRow) -> Result<Message, ClaudeCodeError> {
        let id_str: String = row.get("id");
        let session_id_str: String = row.get("session_id");
        let role_str: String = row.get("role");

        Ok(Message {
            id: Uuid::parse_str(&id_str).map_err(|e| format!("Invalid message ID: {}", e))?,
            session_id: Uuid::parse_str(&session_id_str)
                .map_err(|e| format!("Invalid session ID: {}", e))?,
            role: MessageRole::from_str(&role_str)
                .ok_or_else(|| format!("Invalid role: {}", role_str))?,
            content: row.get("content"),
            created_at: row.get("created_at"),
            model_id: row.get("model_id"),
            input_tokens: row.get("input_tokens"),
            output_tokens: row.get("output_tokens"),
            cost_usd: row.get("cost_usd"),
        })
    }

    fn row_to_config(&self, row: sqlx::sqlite::SqliteRow) -> Result<ClaudeCodeConfig, ClaudeCodeError> {
        let id_str: String = row.get("id");
        let tree_id_str: String = row.get("tree_id");
        let head_str: String = row.get("canonical_head");
        let model_str: String = row.get("model");
        let metadata_json: Option<String> = row.get("metadata");
        let mcp_config_json: Option<String> = row.get("mcp_config");
        let loopback: i32 = row.get("loopback_enabled");

        let tree_id = TreeId::parse_str(&tree_id_str)
            .map_err(|e| format!("Invalid tree ID: {}", e))?;
        let node_id = NodeId::parse_str(&head_str)
            .map_err(|e| format!("Invalid node ID: {}", e))?;
        let model = Model::from_str(&model_str)
            .ok_or_else(|| format!("Invalid model: {}", model_str))?;

        Ok(ClaudeCodeConfig {
            id: Uuid::parse_str(&id_str).map_err(|e| format!("Invalid session ID: {}", e))?,
            name: row.get("name"),
            claude_session_id: row.get("claude_session_id"),
            head: Position::new(tree_id, node_id),
            working_dir: row.get("working_dir"),
            model,
            system_prompt: row.get("system_prompt"),
            mcp_config: mcp_config_json.and_then(|s| serde_json::from_str(&s).ok()),
            loopback_enabled: loopback != 0,
            metadata: metadata_json.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }
}

/// Get current Unix timestamp in seconds
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

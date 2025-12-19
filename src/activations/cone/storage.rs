use super::methods::ConeIdentifier;
use super::types::{ConeConfig, ConeError, ConeId, ConeInfo, Message, MessageId, MessageRole, Position};
use crate::activations::arbor::{Handle, ArborStorage, NodeId, TreeId};
use serde_json::Value;
use sqlx::{sqlite::SqlitePool, Row};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Configuration for Cone storage
#[derive(Debug, Clone)]
pub struct ConeStorageConfig {
    /// Path to SQLite database for cone configs
    pub db_path: PathBuf,
}

impl Default for ConeStorageConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("cones.db"),
        }
    }
}

/// Storage layer for cone configurations
pub struct ConeStorage {
    pool: SqlitePool,
    arbor: Arc<ArborStorage>,
}

impl ConeStorage {
    /// Create a new cone storage instance with a shared Arbor storage
    pub async fn new(config: ConeStorageConfig, arbor: Arc<ArborStorage>) -> Result<Self, ConeError> {
        // Initialize cone database
        let db_url = format!("sqlite:{}?mode=rwc", config.db_path.display());
        let pool = SqlitePool::connect(&db_url)
            .await
            .map_err(|e| format!("Failed to connect to cone database: {}", e))?;

        let storage = Self { pool, arbor };
        storage.run_migrations().await?;

        Ok(storage)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<(), ConeError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cones (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                model_id TEXT NOT NULL,
                system_prompt TEXT,
                tree_id TEXT NOT NULL,
                canonical_head TEXT NOT NULL,
                metadata TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                cone_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                model_id TEXT,
                input_tokens INTEGER,
                output_tokens INTEGER,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (cone_id) REFERENCES cones(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_cones_name ON cones(name);
            CREATE INDEX IF NOT EXISTS idx_cones_tree ON cones(tree_id);
            CREATE INDEX IF NOT EXISTS idx_messages_cone ON messages(cone_id);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to run cone migrations: {}", e))?;

        Ok(())
    }

    /// Get access to the underlying arbor storage
    pub fn arbor(&self) -> &ArborStorage {
        &self.arbor
    }

    // ========================================================================
    // Cone CRUD Operations
    // ========================================================================

    /// Create a new cone with a new conversation tree
    ///
    /// If a cone with the given name already exists, automatically appends `#<uuid>`
    /// to make it unique. For example, "assistant" becomes "assistant#550e8400..."
    pub async fn cone_create(
        &self,
        name: String,
        model_id: String,
        system_prompt: Option<String>,
        metadata: Option<Value>,
    ) -> Result<ConeConfig, ConeError> {
        let cone_id = ConeId::new_v4();
        let now = current_timestamp();

        // Create a new tree for this cone
        let tree_id = self.arbor.tree_create(metadata.clone(), &cone_id.to_string()).await
            .map_err(|e| format!("Failed to create tree for cone: {}", e))?;

        // Get the root node as initial position
        let tree = self.arbor.tree_get(&tree_id).await
            .map_err(|e| format!("Failed to get tree: {}", e))?;
        let head = Position::new(tree_id, tree.root);

        let metadata_json = metadata.as_ref().map(|m| serde_json::to_string(m).unwrap());

        // Try inserting with the original name first
        let final_name = match sqlx::query(
            "INSERT INTO cones (id, name, model_id, system_prompt, tree_id, canonical_head, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(cone_id.to_string())
        .bind(&name)
        .bind(&model_id)
        .bind(&system_prompt)
        .bind(head.tree_id.to_string())
        .bind(head.node_id.to_string())
        .bind(metadata_json.clone())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await {
            Ok(_) => name,  // Success with original name
            Err(e) if e.to_string().contains("UNIQUE constraint failed") => {
                // Name collision - append #uuid to make it unique
                let unique_name = format!("{}#{}", name, cone_id);

                sqlx::query(
                    "INSERT INTO cones (id, name, model_id, system_prompt, tree_id, canonical_head, metadata, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(cone_id.to_string())
                .bind(&unique_name)
                .bind(&model_id)
                .bind(&system_prompt)
                .bind(head.tree_id.to_string())
                .bind(head.node_id.to_string())
                .bind(metadata_json)
                .bind(now)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(|e| format!("Failed to create cone with unique name: {}", e))?;

                unique_name
            }
            Err(e) => return Err(ConeError::from(format!("Failed to create cone: {}", e))),
        };

        Ok(ConeConfig {
            id: cone_id,
            name: final_name,
            model_id,
            system_prompt,
            head,
            metadata,
            created_at: now,
            updated_at: now,
        })
    }

    /// Resolve a cone identifier to a ConeId
    ///
    /// For name lookups, supports partial matching on the name portion before '#':
    /// - "assistant" matches "assistant" or "assistant#550e8400-..."
    /// - "assistant#550e" matches "assistant#550e8400-..."
    ///
    /// Fails if the pattern matches multiple cones (ambiguous).
    pub async fn resolve_cone_identifier(&self, identifier: &ConeIdentifier) -> Result<ConeId, ConeError> {
        match identifier {
            ConeIdentifier::ById { id } => Ok(*id),
            ConeIdentifier::ByName { name } => {
                // Try exact match first
                if let Some(row) = sqlx::query("SELECT id FROM cones WHERE name = ?")
                    .bind(name)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| ConeError::from(format!("Failed to resolve cone by name: {}", e)))?
                {
                    let id_str: String = row.get("id");
                    return Uuid::parse_str(&id_str)
                        .map_err(|e| ConeError::from(format!("Invalid cone ID in database: {}", e)));
                }

                // Try partial match with LIKE pattern
                // Pattern: "name%" matches "name" or "name#uuid"
                let pattern = format!("{}%", name);
                let rows = sqlx::query("SELECT id, name FROM cones WHERE name LIKE ?")
                    .bind(&pattern)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| ConeError::from(format!("Failed to resolve cone by pattern: {}", e)))?;

                match rows.len() {
                    0 => Err(ConeError::from(format!("Cone not found with name: {}", name))),
                    1 => {
                        let id_str: String = rows[0].get("id");
                        Uuid::parse_str(&id_str)
                            .map_err(|e| ConeError::from(format!("Invalid cone ID in database: {}", e)))
                    }
                    _ => {
                        // Multiple matches - list them for user
                        let matches: Vec<String> = rows.iter().map(|r| r.get("name")).collect();
                        Err(ConeError::from(format!(
                            "Ambiguous name '{}' matches multiple cones: {}. Use full name with #uuid to disambiguate.",
                            name,
                            matches.join(", ")
                        )))
                    }
                }
            }
        }
    }

    /// Get a cone by ID
    pub async fn cone_get(&self, cone_id: &ConeId) -> Result<ConeConfig, ConeError> {
        let row = sqlx::query(
            "SELECT id, name, model_id, system_prompt, tree_id, canonical_head, metadata, created_at, updated_at
             FROM cones WHERE id = ?",
        )
        .bind(cone_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch cone: {}", e))?
        .ok_or_else(|| format!("Cone not found: {}", cone_id))?;

        self.row_to_cone_config(row)
    }

    /// Get a cone by identifier (name or UUID)
    pub async fn cone_get_by_identifier(&self, identifier: &ConeIdentifier) -> Result<ConeConfig, ConeError> {
        let cone_id = self.resolve_cone_identifier(identifier).await?;
        self.cone_get(&cone_id).await
    }

    /// List all cones
    pub async fn cone_list(&self) -> Result<Vec<ConeInfo>, ConeError> {
        let rows = sqlx::query(
            "SELECT id, name, model_id, tree_id, canonical_head, created_at FROM cones ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list cones: {}", e))?;

        let cones: Result<Vec<ConeInfo>, ConeError> = rows
            .iter()
            .map(|row| {
                let id_str: String = row.get("id");
                let tree_id_str: String = row.get("tree_id");
                let head_str: String = row.get("canonical_head");

                let tree_id = TreeId::parse_str(&tree_id_str).map_err(|e| format!("Invalid tree ID: {}", e))?;
                let node_id = NodeId::parse_str(&head_str).map_err(|e| format!("Invalid node ID: {}", e))?;

                Ok(ConeInfo {
                    id: Uuid::parse_str(&id_str).map_err(|e| format!("Invalid cone ID: {}", e))?,
                    name: row.get("name"),
                    model_id: row.get("model_id"),
                    head: Position::new(tree_id, node_id),
                    created_at: row.get("created_at"),
                })
            })
            .collect();

        cones
    }

    /// Update cone's canonical head
    pub async fn cone_update_head(
        &self,
        cone_id: &ConeId,
        new_head: NodeId,
    ) -> Result<(), ConeError> {
        let now = current_timestamp();

        let result = sqlx::query(
            "UPDATE cones SET canonical_head = ?, updated_at = ? WHERE id = ?",
        )
        .bind(new_head.to_string())
        .bind(now)
        .bind(cone_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to update cone head: {}", e))?;

        if result.rows_affected() == 0 {
            return Err(format!("Cone not found: {}", cone_id).into());
        }

        Ok(())
    }

    /// Update cone configuration
    pub async fn cone_update(
        &self,
        cone_id: &ConeId,
        name: Option<String>,
        model_id: Option<String>,
        system_prompt: Option<Option<String>>,
        metadata: Option<Value>,
    ) -> Result<(), ConeError> {
        let now = current_timestamp();

        // Get current cone
        let current = self.cone_get(cone_id).await?;

        let new_name = name.unwrap_or(current.name);
        let new_model = model_id.unwrap_or(current.model_id);
        let new_prompt = system_prompt.unwrap_or(current.system_prompt);
        let new_metadata = metadata.or(current.metadata);
        let metadata_json = new_metadata.as_ref().map(|m| serde_json::to_string(m).unwrap());

        sqlx::query(
            "UPDATE cones SET name = ?, model_id = ?, system_prompt = ?, metadata = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&new_name)
        .bind(&new_model)
        .bind(&new_prompt)
        .bind(metadata_json)
        .bind(now)
        .bind(cone_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to update cone: {}", e))?;

        Ok(())
    }

    /// Delete an cone (does not delete the tree)
    pub async fn cone_delete(&self, cone_id: &ConeId) -> Result<(), ConeError> {
        let result = sqlx::query("DELETE FROM cones WHERE id = ?")
            .bind(cone_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to delete cone: {}", e))?;

        if result.rows_affected() == 0 {
            return Err(format!("Cone not found: {}", cone_id).into());
        }

        Ok(())
    }

    // ========================================================================
    // Message Operations
    // ========================================================================

    /// Create a message and return its ID
    pub async fn message_create(
        &self,
        cone_id: &ConeId,
        role: MessageRole,
        content: String,
        model_id: Option<String>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
    ) -> Result<Message, ConeError> {
        let message_id = MessageId::new_v4();
        let now = current_timestamp();

        sqlx::query(
            "INSERT INTO messages (id, cone_id, role, content, model_id, input_tokens, output_tokens, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(message_id.to_string())
        .bind(cone_id.to_string())
        .bind(role.as_str())
        .bind(&content)
        .bind(&model_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create message: {}", e))?;

        Ok(Message {
            id: message_id,
            cone_id: *cone_id,
            role,
            content,
            created_at: now,
            model_id,
            input_tokens,
            output_tokens,
        })
    }

    /// Create an ephemeral message (marked for deletion) and return it
    pub async fn message_create_ephemeral(
        &self,
        cone_id: &ConeId,
        role: MessageRole,
        content: String,
        model_id: Option<String>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
    ) -> Result<Message, ConeError> {
        let message_id = MessageId::new_v4();
        let now = current_timestamp();

        // Use negative timestamp as ephemeral marker for cleanup
        let ephemeral_marker = -now;

        sqlx::query(
            "INSERT INTO messages (id, cone_id, role, content, model_id, input_tokens, output_tokens, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(message_id.to_string())
        .bind(cone_id.to_string())
        .bind(role.as_str())
        .bind(&content)
        .bind(&model_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(ephemeral_marker)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create ephemeral message: {}", e))?;

        Ok(Message {
            id: message_id,
            cone_id: *cone_id,
            role,
            content,
            created_at: ephemeral_marker,
            model_id,
            input_tokens,
            output_tokens,
        })
    }

    /// Get a message by ID
    pub async fn message_get(&self, message_id: &MessageId) -> Result<Message, ConeError> {
        let row = sqlx::query(
            "SELECT id, cone_id, role, content, model_id, input_tokens, output_tokens, created_at
             FROM messages WHERE id = ?",
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
    pub async fn resolve_message_handle(&self, identifier: &str) -> Result<Message, ConeError> {
        // Parse identifier: "msg-{uuid}:{role}:{name}"
        let parts: Vec<&str> = identifier.splitn(3, ':').collect();
        if parts.len() < 2 {
            return Err(format!("Invalid message handle format: {}", identifier).into());
        }

        let msg_part = parts[0];
        if !msg_part.starts_with("msg-") {
            return Err(format!("Invalid message handle format: {}", identifier).into());
        }

        let message_id_str = &msg_part[4..]; // Strip "msg-" prefix
        let message_id = Uuid::parse_str(message_id_str)
            .map_err(|e| format!("Invalid message ID in handle: {}", e))?;

        self.message_get(&message_id).await
    }

    /// Create a handle for a message
    /// Format: "msg-{id}:{role}:{name}"
    pub fn message_to_handle(message: &Message, name: &str) -> Handle {
        Handle {
            source: "cone".to_string(),
            source_version: "1.0.0".to_string(),
            identifier: format!("msg-{}:{}:{}", message.id, message.role.as_str(), name),
            metadata: None,
        }
    }

    // ========================================================================
    // Helper methods
    // ========================================================================

    fn row_to_message(&self, row: sqlx::sqlite::SqliteRow) -> Result<Message, ConeError> {
        let id_str: String = row.get("id");
        let cone_id_str: String = row.get("cone_id");
        let role_str: String = row.get("role");

        Ok(Message {
            id: Uuid::parse_str(&id_str).map_err(|e| format!("Invalid message ID: {}", e))?,
            cone_id: Uuid::parse_str(&cone_id_str).map_err(|e| format!("Invalid cone ID: {}", e))?,
            role: MessageRole::from_str(&role_str).ok_or_else(|| format!("Invalid role: {}", role_str))?,
            content: row.get("content"),
            created_at: row.get("created_at"),
            model_id: row.get("model_id"),
            input_tokens: row.get("input_tokens"),
            output_tokens: row.get("output_tokens"),
        })
    }

    fn row_to_cone_config(&self, row: sqlx::sqlite::SqliteRow) -> Result<ConeConfig, ConeError> {
        let id_str: String = row.get("id");
        let tree_id_str: String = row.get("tree_id");
        let head_str: String = row.get("canonical_head");
        let metadata_json: Option<String> = row.get("metadata");

        let tree_id = TreeId::parse_str(&tree_id_str).map_err(|e| format!("Invalid tree ID: {}", e))?;
        let node_id = NodeId::parse_str(&head_str).map_err(|e| format!("Invalid node ID: {}", e))?;

        Ok(ConeConfig {
            id: Uuid::parse_str(&id_str).map_err(|e| format!("Invalid cone ID: {}", e))?,
            name: row.get("name"),
            model_id: row.get("model_id"),
            system_prompt: row.get("system_prompt"),
            head: Position::new(tree_id, node_id),
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

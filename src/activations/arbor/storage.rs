use super::types::{
    ArborError, ArborId, Node, NodeId, NodeType, ResourceRefs, ResourceState, Tree, TreeId, Handle,
};
use serde_json::Value;
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePool}, ConnectOptions, Row};
use uuid::Uuid;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for Arbor storage
#[derive(Debug, Clone)]
pub struct ArborConfig {
    /// Duration before scheduled resources move to archived (seconds)
    pub scheduled_deletion_window: i64, // Default: 7 days = 604800

    /// Duration before archived resources are purged (seconds)
    pub archive_window: i64, // Default: 30 days = 2592000

    /// Path to SQLite database
    pub db_path: PathBuf,

    /// Enable auto-cleanup background task
    pub auto_cleanup: bool,

    /// Cleanup task interval (seconds)
    pub cleanup_interval: i64, // Default: 1 hour = 3600
}

impl Default for ArborConfig {
    fn default() -> Self {
        Self {
            scheduled_deletion_window: 604800,  // 7 days
            archive_window: 2592000,            // 30 days
            db_path: PathBuf::from("arbor.db"),
            auto_cleanup: true,
            cleanup_interval: 3600,             // 1 hour
        }
    }
}

/// SQLite-based storage for Arbor tree structures.
///
/// # Usage Pattern: Direct Injection
///
/// ArborStorage is **infrastructure** - activations should receive it directly
/// at construction time, NOT via Plexus routing.
///
/// ```ignore
/// // Correct: Direct injection
/// let cone = Cone::new(cone_config, arbor_storage.clone()).await?;
///
/// // Then use directly for tree operations
/// let tree = arbor_storage.tree_get(&tree_id).await?;
/// let node_id = arbor_storage.node_create_external(&tree_id, parent, handle, None).await?;
/// ```
///
/// **Do NOT** route tree operations through Plexus - that adds unnecessary
/// serialization overhead for what should be direct method calls.
///
/// The **only** case where Plexus is needed for Arbor-related data is
/// cross-plugin handle resolution: when you have a Handle pointing to
/// external data (e.g., a Cone message) and need to resolve its content.
///
/// See: `docs/architecture/*_arbor-usage-pattern.md`
pub struct ArborStorage {
    pool: SqlitePool,
    #[allow(dead_code)]
    config: ArborConfig,
}

impl ArborStorage {
    /// Create a new storage instance and run migrations
    pub async fn new(config: ArborConfig) -> Result<Self, ArborError> {
        let db_url = format!("sqlite:{}?mode=rwc", config.db_path.display());
        let mut connect_options: SqliteConnectOptions = db_url.parse()
            .map_err(|e| format!("Failed to parse database URL: {}", e))?;
        connect_options.disable_statement_logging();
        let pool = SqlitePool::connect_with(connect_options.clone())
            .await
            .map_err(|e| format!("Failed to connect to database: {}", e))?;

        let storage = Self { pool, config };
        storage.run_migrations().await?;

        Ok(storage)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<(), ArborError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS trees (
                id TEXT PRIMARY KEY,
                root_node_id TEXT NOT NULL,
                ref_count INTEGER NOT NULL DEFAULT 1,
                state TEXT NOT NULL DEFAULT 'active',
                scheduled_deletion_at INTEGER,
                archived_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                metadata TEXT
            );

            CREATE TABLE IF NOT EXISTS tree_refs (
                tree_id TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                count INTEGER NOT NULL DEFAULT 1,
                claimed_at INTEGER NOT NULL,
                PRIMARY KEY (tree_id, owner_id),
                FOREIGN KEY (tree_id) REFERENCES trees(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                tree_id TEXT NOT NULL,
                parent_id TEXT,
                ref_count INTEGER NOT NULL DEFAULT 1,
                state TEXT NOT NULL DEFAULT 'active',
                scheduled_deletion_at INTEGER,
                archived_at INTEGER,
                node_type TEXT NOT NULL,
                content TEXT,
                handle_plugin_id TEXT,
                handle_version TEXT,
                handle_method TEXT,
                handle_meta TEXT,
                created_at INTEGER NOT NULL,
                metadata TEXT,
                FOREIGN KEY (tree_id) REFERENCES trees(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS node_refs (
                node_id TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                count INTEGER NOT NULL DEFAULT 1,
                claimed_at INTEGER NOT NULL,
                PRIMARY KEY (node_id, owner_id),
                FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS node_children (
                parent_id TEXT NOT NULL,
                child_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY (parent_id, child_id),
                FOREIGN KEY (parent_id) REFERENCES nodes(id) ON DELETE CASCADE,
                FOREIGN KEY (child_id) REFERENCES nodes(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_trees_state ON trees(state);
            CREATE INDEX IF NOT EXISTS idx_trees_scheduled ON trees(scheduled_deletion_at) WHERE state = 'scheduled_delete';
            CREATE INDEX IF NOT EXISTS idx_trees_archived ON trees(archived_at) WHERE state = 'archived';
            CREATE INDEX IF NOT EXISTS idx_nodes_tree ON nodes(tree_id);
            CREATE INDEX IF NOT EXISTS idx_nodes_parent ON nodes(parent_id);
            CREATE INDEX IF NOT EXISTS idx_nodes_state ON nodes(state);
            CREATE INDEX IF NOT EXISTS idx_nodes_scheduled ON nodes(scheduled_deletion_at) WHERE state = 'scheduled_delete';
            CREATE INDEX IF NOT EXISTS idx_node_children_parent ON node_children(parent_id);
            CREATE INDEX IF NOT EXISTS idx_node_children_child ON node_children(child_id);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to run migrations: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // Tree Operations
    // ========================================================================

    /// Create a new tree with a root node
    pub async fn tree_create(
        &self,
        metadata: Option<serde_json::Value>,
        owner_id: &str,
    ) -> Result<TreeId, ArborError> {
        let tree_id = TreeId::new();
        let root_id = NodeId::new();
        let now = current_timestamp();

        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // Create tree
        let metadata_json = metadata.map(|m| serde_json::to_string(&m).unwrap());
        sqlx::query(
            "INSERT INTO trees (id, root_node_id, ref_count, state, created_at, updated_at, metadata)
             VALUES (?, ?, 1, 'active', ?, ?, ?)",
        )
        .bind(tree_id.to_string())
        .bind(root_id.to_string())
        .bind(now)
        .bind(now)
        .bind(metadata_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to create tree: {}", e))?;

        // Create tree ref for owner
        sqlx::query(
            "INSERT INTO tree_refs (tree_id, owner_id, count, claimed_at) VALUES (?, ?, 1, ?)",
        )
        .bind(tree_id.to_string())
        .bind(owner_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to create tree ref: {}", e))?;

        // Create root node (empty text node)
        sqlx::query(
            "INSERT INTO nodes (id, tree_id, parent_id, ref_count, state, node_type, content, created_at)
             VALUES (?, ?, NULL, 1, 'active', 'text', '', ?)",
        )
        .bind(root_id.to_string())
        .bind(tree_id.to_string())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to create root node: {}", e))?;

        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(tree_id)
    }

    /// Get a tree by ID (only active trees)
    pub async fn tree_get(&self, tree_id: &TreeId) -> Result<Tree, ArborError> {
        self.tree_get_internal(tree_id, false).await
    }

    /// Get an archived tree by ID
    pub async fn tree_get_archived(&self, tree_id: &TreeId) -> Result<Tree, ArborError> {
        self.tree_get_internal(tree_id, true).await
    }

    /// Internal tree getter
    async fn tree_get_internal(
        &self,
        tree_id: &TreeId,
        allow_archived: bool,
    ) -> Result<Tree, ArborError> {
        // Get tree metadata
        let tree_row = sqlx::query(
            "SELECT id, root_node_id, ref_count, state, scheduled_deletion_at, archived_at, created_at, updated_at, metadata
             FROM trees WHERE id = ?",
        )
        .bind(tree_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch tree: {}", e))?
        .ok_or_else(|| format!("Tree not found: {}", tree_id))?;

        let state_str: String = tree_row.get("state");
        let state = ResourceState::from_str(&state_str).unwrap_or(ResourceState::Active);

        if !allow_archived && state == ResourceState::Archived {
            return Err("Tree is archived, use tree_get_archived()".into());
        }

        let root_node_id: String = tree_row.get("root_node_id");
        let root_node_id = ArborId::parse_str(&root_node_id)
            .map_err(|e| format!("Invalid root node ID: {}", e))?;

        // Get all nodes for this tree
        let nodes = self.get_nodes_for_tree(tree_id).await?;

        // Get reference information
        let refs = self.get_tree_refs(tree_id).await?;

        let metadata_json: Option<String> = tree_row.get("metadata");
        let metadata = metadata_json.and_then(|s| serde_json::from_str(&s).ok());

        Ok(Tree {
            id: *tree_id,
            root: root_node_id,
            nodes,
            state: Some(state),
            refs: Some(refs),
            scheduled_deletion_at: tree_row.get("scheduled_deletion_at"),
            archived_at: tree_row.get("archived_at"),
            created_at: tree_row.get("created_at"),
            updated_at: tree_row.get("updated_at"),
            metadata,
        })
    }

    /// Get all nodes for a tree
    async fn get_nodes_for_tree(&self, tree_id: &TreeId) -> Result<HashMap<NodeId, Node>, ArborError> {
        let rows = sqlx::query(
            "SELECT id, tree_id, parent_id, ref_count, state, scheduled_deletion_at, archived_at,
                    node_type, content, handle_plugin_id, handle_version, handle_method,
                    handle_meta, created_at, metadata
             FROM nodes WHERE tree_id = ?",
        )
        .bind(tree_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch nodes: {}", e))?;

        let mut nodes = HashMap::new();

        for row in rows {
            let node_id_str: String = row.get("id");
            let node_id = ArborId::parse_str(&node_id_str)
                .map_err(|e| format!("Invalid node ID: {}", e))?;

            let parent_id_str: Option<String> = row.get("parent_id");
            let parent_id = parent_id_str
                .map(|s| ArborId::parse_str(&s).map_err(|e| format!("Invalid parent ID: {}", e)))
                .transpose()?;

            // Get children for this node
            let children = self.get_node_children_internal(&node_id).await?;

            let node_type_str: String = row.get("node_type");
            let data = match node_type_str.as_str() {
                "text" => {
                    let content: String = row.get("content");
                    NodeType::Text { content }
                }
                "external" => {
                    let plugin_id_str: String = row.get("handle_plugin_id");
                    let plugin_id = Uuid::parse_str(&plugin_id_str)
                        .map_err(|e| format!("Invalid handle plugin_id: {}", e))?;
                    let version: String = row.get("handle_version");
                    let method: String = row.get("handle_method");
                    let meta_json: Option<String> = row.get("handle_meta");
                    let meta: Vec<String> = meta_json
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default();

                    NodeType::External {
                        handle: Handle::new(plugin_id, version, method)
                            .with_meta(meta),
                    }
                }
                _ => return Err(format!("Unknown node type: {}", node_type_str).into()),
            };

            let state_str: String = row.get("state");
            let state = ResourceState::from_str(&state_str).unwrap_or(ResourceState::Active);

            let refs = self.get_node_refs(&node_id).await?;

            let metadata_json: Option<String> = row.get("metadata");
            let metadata = metadata_json.and_then(|s| serde_json::from_str(&s).ok());

            let node = Node {
                id: node_id,
                parent: parent_id,
                children,
                data,
                state: Some(state),
                refs: Some(refs),
                scheduled_deletion_at: row.get("scheduled_deletion_at"),
                archived_at: row.get("archived_at"),
                created_at: row.get("created_at"),
                metadata,
            };

            nodes.insert(node_id, node);
        }

        Ok(nodes)
    }

    /// Get children for a node (ordered by position)
    async fn get_node_children_internal(&self, node_id: &NodeId) -> Result<Vec<NodeId>, ArborError> {
        let rows = sqlx::query(
            "SELECT child_id FROM node_children WHERE parent_id = ? ORDER BY position",
        )
        .bind(node_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch node children: {}", e))?;

        let children: Result<Vec<NodeId>, ArborError> = rows
            .iter()
            .map(|row| {
                let child_id_str: String = row.get("child_id");
                ArborId::parse_str(&child_id_str)
                    .map_err(|e| e.into())
            })
            .collect();

        children
    }

    /// Add a child to a parent in the node_children table
    async fn add_child_to_parent(&self, parent_id: &NodeId, child_id: &NodeId) -> Result<(), ArborError> {
        // Get next position for this parent
        let row = sqlx::query(
            "SELECT COALESCE(MAX(position), -1) + 1 as next_pos FROM node_children WHERE parent_id = ?",
        )
        .bind(parent_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to get next position: {}", e))?;

        let next_pos: i64 = row.get("next_pos");

        sqlx::query(
            "INSERT INTO node_children (parent_id, child_id, position) VALUES (?, ?, ?)",
        )
        .bind(parent_id.to_string())
        .bind(child_id.to_string())
        .bind(next_pos)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to add child to parent: {}", e))?;

        Ok(())
    }

    /// Get reference information for a tree
    async fn get_tree_refs(&self, tree_id: &TreeId) -> Result<ResourceRefs, ArborError> {
        let rows = sqlx::query(
            "SELECT owner_id, count FROM tree_refs WHERE tree_id = ?",
        )
        .bind(tree_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch tree refs: {}", e))?;

        let mut owners = HashMap::new();
        let mut total = 0i64;

        for row in rows {
            let owner_id: String = row.get("owner_id");
            let count: i64 = row.get("count");
            owners.insert(owner_id, count);
            total += count;
        }

        Ok(ResourceRefs {
            ref_count: total,
            owners,
        })
    }

    /// Get reference information for a node
    async fn get_node_refs(&self, node_id: &NodeId) -> Result<ResourceRefs, ArborError> {
        let rows = sqlx::query(
            "SELECT owner_id, count FROM node_refs WHERE node_id = ?",
        )
        .bind(node_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch node refs: {}", e))?;

        let mut owners = HashMap::new();
        let mut total = 0i64;

        for row in rows {
            let owner_id: String = row.get("owner_id");
            let count: i64 = row.get("count");
            owners.insert(owner_id, count);
            total += count;
        }

        Ok(ResourceRefs {
            ref_count: total,
            owners,
        })
    }

    /// List all tree IDs (active only by default)
    pub async fn tree_list(&self, include_scheduled: bool) -> Result<Vec<TreeId>, ArborError> {
        let query = if include_scheduled {
            "SELECT id FROM trees WHERE state IN ('active', 'scheduled_delete')"
        } else {
            "SELECT id FROM trees WHERE state = 'active'"
        };

        let rows = sqlx::query(query)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("Failed to list trees: {}", e))?;

        let tree_ids: Result<Vec<TreeId>, ArborError> = rows
            .iter()
            .map(|row| {
                let id_str: String = row.get("id");
                ArborId::parse_str(&id_str)
                    .map_err(|e| format!("Invalid tree ID: {}", e).into())
            })
            .collect();

        tree_ids
    }

    /// List trees scheduled for deletion
    pub async fn tree_list_scheduled(&self) -> Result<Vec<TreeId>, ArborError> {
        let rows = sqlx::query(
            "SELECT id FROM trees WHERE state = 'scheduled_delete'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list scheduled trees: {}", e))?;

        let tree_ids: Result<Vec<TreeId>, ArborError> = rows
            .iter()
            .map(|row| {
                let id_str: String = row.get("id");
                ArborId::parse_str(&id_str)
                    .map_err(|e| format!("Invalid tree ID: {}", e).into())
            })
            .collect();

        tree_ids
    }

    /// List archived trees
    pub async fn tree_list_archived(&self) -> Result<Vec<TreeId>, ArborError> {
        let rows = sqlx::query(
            "SELECT id FROM trees WHERE state = 'archived'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list archived trees: {}", e))?;

        let tree_ids: Result<Vec<TreeId>, ArborError> = rows
            .iter()
            .map(|row| {
                let id_str: String = row.get("id");
                ArborId::parse_str(&id_str)
                    .map_err(|e| format!("Invalid tree ID: {}", e).into())
            })
            .collect();

        tree_ids
    }

    /// Update tree metadata
    pub async fn tree_update_metadata(
        &self,
        tree_id: &TreeId,
        metadata: Value,
    ) -> Result<(), ArborError> {
        let now = current_timestamp();
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        sqlx::query(
            "UPDATE trees SET metadata = ?, updated_at = ? WHERE id = ? AND state = 'active'",
        )
        .bind(metadata_json)
        .bind(now)
        .bind(tree_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to update tree metadata: {}", e))?;

        Ok(())
    }

    /// Claim ownership of a tree (increment reference count)
    pub async fn tree_claim(
        &self,
        tree_id: &TreeId,
        owner_id: &str,
        count: i64,
    ) -> Result<i64, ArborError> {
        let now = current_timestamp();
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // Check if tree exists and is claimable (active or scheduled_delete)
        let tree_row = sqlx::query(
            "SELECT state, scheduled_deletion_at FROM trees WHERE id = ?",
        )
        .bind(tree_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("Failed to fetch tree: {}", e))?
        .ok_or_else(|| format!("Tree not found: {}", tree_id))?;

        let state_str: String = tree_row.get("state");
        let state = ResourceState::from_str(&state_str).unwrap_or(ResourceState::Active);

        if state == ResourceState::Archived {
            return Err("Cannot claim archived tree".into());
        }

        // If scheduled for deletion, reactivate it
        if state == ResourceState::ScheduledDelete {
            sqlx::query(
                "UPDATE trees SET state = 'active', scheduled_deletion_at = NULL, updated_at = ? WHERE id = ?",
            )
            .bind(now)
            .bind(tree_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Failed to reactivate tree: {}", e))?;
        }

        // Update or insert tree_ref
        sqlx::query(
            "INSERT INTO tree_refs (tree_id, owner_id, count, claimed_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(tree_id, owner_id) DO UPDATE SET
                count = count + excluded.count,
                claimed_at = excluded.claimed_at",
        )
        .bind(tree_id.to_string())
        .bind(owner_id)
        .bind(count)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to claim tree: {}", e))?;

        // Update tree ref_count
        sqlx::query(
            "UPDATE trees SET ref_count = ref_count + ?, updated_at = ? WHERE id = ?",
        )
        .bind(count)
        .bind(now)
        .bind(tree_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update tree ref_count: {}", e))?;

        // Get the new ref_count
        let new_count_row = sqlx::query("SELECT ref_count FROM trees WHERE id = ?")
            .bind(tree_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("Failed to fetch new ref_count: {}", e))?;

        let new_count: i64 = new_count_row.get("ref_count");

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(new_count)
    }

    /// Release ownership of a tree (decrement reference count)
    pub async fn tree_release(
        &self,
        tree_id: &TreeId,
        owner_id: &str,
        count: i64,
    ) -> Result<i64, ArborError> {
        let now = current_timestamp();
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // Check current ref count for this owner
        let owner_ref = sqlx::query(
            "SELECT count FROM tree_refs WHERE tree_id = ? AND owner_id = ?",
        )
        .bind(tree_id.to_string())
        .bind(owner_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("Failed to fetch tree ref: {}", e))?
        .ok_or_else(|| format!("No reference found for owner {}", owner_id))?;

        let current_count: i64 = owner_ref.get("count");
        if current_count < count {
            return Err(format!(
                "Cannot release {} references, owner only has {}",
                count, current_count
            )
            .into());
        }

        let new_count = current_count - count;

        // Update or delete tree_ref
        if new_count == 0 {
            sqlx::query("DELETE FROM tree_refs WHERE tree_id = ? AND owner_id = ?")
                .bind(tree_id.to_string())
                .bind(owner_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("Failed to delete tree ref: {}", e))?;
        } else {
            sqlx::query(
                "UPDATE tree_refs SET count = ?, claimed_at = ? WHERE tree_id = ? AND owner_id = ?",
            )
            .bind(new_count)
            .bind(now)
            .bind(tree_id.to_string())
            .bind(owner_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Failed to update tree ref: {}", e))?;
        }

        // Update tree ref_count
        sqlx::query(
            "UPDATE trees SET ref_count = ref_count - ?, updated_at = ? WHERE id = ?",
        )
        .bind(count)
        .bind(now)
        .bind(tree_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update tree ref_count: {}", e))?;

        // Check if ref_count reached 0, schedule for deletion
        let tree_row = sqlx::query("SELECT ref_count FROM trees WHERE id = ?")
            .bind(tree_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("Failed to fetch tree: {}", e))?;

        let ref_count: i64 = tree_row.get("ref_count");
        if ref_count == 0 {
            sqlx::query(
                "UPDATE trees SET state = 'scheduled_delete', scheduled_deletion_at = ?, updated_at = ? WHERE id = ?",
            )
            .bind(now)
            .bind(now)
            .bind(tree_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Failed to schedule tree deletion: {}", e))?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(ref_count)
    }

    /// Create a text node in a tree
    pub async fn node_create_text(
        &self,
        tree_id: &TreeId,
        parent: Option<NodeId>,
        content: String,
        metadata: Option<Value>,
    ) -> Result<NodeId, ArborError> {
        let node_id = NodeId::new();
        let now = current_timestamp();

        let metadata_json = metadata.map(|m| serde_json::to_string(&m).unwrap());

        sqlx::query(
            "INSERT INTO nodes (id, tree_id, parent_id, ref_count, state, node_type, content, metadata, created_at)
             VALUES (?, ?, ?, 1, 'active', 'text', ?, ?, ?)",
        )
        .bind(node_id.to_string())
        .bind(tree_id.to_string())
        .bind(parent.map(|p| p.to_string()))
        .bind(&content)
        .bind(metadata_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create text node: {}", e))?;

        // Add to node_children table if parent is specified
        if let Some(parent_id) = parent {
            self.add_child_to_parent(&parent_id, &node_id).await?;
        }

        Ok(node_id)
    }

    /// Create an external node in a tree
    pub async fn node_create_external(
        &self,
        tree_id: &TreeId,
        parent: Option<NodeId>,
        handle: Handle,
        metadata: Option<Value>,
    ) -> Result<NodeId, ArborError> {
        let node_id = NodeId::new();
        let now = current_timestamp();

        let metadata_json = metadata.map(|m| serde_json::to_string(&m).unwrap());
        let meta_json = serde_json::to_string(&handle.meta).unwrap();

        sqlx::query(
            "INSERT INTO nodes (id, tree_id, parent_id, ref_count, state, node_type, handle_plugin_id, handle_version, handle_method, handle_meta, metadata, created_at)
             VALUES (?, ?, ?, 1, 'active', 'external', ?, ?, ?, ?, ?, ?)",
        )
        .bind(node_id.to_string())
        .bind(tree_id.to_string())
        .bind(parent.map(|p| p.to_string()))
        .bind(handle.plugin_id.to_string())
        .bind(&handle.version)
        .bind(&handle.method)
        .bind(&meta_json)
        .bind(metadata_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create external node: {}", e))?;

        // Add to node_children table if parent is specified
        if let Some(parent_id) = parent {
            self.add_child_to_parent(&parent_id, &node_id).await?;
        }

        Ok(node_id)
    }

    /// Create an external node that is already scheduled for deletion (ephemeral)
    pub async fn node_create_external_ephemeral(
        &self,
        tree_id: &TreeId,
        parent: Option<NodeId>,
        handle: Handle,
        metadata: Option<Value>,
    ) -> Result<NodeId, ArborError> {
        let node_id = NodeId::new();
        let now = current_timestamp();

        let metadata_json = metadata.map(|m| serde_json::to_string(&m).unwrap());
        let meta_json = serde_json::to_string(&handle.meta).unwrap();

        sqlx::query(
            "INSERT INTO nodes (id, tree_id, parent_id, ref_count, state, scheduled_deletion_at, node_type, handle_plugin_id, handle_version, handle_method, handle_meta, metadata, created_at)
             VALUES (?, ?, ?, 0, 'scheduled_delete', ?, 'external', ?, ?, ?, ?, ?, ?)",
        )
        .bind(node_id.to_string())
        .bind(tree_id.to_string())
        .bind(parent.map(|p| p.to_string()))
        .bind(now) // scheduled_deletion_at = now (will be cleaned up by cleanup_scheduled_trees)
        .bind(handle.plugin_id.to_string())
        .bind(&handle.version)
        .bind(&handle.method)
        .bind(&meta_json)
        .bind(metadata_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create ephemeral external node: {}", e))?;

        // Add to node_children table if parent is specified
        if let Some(parent_id) = parent {
            self.add_child_to_parent(&parent_id, &node_id).await?;
        }

        Ok(node_id)
    }

    /// Get a node by ID
    pub async fn node_get(
        &self,
        tree_id: &TreeId,
        node_id: &NodeId,
    ) -> Result<Node, ArborError> {
        let nodes = self.get_nodes_for_tree(tree_id).await?;
        nodes
            .get(node_id)
            .cloned()
            .ok_or_else(|| format!("Node not found: {}", node_id).into())
    }

    /// Get children of a node
    pub async fn node_get_children(
        &self,
        tree_id: &TreeId,
        node_id: &NodeId,
    ) -> Result<Vec<NodeId>, ArborError> {
        let rows = sqlx::query(
            "SELECT id FROM nodes WHERE tree_id = ? AND parent_id = ? AND state = 'active'",
        )
        .bind(tree_id.to_string())
        .bind(node_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch children: {}", e))?;

        let children: Result<Vec<NodeId>, ArborError> = rows
            .iter()
            .map(|row| {
                let id_str: String = row.get("id");
                ArborId::parse_str(&id_str)
                    .map_err(|e| format!("Invalid node ID: {}", e).into())
            })
            .collect();

        children
    }

    /// Get parent of a node
    pub async fn node_get_parent(
        &self,
        tree_id: &TreeId,
        node_id: &NodeId,
    ) -> Result<Option<NodeId>, ArborError> {
        let row = sqlx::query(
            "SELECT parent_id FROM nodes WHERE tree_id = ? AND id = ?",
        )
        .bind(tree_id.to_string())
        .bind(node_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch parent: {}", e))?
        .ok_or_else(|| format!("Node not found: {}", node_id))?;

        let parent_id: Option<String> = row.get("parent_id");
        match parent_id {
            Some(id_str) => Ok(Some(
                ArborId::parse_str(&id_str)
                    .map_err(|e| format!("Invalid parent ID: {}", e))?,
            )),
            None => Ok(None),
        }
    }

    /// Get path from root to a node (list of node IDs)
    pub async fn node_get_path(
        &self,
        tree_id: &TreeId,
        node_id: &NodeId,
    ) -> Result<Vec<NodeId>, ArborError> {
        let mut path = Vec::new();
        let mut current_id = Some(*node_id);

        // Walk up the tree to root
        while let Some(id) = current_id {
            path.push(id);
            current_id = self.node_get_parent(tree_id, &id).await?;
        }

        // Reverse to get root-to-node path
        path.reverse();
        Ok(path)
    }

    /// List all leaf nodes in a tree
    pub async fn context_list_leaves(
        &self,
        tree_id: &TreeId,
    ) -> Result<Vec<NodeId>, ArborError> {
        let rows = sqlx::query(
            "SELECT id FROM nodes
             WHERE tree_id = ? AND state = 'active'
             AND id NOT IN (SELECT DISTINCT parent_id FROM nodes WHERE parent_id IS NOT NULL AND tree_id = ?)",
        )
        .bind(tree_id.to_string())
        .bind(tree_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch leaf nodes: {}", e))?;

        let leaves: Result<Vec<NodeId>, ArborError> = rows
            .iter()
            .map(|row| {
                let id_str: String = row.get("id");
                ArborId::parse_str(&id_str)
                    .map_err(|e| format!("Invalid node ID: {}", e).into())
            })
            .collect();

        leaves
    }

    /// Get the full path data from root to a node (all node data)
    pub async fn context_get_path(
        &self,
        tree_id: &TreeId,
        node_id: &NodeId,
    ) -> Result<Vec<Node>, ArborError> {
        let path_ids = self.node_get_path(tree_id, node_id).await?;
        let nodes = self.get_nodes_for_tree(tree_id).await?;

        let path_nodes: Result<Vec<Node>, ArborError> = path_ids
            .iter()
            .map(|id| {
                nodes
                    .get(id)
                    .cloned()
                    .ok_or_else(|| format!("Node not found in path: {}", id).into())
            })
            .collect();

        path_nodes
    }

    /// Get all external handles in the path to a node
    pub async fn context_get_handles(
        &self,
        tree_id: &TreeId,
        node_id: &NodeId,
    ) -> Result<Vec<Handle>, ArborError> {
        let path_nodes = self.context_get_path(tree_id, node_id).await?;

        let handles: Vec<Handle> = path_nodes
            .iter()
            .filter_map(|node| match &node.data {
                NodeType::External { handle } => Some(handle.clone()),
                NodeType::Text { .. } => None,
            })
            .collect();

        Ok(handles)
    }

    /// Cleanup task: Archive trees scheduled for deletion (after 7 days)
    pub async fn cleanup_scheduled_trees(&self) -> Result<usize, ArborError> {
        let now = current_timestamp();
        let seven_days_ago = now - (7 * 24 * 60 * 60);

        let result = sqlx::query(
            "UPDATE trees
             SET state = 'archived', archived_at = ?, updated_at = ?
             WHERE state = 'scheduled_delete' AND scheduled_deletion_at < ?",
        )
        .bind(now)
        .bind(now)
        .bind(seven_days_ago)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to archive trees: {}", e))?;

        Ok(result.rows_affected() as usize)
    }
}

/// Get current Unix timestamp in seconds
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

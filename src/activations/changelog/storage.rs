use super::types::{ChangelogEntry, QueueEntry, QueueStatus};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::{ConnectOptions, Row};
use std::path::PathBuf;

/// Configuration for changelog storage
#[derive(Debug, Clone)]
pub struct ChangelogStorageConfig {
    pub db_path: PathBuf,
}

impl Default for ChangelogStorageConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("changelog.db"),
        }
    }
}

/// Storage for changelog entries and hash tracking
pub struct ChangelogStorage {
    pool: SqlitePool,
}

impl ChangelogStorage {
    pub async fn new(config: ChangelogStorageConfig) -> Result<Self, String> {
        let mut options = SqliteConnectOptions::new()
            .filename(&config.db_path)
            .create_if_missing(true);
        options.disable_statement_logging();

        let pool = SqlitePool::connect_with(options.clone())
            .await
            .map_err(|e| format!("Failed to connect to changelog database: {}", e))?;

        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    async fn init_schema(&self) -> Result<(), String> {
        // Table for changelog entries
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS changelog_entries (
                hash TEXT PRIMARY KEY,
                previous_hash TEXT,
                created_at INTEGER NOT NULL,
                summary TEXT NOT NULL,
                details TEXT,
                author TEXT,
                queue_id TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create changelog_entries table: {}", e))?;

        // Table for tracking the last known hash
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS hash_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_hash TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create hash_state table: {}", e))?;

        // Table for queue entries (planned changes)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS queue_entries (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                tags TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                status TEXT NOT NULL,
                completed_hash TEXT,
                completed_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to create queue_entries table: {}", e))?;

        Ok(())
    }

    /// Add a new changelog entry
    pub async fn add_entry(&self, entry: &ChangelogEntry) -> Result<(), String> {
        let details_json = serde_json::to_string(&entry.details)
            .map_err(|e| format!("Failed to serialize details: {}", e))?;

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO changelog_entries
            (hash, previous_hash, created_at, summary, details, author, queue_id)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&entry.hash)
        .bind(&entry.previous_hash)
        .bind(entry.created_at)
        .bind(&entry.summary)
        .bind(&details_json)
        .bind(&entry.author)
        .bind(&entry.queue_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to add changelog entry: {}", e))?;

        Ok(())
    }

    /// Get a changelog entry by hash
    pub async fn get_entry(&self, hash: &str) -> Result<Option<ChangelogEntry>, String> {
        let row = sqlx::query(
            r#"
            SELECT hash, previous_hash, created_at, summary, details, author, queue_id
            FROM changelog_entries
            WHERE hash = ?
            "#,
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to get changelog entry: {}", e))?;

        match row {
            Some(row) => {
                let details_json: String = row.get("details");
                let details: Vec<String> = serde_json::from_str(&details_json)
                    .unwrap_or_default();

                Ok(Some(ChangelogEntry {
                    hash: row.get("hash"),
                    previous_hash: row.get("previous_hash"),
                    created_at: row.get("created_at"),
                    summary: row.get("summary"),
                    details,
                    author: row.get("author"),
                    queue_id: row.get("queue_id"),
                }))
            }
            None => Ok(None),
        }
    }

    /// List all changelog entries (newest first)
    pub async fn list_entries(&self) -> Result<Vec<ChangelogEntry>, String> {
        let rows = sqlx::query(
            r#"
            SELECT hash, previous_hash, created_at, summary, details, author, queue_id
            FROM changelog_entries
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list changelog entries: {}", e))?;

        let entries = rows
            .into_iter()
            .map(|row| {
                let details_json: String = row.get("details");
                let details: Vec<String> = serde_json::from_str(&details_json)
                    .unwrap_or_default();

                ChangelogEntry {
                    hash: row.get("hash"),
                    previous_hash: row.get("previous_hash"),
                    created_at: row.get("created_at"),
                    summary: row.get("summary"),
                    details,
                    author: row.get("author"),
                    queue_id: row.get("queue_id"),
                }
            })
            .collect();

        Ok(entries)
    }

    /// Get the last known plexus hash
    pub async fn get_last_hash(&self) -> Result<Option<String>, String> {
        let row = sqlx::query("SELECT last_hash FROM hash_state WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| format!("Failed to get last hash: {}", e))?;

        Ok(row.map(|r| r.get("last_hash")))
    }

    /// Update the last known plexus hash
    pub async fn set_last_hash(&self, hash: &str) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO hash_state (id, last_hash, updated_at)
            VALUES (1, ?, ?)
            "#,
        )
        .bind(hash)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to set last hash: {}", e))?;

        Ok(())
    }

    /// Check if a hash transition is documented
    pub async fn is_documented(&self, hash: &str) -> Result<bool, String> {
        let entry = self.get_entry(hash).await?;
        Ok(entry.is_some())
    }

    // ========== Queue Methods ==========

    /// Add a new queue entry (planned change)
    pub async fn add_queue_entry(&self, entry: &QueueEntry) -> Result<(), String> {
        let tags_json = serde_json::to_string(&entry.tags)
            .map_err(|e| format!("Failed to serialize tags: {}", e))?;
        let status_str = match entry.status {
            QueueStatus::Pending => "pending",
            QueueStatus::Completed => "completed",
        };

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO queue_entries
            (id, description, tags, created_at, status, completed_hash, completed_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&entry.id)
        .bind(&entry.description)
        .bind(&tags_json)
        .bind(entry.created_at)
        .bind(status_str)
        .bind(&entry.completed_hash)
        .bind(entry.completed_at)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to add queue entry: {}", e))?;

        Ok(())
    }

    /// Get a queue entry by ID
    pub async fn get_queue_entry(&self, id: &str) -> Result<Option<QueueEntry>, String> {
        let row = sqlx::query(
            r#"
            SELECT id, description, tags, created_at, status, completed_hash, completed_at
            FROM queue_entries
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to get queue entry: {}", e))?;

        match row {
            Some(row) => Ok(Some(self.row_to_queue_entry(row)?)),
            None => Ok(None),
        }
    }

    /// List all queue entries, optionally filtered by tag
    pub async fn list_queue_entries(&self, tag: Option<&str>) -> Result<Vec<QueueEntry>, String> {
        let rows = sqlx::query(
            r#"
            SELECT id, description, tags, created_at, status, completed_hash, completed_at
            FROM queue_entries
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list queue entries: {}", e))?;

        let mut entries = Vec::new();
        for row in rows {
            let entry = self.row_to_queue_entry(row)?;
            // Filter by tag if specified
            if let Some(filter_tag) = tag {
                if entry.tags.iter().any(|t| t == filter_tag) {
                    entries.push(entry);
                }
            } else {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    /// List pending queue entries, optionally filtered by tag
    pub async fn list_pending_queue_entries(&self, tag: Option<&str>) -> Result<Vec<QueueEntry>, String> {
        let rows = sqlx::query(
            r#"
            SELECT id, description, tags, created_at, status, completed_hash, completed_at
            FROM queue_entries
            WHERE status = 'pending'
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list pending queue entries: {}", e))?;

        let mut entries = Vec::new();
        for row in rows {
            let entry = self.row_to_queue_entry(row)?;
            // Filter by tag if specified
            if let Some(filter_tag) = tag {
                if entry.tags.iter().any(|t| t == filter_tag) {
                    entries.push(entry);
                }
            } else {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    /// Mark a queue entry as complete
    pub async fn complete_queue_entry(&self, id: &str, hash: &str) -> Result<Option<QueueEntry>, String> {
        let entry = self.get_queue_entry(id).await?;
        match entry {
            Some(entry) => {
                let completed = entry.complete(hash.to_string());
                self.add_queue_entry(&completed).await?;
                Ok(Some(completed))
            }
            None => Ok(None),
        }
    }

    /// Helper to convert a row to QueueEntry
    fn row_to_queue_entry(&self, row: sqlx::sqlite::SqliteRow) -> Result<QueueEntry, String> {
        let tags_json: String = row.get("tags");
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "completed" => QueueStatus::Completed,
            _ => QueueStatus::Pending,
        };

        Ok(QueueEntry {
            id: row.get("id"),
            description: row.get("description"),
            tags,
            created_at: row.get("created_at"),
            status,
            completed_hash: row.get("completed_hash"),
            completed_at: row.get("completed_at"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{tempdir, TempDir};

    async fn test_storage() -> (ChangelogStorage, TempDir) {
        let dir = tempdir().unwrap();
        let config = ChangelogStorageConfig {
            db_path: dir.path().join("test_changelog.db"),
        };
        let storage = ChangelogStorage::new(config).await.unwrap();
        (storage, dir)
    }

    #[tokio::test]
    async fn test_add_and_get_entry() {
        let (storage, _dir) = test_storage().await;

        let entry = ChangelogEntry::new(
            "abc123".to_string(),
            Some("xyz789".to_string()),
            "Test change".to_string(),
        )
        .with_details(vec!["Added feature X".to_string(), "Fixed bug Y".to_string()])
        .with_author("test".to_string());

        storage.add_entry(&entry).await.unwrap();

        let retrieved = storage.get_entry("abc123").await.unwrap().unwrap();
        assert_eq!(retrieved.hash, "abc123");
        assert_eq!(retrieved.previous_hash, Some("xyz789".to_string()));
        assert_eq!(retrieved.summary, "Test change");
        assert_eq!(retrieved.details.len(), 2);
        assert_eq!(retrieved.author, Some("test".to_string()));
    }

    #[tokio::test]
    async fn test_hash_state() {
        let (storage, _dir) = test_storage().await;

        // Initially no hash
        assert!(storage.get_last_hash().await.unwrap().is_none());

        // Set hash
        storage.set_last_hash("hash1").await.unwrap();
        assert_eq!(storage.get_last_hash().await.unwrap(), Some("hash1".to_string()));

        // Update hash
        storage.set_last_hash("hash2").await.unwrap();
        assert_eq!(storage.get_last_hash().await.unwrap(), Some("hash2".to_string()));
    }

    #[tokio::test]
    async fn test_is_documented() {
        let (storage, _dir) = test_storage().await;

        // Not documented
        assert!(!storage.is_documented("unknown").await.unwrap());

        // Add entry
        let entry = ChangelogEntry::new(
            "documented".to_string(),
            None,
            "Documented change".to_string(),
        );
        storage.add_entry(&entry).await.unwrap();

        // Now documented
        assert!(storage.is_documented("documented").await.unwrap());
    }
}

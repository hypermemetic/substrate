//! Mustache template storage using SQLite

use super::types::{MustacheError, TemplateInfo};
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePool}, ConnectOptions, Row};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Configuration for Mustache storage
#[derive(Debug, Clone)]
pub struct MustacheStorageConfig {
    /// Path to SQLite database for templates
    pub db_path: PathBuf,
}

impl Default for MustacheStorageConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("templates.db"),
        }
    }
}

/// Storage layer for mustache templates
pub struct MustacheStorage {
    pool: SqlitePool,
}

impl MustacheStorage {
    /// Create a new mustache storage instance
    pub async fn new(config: MustacheStorageConfig) -> Result<Self, MustacheError> {
        let db_url = format!("sqlite:{}?mode=rwc", config.db_path.display());
        let mut connect_options: SqliteConnectOptions = db_url.parse()
            .map_err(|e| format!("Failed to parse database URL: {}", e))?;
        connect_options.disable_statement_logging();
        let pool = SqlitePool::connect_with(connect_options.clone())
            .await
            .map_err(|e| format!("Failed to connect to templates database: {}", e))?;

        let storage = Self { pool };
        storage.run_migrations().await?;

        Ok(storage)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<(), MustacheError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS templates (
                id TEXT PRIMARY KEY,
                plugin_id TEXT NOT NULL,
                method TEXT NOT NULL,
                name TEXT NOT NULL,
                template TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(plugin_id, method, name)
            );

            CREATE INDEX IF NOT EXISTS idx_templates_plugin ON templates(plugin_id);
            CREATE INDEX IF NOT EXISTS idx_templates_lookup ON templates(plugin_id, method, name);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to run mustache migrations: {}", e))?;

        Ok(())
    }

    /// Get a template by plugin_id, method, and name
    pub async fn get_template(
        &self,
        plugin_id: &Uuid,
        method: &str,
        name: &str,
    ) -> Result<Option<String>, MustacheError> {
        let row = sqlx::query(
            "SELECT template FROM templates WHERE plugin_id = ? AND method = ? AND name = ?",
        )
        .bind(plugin_id.to_string())
        .bind(method)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to fetch template: {}", e))?;

        Ok(row.map(|r| r.get("template")))
    }

    /// Set (insert or update) a template
    pub async fn set_template(
        &self,
        plugin_id: &Uuid,
        method: &str,
        name: &str,
        template: &str,
    ) -> Result<TemplateInfo, MustacheError> {
        let now = current_timestamp();

        // Check if template exists
        let existing = sqlx::query(
            "SELECT id, created_at FROM templates WHERE plugin_id = ? AND method = ? AND name = ?",
        )
        .bind(plugin_id.to_string())
        .bind(method)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to check existing template: {}", e))?;

        let (id, created_at) = if let Some(row) = existing {
            let id: String = row.get("id");
            let created_at: i64 = row.get("created_at");

            // Update existing template
            sqlx::query(
                "UPDATE templates SET template = ?, updated_at = ? WHERE id = ?",
            )
            .bind(template)
            .bind(now)
            .bind(&id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to update template: {}", e))?;

            (id, created_at)
        } else {
            let id = Uuid::new_v4().to_string();

            // Insert new template
            sqlx::query(
                "INSERT INTO templates (id, plugin_id, method, name, template, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(plugin_id.to_string())
            .bind(method)
            .bind(name)
            .bind(template)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to insert template: {}", e))?;

            (id, now)
        };

        Ok(TemplateInfo {
            id,
            plugin_id: *plugin_id,
            method: method.to_string(),
            name: name.to_string(),
            created_at,
            updated_at: now,
        })
    }

    /// List all templates for a plugin
    pub async fn list_templates(&self, plugin_id: &Uuid) -> Result<Vec<TemplateInfo>, MustacheError> {
        let rows = sqlx::query(
            "SELECT id, plugin_id, method, name, created_at, updated_at
             FROM templates WHERE plugin_id = ? ORDER BY method, name",
        )
        .bind(plugin_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to list templates: {}", e))?;

        let templates: Result<Vec<TemplateInfo>, MustacheError> = rows
            .iter()
            .map(|row| {
                let plugin_id_str: String = row.get("plugin_id");
                Ok(TemplateInfo {
                    id: row.get("id"),
                    plugin_id: Uuid::parse_str(&plugin_id_str)
                        .map_err(|e| format!("Invalid plugin ID: {}", e))?,
                    method: row.get("method"),
                    name: row.get("name"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect();

        templates
    }

    /// Delete a template
    pub async fn delete_template(
        &self,
        plugin_id: &Uuid,
        method: &str,
        name: &str,
    ) -> Result<bool, MustacheError> {
        let result = sqlx::query(
            "DELETE FROM templates WHERE plugin_id = ? AND method = ? AND name = ?",
        )
        .bind(plugin_id.to_string())
        .bind(method)
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to delete template: {}", e))?;

        Ok(result.rows_affected() > 0)
    }
}

/// Get current Unix timestamp in seconds
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{tempdir, TempDir};

    async fn create_test_storage() -> (MustacheStorage, TempDir) {
        let dir = tempdir().unwrap();
        let config = MustacheStorageConfig {
            db_path: dir.path().join("test_templates.db"),
        };
        let storage = MustacheStorage::new(config).await.unwrap();
        (storage, dir)
    }

    #[tokio::test]
    async fn test_set_and_get_template() {
        let (storage, _dir) = create_test_storage().await;
        let plugin_id = Uuid::new_v4();

        // Set a template
        let info = storage
            .set_template(&plugin_id, "chat", "default", "[{{role}}]: {{content}}")
            .await
            .unwrap();

        assert_eq!(info.plugin_id, plugin_id);
        assert_eq!(info.method, "chat");
        assert_eq!(info.name, "default");

        // Get the template
        let template = storage
            .get_template(&plugin_id, "chat", "default")
            .await
            .unwrap();

        assert_eq!(template, Some("[{{role}}]: {{content}}".to_string()));
    }

    #[tokio::test]
    async fn test_update_template() {
        let (storage, _dir) = create_test_storage().await;
        let plugin_id = Uuid::new_v4();

        // Set initial template
        let info1 = storage
            .set_template(&plugin_id, "chat", "default", "v1")
            .await
            .unwrap();

        // Update template
        let info2 = storage
            .set_template(&plugin_id, "chat", "default", "v2")
            .await
            .unwrap();

        // ID and created_at should be preserved
        assert_eq!(info1.id, info2.id);
        assert_eq!(info1.created_at, info2.created_at);
        assert!(info2.updated_at >= info1.updated_at);

        // Content should be updated
        let template = storage
            .get_template(&plugin_id, "chat", "default")
            .await
            .unwrap();
        assert_eq!(template, Some("v2".to_string()));
    }

    #[tokio::test]
    async fn test_list_templates() {
        let (storage, _dir) = create_test_storage().await;
        let plugin_id = Uuid::new_v4();

        storage
            .set_template(&plugin_id, "chat", "default", "t1")
            .await
            .unwrap();
        storage
            .set_template(&plugin_id, "chat", "compact", "t2")
            .await
            .unwrap();
        storage
            .set_template(&plugin_id, "execute", "default", "t3")
            .await
            .unwrap();

        let templates = storage.list_templates(&plugin_id).await.unwrap();
        assert_eq!(templates.len(), 3);
    }

    #[tokio::test]
    async fn test_delete_template() {
        let (storage, _dir) = create_test_storage().await;
        let plugin_id = Uuid::new_v4();

        storage
            .set_template(&plugin_id, "chat", "default", "content")
            .await
            .unwrap();

        let deleted = storage
            .delete_template(&plugin_id, "chat", "default")
            .await
            .unwrap();
        assert!(deleted);

        let template = storage
            .get_template(&plugin_id, "chat", "default")
            .await
            .unwrap();
        assert!(template.is_none());

        // Deleting again should return false
        let deleted_again = storage
            .delete_template(&plugin_id, "chat", "default")
            .await
            .unwrap();
        assert!(!deleted_again);
    }
}

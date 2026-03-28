use crate::models::{NewToolDef, ToolDef};
use crate::StorageError;
use sqlx::SqlitePool;

pub struct ToolRepo;

impl ToolRepo {
    pub async fn upsert(pool: &SqlitePool, tool: &NewToolDef) -> Result<(), StorageError> {
        let config_str = tool.config.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default());
        let schema_str = tool.schema.as_ref().map(|s| serde_json::to_string(s).unwrap_or_default());

        sqlx::query(
            "INSERT INTO tools (id, name, description, category, config, schema)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                category = excluded.category,
                config = excluded.config,
                schema = excluded.schema,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(&tool.id)
        .bind(&tool.name)
        .bind(&tool.description)
        .bind(&tool.category)
        .bind(&config_str)
        .bind(&schema_str)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list(pool: &SqlitePool) -> Result<Vec<ToolDef>, StorageError> {
        let rows = sqlx::query_as::<_, ToolRow>("SELECT * FROM tools ORDER BY category, name")
            .fetch_all(pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn get(pool: &SqlitePool, id: &str) -> Result<ToolDef, StorageError> {
        let row = sqlx::query_as::<_, ToolRow>("SELECT * FROM tools WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("Tool {id}")))?;
        Ok(row.into())
    }

    pub async fn set_enabled(pool: &SqlitePool, id: &str, enabled: bool) -> Result<(), StorageError> {
        sqlx::query("UPDATE tools SET enabled = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn list_enabled(pool: &SqlitePool) -> Result<Vec<ToolDef>, StorageError> {
        let rows = sqlx::query_as::<_, ToolRow>(
            "SELECT * FROM tools WHERE enabled = 1 ORDER BY category, name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM tools WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct ToolRow {
    id: String,
    name: String,
    description: Option<String>,
    category: String,
    enabled: bool,
    config: Option<String>,
    schema: Option<String>,
    created_at: chrono::NaiveDateTime,
    updated_at: chrono::NaiveDateTime,
}

impl From<ToolRow> for ToolDef {
    fn from(r: ToolRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            category: r.category,
            enabled: r.enabled,
            config: r.config.and_then(|s| serde_json::from_str(&s).ok()),
            schema: r.schema.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

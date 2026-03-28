use crate::StorageError;
use sqlx::SqlitePool;

pub struct ConfigRepo;

impl ConfigRepo {
    pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<serde_json::Value>, StorageError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM configs WHERE key = ?")
                .bind(key)
                .fetch_optional(pool)
                .await?;
        match row {
            Some((val,)) => Ok(Some(serde_json::from_str(&val).unwrap_or(serde_json::Value::String(val)))),
            None => Ok(None),
        }
    }

    pub async fn set(pool: &SqlitePool, key: &str, value: &serde_json::Value) -> Result<(), StorageError> {
        let val_str = serde_json::to_string(value).unwrap_or_default();
        sqlx::query(
            "INSERT INTO configs (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(key)
        .bind(&val_str)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_all(pool: &SqlitePool) -> Result<serde_json::Map<String, serde_json::Value>, StorageError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM configs")
                .fetch_all(pool)
                .await?;
        let mut map = serde_json::Map::new();
        for (k, v) in rows {
            let val = serde_json::from_str(&v).unwrap_or(serde_json::Value::String(v));
            map.insert(k, val);
        }
        Ok(map)
    }

    pub async fn delete(pool: &SqlitePool, key: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM configs WHERE key = ?")
            .bind(key)
            .execute(pool)
            .await?;
        Ok(())
    }
}

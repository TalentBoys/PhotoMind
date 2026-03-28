use crate::models::{NewPhoto, Photo};
use crate::StorageError;
use sqlx::SqlitePool;

pub struct PhotoRepo;

impl PhotoRepo {
    pub async fn insert(pool: &SqlitePool, photo: &NewPhoto) -> Result<i64, StorageError> {
        let result = sqlx::query(
            "INSERT INTO photos (file_path, file_name, file_size, width, height, format, taken_at, file_hash)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&photo.file_path)
        .bind(&photo.file_name)
        .bind(photo.file_size)
        .bind(photo.width)
        .bind(photo.height)
        .bind(&photo.format)
        .bind(photo.taken_at)
        .bind(&photo.file_hash)
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Photo, StorageError> {
        let row = sqlx::query_as::<_, PhotoRow>("SELECT * FROM photos WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("Photo {id}")))?;
        Ok(row.into())
    }

    pub async fn get_by_path(pool: &SqlitePool, path: &str) -> Result<Option<Photo>, StorageError> {
        let row = sqlx::query_as::<_, PhotoRow>("SELECT * FROM photos WHERE file_path = ?")
            .bind(path)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(|r| r.into()))
    }

    pub async fn get_by_hash(pool: &SqlitePool, hash: &str) -> Result<Option<Photo>, StorageError> {
        let row = sqlx::query_as::<_, PhotoRow>("SELECT * FROM photos WHERE file_hash = ?")
            .bind(hash)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(|r| r.into()))
    }

    pub async fn list_unembedded(pool: &SqlitePool, limit: i64) -> Result<Vec<Photo>, StorageError> {
        let rows = sqlx::query_as::<_, PhotoRow>(
            "SELECT * FROM photos WHERE embedded = 0 ORDER BY id LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn mark_embedded(pool: &SqlitePool, id: i64) -> Result<(), StorageError> {
        sqlx::query("UPDATE photos SET embedded = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_path(pool: &SqlitePool, id: i64, new_path: &str) -> Result<(), StorageError> {
        let new_name = std::path::Path::new(new_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(new_path);
        sqlx::query(
            "UPDATE photos SET file_path = ?, file_name = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(new_path)
        .bind(new_name)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn count(pool: &SqlitePool) -> Result<i64, StorageError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photos")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }

    pub async fn count_embedded(pool: &SqlitePool) -> Result<i64, StorageError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photos WHERE embedded = 1")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }

    pub async fn count_unembedded(pool: &SqlitePool) -> Result<i64, StorageError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photos WHERE embedded = 0")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }
}

// Internal row type for sqlx mapping
#[derive(sqlx::FromRow)]
struct PhotoRow {
    id: i64,
    file_path: String,
    file_name: String,
    file_size: Option<i64>,
    width: Option<i32>,
    height: Option<i32>,
    format: Option<String>,
    taken_at: Option<chrono::NaiveDateTime>,
    created_at: chrono::NaiveDateTime,
    updated_at: chrono::NaiveDateTime,
    file_hash: Option<String>,
    embedded: bool,
}

impl From<PhotoRow> for Photo {
    fn from(r: PhotoRow) -> Self {
        Self {
            id: r.id,
            file_path: r.file_path,
            file_name: r.file_name,
            file_size: r.file_size,
            width: r.width,
            height: r.height,
            format: r.format,
            taken_at: r.taken_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
            file_hash: r.file_hash,
            embedded: r.embedded,
        }
    }
}

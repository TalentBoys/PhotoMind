use crate::StorageError;
use sqlx::SqlitePool;

pub struct EmbeddingRepo;

impl EmbeddingRepo {
    pub async fn insert(
        pool: &SqlitePool,
        photo_id: i64,
        vector: &[f32],
        model_name: &str,
    ) -> Result<i64, StorageError> {
        let blob = vector_to_blob(vector);
        let result = sqlx::query(
            "INSERT INTO embeddings (photo_id, vector, model_name) VALUES (?, ?, ?)",
        )
        .bind(photo_id)
        .bind(&blob)
        .bind(model_name)
        .execute(pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn get_by_photo_id(
        pool: &SqlitePool,
        photo_id: i64,
    ) -> Result<Option<Vec<f32>>, StorageError> {
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT vector FROM embeddings WHERE photo_id = ?")
                .bind(photo_id)
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|(blob,)| blob_to_vector(&blob)))
    }

    /// Load all embeddings for the in-memory index: returns (photo_id, vector) pairs.
    pub async fn load_all(pool: &SqlitePool) -> Result<Vec<(i64, Vec<f32>)>, StorageError> {
        let rows: Vec<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT photo_id, vector FROM embeddings ORDER BY photo_id")
                .fetch_all(pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(id, blob)| (id, blob_to_vector(&blob)))
            .collect())
    }

    pub async fn delete_by_photo_id(pool: &SqlitePool, photo_id: i64) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM embeddings WHERE photo_id = ?")
            .bind(photo_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

fn vector_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn blob_to_vector(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

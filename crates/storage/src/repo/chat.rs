use crate::models::NewChatMessage;
use crate::StorageError;
use sqlx::SqlitePool;

pub struct ChatRepo;

#[derive(sqlx::FromRow)]
struct ChatRow {
    id: i64,
    session_id: String,
    role: String,
    content: String,
    metadata: Option<String>,
    created_at: chrono::NaiveDateTime,
}

impl ChatRepo {
    pub async fn insert(pool: &SqlitePool, msg: &NewChatMessage) -> Result<i64, StorageError> {
        let metadata_str = msg.metadata.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default());
        let result = sqlx::query(
            "INSERT INTO chat_messages (session_id, role, content, metadata) VALUES (?, ?, ?, ?)",
        )
        .bind(&msg.session_id)
        .bind(&msg.role)
        .bind(&msg.content)
        .bind(&metadata_str)
        .execute(pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn get_session_messages(
        pool: &SqlitePool,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<crate::models::ChatMessage>, StorageError> {
        let rows = sqlx::query_as::<_, ChatRow>(
            "SELECT * FROM chat_messages WHERE session_id = ? ORDER BY created_at ASC LIMIT ?",
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::models::ChatMessage {
                id: r.id,
                session_id: r.session_id,
                role: r.role,
                content: r.content,
                metadata: r.metadata.and_then(|s| serde_json::from_str(&s).ok()),
                created_at: r.created_at,
            })
            .collect())
    }
}

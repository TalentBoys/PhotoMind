use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;
use tracing::info;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("photomind.db");
        let url = format!("sqlite:{}?mode=rwc", db_path.display());

        let options = SqliteConnectOptions::from_str(&url)?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(30));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
        info!("Database initialized at {}", db_path.display());
        Ok(db)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photos (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path   TEXT UNIQUE NOT NULL,
                file_name   TEXT NOT NULL,
                file_size   INTEGER,
                width       INTEGER,
                height      INTEGER,
                format      TEXT,
                taken_at    DATETIME,
                created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                file_hash   TEXT,
                embedded    BOOLEAN NOT NULL DEFAULT 0
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS embeddings (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                photo_id    INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
                vector      BLOB NOT NULL,
                model_name  TEXT NOT NULL,
                created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS configs (
                key         TEXT PRIMARY KEY,
                value       TEXT NOT NULL,
                updated_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tools (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                description TEXT,
                category    TEXT NOT NULL DEFAULT 'builtin',
                enabled     BOOLEAN NOT NULL DEFAULT 1,
                config      TEXT,
                schema      TEXT,
                created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tool_executions (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                tool_id      TEXT NOT NULL REFERENCES tools(id),
                params       TEXT NOT NULL,
                result       TEXT,
                status       TEXT NOT NULL DEFAULT 'pending_confirm',
                created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                confirmed_at DATETIME
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_messages (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id  TEXT NOT NULL,
                role        TEXT NOT NULL,
                content     TEXT NOT NULL,
                metadata    TEXT,
                created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await?;

        // Indexes
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_photos_file_hash ON photos(file_hash)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_photos_embedded ON photos(embedded)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_embeddings_photo_id ON embeddings(photo_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id, created_at)",
        )
        .execute(&self.pool)
        .await?;

        info!("Database migrations complete");
        Ok(())
    }
}

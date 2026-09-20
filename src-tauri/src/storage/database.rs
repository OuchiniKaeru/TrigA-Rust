use crate::AppError;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub async fn connect(url: &str) -> Result<SqlitePool, AppError> {
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect(url)
        .await
        .map_err(|error| AppError::Storage(error.to_string()))
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS automations (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            definition TEXT NOT NULL,
            enabled INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS execution_history (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            automation_id TEXT NOT NULL,
            automation_name TEXT NOT NULL,
            trigger_type TEXT NOT NULL,
            status TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            duration_ms INTEGER,
            result TEXT,
            error TEXT,
            started_at TEXT NOT NULL,
            finished_at TEXT
        );
        CREATE INDEX IF NOT EXISTS history_started_at_idx ON execution_history(started_at DESC);
        "#,
    )
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| AppError::Storage(error.to_string()))
}

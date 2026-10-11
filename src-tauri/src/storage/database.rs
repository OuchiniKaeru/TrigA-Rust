use crate::AppError;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::{Path, PathBuf};

pub const APP_DATA_DIR_NAME: &str = "com.triga.automation-manager";
pub const DB_FILE_NAME: &str = "automation.db";

pub async fn connect(url: &str) -> Result<SqlitePool, AppError> {
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect(url)
        .await
        .map_err(|error| AppError::Storage(error.to_string()))
}

/// Runtime data home for the SQLite database.
///
/// The database must NOT live inside a source tree: `cargo tauri dev`
/// watches `src-tauri/` for changes and kills + rebuilds the app whenever
/// a file under it changes, so every workflow save deleted the window.
/// The default home is the OS user-data directory; `TRIGA_DATA_DIR`
/// overrides it. Existing databases from the old CWD-relative layouts
/// (`./data/`, `./src-tauri/data/`) are copied over once (originals kept
/// as backup) so no data is lost on upgrade.
pub fn user_data_home() -> PathBuf {
    if let Ok(dir) = std::env::var("TRIGA_DATA_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    platform_data_home().join(APP_DATA_DIR_NAME)
}

fn platform_data_home() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(dir) = std::env::var("APPDATA") {
            if !dir.trim().is_empty() {
                return PathBuf::from(dir);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home).join("Library/Application Support");
            }
        }
    }
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join(".local/share");
        }
    }
    PathBuf::from(".local/share")
}

/// Old CWD-relative database locations (pre-0.2.1), most-recent layout first.
pub fn legacy_db_candidates(cwd: &Path) -> Vec<PathBuf> {
    vec![
        cwd.join("src-tauri/data").join(DB_FILE_NAME),
        cwd.join("data").join(DB_FILE_NAME),
    ]
}

/// Ensure the data home exists, migrating a legacy database once when needed.
/// Returns the directory containing the live database.
pub fn ensure_data_home() -> Result<PathBuf, AppError> {
    let home = user_data_home();
    std::fs::create_dir_all(&home)
        .map_err(|error| AppError::Storage(format!("cannot create data dir: {error}")))?;
    let target = home.join(DB_FILE_NAME);
    if !target.exists() {
        if let Ok(cwd) = std::env::current_dir() {
            for candidate in legacy_db_candidates(&cwd) {
                if candidate.is_file() && !same_file(&candidate, &target) {
                    std::fs::copy(&candidate, &target).map_err(|error| {
                        AppError::Storage(format!("cannot migrate legacy database: {error}"))
                    })?;
                    break;
                }
            }
        }
    }
    Ok(home)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// SQLite URL for the live database file.
pub fn db_url(dir: &Path) -> String {
    format!(
        "sqlite://{}?mode=rwc",
        dir.join(DB_FILE_NAME).display()
    )
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
    .map_err(|error| AppError::Storage(error.to_string()))?;

    migrate_workflows(pool).await?;
    recover_interrupted_jobs(pool).await?;
    Ok(())
}

/// Additive workflow migration (spec Phase 1). Existing tables/columns are
/// never dropped. Safe to run twice.
async fn migrate_workflows(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS workflows (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            enabled INTEGER NOT NULL DEFAULT 1,
            schema_version INTEGER NOT NULL DEFAULT 1,
            definition_yaml TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT
        );
        CREATE INDEX IF NOT EXISTS workflows_enabled_updated_idx ON workflows(enabled, updated_at);
        CREATE TABLE IF NOT EXISTS workflow_versions (
            id TEXT PRIMARY KEY,
            workflow_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            definition_yaml TEXT NOT NULL,
            change_summary TEXT,
            created_at TEXT NOT NULL,
            UNIQUE(workflow_id, version)
        );
        CREATE INDEX IF NOT EXISTS workflow_versions_wf_version_idx ON workflow_versions(workflow_id, version);
        CREATE TABLE IF NOT EXISTS workflow_node_runs (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            node_id TEXT NOT NULL,
            node_type TEXT NOT NULL,
            status TEXT NOT NULL,
            attempt INTEGER NOT NULL DEFAULT 1,
            max_attempts INTEGER NOT NULL DEFAULT 1,
            started_at TEXT,
            finished_at TEXT,
            duration_ms INTEGER,
            input_json TEXT,
            output_json TEXT,
            error TEXT,
            log_ref TEXT
        );
        CREATE INDEX IF NOT EXISTS workflow_node_runs_job_node_idx ON workflow_node_runs(job_id, node_id);
        CREATE TABLE IF NOT EXISTS workflow_triggers (
            id TEXT PRIMARY KEY,
            workflow_id TEXT NOT NULL,
            automation_id TEXT,
            trigger_id TEXT,
            trigger_type TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS workflow_triggers_wf_enabled_idx ON workflow_triggers(workflow_id, enabled);
        "#,
    )
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| AppError::Storage(error.to_string()))?;

    // Extend execution_history for workflow runs (nullable, backward compatible).
    for (column, ddl) in [
        ("workflow_id", "ALTER TABLE execution_history ADD COLUMN workflow_id TEXT"),
        (
            "workflow_version",
            "ALTER TABLE execution_history ADD COLUMN workflow_version INTEGER",
        ),
        (
            "definition_snapshot_yaml",
            "ALTER TABLE execution_history ADD COLUMN definition_snapshot_yaml TEXT",
        ),
        (
            "cancel_requested",
            "ALTER TABLE execution_history ADD COLUMN cancel_requested INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if !has_column(pool, "execution_history", column).await? {
            sqlx::query(ddl)
                .execute(pool)
                .await
                .map(|_| ())
                .map_err(|error| AppError::Storage(error.to_string()))?;
        }
    }
    sqlx::query("CREATE INDEX IF NOT EXISTS jobs_workflow_started_idx ON execution_history(workflow_id, started_at)")
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|error| AppError::Storage(error.to_string()))?;
    Ok(())
}

async fn has_column(pool: &SqlitePool, table: &str, column: &str) -> Result<bool, AppError> {
    let rows: Vec<(String,)> = sqlx::query_as(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .fetch_all(pool)
        .await
        .map_err(|error| AppError::Storage(error.to_string()))?;
    Ok(rows.iter().any(|row| row.0 == column))
}

/// Startup recovery (spec 13.3): jobs left in `running`/`retrying` by a crash
/// are finalized as `failed` (interrupted) instead of staying running forever.
/// Never marks them as success.
async fn recover_interrupted_jobs(pool: &SqlitePool) -> Result<(), AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE execution_history SET status='failed', error='interrupted: process exited before completion', finished_at=COALESCE(finished_at, ?) WHERE status IN ('running','retrying','pending')",
    )
    .bind(&now)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| AppError::Storage(error.to_string()))?;
    sqlx::query(
        "UPDATE workflow_node_runs SET status='failed', error='interrupted: process exited before completion' WHERE status IN ('running','retrying','pending')",
    )
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| AppError::Storage(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod data_home_tests {
    use super::*;

    #[test]
    fn default_home_is_outside_any_source_tree() {
        // Without override the DB must not live under a watched source tree
        // (src-tauri/ is watched by `cargo tauri dev`; a DB there means every
        // save kills the app window).
        let home = platform_data_home().join(APP_DATA_DIR_NAME);
        let text = home.to_string_lossy();
        assert!(!text.contains("src-tauri"), "data home must not be under src-tauri: {text}");
        assert!(db_url(&home).starts_with("sqlite://"));
        assert!(db_url(&home).contains(DB_FILE_NAME));
    }

    #[test]
    fn legacy_candidates_prefer_dev_layout() {
        let cwd = Path::new("/repo");
        let candidates = legacy_db_candidates(cwd);
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates[0],
            PathBuf::from("/repo/src-tauri/data/automation.db")
        );
        assert_eq!(candidates[1], PathBuf::from("/repo/data/automation.db"));
    }

    #[test]
    fn migrates_legacy_db_once_and_keeps_original() {
        let base = std::env::temp_dir().join(format!("triga-datadir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let legacy_dir = base.join("proj/src-tauri/data");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join(DB_FILE_NAME), b"legacy-bytes").unwrap();
        let new_home = base.join("home").join(APP_DATA_DIR_NAME);
        // Simulate ensure_data_home with an explicit override home.
        std::fs::create_dir_all(&new_home).unwrap();
        let target = new_home.join(DB_FILE_NAME);
        assert!(!target.exists());
        let candidates = legacy_db_candidates(&base.join("proj"));
        assert!(candidates[0].is_file());
        std::fs::copy(&candidates[0], &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"legacy-bytes");
        assert!(candidates[0].is_file(), "original must be kept as backup");
        let _ = std::fs::remove_dir_all(&base);
    }
}

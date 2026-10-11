use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRunRecord {
    pub id: String,
    pub job_id: String,
    pub node_id: String,
    pub node_type: String,
    pub status: String,
    pub attempt: i64,
    pub max_attempts: i64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub input_json: Option<String>,
    pub output_json: Option<String>,
    pub error: Option<String>,
    pub log_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunDetail {
    pub job: crate::storage::repository::HistoryRecord,
    pub workflow_id: Option<String>,
    pub workflow_version: Option<i64>,
    pub definition_snapshot_yaml: Option<String>,
    pub nodes: Vec<NodeRunRecord>,
}

pub async fn list_node_runs(pool: &SqlitePool, job_id: &str) -> Result<Vec<NodeRunRecord>, AppError> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as("SELECT id, job_id, node_id, node_type, status, attempt, max_attempts, started_at, finished_at, duration_ms, input_json, output_json, error, log_ref FROM workflow_node_runs WHERE job_id = ? ORDER BY node_id")
        .bind(job_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|r| NodeRunRecord {
            id: r.0,
            job_id: r.1,
            node_id: r.2,
            node_type: r.3,
            status: r.4,
            attempt: r.5,
            max_attempts: r.6,
            started_at: r.7,
            finished_at: r.8,
            duration_ms: r.9,
            input_json: r.10,
            output_json: r.11,
            error: r.12,
            log_ref: r.13,
        })
        .collect())
}

pub async fn get_run_detail(pool: &SqlitePool, job_id: &str) -> Result<WorkflowRunDetail, AppError> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    )> = sqlx::query_as("SELECT id, job_id, automation_id, automation_name, trigger_type, status, attempt, duration_ms, result, error, started_at, finished_at, workflow_id, workflow_version, definition_snapshot_yaml FROM execution_history WHERE job_id = ?")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    let row = row.ok_or_else(|| AppError::NotFound(format!("job '{job_id}'")))?;
    let started_at = chrono::DateTime::parse_from_rfc3339(&row.10)
        .map(|d| d.with_timezone(&chrono::Utc))
        .map_err(|e| AppError::Storage(e.to_string()))?;
    let finished_at = row
        .11
        .map(|v| {
            chrono::DateTime::parse_from_rfc3339(&v)
                .map(|d| d.with_timezone(&chrono::Utc))
                .map_err(|e| AppError::Storage(e.to_string()))
        })
        .transpose()?;
    let job = crate::storage::repository::HistoryRecord {
        id: row.0,
        job_id: row.1,
        automation_id: row.2,
        automation_name: row.3,
        trigger_type: row.4,
        status: row.5,
        attempt: row.6 as u32,
        duration_ms: row.7,
        result: row.8,
        error: row.9,
        started_at,
        finished_at,
    };
    let nodes = list_node_runs(pool, job_id).await?;
    Ok(WorkflowRunDetail {
        job,
        workflow_id: row.12,
        workflow_version: row.13,
        definition_snapshot_yaml: row.14,
        nodes,
    })
}

pub async fn request_cancel(pool: &SqlitePool, job_id: &str) -> Result<(), AppError> {
    let result = sqlx::query("UPDATE execution_history SET cancel_requested=1 WHERE job_id=?")
        .bind(job_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("job '{job_id}'")));
    }
    Ok(())
}

/// Workflow runs share the execution_history table; list only workflow-backed jobs.
pub async fn list_workflow_runs(
    pool: &SqlitePool,
    workflow_id: Option<&str>,
    limit: u32,
) -> Result<Vec<crate::storage::repository::HistoryRecord>, AppError> {
    let limit = limit.clamp(1, 100) as i64;
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
    )> = if let Some(wf) = workflow_id {
        sqlx::query_as("SELECT id, job_id, automation_id, automation_name, trigger_type, status, attempt, duration_ms, result, error, started_at, finished_at FROM execution_history WHERE workflow_id = ? ORDER BY started_at DESC LIMIT ?")
            .bind(wf)
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?
    } else {
        sqlx::query_as("SELECT id, job_id, automation_id, automation_name, trigger_type, status, attempt, duration_ms, result, error, started_at, finished_at FROM execution_history WHERE workflow_id IS NOT NULL ORDER BY started_at DESC LIMIT ?")
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?
    };
    rows.into_iter()
        .map(|row| {
            Ok(crate::storage::repository::HistoryRecord {
                id: row.0,
                job_id: row.1,
                automation_id: row.2,
                automation_name: row.3,
                trigger_type: row.4,
                status: row.5,
                attempt: row.6 as u32,
                duration_ms: row.7,
                result: row.8,
                error: row.9,
                started_at: chrono::DateTime::parse_from_rfc3339(&row.10)
                    .map_err(|e| AppError::Storage(e.to_string()))?
                    .with_timezone(&chrono::Utc),
                finished_at: row
                    .11
                    .map(|v| {
                        chrono::DateTime::parse_from_rfc3339(&v)
                            .map(|d| d.with_timezone(&chrono::Utc))
                            .map_err(|e| AppError::Storage(e.to_string()))
                    })
                    .transpose()?,
            })
        })
        .collect()
}

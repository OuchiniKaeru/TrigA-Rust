use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{core::models::AutomationDefinition, AppError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub job_id: String,
    pub automation_id: String,
    pub automation_name: String,
    pub trigger_type: String,
    pub status: String,
    pub attempt: u32,
    pub duration_ms: Option<i64>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub async fn save_automation(
    pool: &SqlitePool,
    automation: &AutomationDefinition,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let definition =
        serde_json::to_string(automation).map_err(|error| AppError::Storage(error.to_string()))?;
    sqlx::query("INSERT INTO automations (id, name, definition, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name=excluded.name, definition=excluded.definition, enabled=excluded.enabled, updated_at=excluded.updated_at")
        .bind(&automation.id).bind(&automation.name).bind(definition).bind(automation.enabled).bind(&now).bind(&now)
        .execute(pool).await.map(|_| ()).map_err(|error| AppError::Storage(error.to_string()))
}

pub async fn list_automations(pool: &SqlitePool) -> Result<Vec<AutomationDefinition>, AppError> {
    let rows = sqlx::query_scalar::<_, String>("SELECT definition FROM automations ORDER BY name")
        .fetch_all(pool)
        .await
        .map_err(|error| AppError::Storage(error.to_string()))?;
    rows.into_iter()
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| AppError::Storage(error.to_string()))
        })
        .collect()
}

pub async fn get_automation(pool: &SqlitePool, id: &str) -> Result<AutomationDefinition, AppError> {
    let value = sqlx::query_scalar::<_, String>("SELECT definition FROM automations WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| AppError::Storage(error.to_string()))?
        .ok_or_else(|| AppError::NotFound("automation".into()))?;
    serde_json::from_str(&value).map_err(|error| AppError::Storage(error.to_string()))
}

pub async fn delete_automation(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM automations WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|error| AppError::Storage(error.to_string()))
}

pub async fn set_enabled(
    pool: &SqlitePool,
    id: &str,
    enabled: bool,
) -> Result<AutomationDefinition, AppError> {
    let mut automation = get_automation(pool, id).await?;
    automation.enabled = enabled;
    save_automation(pool, &automation).await?;
    Ok(automation)
}

pub async fn insert_history(
    pool: &SqlitePool,
    mut record: HistoryRecord,
) -> Result<HistoryRecord, AppError> {
    if record.id.is_empty() {
        record.id = Uuid::new_v4().to_string();
    }
    sqlx::query("INSERT INTO execution_history (id, job_id, automation_id, automation_name, trigger_type, status, attempt, duration_ms, result, error, started_at, finished_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&record.id).bind(&record.job_id).bind(&record.automation_id).bind(&record.automation_name).bind(&record.trigger_type).bind(&record.status).bind(record.attempt as i64).bind(record.duration_ms).bind(&record.result).bind(&record.error).bind(record.started_at.to_rfc3339()).bind(record.finished_at.map(|value| value.to_rfc3339())).execute(pool).await.map(|_| record).map_err(|error| AppError::Storage(error.to_string()))
}

pub async fn list_history(pool: &SqlitePool, limit: u32) -> Result<Vec<HistoryRecord>, AppError> {
    let limit = limit.clamp(1, 20) as i64;
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, i64, Option<i64>, Option<String>, Option<String>, String, Option<String>)>("SELECT id, job_id, automation_id, automation_name, trigger_type, status, attempt, duration_ms, result, error, started_at, finished_at FROM execution_history ORDER BY started_at DESC LIMIT ?")
        .bind(limit).fetch_all(pool).await.map_err(|error| AppError::Storage(error.to_string()))?;
    rows.into_iter()
        .map(|row| {
            Ok(HistoryRecord {
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
                started_at: DateTime::parse_from_rfc3339(&row.10)
                    .map_err(|error| AppError::Storage(error.to_string()))?
                    .with_timezone(&Utc),
                finished_at: row
                    .11
                    .map(|value| {
                        DateTime::parse_from_rfc3339(&value)
                            .map(|date| date.with_timezone(&Utc))
                            .map_err(|error| AppError::Storage(error.to_string()))
                    })
                    .transpose()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{AutomationDefinition, ExecutionPolicy, TriggerDefinition};
    use crate::storage::database;

    #[tokio::test]
    async fn persists_automation_and_limits_history_to_twenty() {
        let pool = database::connect("sqlite::memory:").await.unwrap();
        database::migrate(&pool).await.unwrap();
        let automation = AutomationDefinition::minimal(
            "demo",
            TriggerDefinition::Manual,
            ExecutionPolicy::default(),
        );
        save_automation(&pool, &automation).await.unwrap();
        assert_eq!(
            get_automation(&pool, &automation.id).await.unwrap().name,
            "demo"
        );
        set_enabled(&pool, &automation.id, false).await.unwrap();
        assert!(!get_automation(&pool, &automation.id).await.unwrap().enabled);
        for index in 0..21 {
            insert_history(
                &pool,
                HistoryRecord {
                    id: index.to_string(),
                    job_id: index.to_string(),
                    automation_id: automation.id.clone(),
                    automation_name: "demo".into(),
                    trigger_type: "manual".into(),
                    status: "success".into(),
                    attempt: 1,
                    duration_ms: Some(1),
                    result: None,
                    error: None,
                    started_at: Utc::now(),
                    finished_at: None,
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(list_history(&pool, 20).await.unwrap().len(), 20);
    }
}

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    workflow::{model::WorkflowDefinition, validator, yaml_codec},
    AppError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub schema_version: i32,
    pub definition_yaml: String,
    pub version: i32,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowVersionRecord {
    pub id: String,
    pub workflow_id: String,
    pub version: i32,
    pub definition_yaml: String,
    pub change_summary: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTriggerRecord {
    pub id: String,
    pub workflow_id: String,
    pub automation_id: Option<String>,
    pub trigger_id: Option<String>,
    pub trigger_type: String,
    pub enabled: bool,
    pub created_at: String,
}

fn now_str() -> String {
    Utc::now().to_rfc3339()
}

fn row_to_record(
    id: String,
    name: String,
    description: String,
    enabled: i64,
    schema_version: i64,
    definition_yaml: String,
    version: i64,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
) -> WorkflowRecord {
    WorkflowRecord {
        id,
        name,
        description,
        enabled: enabled != 0,
        schema_version: schema_version as i32,
        definition_yaml,
        version: version as i32,
        created_at,
        updated_at,
        deleted_at,
    }
}

/// Create a workflow from YAML text. Validates before persisting (spec 6.4).
pub async fn create_from_yaml(
    pool: &SqlitePool,
    yaml: &str,
    change_summary: Option<&str>,
) -> Result<WorkflowRecord, AppError> {
    let mut def = yaml_codec::parse(yaml)?;
    if def.workflow.id.trim().is_empty() {
        def.workflow.id = Uuid::new_v4().to_string();
    }
    let report = validator::validate(&def)?;
    if !report.valid {
        return Err(AppError::Validation(format!(
            "invalid workflow: {}",
            report.errors.join("; ")
        )));
    }
    let canonical = yaml_codec::serialize(&def)?;
    let now = now_str();
    let id = def.workflow.id.clone();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM workflows WHERE id = ? AND deleted_at IS NULL")
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    if existing.is_some() {
        return Err(AppError::Conflict(format!("workflow '{id}' already exists")));
    }
    sqlx::query("INSERT INTO workflows (id, name, description, enabled, schema_version, definition_yaml, version, created_at, updated_at, deleted_at) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, NULL)")
        .bind(&id)
        .bind(&def.workflow.name)
        .bind(&def.workflow.description)
        .bind(if def.workflow.enabled { 1 } else { 0 })
        .bind(def.schema_version as i64)
        .bind(&canonical)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    sqlx::query("INSERT INTO workflow_versions (id, workflow_id, version, definition_yaml, change_summary, created_at) VALUES (?, ?, 1, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string())
        .bind(&id)
        .bind(&canonical)
        .bind(change_summary)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    tx.commit().await.map_err(|e| AppError::Storage(e.to_string()))?;
    get(pool, &id).await
}

/// Update with optimistic locking (spec: version for conflict detection).
pub async fn update_from_yaml(
    pool: &SqlitePool,
    id: &str,
    yaml: &str,
    expected_version: Option<i32>,
    change_summary: Option<&str>,
) -> Result<WorkflowRecord, AppError> {
    let mut def = yaml_codec::parse(yaml)?;
    // Keep the URL id authoritative.
    def.workflow.id = id.to_string();
    let report = validator::validate(&def)?;
    if !report.valid {
        return Err(AppError::Validation(format!(
            "invalid workflow: {}",
            report.errors.join("; ")
        )));
    }
    let current = get(pool, id).await?;
    if let Some(expected) = expected_version {
        if current.version != expected {
            return Err(AppError::Conflict(format!(
                "version conflict: expected {expected} but current is {}",
                current.version
            )));
        }
    }
    // Only create a new version when the definition actually changed (spec 5.6).
    let current_def = yaml_codec::parse(&current.definition_yaml)?;
    let canonical = yaml_codec::serialize(&def)?;
    let now = now_str();
    if canonical == current.definition_yaml {
        return Ok(current);
    }
    let _ = current_def;
    let new_version = current.version + 1;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    sqlx::query("UPDATE workflows SET name=?, description=?, enabled=?, schema_version=?, definition_yaml=?, version=?, updated_at=? WHERE id=?")
        .bind(&def.workflow.name)
        .bind(&def.workflow.description)
        .bind(if def.workflow.enabled { 1 } else { 0 })
        .bind(def.schema_version as i64)
        .bind(&canonical)
        .bind(new_version as i64)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    sqlx::query("INSERT INTO workflow_versions (id, workflow_id, version, definition_yaml, change_summary, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string())
        .bind(id)
        .bind(new_version as i64)
        .bind(&canonical)
        .bind(change_summary)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    tx.commit().await.map_err(|e| AppError::Storage(e.to_string()))?;
    get(pool, id).await
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<WorkflowRecord, AppError> {
    let row: Option<(String, String, String, i64, i64, String, i64, String, String, Option<String>)> =
        sqlx::query_as("SELECT id, name, description, enabled, schema_version, definition_yaml, version, created_at, updated_at, deleted_at FROM workflows WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    row.map(|r| row_to_record(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9))
        .ok_or_else(|| AppError::NotFound(format!("workflow '{id}'")))
}

pub async fn list(
    pool: &SqlitePool,
    enabled_only: Option<bool>,
    search: Option<&str>,
) -> Result<Vec<WorkflowRecord>, AppError> {
    let rows: Vec<(String, String, String, i64, i64, String, i64, String, String, Option<String>)> =
        sqlx::query_as("SELECT id, name, description, enabled, schema_version, definition_yaml, version, created_at, updated_at, deleted_at FROM workflows WHERE deleted_at IS NULL ORDER BY updated_at DESC")
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    let mut out: Vec<WorkflowRecord> = rows
        .into_iter()
        .map(|r| row_to_record(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9))
        .collect();
    if let Some(enabled) = enabled_only {
        out.retain(|w| w.enabled == enabled);
    }
    if let Some(q) = search.filter(|s| !s.trim().is_empty()) {
        let q = q.to_lowercase();
        out.retain(|w| {
            w.name.to_lowercase().contains(&q)
                || w.description.to_lowercase().contains(&q)
                || w.id.to_lowercase().contains(&q)
        });
    }
    Ok(out)
}

/// Logical delete (spec 3.1).
pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let _ = get(pool, id).await?;
    let now = now_str();
    sqlx::query("UPDATE workflows SET deleted_at=?, updated_at=? WHERE id=?")
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

pub async fn set_enabled(pool: &SqlitePool, id: &str, enabled: bool) -> Result<WorkflowRecord, AppError> {
    let rec = get(pool, id).await?;
    let mut def: WorkflowDefinition = yaml_codec::parse(&rec.definition_yaml)?;
    def.workflow.enabled = enabled;
    let yaml = yaml_codec::serialize(&def)?;
    // Keep version unchanged for enable/disable? Spec keeps enabled column mirrored.
    // We update both the column and the YAML without bumping version history when
    // only the flag changed... but to keep them in sync simply, bump version only
    // if YAML differs (it will). Use internal update without conflict check.
    let now = now_str();
    let new_version = rec.version + 1;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    sqlx::query("UPDATE workflows SET name=?, enabled=?, definition_yaml=?, version=?, updated_at=? WHERE id=?")
        .bind(&def.workflow.name)
        .bind(if enabled { 1 } else { 0 })
        .bind(&yaml)
        .bind(new_version as i64)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    sqlx::query("INSERT INTO workflow_versions (id, workflow_id, version, definition_yaml, change_summary, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string())
        .bind(id)
        .bind(new_version as i64)
        .bind(&yaml)
        .bind(if enabled { "enabled" } else { "disabled" })
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;
    tx.commit().await.map_err(|e| AppError::Storage(e.to_string()))?;
    get(pool, id).await
}

pub async fn duplicate(pool: &SqlitePool, id: &str) -> Result<WorkflowRecord, AppError> {
    let rec = get(pool, id).await?;
    let mut def: WorkflowDefinition = yaml_codec::parse(&rec.definition_yaml)?;
    let new_id = Uuid::new_v4().to_string();
    def.workflow.id = new_id.clone();
    def.workflow.name = format!("{} (copy)", def.workflow.name);
    // New node/edge IDs so references are not accidentally shared (spec 9.3).
    let mut id_map: std::collections::HashMap<String, String> = Default::default();
    for n in &mut def.nodes {
        let new_node_id = format!("{}-{}", n.id, &Uuid::new_v4().to_string()[..8]);
        id_map.insert(n.id.clone(), new_node_id.clone());
        n.id = new_node_id;
    }
    for e in &mut def.edges {
        e.id = Uuid::new_v4().to_string();
        if let Some(v) = id_map.get(&e.source) {
            e.source = v.clone();
        }
        if let Some(v) = id_map.get(&e.target) {
            e.target = v.clone();
        }
    }
    // Rewrite nodes.<old>.output refs.
    let yaml_tmp = yaml_codec::serialize(&def)?;
    let mut yaml_new = yaml_tmp;
    for (old, new) in &id_map {
        yaml_new = yaml_new.replace(&format!("nodes.{old}.output"), &format!("nodes.{new}.output"));
    }
    create_from_yaml(pool, &yaml_new, Some("duplicated")).await
}

pub async fn list_versions(pool: &SqlitePool, workflow_id: &str) -> Result<Vec<WorkflowVersionRecord>, AppError> {
    let _ = get(pool, workflow_id).await?;
    let rows: Vec<(String, String, i64, String, Option<String>, String)> =
        sqlx::query_as("SELECT id, workflow_id, version, definition_yaml, change_summary, created_at FROM workflow_versions WHERE workflow_id = ? ORDER BY version DESC")
            .bind(workflow_id)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|r| WorkflowVersionRecord {
            id: r.0,
            workflow_id: r.1,
            version: r.2 as i32,
            definition_yaml: r.3,
            change_summary: r.4,
            created_at: r.5,
        })
        .collect())
}

/// Restore = save past definition as a NEW version (spec 5.6).
pub async fn restore_version(
    pool: &SqlitePool,
    workflow_id: &str,
    version: i32,
) -> Result<WorkflowRecord, AppError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT definition_yaml FROM workflow_versions WHERE workflow_id = ? AND version = ?")
            .bind(workflow_id)
            .bind(version as i64)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    let yaml = row
        .map(|r| r.0)
        .ok_or_else(|| AppError::NotFound(format!("version {version}")))?;
    let current = get(pool, workflow_id).await?;
    update_from_yaml(
        pool,
        workflow_id,
        &yaml,
        Some(current.version),
        Some(&format!("restored from version {version}")),
    )
    .await
}

/// Import with ID collision policy (spec 9.5).
pub async fn import_yaml(
    pool: &SqlitePool,
    yaml: &str,
    policy: &str,
) -> Result<WorkflowRecord, AppError> {
    let def = yaml_codec::parse(yaml)?;
    let id = def.workflow.id.clone();
    let exists = if id.trim().is_empty() {
        false
    } else {
        get(pool, &id).await.is_ok()
    };
    if !exists {
        return create_from_yaml(pool, yaml, Some("imported")).await;
    }
    match policy {
        "update" => {
            let current = get(pool, &id).await?;
            update_from_yaml(pool, &id, yaml, Some(current.version), Some("imported")).await
        }
        "new_id" => {
            let mut def = def;
            def.workflow.id = Uuid::new_v4().to_string();
            let yaml_new = yaml_codec::serialize(&def)?;
            create_from_yaml(pool, &yaml_new, Some("imported with new id")).await
        }
        _ => Err(AppError::Conflict(format!(
            "workflow '{id}' already exists (policy: update | new_id | cancel)"
        ))),
    }
}

// --- workflow_triggers ---

pub async fn list_triggers(pool: &SqlitePool, workflow_id: &str) -> Result<Vec<WorkflowTriggerRecord>, AppError> {
    let rows: Vec<(String, String, Option<String>, Option<String>, String, i64, String)> =
        sqlx::query_as("SELECT id, workflow_id, automation_id, trigger_id, trigger_type, enabled, created_at FROM workflow_triggers WHERE workflow_id = ? ORDER BY created_at")
            .bind(workflow_id)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|r| WorkflowTriggerRecord {
            id: r.0,
            workflow_id: r.1,
            automation_id: r.2,
            trigger_id: r.3,
            trigger_type: r.4,
            enabled: r.5 != 0,
            created_at: r.6,
        })
        .collect())
}

/// Link an existing automation as a trigger source for a workflow.
/// The automation keeps its own trigger_definition (no duplication, spec 5.3).
pub async fn attach_automation_trigger(
    pool: &SqlitePool,
    workflow_id: &str,
    automation_id: &str,
    enabled: bool,
) -> Result<WorkflowTriggerRecord, AppError> {
    let _ = get(pool, workflow_id).await?;
    // Verify automation exists.
    crate::storage::repository::get_automation(pool, automation_id).await?;
    let now = now_str();
    let automation = crate::storage::repository::get_automation(pool, automation_id).await?;
    let trigger_type = automation.trigger.trigger_type().to_string();
    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO workflow_triggers (id, workflow_id, automation_id, trigger_id, trigger_type, enabled, created_at) VALUES (?, ?, ?, NULL, ?, ?, ?)")
        .bind(&id)
        .bind(workflow_id)
        .bind(automation_id)
        .bind(&trigger_type)
        .bind(if enabled { 1 } else { 0 })
        .bind(&now)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))?;
    Ok(WorkflowTriggerRecord {
        id,
        workflow_id: workflow_id.into(),
        automation_id: Some(automation_id.into()),
        trigger_id: None,
        trigger_type,
        enabled,
        created_at: now,
    })
}

pub async fn detach_trigger(pool: &SqlitePool, trigger_link_id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM workflow_triggers WHERE id = ?")
        .bind(trigger_link_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

/// Find enabled workflows linked to an automation (for trigger fan-out).
pub async fn workflows_for_automation(
    pool: &SqlitePool,
    automation_id: &str,
) -> Result<Vec<WorkflowRecord>, AppError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT workflow_id FROM workflow_triggers WHERE automation_id = ? AND enabled != 0")
            .bind(automation_id)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    let mut out = Vec::new();
    for (wf_id,) in rows {
        if let Ok(rec) = get(pool, &wf_id).await {
            if rec.enabled {
                out.push(rec);
            }
        }
    }
    Ok(out)
}

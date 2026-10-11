use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    engine::application_service::AppState,
    storage::{repository, workflow_repository, workflow_run_repository},
    workflow::{
        context::{event_map_from_legacy, RunContext},
        engine,
        model::WorkflowDefinition,
        validator,
        yaml_codec,
    },
    AppError,
};

/// Preview validation without persisting (used by validate commands/API).
pub fn validate_yaml_text(yaml: &str) -> Result<validator::ValidationReport, AppError> {
    let def = yaml_codec::parse(yaml)?;
    let report = validator::validate(&def)?;
    // Security gate: invalid definitions are rejected by callers.
    Ok(report)
}

/// Manual run entry point. Allowed even when enabled=false (spec 8.2).
/// Returns job_id immediately; execution continues in background (spec 10.1).
pub async fn run_workflow(
    state: &Arc<AppState>,
    workflow_id: &str,
    inputs: HashMap<String, serde_json::Value>,
    trigger_type: &str,
    event: HashMap<String, String>,
) -> Result<String, AppError> {
    let rec = workflow_repository::get(&state.pool, workflow_id).await?;
    let def: WorkflowDefinition = yaml_codec::parse(&rec.definition_yaml)?;
    run_definition(state, &rec.id, &rec.name, rec.version, &def, inputs, trigger_type, event, true).await
}

/// Merge caller inputs with `inputs:` defaults and enforce `required`.
/// Missing required inputs are a validation error (never silently run).
pub fn resolve_inputs(
    def: &WorkflowDefinition,
    provided: HashMap<String, serde_json::Value>,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let mut merged: HashMap<String, serde_json::Value> = HashMap::new();
    for (name, decl) in &def.inputs {
        if let Some(default) = &decl.default {
            merged.insert(name.clone(), default.clone());
        }
    }
    for (k, v) in provided {
        merged.insert(k, v);
    }
    for (name, decl) in &def.inputs {
        if decl.required == Some(true) {
            match merged.get(name) {
                Some(serde_json::Value::Null) | None => {
                    return Err(AppError::Validation(format!(
                        "missing required input '{name}'"
                    )))
                }
                Some(serde_json::Value::String(s)) if s.trim().is_empty() => {
                    return Err(AppError::Validation(format!(
                        "missing required input '{name}'"
                    )))
                }
                _ => {}
            }
        }
    }
    Ok(merged)
}

/// Build inputs for linked (automatic) runs: fill workflow-declared inputs
/// from the trigger event when names match (e.g. input `file_path` gets the
/// event's file path). Explicit values win; defaults apply via resolve_inputs.
fn event_inputs_for_linked(
    def: &WorkflowDefinition,
    event: &HashMap<String, String>,
) -> HashMap<String, serde_json::Value> {
    let mut out = HashMap::new();
    for name in def.inputs.keys() {
        if let Some(v) = event.get(name) {
            out.insert(name.clone(), serde_json::Value::String(v.clone()));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
async fn run_definition(
    state: &Arc<AppState>,
    workflow_id: &str,
    workflow_name: &str,
    workflow_version: i32,
    def: &WorkflowDefinition,
    inputs: HashMap<String, serde_json::Value>,
    trigger_type: &str,
    event: HashMap<String, String>,
    is_manual: bool,
) -> Result<String, AppError> {
    // Validate before every run (spec 6.4: validate before save AND run).
    let report = validator::validate(def)?;
    if !report.valid {
        return Err(AppError::Validation(format!(
            "invalid workflow: {}",
            report.errors.join("; ")
        )));
    }
    if !def.workflow.enabled && !is_manual {
        return Err(AppError::Validation("workflow is disabled".into()));
    }
    // trigger.input semantics: `inputs:` declares runtime inputs.
    // Apply defaults and enforce required before anything else.
    let inputs = resolve_inputs(def, inputs)?;
    let settings = def.effective_settings();
    // Policy ranges (defense in depth; validator already checked).
    if settings.default_timeout_seconds == 0 || settings.default_timeout_seconds > 86_400 {
        return Err(AppError::Validation("timeout out of range".into()));
    }

    // Deduplication reuses the existing trigger-event key scheme (spec 7.3).
    if settings.deduplication_enabled {
        let window = Duration::from_secs(settings.deduplication_window_seconds);
        let mut key = format!("wf:{workflow_id}|{trigger_type}");
        let mut event_keys: Vec<String> = event.iter().map(|(k, v)| format!("{k}={v}")).collect();
        event_keys.sort();
        key.push('|');
        key.push_str(&event_keys.join(","));
        if !state.scheduler.check_dedup(&key, window).await {
            // Record a skipped run so history shows the dedup (consistent with legacy).
            let job_id = Uuid::new_v4().to_string();
            let now = Utc::now();
            repository::insert_history(
                &state.pool,
                repository::HistoryRecord {
                    id: Uuid::new_v4().to_string(),
                    job_id: job_id.clone(),
                    automation_id: String::new(),
                    automation_name: workflow_name.to_string(),
                    trigger_type: trigger_type.into(),
                    status: "skipped".into(),
                    attempt: 0,
                    duration_ms: Some(0),
                    result: None,
                    error: Some("deduplicated".into()),
                    started_at: now,
                    finished_at: Some(now),
                },
            )
            .await?;
            mark_workflow_columns(&state.pool, &job_id, workflow_id, workflow_version, def).await?;
            return Ok(job_id);
        }
    }

    // Overlap policy (spec 7.3): skip/queue. parallel is deferred.
    let overlap_key = format!("wf:{workflow_id}");
    match settings.overlap_policy {
        crate::workflow::model::OverlapName::Skip => {
            if state.scheduler.is_running(&overlap_key).await {
                let job_id = Uuid::new_v4().to_string();
                let now = Utc::now();
                repository::insert_history(
                    &state.pool,
                    repository::HistoryRecord {
                        id: Uuid::new_v4().to_string(),
                        job_id: job_id.clone(),
                        automation_id: String::new(),
                        automation_name: workflow_name.to_string(),
                        trigger_type: trigger_type.into(),
                        status: "skipped".into(),
                        attempt: 0,
                        duration_ms: Some(0),
                        result: None,
                        error: Some("skipped: previous workflow run still running".into()),
                        started_at: now,
                        finished_at: Some(now),
                    },
                )
                .await?;
                mark_workflow_columns(&state.pool, &job_id, workflow_id, workflow_version, def).await?;
                return Ok(job_id);
            }
            state.scheduler.mark_running(&overlap_key).await;
        }
        crate::workflow::model::OverlapName::Queue => {
            state.scheduler.mark_running(&overlap_key).await;
        }
    }

    // Snapshot the definition at start (spec 3.2.6).
    let snapshot = yaml_codec::serialize(def)?;
    let job_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let history_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO execution_history (id, job_id, automation_id, automation_name, trigger_type, status, attempt, duration_ms, result, error, started_at, finished_at, workflow_id, workflow_version, definition_snapshot_yaml, cancel_requested) VALUES (?, ?, ?, ?, ?, 'running', 1, NULL, NULL, NULL, ?, NULL, ?, ?, ?, 0)")
        .bind(&history_id)
        .bind(&job_id)
        .bind("")
        .bind(workflow_name)
        .bind(trigger_type)
        .bind(now.to_rfc3339())
        .bind(workflow_id)
        .bind(workflow_version as i64)
        .bind(&snapshot)
        .execute(&state.pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))?;

    // Background execution on the shared Tokio runtime (Job Queue / Worker Pool
    // role). The Tauri command returns the job_id without waiting (spec 10.1).
    // No DB transaction is held across process waits (spec 7.2).
    let bg_state = Arc::clone(state);
    let bg_def = def.clone();
    let bg_job_id = job_id.clone();
    let bg_overlap_key = overlap_key.clone();
    let bg_trigger = trigger_type.to_string();
    tokio::spawn(async move {
        let mut ctx = RunContext {
            job_id: bg_job_id.clone(),
            trigger_type: bg_trigger,
            inputs,
            event,
            outputs: HashMap::new(),
        };
        let settings = bg_def.effective_settings();
        let result = engine::run_graph(
            &bg_state.pool,
            &bg_job_id,
            &bg_def,
            &mut ctx,
            settings.default_timeout_seconds,
            settings.default_max_attempts.max(1),
            &bg_state.security,
        )
        .await;
        if let Err(e) = result {
            // run_graph already finalizes the job row on node failure; this
            // covers unexpected engine-level errors.
            let _ = sqlx::query("UPDATE execution_history SET status='failed', error=?, finished_at=? WHERE job_id=?")
                .bind(e.to_string())
                .bind(Utc::now().to_rfc3339())
                .bind(&bg_job_id)
                .execute(&bg_state.pool)
                .await;
        }
        bg_state.scheduler.mark_finished(&bg_overlap_key).await;
    });

    Ok(job_id)
}

async fn mark_workflow_columns(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    workflow_id: &str,
    version: i32,
    def: &WorkflowDefinition,
) -> Result<(), AppError> {
    let snapshot = yaml_codec::serialize(def)?;
    sqlx::query("UPDATE execution_history SET workflow_id=?, workflow_version=?, definition_snapshot_yaml=? WHERE job_id=?")
        .bind(workflow_id)
        .bind(version as i64)
        .bind(&snapshot)
        .bind(job_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

/// Trigger adapter (spec 8): fan out from an existing automation trigger to
/// linked workflows. Called by the automation execution path AFTER the legacy
/// action run is queued, so both share the same trigger event. When no
/// workflow is linked, this is a no-op (no double execution, spec T4-2).
pub async fn run_linked_workflows_for_automation(
    state: &Arc<AppState>,
    automation: &crate::core::models::AutomationDefinition,
    trigger_type: &str,
    context: &crate::services::variables::EventContext,
) -> Vec<String> {
    let linked = workflow_repository::workflows_for_automation(&state.pool, &automation.id)
        .await
        .unwrap_or_default();
    let mut job_ids = Vec::new();
    for wf in linked {
        // Manual gating does not apply here; disabled workflows never auto-start.
        if !wf.enabled {
            continue;
        }
        let def: Result<WorkflowDefinition, _> = yaml_codec::parse(&wf.definition_yaml);
        let Ok(def) = def else { continue };
        let event = event_map_from_legacy(
            &context.file_path,
            &context.file_name,
            &context.file_name_without_ext,
            &context.folder_path,
            &context.folder_name,
            &context.extension,
            &context.timestamp,
            &context.date,
            &context.time,
            &context.automation_id,
            &context.job_id,
            context.attempt,
        );
        // Linked runs are automatic (is_manual=false) so disabled is rejected.
        // Auto-fill declared inputs from the event (e.g. input `file_path`).
        let linked_inputs = event_inputs_for_linked(&def, &event);
        match run_definition(
            state,
            &wf.id,
            &wf.name,
            wf.version,
            &def,
            linked_inputs,
            trigger_type,
            event,
            false,
        )
        .await
        {
            Ok(job_id) => job_ids.push(job_id),
            Err(_) => continue,
        }
    }
    job_ids
}

pub async fn cancel_run(state: &AppState, job_id: &str) -> Result<(), AppError> {
    // Verify the job exists (workflow or legacy). Legacy cancel is a no-op
    // acknowledge (existing behavior); workflow sets cooperative flag.
    let detail = workflow_run_repository::get_run_detail(&state.pool, job_id).await;
    match detail {
        Ok(_) => workflow_run_repository::request_cancel(&state.pool, job_id).await,
        Err(_) => {
            // Fall back: is it a legacy history row?
            let history = repository::list_history(&state.pool, 20).await?;
            if history.iter().any(|h| h.job_id == job_id) {
                Ok(())
            } else {
                Err(AppError::NotFound(format!("job '{job_id}'")))
            }
        }
    }
}

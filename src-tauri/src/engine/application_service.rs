use super::{condition_engine, retry};
use crate::{
    actions,
    core::{
        models::{AutomationDefinition, OverlapPolicy},
        validation::validate_automation,
    },
    services::{security::SecurityPolicy, variables::EventContext},
    storage::repository,
    AppError,
};
use chrono::Utc;
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct SchedulerRegistry {
    tasks: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    watcher: Mutex<Option<tokio::task::JoinHandle<()>>>,
    running: Mutex<HashSet<String>>,
    dedup: Mutex<HashMap<String, Instant>>,
}

impl SchedulerRegistry {
    pub async fn mark_running(&self, automation_id: &str) -> bool {
        let mut guard = self.running.lock().await;
        if guard.contains(automation_id) {
            return false;
        }
        guard.insert(automation_id.to_string());
        true
    }

    pub async fn mark_finished(&self, automation_id: &str) {
        self.running.lock().await.remove(automation_id);
    }

    pub async fn is_running(&self, automation_id: &str) -> bool {
        self.running.lock().await.contains(automation_id)
    }

    /// Deduplication check. Returns true when the event is accepted,
    /// false when it falls inside the dedup window and must be skipped.
    pub async fn check_dedup(&self, key: &str, window: Duration) -> bool {
        if window.is_zero() {
            return true;
        }
        let mut guard = self.dedup.lock().await;
        let now = Instant::now();
        if let Some(last) = guard.get(key) {
            if now.duration_since(*last) < window {
                return false;
            }
        }
        guard.insert(key.to_string(), now);
        // Opportunistic cleanup to bound memory.
        if guard.len() > 4096 {
            let cutoff = now - window - Duration::from_secs(60);
            guard.retain(|_, instant| *instant > cutoff);
        }
        true
    }

    pub async fn insert_task(&self, automation_id: String, handle: tokio::task::JoinHandle<()>) {
        let mut guard = self.tasks.lock().await;
        if let Some(old) = guard.insert(automation_id, handle) {
            old.abort();
        }
    }

    pub async fn remove_task(&self, automation_id: &str) {
        let mut guard = self.tasks.lock().await;
        if let Some(handle) = guard.remove(automation_id) {
            handle.abort();
        }
    }

    /// Replace the singleton file-watcher task, aborting the previous one so
    /// events are never processed twice (which would create duplicate jobs
    /// and duplicate/skipped history rows).
    pub async fn replace_watcher(&self, handle: tokio::task::JoinHandle<()>) {
        let mut guard = self.watcher.lock().await;
        if let Some(old) = guard.replace(handle) {
            old.abort();
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub security: SecurityPolicy,
    pub scheduler: Arc<SchedulerRegistry>,
}

impl AppState {
    pub fn new(pool: SqlitePool, security: SecurityPolicy) -> Self {
        Self {
            pool,
            security,
            scheduler: Arc::new(SchedulerRegistry::default()),
        }
    }
}

pub async fn create_automation(
    state: &AppState,
    automation: AutomationDefinition,
) -> Result<AutomationDefinition, AppError> {
    validate_automation(&automation)?;
    // Security validation hooks: executables must pass the allowlist and
    // trigger paths must resolve inside allowed roots when configured.
    for action in &automation.actions {
        match action {
            crate::core::models::ActionDefinition::Command { executable, .. } => {
                state.security.validate_executable(executable)?;
            }
            crate::core::models::ActionDefinition::Python { .. } => {
                let interpreter = if cfg!(windows) { "python" } else { "python3" };
                state.security.validate_executable(interpreter)?;
            }
            crate::core::models::ActionDefinition::Shell { .. } => {
                let interpreter = if cfg!(windows) { "cmd" } else { "sh" };
                state.security.validate_executable(interpreter)?;
            }
            crate::core::models::ActionDefinition::Copy { source, destination }
            | crate::core::models::ActionDefinition::Move { source, destination } => {
                // Lexical validation only; existence is checked at execution
                // time. Fail only when the path is outside allowed roots and
                // an allowlist is configured.
                if !state.security.allowed_paths.is_empty() {
                    state
                        .security
                        .validate_path(std::path::Path::new(source))?;
                    state
                        .security
                        .validate_path(std::path::Path::new(destination))?;
                }
            }
        }
    }
    repository::save_automation(&state.pool, &automation).await?;
    Ok(automation)
}

/// Manual execution entry point. Allowed even when `enabled == false`
/// (spec #13.3): only automatic triggers are gated by `enabled`.
pub async fn run_automation(
    state: &AppState,
    automation: &AutomationDefinition,
) -> Result<String, AppError> {
    let job_id = Uuid::new_v4().to_string();
    let context = EventContext {
        automation_id: automation.id.clone(),
        job_id: job_id.clone(),
        timestamp: Utc::now().to_rfc3339(),
        date: Utc::now().format("%Y-%m-%d").to_string(),
        time: Utc::now().format("%H:%M:%S").to_string(),
        ..Default::default()
    };
    run_automation_with_context(state, automation, "manual", context).await
}

pub async fn run_automation_with_context(
    state: &AppState,
    automation: &AutomationDefinition,
    trigger_type: &str,
    mut context: EventContext,
) -> Result<String, AppError> {
    validate_automation(automation)?;
    let job_id = if context.job_id.is_empty() {
        let id = Uuid::new_v4().to_string();
        context.job_id = id.clone();
        id
    } else {
        context.job_id.clone()
    };
    context.automation_id = automation.id.clone();
    let started = Utc::now();

    // Condition evaluation (spec #26 manual flow, #85 file flow).
    for condition in &automation.conditions {
        let matched = condition_engine::evaluate_condition(condition, &context)?;
        if !matched {
            repository::insert_history(
                &state.pool,
                repository::HistoryRecord {
                    id: String::new(),
                    job_id: job_id.clone(),
                    automation_id: automation.id.clone(),
                    automation_name: automation.name.clone(),
                    trigger_type: trigger_type.into(),
                    status: "skipped".into(),
                    attempt: 0,
                    duration_ms: Some((Utc::now() - started).num_milliseconds()),
                    result: None,
                    error: Some("condition did not match".into()),
                    started_at: started,
                    finished_at: Some(Utc::now()),
                },
            )
            .await?;
            return Ok(job_id);
        }
    }

    // Deduplication (spec #40).
    if automation.execution.deduplication.enabled {
        let window = Duration::from_secs(automation.execution.deduplication.window_seconds);
        let key = format!(
            "{}|{}|{}|{}",
            automation.id, trigger_type, context.file_path, context.folder_path
        );
        if !state.scheduler.check_dedup(&key, window).await {
            repository::insert_history(
                &state.pool,
                repository::HistoryRecord {
                    id: String::new(),
                    job_id: job_id.clone(),
                    automation_id: automation.id.clone(),
                    automation_name: automation.name.clone(),
                    trigger_type: trigger_type.into(),
                    status: "skipped".into(),
                    attempt: 0,
                    duration_ms: Some((Utc::now() - started).num_milliseconds()),
                    result: None,
                    error: Some("deduplicated".into()),
                    started_at: started,
                    finished_at: Some(Utc::now()),
                },
            )
            .await?;
            return Ok(job_id);
        }
    }

    // Overlap policy (spec #24, #39). Default is Skip.
    match automation.execution.concurrency.overlap_policy {
        OverlapPolicy::Skip => {
            if state.scheduler.is_running(&automation.id).await {
                repository::insert_history(
                    &state.pool,
                    repository::HistoryRecord {
                        id: String::new(),
                        job_id: job_id.clone(),
                        automation_id: automation.id.clone(),
                        automation_name: automation.name.clone(),
                        trigger_type: trigger_type.into(),
                        status: "skipped".into(),
                        attempt: 0,
                        duration_ms: Some((Utc::now() - started).num_milliseconds()),
                        result: None,
                        error: Some("skipped: previous job still running".into()),
                        started_at: started,
                        finished_at: Some(Utc::now()),
                    },
                )
                .await?;
                return Ok(job_id);
            }
            state.scheduler.mark_running(&automation.id).await;
        }
        OverlapPolicy::Queue => {
            // Queue allows the tick to proceed; per-automation serialization
            // beyond this is handled by the worker/queue layer. For the
            // current direct-execution path we still track running state so
            // UI can report Running, but do not skip.
            state.scheduler.mark_running(&automation.id).await;
        }
    }

    let result = execute_with_retry(state, automation, &context).await;
    state.scheduler.mark_finished(&automation.id).await;

    let (final_status, attempt, error_message) = result;
    // (The `result` column is otherwise unused in V1.)
    let trigger_target: Option<String> = match trigger_type {
        "file_created" | "file_modified" if !context.file_path.is_empty() => {
            Some(context.file_path.clone())
        }
        "folder_created" if !context.folder_path.is_empty() => {
            Some(context.folder_path.clone())
        }
        _ => None,
    };
    repository::insert_history(
        &state.pool,
        repository::HistoryRecord {
            id: String::new(),
            job_id: job_id.clone(),
            automation_id: automation.id.clone(),
            automation_name: automation.name.clone(),
            trigger_type: trigger_type.into(),
            status: final_status.into(),
            attempt,
            duration_ms: Some((Utc::now() - started).num_milliseconds()),
            result: trigger_target,
            error: error_message,
            started_at: started,
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    // Workflow trigger fan-out (spec 8): linked workflows share the same
    // trigger event. No link => no extra run (no unintended duplication).
    // Linked runs are background jobs; failures here never fail the legacy job.
    {
        let linked_state = Arc::new(AppState {
            pool: state.pool.clone(),
            security: state.security.clone(),
            scheduler: Arc::clone(&state.scheduler),
        });
        let _ = crate::services::workflow_service::run_linked_workflows_for_automation(
            &linked_state,
            automation,
            trigger_type,
            &context,
        )
        .await;
    }
    Ok(job_id)
}

async fn execute_with_retry(
    state: &AppState,
    automation: &AutomationDefinition,
    context: &EventContext,
) -> (String, u32, Option<String>) {
    let mut attempt: u32 = 1;
    loop {
        let mut timed_out = false;
        let mut attempt_error: Option<String> = None;
        for action in &automation.actions {
            let mut action_context = context.clone();
            action_context.attempt = attempt;
            match actions::execute(
                action,
                &action_context,
                Duration::from_secs(automation.execution.timeout_seconds),
                &state.security,
            )
            .await
            {
                Ok(result) if result.status == "success" => {}
                Ok(result) => {
                    attempt_error = Some(if result.stderr.is_empty() {
                        format!("action failed (exit {:?})", result.exit_code)
                    } else {
                        result.stderr
                    });
                    break;
                }
                Err(error) => {
                    timed_out = matches!(error, AppError::Timeout);
                    attempt_error = Some(error.to_string());
                    break;
                }
            }
        }
        if attempt_error.is_none() {
            return ("success".to_string(), attempt, None);
        }
        // Retryable errors only: never retry validation/security/config errors.
        let retryable = match &attempt_error {
            Some(message)
                if message.contains("validation error")
                    || message.contains("security error") =>
            {
                false
            }
            _ => true,
        };
        if !retryable
            || !retry::should_retry(&automation.execution.retry, attempt, timed_out)
        {
            let status = if timed_out { "timeout" } else { "failed" };
            return (status.to_string(), attempt, attempt_error);
        }
        tokio::time::sleep(retry::retry_delay(&automation.execution.retry, attempt)).await;
        attempt += 1;
    }
}

pub type SharedState = Arc<AppState>;

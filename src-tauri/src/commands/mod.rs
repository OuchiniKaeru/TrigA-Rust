use std::sync::Arc;

use crate::{
    core::models::AutomationDefinition,
    engine::application_service::{self, AppState},
    storage::repository,
};

fn state_error(error: crate::AppError) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn list_automations(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<AutomationDefinition>, String> {
    repository::list_automations(&state.pool)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn get_automation(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<AutomationDefinition, String> {
    repository::get_automation(&state.pool, &id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn create_automation(
    state: tauri::State<'_, Arc<AppState>>,
    automation: AutomationDefinition,
) -> Result<AutomationDefinition, String> {
    let created = application_service::create_automation(&state, automation)
        .await
        .map_err(state_error)?;
    let state_ref: Arc<AppState> = (*state).clone();
    crate::triggers::scheduler::start_automation(state_ref.clone(), created.clone()).await;
    let _ = crate::triggers::watcher::start(state_ref).await;
    Ok(created)
}

#[tauri::command]
pub async fn update_automation(
    state: tauri::State<'_, Arc<AppState>>,
    automation: AutomationDefinition,
) -> Result<AutomationDefinition, String> {
    let updated = application_service::create_automation(&state, automation)
        .await
        .map_err(state_error)?;
    let state_ref: Arc<AppState> = (*state).clone();
    crate::triggers::scheduler::start_automation(state_ref.clone(), updated.clone()).await;
    let _ = crate::triggers::watcher::start(state_ref).await;
    Ok(updated)
}

#[tauri::command]
pub async fn delete_automation(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    repository::delete_automation(&state.pool, &id)
        .await
        .map_err(state_error)?;
    crate::triggers::scheduler::stop_automation(&state, &id).await;
    let state_ref: Arc<AppState> = (*state).clone();
    let _ = crate::triggers::watcher::start(state_ref).await;
    Ok(())
}

#[tauri::command]
pub async fn enable_automation(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<AutomationDefinition, String> {
    let automation = repository::set_enabled(&state.pool, &id, true)
        .await
        .map_err(state_error)?;
    let state_ref: Arc<AppState> = (*state).clone();
    crate::triggers::scheduler::start_automation(state_ref.clone(), automation.clone()).await;
    let _ = crate::triggers::watcher::start(state_ref).await;
    Ok(automation)
}

#[tauri::command]
pub async fn disable_automation(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<AutomationDefinition, String> {
    let automation = repository::set_enabled(&state.pool, &id, false)
        .await
        .map_err(state_error)?;
    crate::triggers::scheduler::stop_automation(&state, &id).await;
    let state_ref: Arc<AppState> = (*state).clone();
    let _ = crate::triggers::watcher::start(state_ref).await;
    Ok(automation)
}

#[tauri::command]
pub async fn run_automation(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<String, String> {
    let automation = repository::get_automation(&state.pool, &id)
        .await
        .map_err(state_error)?;
    // Manual run is allowed even when disabled (spec #13.3).
    application_service::run_automation(&state, &automation)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn list_history(
    state: tauri::State<'_, Arc<AppState>>,
    limit: Option<u32>,
) -> Result<Vec<repository::HistoryRecord>, String> {
    repository::list_history(&state.pool, limit.unwrap_or(20))
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn list_jobs(
    state: tauri::State<'_, Arc<AppState>>,
    limit: Option<u32>,
) -> Result<Vec<repository::HistoryRecord>, String> {
    repository::list_history(&state.pool, limit.unwrap_or(20))
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn get_job(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<Option<repository::HistoryRecord>, String> {
    let history = repository::list_history(&state.pool, 20)
        .await
        .map_err(state_error)?;
    Ok(history.into_iter().find(|item| item.job_id == id))
}

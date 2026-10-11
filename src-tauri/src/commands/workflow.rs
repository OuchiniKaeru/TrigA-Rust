use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{
    engine::application_service::AppState,
    services::workflow_service,
    storage::{workflow_repository, workflow_run_repository},
    workflow::{node_registry, validator, yaml_codec},
};

fn state_error(error: crate::AppError) -> String {
    error.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkflowInput {
    pub yaml: String,
    pub change_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkflowInput {
    pub id: String,
    pub yaml: String,
    pub expected_version: Option<i32>,
    pub change_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunWorkflowInput {
    pub id: String,
    pub inputs: Option<HashMap<String, serde_json::Value>>,
}

#[tauri::command]
pub async fn list_workflows(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<workflow_repository::WorkflowRecord>, String> {
    workflow_repository::list(&state.pool, None, None)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn get_workflow(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<workflow_repository::WorkflowRecord, String> {
    workflow_repository::get(&state.pool, &id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn create_workflow(
    state: tauri::State<'_, Arc<AppState>>,
    input: CreateWorkflowInput,
) -> Result<workflow_repository::WorkflowRecord, String> {
    if input.yaml.len() > 512 * 1024 {
        return Err("workflow YAML is too large".into());
    }
    workflow_repository::create_from_yaml(&state.pool, &input.yaml, input.change_summary.as_deref())
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn update_workflow(
    state: tauri::State<'_, Arc<AppState>>,
    input: UpdateWorkflowInput,
) -> Result<workflow_repository::WorkflowRecord, String> {
    workflow_repository::update_from_yaml(
        &state.pool,
        &input.id,
        &input.yaml,
        input.expected_version,
        input.change_summary.as_deref(),
    )
    .await
    .map_err(state_error)
}

#[tauri::command]
pub async fn delete_workflow(state: tauri::State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    workflow_repository::delete(&state.pool, &id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn duplicate_workflow(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<workflow_repository::WorkflowRecord, String> {
    workflow_repository::duplicate(&state.pool, &id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn validate_workflow(
    _state: tauri::State<'_, Arc<AppState>>,
    yaml: String,
) -> Result<validator::ValidationReport, String> {
    // Validate without touching the DB (spec 9.5: no save on invalid YAML).
    // Note: ValidationReport must be serializable for Tauri IPC.
    workflow_service::validate_yaml_text(&yaml).map_err(state_error)
}

#[tauri::command]
pub async fn import_workflow_yaml(
    state: tauri::State<'_, Arc<AppState>>,
    yaml: String,
    policy: Option<String>,
) -> Result<workflow_repository::WorkflowRecord, String> {
    // Import alone never executes (spec 9.5 / 15.2 item 9).
    workflow_repository::import_yaml(&state.pool, &yaml, policy.as_deref().unwrap_or("cancel"))
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn export_workflow_yaml(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<String, String> {
    let rec = workflow_repository::get(&state.pool, &id)
        .await
        .map_err(state_error)?;
    // Execution history / logs / secrets are never included: export is the
    // stored definition only (spec 6.2).
    Ok(rec.definition_yaml)
}

#[tauri::command]
pub async fn list_node_types(
    _state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<node_registry::NodeTypeInfo>, String> {
    Ok(node_registry::all_node_types())
}

#[tauri::command]
pub async fn run_workflow(
    state: tauri::State<'_, Arc<AppState>>,
    input: RunWorkflowInput,
) -> Result<String, String> {
    let state_ref: Arc<AppState> = (*state).clone();
    workflow_service::run_workflow(
        &state_ref,
        &input.id,
        input.inputs.unwrap_or_default(),
        "manual",
        HashMap::new(),
    )
    .await
    .map_err(state_error)
}

#[tauri::command]
pub async fn cancel_workflow_run(
    state: tauri::State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    workflow_service::cancel_run(&state, &job_id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn get_workflow_run(
    state: tauri::State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<workflow_run_repository::WorkflowRunDetail, String> {
    workflow_run_repository::get_run_detail(&state.pool, &job_id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn list_workflow_runs(
    state: tauri::State<'_, Arc<AppState>>,
    workflow_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<crate::storage::repository::HistoryRecord>, String> {
    workflow_run_repository::list_workflow_runs(&state.pool, workflow_id.as_deref(), limit.unwrap_or(20))
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn get_workflow_node_runs(
    state: tauri::State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<Vec<workflow_run_repository::NodeRunRecord>, String> {
    workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn get_workflow_versions(
    state: tauri::State<'_, Arc<AppState>>,
    workflow_id: String,
) -> Result<Vec<workflow_repository::WorkflowVersionRecord>, String> {
    workflow_repository::list_versions(&state.pool, &workflow_id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn restore_workflow_version(
    state: tauri::State<'_, Arc<AppState>>,
    workflow_id: String,
    version: i32,
) -> Result<workflow_repository::WorkflowRecord, String> {
    workflow_repository::restore_version(&state.pool, &workflow_id, version)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn enable_workflow(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<workflow_repository::WorkflowRecord, String> {
    workflow_repository::set_enabled(&state.pool, &id, true)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn disable_workflow(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<workflow_repository::WorkflowRecord, String> {
    workflow_repository::set_enabled(&state.pool, &id, false)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn list_workflow_triggers(
    state: tauri::State<'_, Arc<AppState>>,
    workflow_id: String,
) -> Result<Vec<workflow_repository::WorkflowTriggerRecord>, String> {
    workflow_repository::list_triggers(&state.pool, &workflow_id)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn attach_workflow_trigger(
    state: tauri::State<'_, Arc<AppState>>,
    workflow_id: String,
    automation_id: String,
) -> Result<workflow_repository::WorkflowTriggerRecord, String> {
    workflow_repository::attach_automation_trigger(&state.pool, &workflow_id, &automation_id, true)
        .await
        .map_err(state_error)
}

#[tauri::command]
pub async fn detach_workflow_trigger(
    state: tauri::State<'_, Arc<AppState>>,
    trigger_link_id: String,
) -> Result<(), String> {
    workflow_repository::detach_trigger(&state.pool, &trigger_link_id)
        .await
        .map_err(state_error)
}

/// Serialize both YAML and validation in one call for the YAML editor
/// (Validate → Apply flow, spec 9.5).
#[tauri::command]
pub async fn parse_workflow_yaml(
    _state: tauri::State<'_, Arc<AppState>>,
    yaml: String,
) -> Result<serde_json::Value, String> {
    let def = yaml_codec::parse(&yaml).map_err(state_error)?;
    serde_json::to_value(&def).map_err(|e| e.to_string())
}

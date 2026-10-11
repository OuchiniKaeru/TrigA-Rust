use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    engine::application_service::AppState,
    services::workflow_service,
    storage::{workflow_repository, workflow_run_repository},
    workflow::{node_registry, validator, yaml_codec},
};

#[derive(Debug, Deserialize, Default)]
pub struct WorkflowListQuery {
    pub enabled: Option<bool>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RunQuery {
    pub trigger_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct YamlBody {
    pub yaml: String,
    pub change_summary: Option<String>,
    pub expected_version: Option<i32>,
    pub policy: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunBody {
    pub inputs: Option<HashMap<String, serde_json::Value>>,
    pub trigger_type: Option<String>,
    pub event: Option<HashMap<String, String>>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/workflows", get(list_workflows).post(create_workflow))
        .route("/api/workflows/parse", post(parse_workflow))
        .route("/api/workflows/validate-yaml", post(validate_yaml_body))
        .route(
            "/api/workflows/{id}",
            get(get_workflow).put(update_workflow).delete(delete_workflow),
        )
        .route("/api/workflows/{id}/enable", post(enable_workflow))
        .route("/api/workflows/{id}/disable", post(disable_workflow))
        .route("/api/workflows/{id}/duplicate", post(duplicate_workflow))
        .route("/api/workflows/{id}/validate", post(validate_workflow))
        .route("/api/workflows/{id}/run", post(run_workflow))
        .route("/api/workflows/{id}/export", get(export_workflow))
        .route("/api/workflows/{id}/versions", get(list_versions))
        .route(
            "/api/workflows/{id}/versions/{version}/restore",
            post(restore_version),
        )
        .route(
            "/api/workflows/{id}/triggers",
            get(list_triggers).post(attach_trigger),
        )
        .route("/api/workflow-triggers/{link_id}", axum::routing::delete(detach_trigger))
        .route("/api/workflows/import", post(import_workflow))
        .route("/api/workflow-runs", get(list_runs))
        .route("/api/workflow-runs/{job_id}", get(get_run))
        .route("/api/workflow-runs/{job_id}/nodes", get(get_nodes))
        .route("/api/workflow-runs/{job_id}/cancel", post(cancel_run))
        .route("/api/schema/workflow", get(workflow_schema))
        .route("/api/schema/node-types", get(node_types))
}

async fn list_workflows(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkflowListQuery>,
) -> impl IntoResponse {
    match workflow_repository::list(&state.pool, query.enabled, query.search.as_deref()).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn get_workflow(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> impl IntoResponse {
    match workflow_repository::get(&state.pool, &id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn create_workflow(
    State(state): State<Arc<AppState>>,
    Json(body): Json<YamlBody>,
) -> impl IntoResponse {
    if body.yaml.len() > 512 * 1024 {
        return super::error_response(crate::AppError::Validation("YAML too large".into()));
    }
    match workflow_repository::create_from_yaml(&state.pool, &body.yaml, body.change_summary.as_deref())
        .await
    {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn update_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<YamlBody>,
) -> impl IntoResponse {
    match workflow_repository::update_from_yaml(
        &state.pool,
        &id,
        &body.yaml,
        body.expected_version,
        body.change_summary.as_deref(),
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn delete_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::delete(&state.pool, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn enable_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::set_enabled(&state.pool, &id, true).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn disable_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::set_enabled(&state.pool, &id, false).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn duplicate_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::duplicate(&state.pool, &id).await {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn validate_workflow(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Validate the STORED definition.
    let rec = match workflow_repository::get(&_state.pool, &id).await {
        Ok(value) => value,
        Err(error) => return super::error_response(error),
    };
    match yaml_codec::parse(&rec.definition_yaml) {
        Ok(def) => match validator::validate(&def) {
            Ok(report) => Json(report).into_response(),
            Err(error) => super::error_response(error),
        },
        Err(error) => super::error_response(error),
    }
}

/// Parse arbitrary YAML into a definition JSON without persisting anything.
/// The GUI editor uses this for a faithful YAML round-trip instead of
/// client-side YAML parsing.
async fn parse_workflow(Json(body): Json<YamlBody>) -> impl IntoResponse {
    if body.yaml.len() > 512 * 1024 {
        return super::error_response(crate::AppError::Validation("YAML too large".into()));
    }
    match yaml_codec::parse(&body.yaml) {
        Ok(def) => match serde_json::to_value(&def) {
            Ok(value) => Json(value).into_response(),
            Err(error) => super::error_response(crate::AppError::Storage(error.to_string())),
        },
        Err(error) => super::error_response(error),
    }
}

/// Validate arbitrary YAML without persisting anything (Validate-before-Apply).
async fn validate_yaml_body(Json(body): Json<YamlBody>) -> impl IntoResponse {
    if body.yaml.len() > 512 * 1024 {
        return super::error_response(crate::AppError::Validation("YAML too large".into()));
    }
    match yaml_codec::parse(&body.yaml) {
        Ok(def) => match validator::validate(&def) {
            Ok(report) => Json(report).into_response(),
            Err(error) => super::error_response(error),
        },
        Err(error) => super::error_response(error),
    }
}

async fn run_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<Option<RunBody>>,
) -> impl IntoResponse {
    let body = body.unwrap_or(RunBody {
        inputs: None,
        trigger_type: None,
        event: None,
    });
    // GUI and API share the same validation (spec 11).
    match workflow_service::run_workflow(
        &state,
        &id,
        body.inputs.unwrap_or_default(),
        body.trigger_type.as_deref().unwrap_or("manual"),
        body.event.unwrap_or_default(),
    )
    .await
    {
        Ok(job_id) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"job_id": job_id})),
        )
            .into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn export_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::get(&state.pool, &id).await {
        Ok(rec) => (
            StatusCode::OK,
            [("content-type", "application/x-yaml")],
            rec.definition_yaml,
        )
            .into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn list_versions(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::list_versions(&state.pool, &id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn restore_version(
    State(state): State<Arc<AppState>>,
    Path((id, version)): Path<(String, i32)>,
) -> impl IntoResponse {
    match workflow_repository::restore_version(&state.pool, &id, version).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct AttachBody {
    pub automation_id: String,
}

async fn list_triggers(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::list_triggers(&state.pool, &id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn attach_trigger(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<AttachBody>,
) -> impl IntoResponse {
    match workflow_repository::attach_automation_trigger(&state.pool, &id, &body.automation_id, true)
        .await
    {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn detach_trigger(
    State(state): State<Arc<AppState>>,
    Path(link_id): Path<String>,
) -> impl IntoResponse {
    match workflow_repository::detach_trigger(&state.pool, &link_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn import_workflow(
    State(state): State<Arc<AppState>>,
    Json(body): Json<YamlBody>,
) -> impl IntoResponse {
    // Import alone never executes (spec 9.5).
    match workflow_repository::import_yaml(
        &state.pool,
        &body.yaml,
        body.policy.as_deref().unwrap_or("cancel"),
    )
    .await
    {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(error) => super::error_response(error),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct RunListQuery {
    pub workflow_id: Option<String>,
    pub limit: Option<u32>,
}

async fn list_runs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RunListQuery>,
) -> impl IntoResponse {
    match workflow_run_repository::list_workflow_runs(
        &state.pool,
        query.workflow_id.as_deref(),
        query.limit.unwrap_or(20),
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn get_run(State(state): State<Arc<AppState>>, Path(job_id): Path<String>) -> impl IntoResponse {
    match workflow_run_repository::get_run_detail(&state.pool, &job_id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn get_nodes(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match workflow_run_repository::list_node_runs(&state.pool, &job_id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn cancel_run(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match workflow_service::cancel_run(&state, &job_id).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"status": "cancelled"})),
        )
            .into_response(),
        Err(error) => super::error_response(error),
    }
}

async fn workflow_schema() -> impl IntoResponse {
    Json(serde_json::json!({
        "schema_version": 1,
        "required": ["schema_version", "workflow.id", "workflow.name", "workflow.enabled", "nodes", "edges"],
        "node_types": node_registry::all_node_types().iter().map(|n| &n.r#type).collect::<Vec<_>>(),
        "trigger_input": {
            "declare": "top-level `inputs:` maps names to {description, default, required}",
            "run": "POST /api/workflows/{id}/run with {\"inputs\": {\"<name>\": value}}; defaults apply, required enforced",
            "refs": ["workflow.input.<name>", "nodes.<trigger_input_id>.output.<name>"],
            "linked_runs": "linked automation triggers auto-fill matching input names from the file/schedule event"
        },
        "node_outputs": {
            "command.exec": ["stdout", "stderr", "exit_code"],
            "python.script": ["stdout", "stderr", "exit_code"],
            "shell.exec": ["stdout", "stderr", "exit_code"],
            "file.list": ["files", "path"],
            "file.copy": ["source", "destination"],
            "file.move": ["source", "destination"],
            "file.wait_ready": ["path", "size", "stable_seconds", "waited_ms"],
            "logic.condition": ["branch"],
            "logic.switch": ["branch", "value"],
            "notification.log": ["message", "level"],
            "notify.desktop": ["delivered", "title", "message"],
            "http.request": ["status_code", "headers", "body", "body_json", "is_success", "url", "elapsed_ms"],
            "data.transform": ["result", "steps"],
            "flow.delay": ["waited_ms", "requested_ms"],
            "flow.for_each": ["count", "items", "item", "index"],
            "flow.parallel": ["branches"],
            "trigger.input": "dynamic: declared input names",
            "trigger.url_changed": ["url", "status_code", "body", "hash", "changed", "bytes"]
        },
        "branches": {
            "logic.condition": "ports true/false; non-taken path becomes skipped",
            "logic.switch": "config {value, cases: [{equals, port}], default_port}; taken port runs, others become skipped",
            "flow.for_each": "ports next (body per item) / done (after loop); loop vars nodes.<id>.output.item|index|count",
            "flow.parallel": "every port except done is a branch; done is the join continuation (v1: branches run sequentially in port order)"
        },
        "references": ["workflow.input.<name>", "workflow.context.job_id", "workflow.context.trigger_type", "workflow.context.event.*", "nodes.<node_id>.output.<name>"],
        "execution": {
            "timeout_seconds": {"min": 1, "max": 86400},
            "max_attempts": {"min": 1, "max": 10},
            "max_workers": {"min": 1, "max": 32},
            "overlap_policy": ["skip", "queue"],
            "failure_policy": ["stop"]
        }
    }))
}

async fn node_types() -> impl IntoResponse {
    Json(node_registry::all_node_types())
}

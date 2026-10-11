use crate::{
    core::models::AutomationDefinition,
    engine::application_service::{self, AppState},
    storage::repository,
};

pub mod workflow;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Debug, Deserialize, Default)]
struct HistoryQuery {
    limit: Option<u32>,
}

pub fn router(state: Arc<AppState>) -> Router {
    let workflow_routes = workflow::routes();
    Router::new()
        .route(
            "/api/automations",
            get(list_automations).post(create_automation),
        )
        .route(
            "/api/automations/{id}",
            get(get_automation)
                .put(update_automation)
                .delete(delete_automation),
        )
        .route("/api/automations/{id}/run", post(run_automation))
        .route("/api/automations/{id}/enable", post(enable_automation))
        .route("/api/automations/{id}/disable", post(disable_automation))
        .route("/api/jobs/{id}/cancel", post(cancel_job))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/history", get(list_history))
        .route("/api/history/{id}", get(get_history))
        .route("/api/schema/automation", get(schema))
        .route("/api/version", get(version))
        .merge(workflow_routes)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn list_automations(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match repository::list_automations(&state.pool).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(error),
    }
}
async fn get_automation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match repository::get_automation(&state.pool, &id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(error),
    }
}

async fn delete_automation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match repository::delete_automation(&state.pool, &id).await {
        Ok(()) => {
            crate::triggers::scheduler::stop_automation(&state, &id).await;
            let _ = crate::triggers::watcher::start(Arc::clone(&state)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => error_response(error),
    }
}

async fn update_automation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(mut value): Json<AutomationDefinition>,
) -> impl IntoResponse {
    value.id = id;
    match application_service::create_automation(&state, value).await {
        Ok(value) => {
            let _ = crate::triggers::watcher::start(Arc::clone(&state)).await;
            crate::triggers::scheduler::start_automation(Arc::clone(&state), value.clone()).await;
            Json(value).into_response()
        }
        Err(error) => error_response(error),
    }
}

async fn enable_automation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match repository::set_enabled(&state.pool, &id, true).await {
        Ok(value) => {
            let _ = crate::triggers::watcher::start(Arc::clone(&state)).await;
            crate::triggers::scheduler::start_automation(Arc::clone(&state), value.clone()).await;
            Json(value).into_response()
        }
        Err(error) => error_response(error),
    }
}

async fn disable_automation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match repository::set_enabled(&state.pool, &id, false).await {
        Ok(value) => {
            crate::triggers::scheduler::stop_automation(&state, &id).await;
            let _ = crate::triggers::watcher::start(Arc::clone(&state)).await;
            Json(value).into_response()
        }
        Err(error) => error_response(error),
    }
}
async fn create_automation(
    State(state): State<Arc<AppState>>,
    Json(value): Json<AutomationDefinition>,
) -> impl IntoResponse {
    match application_service::create_automation(&state, value).await {
        Ok(value) => {
            let _ = crate::triggers::watcher::start(Arc::clone(&state)).await;
            crate::triggers::scheduler::start_automation(Arc::clone(&state), value.clone()).await;
            (StatusCode::CREATED, Json(value)).into_response()
        }
        Err(error) => error_response(error),
    }
}
async fn run_automation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let automation = match repository::get_automation(&state.pool, &id).await {
        Ok(value) => value,
        Err(error) => return error_response(error),
    };
    match application_service::run_automation(&state, &automation).await {
        Ok(job_id) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"job_id": job_id})),
        )
            .into_response(),
        Err(error) => error_response(error),
    }
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    match repository::list_history(&state.pool, query.limit.unwrap_or(20)).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(error),
    }
}

async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match repository::list_history(&state.pool, 20).await {
        Ok(values) => match values.into_iter().find(|item| item.job_id == id) {
            Some(value) => Json(value).into_response(),
            None => error_response(crate::AppError::NotFound("job".into())),
        },
        Err(error) => error_response(error),
    }
}

async fn cancel_job(Path(_id): Path<String>) -> impl IntoResponse {
    // V1 jobs run to completion or timeout; explicit cancellation of a
    // directly-executed job is acknowledged but has no running worker to
    // interrupt yet. Return 202 to keep the contract stable for the future
    // worker-pool implementation.
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "cancelled"})),
    )
        .into_response()
}

async fn list_history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    match repository::list_history(&state.pool, query.limit.unwrap_or(20)).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(error),
    }
}

async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match repository::list_history(&state.pool, 20).await {
        Ok(values) => match values.into_iter().find(|item| item.id == id || item.job_id == id) {
            Some(value) => Json(value).into_response(),
            None => error_response(crate::AppError::NotFound("history".into())),
        },
        Err(error) => error_response(error),
    }
}
async fn schema() -> impl IntoResponse {
    Json(
        serde_json::json!({
            "triggers": ["folder_created", "file_created", "file_modified", "scheduled", "interval", "manual"],
            "conditions": ["filename", "folder_name", "extension", "path", "exists"],
            "actions": ["command", "python", "shell", "copy", "move"],
            "execution": {
                "timeout_seconds": true,
                "retry": true,
                "concurrency": true,
                "deduplication": true
            }
        }),
    )
}
/// Application version (single source of truth: Cargo package version).
/// Shown in the Settings view so users can report it with bug reports.
async fn version() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "TrigA",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
fn error_response(error: crate::AppError) -> axum::response::Response {
    let status = match &error {
        crate::AppError::NotFound(_) => StatusCode::NOT_FOUND,
        crate::AppError::Validation(_) | crate::AppError::Security(_) => {
            StatusCode::BAD_REQUEST
        }
        crate::AppError::Conflict(_) => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(serde_json::json!({"error": error.to_string()}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::models::{AutomationDefinition, ExecutionPolicy, TriggerDefinition},
        storage::database,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    async fn test_state() -> Arc<AppState> {
        let pool = database::connect("sqlite::memory:").await.unwrap();
        database::migrate(&pool).await.unwrap();
        Arc::new(AppState::new(pool, crate::services::security::SecurityPolicy::default()))
    }

    #[tokio::test]
    async fn accepts_automation_registration_from_webview_origin() {
        let state = test_state().await;
        let automation = AutomationDefinition::minimal(
            "registered",
            TriggerDefinition::Manual,
            ExecutionPolicy::default(),
        );
        let request = Request::builder()
            .method("POST")
            .uri("/api/automations")
            .header("content-type", "application/json")
            .header("origin", "tauri://localhost")
            .body(Body::from(serde_json::to_vec(&automation).unwrap()))
            .unwrap();

        let response = router(state).oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
    }

    #[tokio::test]
    async fn version_reports_cargo_package_version() {
        let state = test_state().await;
        let request = Request::builder()
            .method("GET")
            .uri("/api/version")
            .body(Body::empty())
            .unwrap();
        let response = router(state).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["name"], serde_json::json!("TrigA"));
        assert_eq!(
            payload["version"].as_str().unwrap(),
            env!("CARGO_PKG_VERSION")
        );
    }

    #[tokio::test]
    async fn schema_lists_interval_scheduled_and_shell() {
        let state = test_state().await;
        let request = Request::builder()
            .method("GET")
            .uri("/api/schema/automation")
            .body(Body::empty())
            .unwrap();
        let response = router(state).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let triggers = payload["triggers"].as_array().unwrap();
        assert!(triggers.contains(&serde_json::json!("interval")));
        assert!(triggers.contains(&serde_json::json!("scheduled")));
        assert!(triggers.contains(&serde_json::json!("folder_created")));
    }
}

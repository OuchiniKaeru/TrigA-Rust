pub mod actions;
pub mod api;
pub mod commands;
pub mod core;
pub mod engine;
pub mod services;
pub mod storage;
pub mod triggers;

use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation error: {0}")]
    Validation(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("security error: {0}")]
    Security(String),
    #[error("action error: {0}")]
    Action(String),
    #[error("process error: {0}")]
    Process(String),
    #[error("action timed out")]
    Timeout,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("trigger error: {0}")]
    Trigger(String),
}

pub fn run() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
    let state = runtime.block_on(async {
        std::fs::create_dir_all("data").expect("failed to create data directory");
        let pool = storage::database::connect("sqlite://data/automation.db?mode=rwc")
            .await
            .expect("failed to connect to SQLite");
        storage::database::migrate(&pool)
            .await
            .expect("failed to migrate SQLite");
        Arc::new(engine::application_service::AppState::new(
            pool,
            services::security::SecurityPolicy::default(),
        ))
    });
    let api_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let api_runtime = tokio::runtime::Runtime::new().expect("failed to create API runtime");
        api_runtime.block_on(async move {
            let _ = triggers::watcher::start(Arc::clone(&api_state)).await;
            let _ = triggers::scheduler::start(Arc::clone(&api_state)).await;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:47821")
                .await
                .expect("failed to bind localhost API on 127.0.0.1:47821");
            axum::serve(listener, api::router(api_state))
                .await
                .expect("localhost API server stopped unexpectedly");
        });
    });
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::list_automations,
            commands::get_automation,
            commands::create_automation,
            commands::update_automation,
            commands::delete_automation,
            commands::enable_automation,
            commands::disable_automation,
            commands::run_automation,
            commands::list_history,
            commands::list_jobs,
            commands::get_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Automation Manager");
}

#[cfg(test)]
mod validation_tests {
    use super::core::models::{AutomationDefinition, ExecutionPolicy, TriggerDefinition};
    use super::core::validation::validate_automation;

    #[test]
    fn rejects_zero_timeout() {
        let automation = AutomationDefinition::minimal(
            "invalid",
            TriggerDefinition::Manual,
            ExecutionPolicy {
                timeout_seconds: 0,
                ..ExecutionPolicy::default()
            },
        );

        assert!(validate_automation(&automation).is_err());
    }
}

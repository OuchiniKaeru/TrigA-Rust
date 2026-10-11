use crate::{
    core::models::ActionDefinition,
    services::{security::SecurityPolicy, variables::EventContext},
    workflow::context::RunContext,
    AppError,
};
use std::time::Duration;

/// Shared helpers for node executors.
pub fn node_timeout(config: &serde_json::Value, default_secs: u64) -> Duration {
    let secs = config
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(default_secs)
        .clamp(1, 86_400);
    Duration::from_secs(secs)
}

pub fn node_max_attempts(config: &serde_json::Value, default_attempts: u32) -> u32 {
    // config.retry.max_attempts > config.max_attempts > default
    if let Some(retry) = config.get("retry").and_then(|v| v.as_object()) {
        if let Some(m) = retry.get("max_attempts").and_then(|v| v.as_u64()) {
            return (m as u32).clamp(1, 10);
        }
    }
    if let Some(m) = config.get("max_attempts").and_then(|v| v.as_u64()) {
        return (m as u32).clamp(1, 10);
    }
    default_attempts.clamp(1, 10)
}

/// Convert resolved args value into Vec<String>.
pub fn to_string_args(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(m) if m.contains_key("value") => match &m["value"] {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                },
                other => other.to_string().trim_matches('"').to_string(),
            })
            .collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        _ => vec![],
    }
}

/// Minimal EventContext bridge so existing actions::execute can be reused
/// with workflow-resolved values (spec 2.2: reuse Action Engine).
pub fn bridge_context(ctx: &RunContext) -> EventContext {
    let get = |k: &str| ctx.event.get(k).cloned().unwrap_or_default();
    EventContext {
        file_path: get("file_path"),
        file_name: get("file_name"),
        file_name_without_ext: get("file_name_without_ext"),
        folder_path: get("folder_path"),
        folder_name: get("folder_name"),
        extension: get("extension"),
        timestamp: get("timestamp"),
        date: get("date"),
        time: get("time"),
        automation_id: get("automation_id"),
        job_id: ctx.job_id.clone(),
        attempt: 1,
    }
}

pub fn action_from_command(program: String, args: Vec<String>) -> ActionDefinition {
    ActionDefinition::Command {
        executable: program,
        args,
    }
}

pub async fn check_security_executable(
    security: &SecurityPolicy,
    executable: &str,
) -> Result<(), AppError> {
    security.validate_executable(executable)
}

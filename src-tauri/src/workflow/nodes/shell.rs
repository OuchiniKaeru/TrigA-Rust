use super::common;
use crate::{
    actions,
    core::models::ActionDefinition,
    services::security::SecurityPolicy,
    workflow::context::RunContext,
    AppError,
};
use std::collections::HashMap;

/// shell.exec: reuse existing shell action with explicit danger.
/// GUI must display a warning (spec 12.2); execution path is identical.
pub async fn execute(
    config: &serde_json::Value,
    ctx: &RunContext,
    default_timeout_secs: u64,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let command = resolved
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Validation("shell.exec: command is required".into()))?
        .to_string();
    if command.trim().is_empty() {
        return Err(AppError::Validation("shell.exec: command must not be empty".into()));
    }
    let timeout = common::node_timeout(&resolved, default_timeout_secs);
    let action = ActionDefinition::Shell { command };
    let result = actions::execute(&action, &common::bridge_context(ctx), timeout, security).await?;
    if result.status != "success" {
        let msg = if result.stderr.is_empty() {
            format!("shell failed (exit {:?})", result.exit_code)
        } else {
            crate::workflow::model::truncate_text(&result.stderr, 4000)
        };
        return Err(AppError::Action(msg));
    }
    let mut out = HashMap::new();
    out.insert("stdout".into(), serde_json::Value::String(result.stdout));
    out.insert("stderr".into(), serde_json::Value::String(result.stderr));
    out.insert(
        "exit_code".into(),
        result.exit_code.map(|c| c.into()).unwrap_or(serde_json::Value::Null),
    );
    Ok(out)
}

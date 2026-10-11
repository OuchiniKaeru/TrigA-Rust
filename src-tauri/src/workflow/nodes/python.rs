use super::common;
use crate::{
    actions,
    core::models::ActionDefinition,
    services::security::SecurityPolicy,
    workflow::context::RunContext,
    AppError,
};
use std::collections::HashMap;

/// python.script: reuse existing python execution.
pub async fn execute(
    config: &serde_json::Value,
    ctx: &RunContext,
    default_timeout_secs: u64,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let script = resolved
        .get("script")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Validation("python.script: script is required".into()))?
        .to_string();
    if script.trim().is_empty() {
        return Err(AppError::Validation("python.script: script must not be empty".into()));
    }
    let args = resolved
        .get("args")
        .map(common::to_string_args)
        .unwrap_or_default();
    let timeout = common::node_timeout(&resolved, default_timeout_secs);
    let action = ActionDefinition::Python { script, args };
    let result = actions::execute(&action, &common::bridge_context(ctx), timeout, security).await?;
    if result.status != "success" {
        let msg = if result.stderr.is_empty() {
            format!("python failed (exit {:?})", result.exit_code)
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

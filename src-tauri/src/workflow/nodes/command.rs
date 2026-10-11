use super::common;
use crate::{
    actions,
    services::security::SecurityPolicy,
    workflow::context::RunContext,
    AppError,
};
use std::collections::HashMap;

/// command.exec: program + args executed WITHOUT shell (spec 12.1).
pub async fn execute(
    config: &serde_json::Value,
    ctx: &RunContext,
    default_timeout_secs: u64,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx
        .resolve(config)
        .map_err(AppError::Validation)?;
    let program = resolved
        .get("program")
        .or_else(|| resolved.get("executable"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Validation("command.exec: program is required".into()))?
        .to_string();
    if program.trim().is_empty() {
        return Err(AppError::Validation("command.exec: program must not be empty".into()));
    }
    security.validate_executable(&program)?;
    let args = resolved
        .get("args")
        .map(common::to_string_args)
        .unwrap_or_default();
    let timeout = common::node_timeout(&resolved, default_timeout_secs);
    let action = common::action_from_command(program.clone(), args.clone());
    let result = actions::execute(&action, &common::bridge_context(ctx), timeout, security).await?;
    if result.status != "success" {
        let msg = if result.stderr.is_empty() {
            format!("command failed (exit {:?})", result.exit_code)
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

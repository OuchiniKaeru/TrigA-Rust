use crate::{workflow::context::RunContext, AppError};
use std::collections::HashMap;

/// notification.log: records a log message as node output (existing logging).
pub fn execute(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let message = match resolved.get("message") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => String::new(),
    };
    let level = resolved
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("info")
        .to_string();
    let mut out = HashMap::new();
    out.insert("message".into(), serde_json::Value::String(message));
    out.insert("level".into(), serde_json::Value::String(level));
    Ok(out)
}

/// notify.desktop: OS desktop notification (Linux/Windows/macOS).
/// Best-effort: headless environments return delivered=false instead of
/// failing the workflow (use notification.log for guaranteed history).
/// Config: {title?: string|$ref (default "TrigA"), message: string|$ref,
///   level?: info|warning|error|success, timeout_ms?: 5000}
/// Outputs: {delivered: bool, title, message}
pub fn execute_desktop(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let title = match resolved.get("title") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {
            crate::workflow::model::truncate_text(s.trim(), 120)
        }
        _ => "TrigA".to_string(),
    };
    let message = match resolved.get("message") {
        Some(serde_json::Value::String(s)) => {
            crate::workflow::model::truncate_text(s, 500)
        }
        Some(other) => crate::workflow::model::truncate_text(
            &serde_json::to_string(other).unwrap_or_default(),
            500,
        ),
        None => String::new(),
    };
    if message.trim().is_empty() {
        return Err(AppError::Validation(
            "notify.desktop: message is required".into(),
        ));
    }
    let level = resolved
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("info")
        .to_lowercase();
    if !matches!(
        level.as_str(),
        "info" | "warning" | "error" | "success"
    ) {
        return Err(AppError::Validation(format!(
            "notify.desktop: unknown level '{level}'"
        )));
    }
    let delivered = try_notify(&title, &message, &level);
    let mut out = HashMap::new();
    out.insert("delivered".into(), serde_json::Value::Bool(delivered));
    out.insert("title".into(), serde_json::Value::String(title));
    out.insert("message".into(), serde_json::Value::String(message));
    Ok(out)
}

fn try_notify(title: &str, message: &str, level: &str) -> bool {
    let mut n = notify_rust::Notification::new();
    n.summary(title).body(message);
    match level {
        "error" => {
            n.urgency(notify_rust::Urgency::Critical);
        }
        "warning" => {
            n.urgency(notify_rust::Urgency::Normal);
        }
        _ => {
            n.urgency(notify_rust::Urgency::Low);
        }
    }
    // Timeouts/DBUS errors are non-fatal for the workflow.
    n.show().is_ok()
}

use crate::{workflow::context::RunContext, AppError};
use std::collections::HashMap;

/// flow.delay: wait a fixed time (async sleep, cancellable via engine).
/// Config: {seconds: number|$ref (0..86400), milliseconds?: number|$ref}
/// Outputs: {waited_ms, requested_ms}
pub async fn execute_delay(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let seconds = resolved
        .get("seconds")
        .map(|v| match v {
            serde_json::Value::Number(n) => n.as_f64().ok_or_else(|| {
                AppError::Validation("flow.delay: seconds must be a number".into())
            }),
            serde_json::Value::String(s) => s.parse::<f64>().map_err(|_| {
                AppError::Validation("flow.delay: seconds must be a number".into())
            }),
            _ => Err(AppError::Validation(
                "flow.delay: seconds must be a number".into(),
            )),
        })
        .transpose()?
        .unwrap_or(0.0);
    let millis_extra = resolved
        .get("milliseconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as f64;
    if !seconds.is_finite() || seconds < 0.0 || seconds > 86_400.0 {
        return Err(AppError::Validation(
            "flow.delay: seconds must be 0..86400".into(),
        ));
    }
    if millis_extra > 86_400_000.0 {
        return Err(AppError::Validation(
            "flow.delay: milliseconds is too large".into(),
        ));
    }
    let total_ms = (seconds * 1000.0 + millis_extra).round() as u64;
    if total_ms > 86_400_000 {
        return Err(AppError::Validation(
            "flow.delay: total wait must not exceed 24h".into(),
        ));
    }
    let started = std::time::Instant::now();
    // Async sleep: the Tokio worker thread is NOT blocked; the engine's
    // cooperative-cancel race still applies for long waits.
    tokio::time::sleep(std::time::Duration::from_millis(total_ms)).await;
    let waited = started.elapsed().as_millis() as i64;
    let mut out = HashMap::new();
    out.insert("waited_ms".into(), waited.into());
    out.insert("requested_ms".into(), (total_ms as i64).into());
    Ok(out)
}

/// Resolve for_each items (shared by validator + engine).
pub fn resolve_items(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<Vec<serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let items = resolved
        .get("items")
        .ok_or_else(|| AppError::Validation("flow.for_each: items is required".into()))?;
    match items {
        serde_json::Value::Array(arr) => {
            let max = resolved
                .get("max_items")
                .and_then(|v| v.as_u64())
                .unwrap_or(100);
            if max == 0 || max > 1000 {
                return Err(AppError::Validation(
                    "flow.for_each: max_items must be 1..1000".into(),
                ));
            }
            if arr.len() as u64 > max {
                return Err(AppError::Validation(format!(
                    "flow.for_each: {} items exceed max_items={max}",
                    arr.len()
                )));
            }
            Ok(arr.clone())
        }
        _ => Err(AppError::Validation(
            "flow.for_each: items must be an array (use data.transform or file.list output)".into(),
        )),
    }
}

/// Validate parallel config. Returns (wait_for_all, max_branches).
pub fn parallel_policy(config: &serde_json::Value) -> Result<(bool, u64), AppError> {
    let wait = config
        .get("wait_for_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let max = config
        .get("max_branches")
        .and_then(|v| v.as_u64())
        .unwrap_or(8);
    if max == 0 || max > 32 {
        return Err(AppError::Validation(
            "flow.parallel: max_branches must be 1..32".into(),
        ));
    }
    Ok((wait, max))
}

use crate::{core::models::ConditionDefinition, engine::condition_engine, workflow::context::RunContext, AppError};

/// logic.condition: returns the selected output port ("true" / "false").
/// Supports:
///   {left: $ref|value, operator: eq|ne|gt|gte|lt|lte|contains|starts_with|ends_with|exists, right: value}
///   {condition: {type: filename|folder_name|extension|path|exists, ...}} (legacy engine)
pub fn evaluate(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<String, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    if let Some(cond) = resolved.get("condition") {
        return evaluate_legacy(cond, ctx);
    }
    let left = resolved.get("left").cloned().unwrap_or(serde_json::Value::Null);
    let operator = resolved
        .get("operator")
        .and_then(|v| v.as_str())
        .unwrap_or("eq")
        .to_lowercase();
    let right = resolved.get("right").cloned().unwrap_or(serde_json::Value::Null);
    let result = match operator.as_str() {
        "eq" | "==" | "equals" => json_eq(&left, &right),
        "ne" | "!=" | "not_equals" => !json_eq(&left, &right),
        "gt" | ">" => json_num_cmp(&left, &right).is_some_and(|o| o == std::cmp::Ordering::Greater),
        "gte" | ">=" => json_num_cmp(&left, &right).is_some_and(|o| o != std::cmp::Ordering::Less),
        "lt" | "<" => json_num_cmp(&left, &right).is_some_and(|o| o == std::cmp::Ordering::Less),
        "lte" | "<=" => json_num_cmp(&left, &right).is_some_and(|o| o != std::cmp::Ordering::Greater),
        "contains" => to_str(&left).contains(to_str(&right).as_str()),
        "starts_with" => to_str(&left).starts_with(to_str(&right).as_str()),
        "ends_with" => to_str(&left).ends_with(to_str(&right).as_str()),
        "exists" => {
            let path = to_str(&left);
            let expected = match &right {
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::String(s) => s == "true",
                _ => true,
            };
            std::path::Path::new(&path).exists() == expected
        }
        other => return Err(AppError::Validation(format!("unknown condition operator '{other}'"))),
    };
    Ok(if result { "true".into() } else { "false".into() })
}

fn evaluate_legacy(cond: &serde_json::Value, ctx: &RunContext) -> Result<String, AppError> {
    let def: ConditionDefinition = serde_json::from_value(cond.clone())
        .map_err(|e| AppError::Validation(format!("invalid legacy condition: {e}")))?;
    let event = crate::services::variables::EventContext {
        file_path: ctx.event.get("file_path").cloned().unwrap_or_default(),
        file_name: ctx.event.get("file_name").cloned().unwrap_or_default(),
        file_name_without_ext: ctx.event.get("file_name_without_ext").cloned().unwrap_or_default(),
        folder_path: ctx.event.get("folder_path").cloned().unwrap_or_default(),
        folder_name: ctx.event.get("folder_name").cloned().unwrap_or_default(),
        extension: ctx.event.get("extension").cloned().unwrap_or_default(),
        ..Default::default()
    };
    let matched = condition_engine::evaluate_condition(&def, &event)?;
    Ok(if matched { "true".into() } else { "false".into() })
}

fn to_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn json_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => {
            x.as_f64() == y.as_f64()
        }
        _ => to_str(a) == to_str(b),
    }
}

fn json_num_cmp(a: &serde_json::Value, b: &serde_json::Value) -> Option<std::cmp::Ordering> {
    let x = to_num(a)?;
    let y = to_num(b)?;
    x.partial_cmp(&y)
}

fn to_num(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        serde_json::Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

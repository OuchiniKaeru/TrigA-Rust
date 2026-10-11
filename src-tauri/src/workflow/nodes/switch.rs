use crate::{workflow::context::RunContext, AppError};

/// logic.switch: multi-branch routing by value equality.
/// Config: { value: <ref|literal>, cases: [{equals, port}], default_port?: string }
/// Returns the selected output port name. Comparison is string-based
/// (numbers/bools are stringified, matching logic.condition semantics).
pub fn evaluate(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<(String, String), AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let value = resolved.get("value").cloned().unwrap_or(serde_json::Value::Null);
    let value_str = to_str(&value);
    let cases = resolved
        .get("cases")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::Validation("logic.switch: cases array is required".into()))?;
    for case in cases {
        let port = case
            .get("port")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if port.is_empty() {
            continue;
        }
        let expected = case.get("equals").cloned().unwrap_or(serde_json::Value::Null);
        if value_str == to_str(&expected) {
            return Ok((port, value_str));
        }
    }
    let default_port = resolved
        .get("default_port")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    Ok((default_port, value_str))
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

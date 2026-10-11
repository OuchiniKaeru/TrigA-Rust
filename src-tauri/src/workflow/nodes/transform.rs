use crate::{workflow::context::RunContext, workflow::model::truncate_text, AppError};
use std::collections::HashMap;

/// data.transform: convert data between nodes without arbitrary code.
/// Config: EITHER a single operation object OR {operations: [...]}.
/// Each operation has `op` plus params. Operations run in order; each step's
/// result becomes the next step's `value` when `value` is omitted.
///
/// Supported ops:
/// - replace: {value, search, replace, all?:bool(default true)}
/// - template: {template, vars?: {k: v}} — replaces {{k}} with vars[k]
/// - extract: {value, path: "a.b.0.c"} — dot-path into JSON (arrays by index)
/// - filter: {value: array, where: {field?, equals?|contains?|starts_with?|ends_with?|gt?|lt?}} —
///   keep array items matching the predicate (object items use `field`, else the item itself)
/// - map: {value: array, field: "name"} — collect field from each object item
/// - join: {value: array, separator?: string} — join stringified items
/// - split: {value: string, separator: string, limit?: u64}
/// - trim | upper | lower: {value: string}
/// - date_format: {value: string, input_format?: strptime|RFC3339(auto), output_format: "%Y-%m-%d %H:%M"}
/// Outputs: {result} (final value), {steps: n}
pub fn execute(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let ops: Vec<serde_json::Value> = if resolved.get("op").is_some() {
        vec![resolved.clone()]
    } else if let Some(arr) = resolved.get("operations").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        return Err(AppError::Validation(
            "data.transform: provide an operation object or {operations: [...]}".into(),
        ));
    };
    if ops.is_empty() || ops.len() > 32 {
        return Err(AppError::Validation(
            "data.transform: operations must have 1..32 entries".into(),
        ));
    }
    let mut current = resolved.get("value").cloned().unwrap_or(serde_json::Value::Null);
    let mut last_was_explicit = resolved.get("value").is_some();
    for (i, op) in ops.iter().enumerate() {
        let name = op
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Validation(format!("data.transform: operations[{i}] needs 'op'"))
            })?;
        // `value` omitted in a step inherits the previous result (chaining).
        let input = if op.get("value").is_some() {
            op.get("value").cloned().unwrap()
        } else if last_was_explicit || i > 0 {
            current.clone()
        } else {
            serde_json::Value::Null
        };
        current = apply_op(name, op, &input, i)?;
        last_was_explicit = true;
    }
    // Safety cap on output size.
    let capped = cap_value(&current, 64_000);
    let mut out = HashMap::new();
    out.insert("result".into(), capped);
    out.insert("steps".into(), serde_json::json!(ops.len()));
    Ok(out)
}

fn apply_op(
    name: &str,
    op: &serde_json::Value,
    input: &serde_json::Value,
    i: usize,
) -> Result<serde_json::Value, AppError> {
    let err = |msg: &str| AppError::Validation(format!("data.transform: operations[{i}] {msg}"));
    match name {
        "replace" => {
            let search = op.get("search").and_then(|v| v.as_str()).ok_or_else(|| err("needs string 'search'"))?;
            let replace = op.get("replace").and_then(|v| v.as_str()).unwrap_or("");
            let all = op.get("all").and_then(|v| v.as_bool()).unwrap_or(true);
            let s = to_str(input);
            Ok(serde_json::Value::String(if all {
                s.replace(search, replace)
            } else {
                s.replacen(search, replace, 1)
            }))
        }
        "template" => {
            let template = op.get("template").and_then(|v| v.as_str()).ok_or_else(|| err("needs string 'template'"))?;
            let vars = op.get("vars").and_then(|v| v.as_object());
            let mut out = template.to_string();
            if let Some(vars) = vars {
                for (k, v) in vars {
                    out = out.replace(&format!("{{{{{k}}}}}"), &to_str(v));
                }
            }
            // {{value}} refers to the chained input.
            out = out.replace("{{value}}", &to_str(input));
            Ok(serde_json::Value::String(out))
        }
        "extract" => {
            let path = op.get("path").and_then(|v| v.as_str()).ok_or_else(|| err("needs string 'path' like 'a.b.0.c'"))?;
            extract_path(input, path).ok_or_else(|| err(&format!("path '{path}' not found")))
        }
        "filter" => {
            let arr = input.as_array().ok_or_else(|| err("'value' must be an array"))?;
            let cond = op.get("where").and_then(|v| v.as_object()).ok_or_else(|| err("needs 'where' condition"))?;
            let field = cond.get("field").and_then(|v| v.as_str());
            let mut kept = Vec::new();
            for item in arr {
                let target = match field {
                    Some(f) => item.get(f).unwrap_or(&serde_json::Value::Null).clone(),
                    None => item.clone(),
                };
                if predicate_matches(&target, cond) {
                    kept.push(item.clone());
                }
            }
            Ok(serde_json::Value::Array(kept))
        }
        "map" => {
            let arr = input.as_array().ok_or_else(|| err("'value' must be an array"))?;
            let field = op.get("field").and_then(|v| v.as_str()).ok_or_else(|| err("needs string 'field'"))?;
            Ok(serde_json::Value::Array(
                arr.iter().map(|item| item.get(field).cloned().unwrap_or(serde_json::Value::Null)).collect(),
            ))
        }
        "join" => {
            let arr = input.as_array().ok_or_else(|| err("'value' must be an array"))?;
            let sep = op.get("separator").and_then(|v| v.as_str()).unwrap_or("\n");
            Ok(serde_json::Value::String(arr.iter().map(to_str).collect::<Vec<_>>().join(sep)))
        }
        "split" => {
            let sep = op.get("separator").and_then(|v| v.as_str()).ok_or_else(|| err("needs string 'separator'"))?;
            let limit = op.get("limit").and_then(|v| v.as_u64()).unwrap_or(1000).clamp(1, 5000) as usize;
            let parts: Vec<serde_json::Value> = to_str(input).split(&sep_to_pattern(sep)).take(limit).map(|s| serde_json::Value::String(s.to_string())).collect();
            Ok(serde_json::Value::Array(parts))
        }
        "trim" => Ok(serde_json::Value::String(to_str(input).trim().to_string())),
        "upper" => Ok(serde_json::Value::String(to_str(input).to_uppercase())),
        "lower" => Ok(serde_json::Value::String(to_str(input).to_lowercase())),
        "date_format" => {
            let out_fmt = op.get("output_format").and_then(|v| v.as_str()).ok_or_else(|| err("needs 'output_format' (strftime)"))?;
            let in_fmt = op.get("input_format").and_then(|v| v.as_str());
            let s = to_str(input);
            let dt = parse_date(&s, in_fmt).ok_or_else(|| err(&format!("cannot parse date '{s}'")))?;
            Ok(serde_json::Value::String(dt.format(out_fmt).to_string()))
        }
        other => Err(err(&format!("unknown op '{other}'"))),
    }
}

fn sep_to_pattern(sep: &str) -> String {
    sep.to_string()
}

fn predicate_matches(target: &serde_json::Value, cond: &serde_json::Map<String, serde_json::Value>) -> bool {
    if let Some(eq) = cond.get("equals") {
        return to_str(target) == to_str(eq);
    }
    if let Some(c) = cond.get("contains").and_then(|v| v.as_str()) {
        return to_str(target).contains(c);
    }
    if let Some(p) = cond.get("starts_with").and_then(|v| v.as_str()) {
        return to_str(target).starts_with(p);
    }
    if let Some(p) = cond.get("ends_with").and_then(|v| v.as_str()) {
        return to_str(target).ends_with(p);
    }
    if let (Some(a), Some(b)) = (to_num(target), cond.get("gt").and_then(to_num)) {
        return a > b;
    }
    if let (Some(a), Some(b)) = (to_num(target), cond.get("lt").and_then(to_num)) {
        return a < b;
    }
    false
}

fn extract_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    if path.trim().is_empty() {
        return Some(value.clone());
    }
    let mut cur = value.clone();
    for seg in path.split('.') {
        if let Ok(idx) = seg.parse::<usize>() {
            cur = cur.as_array()?.get(idx)?.clone();
        } else {
            cur = cur.as_object()?.get(seg)?.clone();
        }
    }
    Some(cur)
}

fn parse_date(s: &str, input_format: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Some(fmt) = input_format {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(naive.and_utc());
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
        }
        return None;
    }
    // Auto: RFC3339, then common fallbacks.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%d", "%Y/%m/%d", "%d/%m/%Y"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(naive.and_utc());
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    // Unix timestamp (seconds).
    if let Ok(ts) = s.trim().parse::<i64>() {
        if (0..4_000_000_000).contains(&ts) {
            return chrono::DateTime::from_timestamp(ts, 0);
        }
    }
    None
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

fn to_num(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        serde_json::Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn cap_value(v: &serde_json::Value, limit: usize) -> serde_json::Value {
    match v {
        serde_json::Value::String(s) => serde_json::Value::String(truncate_text(s, limit)),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items.iter().take(5000).map(|x| cap_value(x, limit / 4 + 1)).collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, val) in map {
                out.insert(k.clone(), cap_value(val, limit / 4 + 1));
            }
            serde_json::Value::Object(out)
        }
        _ => v.clone(),
    }
}

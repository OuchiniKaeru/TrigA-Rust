use crate::{
    services::security::SecurityPolicy, workflow::context::RunContext, AppError,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// trigger.url_changed: fetch a URL and report whether it changed.
/// Usable as a start node (first fetch => changed=true) or mid-graph check.
/// Automatic polling pattern: link an `interval` automation to the workflow
/// and branch on `changed` (see README). A dedicated URL scheduler is a
/// follow-up; this node provides fetch+hash+compare without new infra.
///
/// Config:
///   url: string|$ref (required, http/https)
///   method?: GET|HEAD (default GET)
///   headers?: {k: v}
///   timeout_seconds?: u64
///   max_response_bytes?: u64 (default 1MB, max 10MB)
///   json_path?: "a.b.0" — hash only the extracted sub-value (for APIs)
///   expected_hash?: string — compare against this (else changed=true)
///   previous_body?: $ref — compare against a prior node's body instead
/// Outputs: {url, status_code, body, hash, changed, bytes}
pub async fn execute(
    config: &serde_json::Value,
    ctx: &RunContext,
    default_timeout_secs: u64,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let url_raw = resolved
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Validation("trigger.url_changed: url is required".into()))?;
    let url = security.validate_url(url_raw)?;
    let method = resolved
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_uppercase();
    if !matches!(method.as_str(), "GET" | "HEAD") {
        return Err(AppError::Validation(format!(
            "trigger.url_changed: method must be GET or HEAD (got '{method}')"
        )));
    }
    let timeout_secs = resolved
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(default_timeout_secs)
        .clamp(1, 86_400);
    let max_bytes = resolved
        .get("max_response_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(1_048_576)
        .clamp(1024, 10_485_760) as usize;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| AppError::Action(format!("url_changed: client failed: {e}")))?;
    let mut req = if method == "HEAD" {
        client.head(&url)
    } else {
        client.get(&url)
    };
    if let Some(h) = resolved.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in h {
            req = req.header(k.as_str(), stringify(v).as_str());
        }
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            AppError::Timeout
        } else {
            AppError::Action(format!("url_changed fetch failed: {e}"))
        }
    })?;
    let status = resp.status().as_u16() as i64;
    if !(200..300).contains(&status) {
        return Err(AppError::Action(format!(
            "url_changed: unexpected status {status} for {url}"
        )));
    }
    let bytes = resp.bytes().await.map_err(|e| {
        if e.is_timeout() {
            AppError::Timeout
        } else {
            AppError::Action(format!("url_changed read failed: {e}"))
        }
    })?;
    if bytes.len() > max_bytes {
        return Err(AppError::Validation(format!(
            "url_changed: response too large ({} > {max_bytes})",
            bytes.len()
        )));
    }
    let body_str = String::from_utf8_lossy(&bytes).into_owned();

    // Optional sub-selection before hashing (e.g. watch one API field).
    let watched: String = if let Some(path) = resolved.get("json_path").and_then(|v| v.as_str()) {
        match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(json) => match extract_path(&json, path) {
                Some(sub) => serde_json::to_string(&sub).unwrap_or_default(),
                None => {
                    return Err(AppError::Validation(format!(
                        "url_changed: json_path '{path}' not found"
                    )))
                }
            },
            Err(_) => {
                return Err(AppError::Validation(
                    "url_changed: json_path needs a JSON response".into(),
                ))
            }
        }
    } else {
        body_str.clone()
    };
    let mut hasher = Sha256::new();
    hasher.update(watched.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    // Comparison: expected_hash wins; else previous_body hash; else first-run=true.
    let changed = if let Some(expected) = resolved.get("expected_hash").and_then(|v| v.as_str()) {
        hash != expected.trim()
    } else if let Some(prev) = resolved.get("previous_body") {
        let prev_str = match prev {
            serde_json::Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };
        let mut h = Sha256::new();
        h.update(prev_str.as_bytes());
        hash != format!("{:x}", h.finalize())
    } else {
        true
    };

    let mut out = HashMap::new();
    out.insert("url".into(), serde_json::Value::String(url));
    out.insert("status_code".into(), status.into());
    out.insert(
        "body".into(),
        serde_json::Value::String(crate::workflow::model::truncate_text(&body_str, 32_768)),
    );
    out.insert("hash".into(), serde_json::Value::String(hash));
    out.insert("changed".into(), serde_json::Value::Bool(changed));
    out.insert("bytes".into(), serde_json::json!(bytes.len()));
    Ok(out)
}

fn stringify(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
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

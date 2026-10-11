use crate::{
    services::security::SecurityPolicy, workflow::context::RunContext,
    workflow::model::truncate_text, AppError,
};
use std::{collections::HashMap, time::Duration};

/// http.request: call an external Web API.
/// Config:
///   method: GET|POST|PUT|DELETE|PATCH|HEAD (default GET)
///   url: string | $ref (required, http/https only)
///   headers: {name: value|$ref} (optional)
///   query: {name: value|$ref} (optional, appended to URL)
///   body: value|$ref (optional; objects/arrays sent as JSON, strings as-is)
///   body_json: value|$ref (optional alias, always JSON)
///   timeout_seconds: u64 (optional, default workflow default)
///   max_response_bytes: u64 (optional, default 1MB, max 10MB)
/// Outputs: status_code, headers, body, body_json, is_success, url, elapsed_ms
pub async fn execute(
    config: &serde_json::Value,
    ctx: &RunContext,
    default_timeout_secs: u64,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let method = resolved
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD"
    ) {
        return Err(AppError::Validation(format!(
            "http.request: unsupported method '{method}'"
        )));
    }
    let url_raw = resolved
        .get("url")
        .map(|v| match v {
            serde_json::Value::String(s) => Ok(s.clone()),
            other => Err(AppError::Validation(format!(
                "http.request: url must be a string (got {other})"
            ))),
        })
        .ok_or_else(|| AppError::Validation("http.request: url is required".into()))??;
    let url = security.validate_url(&url_raw)?;

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
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| AppError::Action(format!("http.request: client build failed: {e}")))?;

    let mut req = match method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "HEAD" => client.head(&url),
        _ => unreachable!(),
    };

    if let Some(q) = resolved.get("query").and_then(|v| v.as_object()) {
        let pairs: Vec<(String, String)> = q
            .iter()
            .map(|(k, v)| (k.clone(), stringify(v)))
            .collect();
        req = req.query(&pairs);
    }
    if let Some(h) = resolved.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in h {
            let vs = stringify(v);
            req = req.header(k.as_str(), vs.as_str());
        }
    }
    // Body: body_json wins as JSON; body object/array -> JSON; string -> raw.
    if let Some(bj) = resolved.get("body_json") {
        if !bj.is_null() {
            req = req.json(bj);
        }
    } else if let Some(body) = resolved.get("body") {
        match body {
            serde_json::Value::Null => {}
            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                req = req.json(body);
            }
            serde_json::Value::String(s) => {
                req = req.body(s.clone());
            }
            other => {
                req = req.body(other.to_string());
            }
        }
    }

    let started = std::time::Instant::now();
    let resp = req
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                AppError::Timeout
            } else {
                AppError::Action(format!("http.request failed: {e}"))
            }
        })?;
    let status = resp.status().as_u16() as i64;
    let resp_headers: serde_json::Map<String, serde_json::Value> = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
            )
        })
        .collect();
    let bytes = resp.bytes().await.map_err(|e| {
        if e.is_timeout() {
            AppError::Timeout
        } else {
            AppError::Action(format!("http.request read failed: {e}"))
        }
    })?;
    if bytes.len() > max_bytes {
        return Err(AppError::Validation(format!(
            "http.request: response too large ({} bytes > limit {max_bytes})",
            bytes.len()
        )));
    }
    let elapsed_ms = started.elapsed().as_millis() as i64;
    let body_str = String::from_utf8_lossy(&bytes).into_owned();
    let body_capped = truncate_text(&body_str, 32_768);
    let body_json: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);

    let mut out = HashMap::new();
    out.insert("status_code".into(), status.into());
    out.insert(
        "headers".into(),
        serde_json::Value::Object(resp_headers),
    );
    out.insert("body".into(), serde_json::Value::String(body_capped));
    out.insert("body_json".into(), body_json);
    out.insert(
        "is_success".into(),
        serde_json::Value::Bool((200..300).contains(&status)),
    );
    out.insert("url".into(), serde_json::Value::String(url));
    out.insert("elapsed_ms".into(), elapsed_ms.into());
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

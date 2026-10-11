use crate::{
    services::security::SecurityPolicy,
    workflow::context::RunContext,
    AppError,
};
use std::collections::HashMap;

/// file.* nodes. file.delete is intentionally NOT supported (spec 12.6).
pub async fn execute_list(
    config: &serde_json::Value,
    ctx: &RunContext,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let path_str = match resolved.get("path") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => {
            return Err(AppError::Validation(format!(
                "file.list: path must be a string (got {other})"
            )))
        }
        None => return Err(AppError::Validation("file.list: path is required".into())),
    };
    if path_str.trim().is_empty() {
        return Err(AppError::Validation("file.list: path must not be empty".into()));
    }
    let validated = security.validate_path(std::path::Path::new(&path_str))?;
    let recursive = resolved
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut files = Vec::new();
    collect(&validated, recursive, &mut files, 10_000).await?;
    files.sort();
    let mut out = HashMap::new();
    out.insert(
        "files".into(),
        serde_json::Value::Array(files.into_iter().map(serde_json::Value::String).collect()),
    );
    out.insert("path".into(), serde_json::Value::String(path_str));
    Ok(out)
}

async fn collect(
    dir: &std::path::Path,
    recursive: bool,
    out: &mut Vec<String>,
    limit: usize,
) -> Result<(), AppError> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| AppError::Action(format!("file.list failed: {e}")))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| AppError::Action(e.to_string()))?
    {
        if out.len() >= limit {
            break;
        }
        let path = entry.path();
        out.push(path.to_string_lossy().into_owned());
        if recursive {
            if let Ok(ft) = entry.file_type().await {
                if ft.is_dir() {
                    // Recurse without importing async_recursion: iterative via Box.
                    collect_boxed(path, recursive, out, limit).await?;
                }
            }
        }
    }
    Ok(())
}

async fn collect_boxed(
    path: std::path::PathBuf,
    recursive: bool,
    out: &mut Vec<String>,
    limit: usize,
) -> Result<(), AppError> {
    Box::pin(collect(&path, recursive, out, limit)).await
}

pub async fn execute_copy(
    config: &serde_json::Value,
    ctx: &RunContext,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let (source, destination) = resolve_src_dst(config, ctx)?;
    let src = security.validate_path(std::path::Path::new(&source))?;
    let dst = security.validate_path(std::path::Path::new(&destination))?;
    if let Some(parent) = dst.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Action(e.to_string()))?;
        }
    }
    tokio::fs::copy(&src, &dst)
        .await
        .map_err(|e| AppError::Action(e.to_string()))?;
    let mut out = HashMap::new();
    out.insert("source".into(), serde_json::Value::String(source));
    out.insert("destination".into(), serde_json::Value::String(destination));
    Ok(out)
}

pub async fn execute_move(
    config: &serde_json::Value,
    ctx: &RunContext,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let (source, destination) = resolve_src_dst(config, ctx)?;
    let src = security.validate_path(std::path::Path::new(&source))?;
    let dst = security.validate_path(std::path::Path::new(&destination))?;
    if let Some(parent) = dst.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Action(e.to_string()))?;
        }
    }
    tokio::fs::rename(&src, &dst)
        .await
        .map_err(|e| AppError::Action(e.to_string()))?;
    let mut out = HashMap::new();
    out.insert("source".into(), serde_json::Value::String(source));
    out.insert("destination".into(), serde_json::Value::String(destination));
    Ok(out)
}

/// file.wait_ready: wait until a file exists AND its size/mtime stay
/// unchanged for `stable_seconds`. Prevents reading half-written files.
/// Config: {path, stable_seconds?=2 (1..300), timeout_seconds?=60 (1..86400),
///   check_interval_ms?=500 (100..5000)}
/// Outputs: {path, size, stable_seconds, waited_ms}
pub async fn execute_wait_ready(
    config: &serde_json::Value,
    ctx: &RunContext,
    security: &SecurityPolicy,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let path_str = match resolved.get("path") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => {
            return Err(AppError::Validation(format!(
                "file.wait_ready: path must be a string (got {other})"
            )))
        }
        None => {
            return Err(AppError::Validation(
                "file.wait_ready: path is required".into(),
            ))
        }
    };
    if path_str.trim().is_empty() {
        return Err(AppError::Validation(
            "file.wait_ready: path must not be empty".into(),
        ));
    }
    let validated = security.validate_path(std::path::Path::new(&path_str))?;
    let stable_secs = resolved
        .get("stable_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(2);
    if stable_secs == 0 || stable_secs > 300 {
        return Err(AppError::Validation(
            "file.wait_ready: stable_seconds must be 1..300".into(),
        ));
    }
    let timeout_secs = resolved
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(60);
    if timeout_secs == 0 || timeout_secs > 86_400 {
        return Err(AppError::Validation(
            "file.wait_ready: timeout_seconds must be 1..86400".into(),
        ));
    }
    let interval_ms = resolved
        .get("check_interval_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(500)
        .clamp(100, 5000);
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_secs(timeout_secs);
    let mut last_key: Option<(u64, i64)> = None;
    let mut stable_since: Option<std::time::Instant> = None;
    loop {
        if std::time::Instant::now() > deadline {
            return Err(AppError::Timeout);
        }
        match tokio::fs::metadata(&validated).await {
            Ok(meta) => {
                if !meta.is_file() {
                    return Err(AppError::Validation(format!(
                        "file.wait_ready: not a file: {path_str}"
                    )));
                }
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let key = (meta.len(), mtime);
                let now = std::time::Instant::now();
                if last_key == Some(key) {
                    let since = stable_since.unwrap_or(now);
                    if now.duration_since(since).as_secs() >= stable_secs {
                        let mut out = HashMap::new();
                        out.insert("path".into(), serde_json::Value::String(path_str));
                        out.insert("size".into(), serde_json::json!(meta.len()));
                        out.insert("stable_seconds".into(), serde_json::json!(stable_secs));
                        out.insert(
                            "waited_ms".into(),
                            serde_json::json!(started.elapsed().as_millis() as i64),
                        );
                        return Ok(out);
                    }
                } else {
                    last_key = Some(key);
                    stable_since = Some(now);
                    // A large stable window with an instantly-stable file still
                    // waits the full window by design.
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                last_key = None;
                stable_since = None;
            }
            Err(e) => return Err(AppError::Action(format!("file.wait_ready failed: {e}"))),
        }
        tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
    }
}

fn resolve_src_dst(
    config: &serde_json::Value,
    ctx: &RunContext,
) -> Result<(String, String), AppError> {
    let resolved = ctx.resolve(config).map_err(AppError::Validation)?;
    let source = resolved
        .get("source")
        .map(|v| match v {
            serde_json::Value::String(s) => Ok(s.clone()),
            _ => Err(AppError::Validation("file op: source must be a string".into())),
        })
        .ok_or_else(|| AppError::Validation("file op: source is required".into()))??;
    let destination = resolved
        .get("destination")
        .map(|v| match v {
            serde_json::Value::String(s) => Ok(s.clone()),
            _ => Err(AppError::Validation("file op: destination must be a string".into())),
        })
        .ok_or_else(|| AppError::Validation("file op: destination is required".into()))??;
    if source.trim().is_empty() || destination.trim().is_empty() {
        return Err(AppError::Validation("file op: source/destination must not be empty".into()));
    }
    Ok((source, destination))
}

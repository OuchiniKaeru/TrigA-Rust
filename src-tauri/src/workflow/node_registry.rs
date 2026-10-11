use super::model::WorkflowDefinition;
use crate::AppError;

/// Node type metadata exposed to UI/API (spec 7.1).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeTypeInfo {
    pub r#type: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub icon: String,
    pub input_ports: Vec<PortInfo>,
    pub output_ports: Vec<PortInfo>,
    /// Data outputs available as `nodes.<id>.output.<name>`.
    /// Empty + `dynamic_outputs=true` means any declared input name (trigger.input).
    pub data_outputs: Vec<String>,
    pub dynamic_outputs: bool,
    pub config_schema: serde_json::Value,
    pub supports_timeout_retry: bool,
    pub danger_level: String,
    pub reuses_engine: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortInfo {
    pub name: String,
    pub port_type: String,
    pub required: bool,
}

fn port(name: &str, required: bool) -> PortInfo {
    PortInfo {
        name: name.into(),
        port_type: "any".into(),
        required,
    }
}

fn info(
    t: &str,
    display: &str,
    desc: &str,
    cat: &str,
    inputs: Vec<PortInfo>,
    outputs: Vec<PortInfo>,
    data_outputs: &[&str],
    dynamic_outputs: bool,
    schema: serde_json::Value,
    supports: bool,
    danger: &str,
    reuses: &str,
) -> NodeTypeInfo {
    NodeTypeInfo {
        r#type: t.into(),
        display_name: display.into(),
        description: desc.into(),
        category: cat.into(),
        icon: t.into(),
        input_ports: inputs,
        output_ports: outputs,
        data_outputs: data_outputs.iter().map(|s| s.to_string()).collect(),
        dynamic_outputs,
        config_schema: schema,
        supports_timeout_retry: supports,
        danger_level: danger.into(),
        reuses_engine: reuses.into(),
    }
}

/// Data outputs per node type for `nodes.<id>.output.<name>` refs.
/// `None` means dynamic (trigger.input: any declared input name).
pub fn known_data_outputs(node_type: &str) -> Option<Vec<String>> {
    all_node_types()
        .into_iter()
        .find(|n| n.r#type == node_type)
        .map(|n| {
            if n.dynamic_outputs {
                // Dynamic: caller validates against declared inputs.
                vec![]
            } else {
                n.data_outputs
            }
        })
}

pub fn is_dynamic_outputs(node_type: &str) -> bool {
    all_node_types()
        .iter()
        .find(|n| n.r#type == node_type)
        .is_some_and(|n| n.dynamic_outputs)
}

/// All node types supported by the initial release (spec 3.3 + logic.switch).
pub fn all_node_types() -> Vec<NodeTypeInfo> {
    vec![
        info(
            "trigger.manual",
            "Manual trigger",
            "Manual start of the workflow",
            "Trigger",
            vec![],
            vec![port("next", false)],
            &[],
            false,
            serde_json::json!({"type":"object","properties":{}}),
            false,
            "safe",
            "Manual Run",
        ),
        info(
            "trigger.input",
            "Input",
            "Start node exposing workflow inputs: declare values under top-level `inputs:`, pass them at run time, read via workflow.input.<name> or nodes.<this_id>.output.<name>",
            "Trigger",
            vec![],
            vec![port("next", false)],
            &[],
            true,
            serde_json::json!({"type":"object","properties":{"inputs":{"type":"object"}}}),
            false,
            "safe",
            "new",
        ),
        info(
            "flow.end",
            "End",
            "Workflow end",
            "Flow",
            vec![port("in", true)],
            vec![],
            &[],
            false,
            serde_json::json!({"type":"object","properties":{}}),
            false,
            "safe",
            "new",
        ),
        info(
            "command.exec",
            "Command",
            "Run a program with arguments (no shell). Outputs: stdout, stderr, exit_code",
            "Command",
            vec![port("in", true)],
            vec![port("next", false)],
            &["stdout", "stderr", "exit_code"],
            false,
            serde_json::json!({"type":"object","required":["program"],"properties":{"program":{"type":"string"},"args":{"type":"array"},"workdir":{"type":"string"},"timeout_seconds":{"type":"integer"},"retry":{"type":"object"}}}),
            true,
            "caution",
            "command",
        ),
        info(
            "python.script",
            "Python",
            "Run a Python script. Outputs: stdout, stderr, exit_code",
            "Command",
            vec![port("in", true)],
            vec![port("next", false)],
            &["stdout", "stderr", "exit_code"],
            false,
            serde_json::json!({"type":"object","required":["script"],"properties":{"interpreter":{"type":"string"},"script":{"type":"string"},"args":{"type":"array"},"timeout_seconds":{"type":"integer"},"retry":{"type":"object"}}}),
            true,
            "caution",
            "python",
        ),
        info(
            "shell.exec",
            "Shell",
            "Run via shell (dangerous, explicit only). Outputs: stdout, stderr, exit_code",
            "Command",
            vec![port("in", true)],
            vec![port("next", false)],
            &["stdout", "stderr", "exit_code"],
            false,
            serde_json::json!({"type":"object","required":["command"],"properties":{"command":{"type":"string"},"timeout_seconds":{"type":"integer"}}}),
            true,
            "dangerous",
            "shell",
        ),
        info(
            "file.list",
            "List files",
            "List files in a directory. Outputs: files, path",
            "File",
            vec![port("in", true)],
            vec![port("next", false)],
            &["files", "path"],
            false,
            serde_json::json!({"type":"object","required":["path"],"properties":{"path":{},"recursive":{"type":"boolean"}}}),
            true,
            "safe",
            "File Service",
        ),
        info(
            "file.copy",
            "Copy file",
            "Copy a file. Outputs: source, destination",
            "File",
            vec![port("in", true)],
            vec![port("next", false)],
            &["source", "destination"],
            false,
            serde_json::json!({"type":"object","required":["source","destination"],"properties":{"source":{},"destination":{}}}),
            true,
            "caution",
            "copy",
        ),
        info(
            "file.move",
            "Move file",
            "Move a file. Outputs: source, destination",
            "File",
            vec![port("in", true)],
            vec![port("next", false)],
            &["source", "destination"],
            false,
            serde_json::json!({"type":"object","required":["source","destination"],"properties":{"source":{},"destination":{}}}),
            true,
            "caution",
            "move",
        ),
        info(
            "logic.condition",
            "Condition (branch)",
            "Branch on true/false. Outputs: branch. Connect `true` and `false` ports to different paths.",
            "Logic",
            vec![port("in", true)],
            vec![port("true", false), port("false", false)],
            &["branch"],
            false,
            serde_json::json!({"type":"object","properties":{"left":{},"operator":{"type":"string"},"right":{},"condition":{"type":"object"}}}),
            false,
            "safe",
            "Condition Engine",
        ),
        info(
            "logic.switch",
            "Switch (multi-branch)",
            "Route to one of N ports by value equality. Outputs: branch, value. Config: {value, cases: [{equals, port}], default_port}",
            "Logic",
            vec![port("in", true)],
            vec![port("case1", false), port("case2", false), port("default", false)],
            &["branch", "value"],
            false,
            serde_json::json!({"type":"object","required":["value","cases"],"properties":{"value":{},"cases":{"type":"array"},"default_port":{"type":"string"}}}),
            false,
            "safe",
            "Condition Engine",
        ),
        info(
            "notification.log",
            "Log",
            "Write a log message. Outputs: message, level",
            "Notification",
            vec![port("in", true)],
            vec![port("next", false)],
            &["message", "level"],
            false,
            serde_json::json!({"type":"object","properties":{"message":{},"level":{"type":"string"}}}),
            false,
            "safe",
            "Logging",
        ),
        info(
            "http.request",
            "HTTP Request",
            "Call an external Web API (GET/POST/PUT/DELETE/PATCH/HEAD). Outputs: status_code, headers, body, body_json, is_success, url, elapsed_ms",
            "Web",
            vec![port("in", true)],
            vec![port("next", false)],
            &["status_code", "headers", "body", "body_json", "is_success", "url", "elapsed_ms"],
            false,
            serde_json::json!({"type":"object","required":["url"],"properties":{"method":{"type":"string"},"url":{},"headers":{"type":"object"},"query":{"type":"object"},"body":{},"body_json":{},"timeout_seconds":{"type":"integer"},"max_response_bytes":{"type":"integer"}}}),
            true,
            "caution",
            "new (reqwest)",
        ),
        info(
            "data.transform",
            "Data Transform",
            "Convert data between nodes (replace/template/extract/filter/map/join/split/trim/upper/lower/date_format). No arbitrary code. Outputs: result, steps",
            "Data",
            vec![port("in", true)],
            vec![port("next", false)],
            &["result", "steps"],
            false,
            serde_json::json!({"type":"object","properties":{"op":{"type":"string"},"operations":{"type":"array"},"value":{}}}),
            false,
            "safe",
            "new",
        ),
        info(
            "flow.delay",
            "Delay / Wait",
            "Wait a fixed time (async sleep, cancellable). Outputs: waited_ms, requested_ms",
            "Flow",
            vec![port("in", true)],
            vec![port("next", false)],
            &["waited_ms", "requested_ms"],
            false,
            serde_json::json!({"type":"object","properties":{"seconds":{},"milliseconds":{"type":"integer"}}}),
            false,
            "safe",
            "new",
        ),
        info(
            "file.wait_ready",
            "Wait for File Ready",
            "Wait until a file exists and size/mtime stay unchanged for stable_seconds. Outputs: path, size, stable_seconds, waited_ms",
            "File",
            vec![port("in", true)],
            vec![port("next", false)],
            &["path", "size", "stable_seconds", "waited_ms"],
            false,
            serde_json::json!({"type":"object","required":["path"],"properties":{"path":{},"stable_seconds":{"type":"integer"},"timeout_seconds":{"type":"integer"},"check_interval_ms":{"type":"integer"}}}),
            true,
            "safe",
            "File Service",
        ),
        info(
            "notify.desktop",
            "Notify Desktop",
            "OS desktop notification (best-effort; headless => delivered=false, workflow still succeeds). Outputs: delivered, title, message",
            "Notify",
            vec![port("in", true)],
            vec![port("next", false)],
            &["delivered", "title", "message"],
            false,
            serde_json::json!({"type":"object","required":["message"],"properties":{"title":{},"message":{},"level":{"type":"string"}}}),
            false,
            "safe",
            "new (notify-rust)",
        ),
        info(
            "trigger.url_changed",
            "URL Changed Trigger",
            "Fetch a URL and report changed (sha256 hash; optional json_path/expected_hash). Start node or mid-graph check. Outputs: url, status_code, body, hash, changed, bytes",
            "Trigger",
            vec![port("in", false)],
            vec![port("next", false)],
            &["url", "status_code", "body", "hash", "changed", "bytes"],
            false,
            serde_json::json!({"type":"object","required":["url"],"properties":{"url":{},"method":{"type":"string"},"headers":{"type":"object"},"timeout_seconds":{"type":"integer"},"max_response_bytes":{"type":"integer"},"json_path":{"type":"string"},"expected_hash":{"type":"string"}}}),
            true,
            "caution",
            "new (reqwest)",
        ),
        info(
            "flow.for_each",
            "For Each (loop)",
            "Run the `next` body once per array item, then continue at `done`. Loop vars: nodes.<this>.output.item/index/count. Outputs: count, items",
            "Flow",
            vec![port("in", true)],
            vec![port("next", false), port("done", false)],
            &["count", "items", "item", "index"],
            false,
            serde_json::json!({"type":"object","required":["items"],"properties":{"items":{},"max_items":{"type":"integer"}}}),
            false,
            "safe",
            "new",
        ),
        info(
            "flow.parallel",
            "Parallel (fan-out)",
            "Run all branch subgraphs (every port except `done`), then continue at `done`. v1 executes branches sequentially in port order. Outputs: branches",
            "Flow",
            vec![port("in", true)],
            vec![port("branch1", false), port("branch2", false), port("done", false)],
            &["branches"],
            false,
            serde_json::json!({"type":"object","properties":{"wait_for_all":{"type":"boolean"},"max_branches":{"type":"integer"}}}),
            false,
            "safe",
            "new",
        ),
    ]
}

pub fn is_known_type(t: &str) -> bool {
    all_node_types().iter().any(|n| n.r#type == t)
}

pub fn ports_for(node_type: &str) -> Option<(Vec<String>, Vec<String>)> {
    all_node_types()
        .into_iter()
        .find(|n| n.r#type == node_type)
        .map(|n| {
            (
                n.input_ports.into_iter().map(|p| p.name).collect(),
                n.output_ports.into_iter().map(|p| p.name).collect(),
            )
        })
}

/// Per-node config validation (spec 6.4 item 10 + security).
pub fn validate_node_config(def: &WorkflowDefinition) -> Result<(), AppError> {
    for node in &def.nodes {
        let cfg = &node.config;
        let cfg_obj = cfg.as_object();
        match node.r#type.as_str() {
            "command.exec" => {
                let program = cfg.get("program").or_else(|| cfg.get("executable"));
                match program {
                    Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {}
                    Some(other) if other.is_object() => {} // $ref allowed, resolved at runtime
                    _ => {
                        return Err(AppError::Validation(format!(
                            "node '{}': command.exec requires config.program",
                            node.id
                        )))
                    }
                }
            }
            "python.script" => {
                let script = cfg.get("script").or_else(|| cfg.get("code"));
                match script {
                    Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {}
                    Some(other) if other.is_object() => {}
                    _ => {
                        return Err(AppError::Validation(format!(
                            "node '{}': python.script requires config.script",
                            node.id
                        )))
                    }
                }
            }
            "shell.exec" => {
                match cfg.get("command") {
                    Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {}
                    Some(other) if other.is_object() => {}
                    _ => {
                        return Err(AppError::Validation(format!(
                            "node '{}': shell.exec requires config.command",
                            node.id
                        )))
                    }
                }
            }
            "file.list" => {
                if cfg.get("path").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': file.list requires config.path",
                        node.id
                    )));
                }
            }
            "file.copy" | "file.move" => {
                if cfg.get("source").is_none() || cfg.get("destination").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': {} requires source/destination",
                        node.id, node.r#type
                    )));
                }
            }
            "logic.condition" => {
                // Accept either {left,operator,right} or legacy {condition:{type,...}}.
                let has_new = cfg.get("left").is_some() || cfg.get("operator").is_some();
                let has_legacy = cfg.get("condition").is_some();
                if !has_new && !has_legacy {
                    return Err(AppError::Validation(format!(
                        "node '{}': logic.condition requires left/operator/right or condition",
                        node.id
                    )));
                }
                let _ = cfg_obj;
            }
            "logic.switch" => {
                // {value, cases: [{equals, port}], default_port?}
                if cfg.get("value").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': logic.switch requires config.value",
                        node.id
                    )));
                }
                let cases = cfg.get("cases").and_then(|v| v.as_array()).ok_or_else(|| {
                    AppError::Validation(format!(
                        "node '{}': logic.switch requires config.cases array",
                        node.id
                    ))
                })?;
                if cases.is_empty() {
                    return Err(AppError::Validation(format!(
                        "node '{}': logic.switch cases must not be empty",
                        node.id
                    )));
                }
                let mut seen = std::collections::HashSet::new();
                for c in cases {
                    let port = c.get("port").and_then(|v| v.as_str()).unwrap_or("");
                    if port.trim().is_empty() {
                        return Err(AppError::Validation(format!(
                            "node '{}': logic.switch each case needs a port name",
                            node.id
                        )));
                    }
                    if !seen.insert(port.to_string()) {
                        return Err(AppError::Validation(format!(
                            "node '{}': logic.switch duplicate port '{port}'",
                            node.id
                        )));
                    }
                    if c.get("equals").is_none() {
                        return Err(AppError::Validation(format!(
                            "node '{}': logic.switch case '{port}' needs 'equals'",
                            node.id
                        )));
                    }
                }
            }
            "http.request" => {
                if cfg.get("url").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': http.request requires config.url",
                        node.id
                    )));
                }
                if let Some(m) = cfg.get("method").and_then(|v| v.as_str()) {
                    let up = m.to_uppercase();
                    if !matches!(up.as_str(), "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD") {
                        return Err(AppError::Validation(format!(
                            "node '{}': http.request unsupported method '{m}'",
                            node.id
                        )));
                    }
                }
                if let Some(n) = cfg.get("max_response_bytes").and_then(|v| v.as_u64()) {
                    if n < 1024 || n > 10_485_760 {
                        return Err(AppError::Validation(format!(
                            "node '{}': max_response_bytes must be 1024..10485760",
                            node.id
                        )));
                    }
                }
            }
            "data.transform" => {
                let has_single = cfg.get("op").and_then(|v| v.as_str()).is_some();
                let has_list = cfg.get("operations").and_then(|v| v.as_array()).is_some();
                if !has_single && !has_list {
                    return Err(AppError::Validation(format!(
                        "node '{}': data.transform needs 'op' or 'operations'",
                        node.id
                    )));
                }
                if let Some(arr) = cfg.get("operations").and_then(|v| v.as_array()) {
                    if arr.is_empty() || arr.len() > 32 {
                        return Err(AppError::Validation(format!(
                            "node '{}': data.transform operations must have 1..32 entries",
                            node.id
                        )));
                    }
                    for (i, op) in arr.iter().enumerate() {
                        let name = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
                        if !matches!(
                            name,
                            "replace" | "template" | "extract" | "filter" | "map"
                                | "join" | "split" | "trim" | "upper" | "lower" | "date_format"
                        ) {
                            return Err(AppError::Validation(format!(
                                "node '{}': data.transform operations[{i}] unknown op '{name}'",
                                node.id
                            )));
                        }
                    }
                }
            }
            "flow.delay" => {
                let secs = cfg.get("seconds").and_then(|v| v.as_f64());
                let ms = cfg.get("milliseconds").and_then(|v| v.as_u64());
                if secs.is_none() && ms.is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': flow.delay needs seconds and/or milliseconds",
                        node.id
                    )));
                }
                if let Some(s) = secs {
                    if !s.is_finite() || s < 0.0 || s > 86_400.0 {
                        return Err(AppError::Validation(format!(
                            "node '{}': flow.delay seconds must be 0..86400",
                            node.id
                        )));
                    }
                }
            }
            "file.wait_ready" => {
                if cfg.get("path").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': file.wait_ready requires config.path",
                        node.id
                    )));
                }
                if let Some(n) = cfg.get("stable_seconds").and_then(|v| v.as_u64()) {
                    if n == 0 || n > 300 {
                        return Err(AppError::Validation(format!(
                            "node '{}': stable_seconds must be 1..300",
                            node.id
                        )));
                    }
                }
            }
            "notify.desktop" => {
                if cfg.get("message").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': notify.desktop requires config.message",
                        node.id
                    )));
                }
                if let Some(l) = cfg.get("level").and_then(|v| v.as_str()) {
                    if !matches!(l.to_lowercase().as_str(), "info" | "warning" | "error" | "success") {
                        return Err(AppError::Validation(format!(
                            "node '{}': notify.desktop unknown level '{l}'",
                            node.id
                        )));
                    }
                }
            }
            "trigger.url_changed" => {
                if cfg.get("url").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': trigger.url_changed requires config.url",
                        node.id
                    )));
                }
                if let Some(m) = cfg.get("method").and_then(|v| v.as_str()) {
                    if !matches!(m.to_uppercase().as_str(), "GET" | "HEAD") {
                        return Err(AppError::Validation(format!(
                            "node '{}': trigger.url_changed method must be GET or HEAD",
                            node.id
                        )));
                    }
                }
            }
            "flow.for_each" => {
                if cfg.get("items").is_none() {
                    return Err(AppError::Validation(format!(
                        "node '{}': flow.for_each requires config.items (array or $ref)",
                        node.id
                    )));
                }
                // Literal arrays are range-checked here; $refs at runtime.
                if let Some(arr) = cfg.get("items").and_then(|v| v.as_array()) {
                    let max = cfg.get("max_items").and_then(|v| v.as_u64()).unwrap_or(100);
                    if max == 0 || max > 1000 {
                        return Err(AppError::Validation(format!(
                            "node '{}': max_items must be 1..1000",
                            node.id
                        )));
                    }
                    if arr.len() as u64 > max {
                        return Err(AppError::Validation(format!(
                            "node '{}': {} items exceed max_items={max}",
                            node.id,
                            arr.len()
                        )));
                    }
                }
            }
            "flow.parallel" => {
                if let Some(m) = cfg.get("max_branches").and_then(|v| v.as_u64()) {
                    if m == 0 || m > 32 {
                        return Err(AppError::Validation(format!(
                            "node '{}': max_branches must be 1..32",
                            node.id
                        )));
                    }
                }
            }
            "trigger.manual" | "trigger.input" | "flow.end" | "notification.log" => {}
            other => {
                return Err(AppError::Validation(format!(
                    "node '{}': unknown node type '{other}'",
                    node.id
                )))
            }
        }
        // Execution policy ranges per node override.
        if let Some(t) = cfg.get("timeout_seconds") {
            if let Some(n) = t.as_u64() {
                if n == 0 || n > 86_400 {
                    return Err(AppError::Validation(format!(
                        "node '{}': timeout_seconds must be 1..86400",
                        node.id
                    )));
                }
            } else if !t.is_object() {
                return Err(AppError::Validation(format!(
                    "node '{}': timeout_seconds must be integer",
                    node.id
                )));
            }
        }
        if let Some(retry) = cfg.get("retry") {
            if let Some(obj) = retry.as_object() {
                if let Some(m) = obj.get("max_attempts").and_then(|v| v.as_u64()) {
                    if m == 0 || m > 10 {
                        return Err(AppError::Validation(format!(
                            "node '{}': max_attempts must be 1..10",
                            node.id
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

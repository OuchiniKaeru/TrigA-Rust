use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Canonical workflow definition (YAML codec input/output).
/// `definition_yaml` in SQLite is the source of truth; `name`/`enabled`/
/// `version` columns mirror values inside this model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDefinition {
    pub schema_version: i32,
    #[serde(default)]
    pub workflow: WorkflowMeta,
    #[serde(default)]
    pub settings: WorkflowSettings,
    #[serde(default)]
    pub execution: Option<ExecutionBlock>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTriggerDecl>,
    #[serde(default)]
    pub inputs: HashMap<String, InputDecl>,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowMeta {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSettings {
    #[serde(default = "default_timeout")]
    pub default_timeout_seconds: u64,
    #[serde(default = "default_attempts")]
    pub default_max_attempts: u32,
    #[serde(default = "default_delay")]
    pub retry_delay_seconds: u64,
    #[serde(default)]
    pub retry_backoff: BackoffName,
    #[serde(default = "default_max_delay")]
    pub max_retry_delay_seconds: u64,
    #[serde(default)]
    pub retry_on_timeout: bool,
    #[serde(default)]
    pub failure_policy: FailurePolicy,
    #[serde(default = "default_workers")]
    pub max_workers: u32,
    #[serde(default)]
    pub overlap_policy: OverlapName,
    #[serde(default = "default_true")]
    pub deduplication_enabled: bool,
    #[serde(default = "default_dedup_window")]
    pub deduplication_window_seconds: u64,
}

fn default_timeout() -> u64 {
    300
}
fn default_attempts() -> u32 {
    1
}
fn default_delay() -> u64 {
    10
}
fn default_max_delay() -> u64 {
    300
}
fn default_workers() -> u32 {
    1
}
fn default_true() -> bool {
    true
}
fn default_dedup_window() -> u64 {
    5
}

impl Default for WorkflowSettings {
    fn default() -> Self {
        Self {
            default_timeout_seconds: 300,
            default_max_attempts: 1,
            retry_delay_seconds: 10,
            retry_backoff: BackoffName::Fixed,
            max_retry_delay_seconds: 300,
            retry_on_timeout: true,
            failure_policy: FailurePolicy::Stop,
            max_workers: 1,
            overlap_policy: OverlapName::Skip,
            deduplication_enabled: true,
            deduplication_window_seconds: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackoffName {
    #[default]
    #[serde(alias = "Fixed")]
    #[serde(alias = "FIXED")]
    Fixed,
    #[serde(alias = "Exponential")]
    #[serde(alias = "EXPONENTIAL")]
    Exponential,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FailurePolicy {
    #[default]
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OverlapName {
    #[default]
    Skip,
    Queue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ExecutionBlock {
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub retry: Option<RetryBlock>,
    #[serde(default)]
    pub concurrency: Option<ConcurrencyBlock>,
    #[serde(default)]
    pub deduplication: Option<DedupBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RetryBlock {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_attempts: Option<u32>,
    #[serde(default)]
    pub delay_seconds: Option<u64>,
    #[serde(default)]
    pub backoff: Option<BackoffName>,
    #[serde(default)]
    pub max_delay_seconds: Option<u64>,
    #[serde(default)]
    pub retry_on_timeout: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ConcurrencyBlock {
    #[serde(default)]
    pub max_workers: Option<u32>,
    #[serde(default)]
    pub overlap_policy: Option<OverlapName>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DedupBlock {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub window_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowTriggerDecl {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub interval_seconds: Option<u64>,
    #[serde(default)]
    pub start_immediately: Option<bool>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub automation_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct InputDecl {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowNode {
    pub id: String,
    pub r#type: String,
    #[serde(default)]
    pub position: Position,
    #[serde(default = "empty_config")]
    pub config: serde_json::Value,
}

fn empty_config() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Position {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    #[serde(default = "default_port_next")]
    pub source_port: String,
    pub target: String,
    #[serde(default = "default_port_in")]
    pub target_port: String,
}

fn default_port_next() -> String {
    "next".into()
}
fn default_port_in() -> String {
    "in".into()
}

impl WorkflowDefinition {
    /// Merge `settings` + `execution` block into effective settings.
    /// `execution` wins when present (spec 7.3).
    pub fn effective_settings(&self) -> WorkflowSettings {
        let mut out = self.settings.clone();
        if let Some(exec) = &self.execution {
            if let Some(t) = exec.timeout_seconds {
                out.default_timeout_seconds = t;
            }
            if let Some(r) = &exec.retry {
                if let Some(v) = r.max_attempts {
                    out.default_max_attempts = v;
                }
                if let Some(v) = r.delay_seconds {
                    out.retry_delay_seconds = v;
                }
                if let Some(v) = &r.backoff {
                    out.retry_backoff = v.clone();
                }
                if let Some(v) = r.max_delay_seconds {
                    out.max_retry_delay_seconds = v;
                }
                if let Some(v) = r.retry_on_timeout {
                    out.retry_on_timeout = v;
                }
                if let Some(v) = r.enabled {
                    if !v {
                        out.default_max_attempts = 1;
                    } else if out.default_max_attempts < 1 {
                        out.default_max_attempts = 1;
                    }
                }
            }
            if let Some(c) = &exec.concurrency {
                if let Some(v) = c.max_workers {
                    out.max_workers = v;
                }
                if let Some(v) = &c.overlap_policy {
                    out.overlap_policy = v.clone();
                }
            }
            if let Some(d) = &exec.deduplication {
                if let Some(v) = d.enabled {
                    out.deduplication_enabled = v;
                }
                if let Some(v) = d.window_seconds {
                    out.deduplication_window_seconds = v;
                }
            }
        }
        out
    }

    pub fn node(&self, id: &str) -> Option<&WorkflowNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// Mask secret-looking keys before persisting inputs/outputs.
pub fn mask_secrets(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let lower = k.to_lowercase();
                if lower.contains("password")
                    || lower.contains("secret")
                    || lower.contains("token")
                    || lower.contains("api_key")
                    || lower.contains("apikey")
                    || lower == "key"
                {
                    out.insert(k.clone(), serde_json::Value::String("***".into()));
                } else {
                    out.insert(k.clone(), mask_secrets(v));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(mask_secrets).collect())
        }
        _ => value.clone(),
    }
}

/// Truncate a string for log storage (avoid huge stdout in DB).
pub fn truncate_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    format!("{}…[truncated {} bytes]", &text[..limit], text.len() - limit)
}

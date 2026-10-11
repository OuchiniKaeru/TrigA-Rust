use std::collections::HashMap;

/// Runtime context for one workflow run (spec 6.3, 8.3).
#[derive(Debug, Clone, Default)]
pub struct RunContext {
    pub job_id: String,
    pub trigger_type: String,
    pub inputs: HashMap<String, serde_json::Value>,
    pub event: HashMap<String, String>,
    pub outputs: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl RunContext {
    /// Resolve a config value: replaces `{"$ref": "..."}` objects with typed values.
    pub fn resolve(&self, value: &serde_json::Value) -> Result<serde_json::Value, String> {
        match value {
            serde_json::Value::Object(map) if map.len() == 1 && map.contains_key("$ref") => {
                let r = map["$ref"].as_str().unwrap_or("");
                self.resolve_ref(r)
            }
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    out.insert(k.clone(), self.resolve(v).map_err(|e| e)?);
                }
                Ok(serde_json::Value::Object(out))
            }
            serde_json::Value::Array(items) => {
                let mut out = Vec::new();
                for v in items {
                    out.push(self.resolve(v)?);
                }
                Ok(serde_json::Value::Array(out))
            }
            _ => Ok(value.clone()),
        }
    }

    pub fn resolve_ref(&self, reference: &str) -> Result<serde_json::Value, String> {
        if let Some(rest) = reference.strip_prefix("workflow.input.") {
            return self
                .inputs
                .get(rest)
                .cloned()
                .ok_or_else(|| format!("unknown workflow.input '{rest}'"));
        }
        if reference == "workflow.context.job_id" {
            return Ok(serde_json::Value::String(self.job_id.clone()));
        }
        if reference == "workflow.context.trigger_type" {
            return Ok(serde_json::Value::String(self.trigger_type.clone()));
        }
        if let Some(rest) = reference.strip_prefix("workflow.context.event.") {
            return Ok(serde_json::Value::String(
                self.event.get(rest).cloned().unwrap_or_default(),
            ));
        }
        if let Some(rest) = reference.strip_prefix("nodes.") {
            let mut parts = rest.splitn(3, '.');
            let node_id = parts.next().unwrap_or("");
            let kw = parts.next().unwrap_or("");
            let name = parts.next().unwrap_or("");
            if kw != "output" || node_id.is_empty() || name.is_empty() {
                return Err(format!("invalid node reference '{reference}'"));
            }
            return self
                .outputs
                .get(node_id)
                .and_then(|m| m.get(name))
                .cloned()
                .ok_or_else(|| format!("output '{reference}' is not available (node not completed?)"));
        }
        Err(format!("unsupported reference '{reference}'"))
    }

    /// Resolve to a plain string (for paths/commands). Objects/arrays become JSON.
    pub fn resolve_string(&self, value: &serde_json::Value) -> Result<String, String> {
        let v = self.resolve(value)?;
        match v {
            serde_json::Value::String(s) => Ok(s),
            serde_json::Value::Null => Ok(String::new()),
            serde_json::Value::Number(n) => Ok(n.to_string()),
            serde_json::Value::Bool(b) => Ok(b.to_string()),
            other => serde_json::to_string(&other).map_err(|e| e.to_string()),
        }
    }
}

/// Build event map from legacy EventContext fields (spec 8.3).
pub fn event_map_from_legacy(
    file_path: &str,
    file_name: &str,
    file_name_without_ext: &str,
    folder_path: &str,
    folder_name: &str,
    extension: &str,
    timestamp: &str,
    date: &str,
    time: &str,
    automation_id: &str,
    job_id: &str,
    attempt: u32,
) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("file_path".into(), file_path.into());
    m.insert("file_name".into(), file_name.into());
    m.insert(
        "file_name_without_ext".into(),
        file_name_without_ext.into(),
    );
    m.insert("folder_path".into(), folder_path.into());
    m.insert("folder_name".into(), folder_name.into());
    m.insert("extension".into(), extension.into());
    m.insert("timestamp".into(), timestamp.into());
    m.insert("date".into(), date.into());
    m.insert("time".into(), time.into());
    m.insert("automation_id".into(), automation_id.into());
    m.insert("job_id".into(), job_id.into());
    m.insert("attempt".into(), attempt.to_string());
    m
}

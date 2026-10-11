use super::model::WorkflowDefinition;
use crate::AppError;

/// Parse YAML text into a definition. Never executes commands.
pub fn parse(yaml: &str) -> Result<WorkflowDefinition, AppError> {
    if yaml.len() > 512 * 1024 {
        return Err(AppError::Validation("workflow YAML is too large".into()));
    }
    serde_yaml::from_str(yaml).map_err(|e| AppError::Validation(format!("invalid YAML: {e}")))
}

/// Serialize a definition to canonical YAML (for export / DB storage).
pub fn serialize(def: &WorkflowDefinition) -> Result<String, AppError> {
    serde_yaml::to_string(def).map_err(|e| AppError::Storage(e.to_string()))
}

/// Round-trip check used in tests: parse(serialize(def)) == def (semantically).
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_example() {
        let yaml = r#"
schema_version: 1
workflow:
  id: "wf-1"
  name: "demo"
  enabled: true
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 80, y: 160 }
    config: {}
  - id: "finish"
    type: "flow.end"
    position: { x: 200, y: 160 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
        let def = parse(yaml).unwrap();
        assert_eq!(def.workflow.id, "wf-1");
        let out = serialize(&def).unwrap();
        let back = parse(&out).unwrap();
        assert_eq!(def, back);
    }
}

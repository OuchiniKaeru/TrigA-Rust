use super::model::WorkflowDefinition;
use super::node_registry;
use crate::AppError;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ValidationWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<ValidationWarning>,
}

/// Full validation: schema rules (6.2) + graph checks (6.4) + policy (7.3) + security (12).
pub fn validate(def: &WorkflowDefinition) -> Result<ValidationReport, AppError> {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<ValidationWarning> = Vec::new();

    // 6.2 required fields
    if def.schema_version != 1 {
        errors.push(format!(
            "unsupported schema_version {} (expected 1)",
            def.schema_version
        ));
    }
    if def.workflow.id.trim().is_empty() {
        errors.push("workflow.id is required".into());
    }
    if def.workflow.name.trim().is_empty() {
        errors.push("workflow.name is required".into());
    }
    if def.nodes.is_empty() {
        errors.push("nodes must not be empty".into());
    }

    // Unique IDs
    let mut node_ids = HashSet::new();
    for n in &def.nodes {
        if !node_ids.insert(n.id.clone()) {
            errors.push(format!("duplicate node id '{}'", n.id));
        }
        if !node_registry::is_known_type(&n.r#type) {
            errors.push(format!("unknown node type '{}' (node '{}')", n.r#type, n.id));
        }
    }
    let mut edge_ids = HashSet::new();
    for e in &def.edges {
        if !edge_ids.insert(e.id.clone()) {
            errors.push(format!("duplicate edge id '{}'", e.id));
        }
    }

    // Node existence + ports + type compat
    let node_map: HashMap<&str, &str> = def.nodes.iter().map(|n| (n.id.as_str(), n.r#type.as_str())).collect();
    let node_by_id: HashMap<&str, &crate::workflow::model::WorkflowNode> =
        def.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    for e in &def.edges {
        let src_type = node_map.get(e.source.as_str());
        let dst_type = node_map.get(e.target.as_str());
        if src_type.is_none() {
            errors.push(format!("edge '{}': unknown source node '{}'", e.id, e.source));
            continue;
        }
        if dst_type.is_none() {
            errors.push(format!("edge '{}': unknown target node '{}'", e.id, e.target));
            continue;
        }
        let src_type_str = *src_type.unwrap();
        // flow.parallel accepts any branch port name (`done` is the join port).
        let is_parallel = src_type_str == "flow.parallel";
        // logic.switch has dynamic output ports from config.cases + default_port.
        let src_outputs: Vec<String> = if is_parallel {
            // Any non-empty port is accepted; branch identity is the edge itself.
            vec![e.source_port.clone()]
        } else if src_type_str == "logic.switch" {
            let mut ports = Vec::new();
            if let Some(src_node) = node_by_id.get(e.source.as_str()) {
                if let Some(cases) = src_node.config.get("cases").and_then(|v| v.as_array()) {
                    for c in cases {
                        if let Some(p) = c.get("port").and_then(|v| v.as_str()) {
                            ports.push(p.to_string());
                        }
                    }
                }
                let default_port = src_node
                    .config
                    .get("default_port")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                ports.push(default_port.to_string());
            }
            ports
        } else {
            node_registry::ports_for(src_type_str)
                .map(|(_, o)| o)
                .unwrap_or_default()
        };
        let (dst_inputs, _dst_outputs) =
            node_registry::ports_for(dst_type.unwrap()).unwrap_or_default();
        if !src_outputs.contains(&e.source_port) {
            errors.push(format!(
                "edge '{}': unknown source port '{}' on node '{}'",
                e.id, e.source_port, e.source
            ));
        }
        if !dst_inputs.contains(&e.target_port) {
            errors.push(format!(
                "edge '{}': unknown target port '{}' on node '{}'",
                e.id, e.target_port, e.target
            ));
        }
    }

    // Node config validation
    if let Err(e) = node_registry::validate_node_config(def) {
        errors.push(e.to_string());
    }

    // Reference validation (6.3): every nodes.<id>.output.<name> must exist.
    for n in &def.nodes {
        for r in collect_refs(&n.config) {
            if let Err(msg) = check_ref(def, &n.id, &r, &node_map) {
                errors.push(format!("node '{}': {msg}", n.id));
            } else if let Some(input_name) = r.strip_prefix("workflow.input.") {
                // Warn when referencing an undeclared input (typo guard).
                // Undeclared inputs still resolve at runtime only if passed;
                // declaring them under top-level `inputs:` documents defaults.
                if !def.inputs.contains_key(input_name) {
                    warnings.push(ValidationWarning {
                        code: "undeclared-input".into(),
                        message: format!(
                            "node '{}' references undeclared input 'workflow.input.{input_name}'; declare it under top-level `inputs:`",
                            n.id
                        ),
                    });
                }
            }
        }
    }
    // trigger.input guidance: inputs used but no trigger.input start node.
    let uses_inputs = def.nodes.iter().any(|n| {
        collect_refs(&n.config)
            .iter()
            .any(|r| r.starts_with("workflow.input."))
    });
    if uses_inputs && !def.nodes.iter().any(|n| n.r#type == "trigger.input") {
        warnings.push(ValidationWarning {
            code: "no-input-node".into(),
            message: "workflow uses workflow.input.* but has no trigger.input node; add one as the start node to document runtime inputs".into(),
        });
    }

    // DAG check (6.4 item 8): initial release is DAG-only.
    if let Some(cycle) = find_cycle(def) {
        errors.push(format!("graph contains a cycle: {}", cycle.join(" -> ")));
    }

    // Start/end + reachability warnings
    let targets: HashSet<&str> = def.edges.iter().map(|e| e.target.as_str()).collect();
    let sources: HashSet<&str> = def.edges.iter().map(|e| e.source.as_str()).collect();
    let starts: Vec<&str> = def
        .nodes
        .iter()
        .filter(|n| !targets.contains(n.id.as_str()))
        .map(|n| n.id.as_str())
        .collect();
    if starts.is_empty() && !def.nodes.is_empty() {
        errors.push("no start node (every node has an incoming edge)".into());
    }
    let has_end = def.nodes.iter().any(|n| n.r#type == "flow.end");
    if !has_end {
        warnings.push(ValidationWarning {
            code: "no-end-node".into(),
            message: "no flow.end node; workflow will run to leaf nodes".into(),
        });
    }
    // Control-node wiring warnings (for_each needs next/done, parallel needs branches).
    for n in &def.nodes {
        if n.r#type == "flow.for_each" {
            let outs: Vec<&str> = def
                .edges
                .iter()
                .filter(|e| e.source == n.id)
                .map(|e| e.source_port.as_str())
                .collect();
            if !outs.contains(&"next") {
                warnings.push(ValidationWarning {
                    code: "for-each-no-body".into(),
                    message: format!(
                        "node '{}' (flow.for_each) has no `next` body edge; the loop runs zero nodes",
                        n.id
                    ),
                });
            }
            if !outs.contains(&"done") {
                warnings.push(ValidationWarning {
                    code: "for-each-no-done".into(),
                    message: format!(
                        "node '{}' (flow.for_each) has no `done` edge; the workflow ends after the loop",
                        n.id
                    ),
                });
            }
        }
        if n.r#type == "flow.parallel" {
            let branches = def
                .edges
                .iter()
                .filter(|e| e.source == n.id && e.source_port != "done")
                .count();
            if branches < 2 {
                warnings.push(ValidationWarning {
                    code: "parallel-single-branch".into(),
                    message: format!(
                        "node '{}' (flow.parallel) has {branches} branch edge(s); use 2+ for fan-out",
                        n.id
                    ),
                });
            }
        }
    }
    // Reachability from starts
    let reachable = reachable_from(def, &starts);
    for n in &def.nodes {
        if !reachable.contains(n.id.as_str()) {
            warnings.push(ValidationWarning {
                code: "unreachable".into(),
                message: format!("node '{}' is unreachable from start nodes", n.id),
            });
        }
        if !sources.contains(n.id.as_str()) && n.r#type != "flow.end" && !is_trigger(&n.r#type) {
            // leaf non-end node
            warnings.push(ValidationWarning {
                code: "leaf-node".into(),
                message: format!("node '{}' has no outgoing edge", n.id),
            });
        }
    }

    // Execution policy ranges (7.3)
    let s = def.effective_settings();
    if s.default_timeout_seconds == 0 || s.default_timeout_seconds > 86_400 {
        errors.push("settings.default_timeout_seconds must be 1..86400".into());
    }
    if s.default_max_attempts == 0 || s.default_max_attempts > 10 {
        errors.push("settings.default_max_attempts must be 1..10".into());
    }
    if s.retry_delay_seconds > s.max_retry_delay_seconds {
        errors.push("retry_delay_seconds must not exceed max_retry_delay_seconds".into());
    }
    if s.max_workers == 0 || s.max_workers > 32 {
        errors.push("max_workers must be 1..32".into());
    }
    if s.deduplication_window_seconds > 86_400 {
        errors.push("deduplication window is too large".into());
    }
    if !matches!(s.failure_policy, super::model::FailurePolicy::Stop) {
        errors.push("only failure_policy 'stop' is supported in this release".into());
    }

    // Security: no secrets in YAML (6.2), no file.delete.
    let yaml_text = serde_yaml::to_string(def).unwrap_or_default().to_lowercase();
    for key in ["api_key", "apikey", "password", "auth_token", "authtoken"] {
        if yaml_text.contains(key) {
            warnings.push(ValidationWarning {
                code: "possible-secret".into(),
                message: format!("definition contains '{key}'; do not store secrets in YAML"),
            });
        }
    }

    Ok(ValidationReport {
        valid: errors.is_empty(),
        errors,
        warnings,
    })
}

fn is_trigger(t: &str) -> bool {
    t == "trigger.manual" || t == "trigger.input" || t == "trigger.url_changed"
}

/// Collect all `$ref` strings inside a config value.
fn collect_refs(value: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    match value {
        serde_json::Value::Object(map) => {
            if let Some(r) = map.get("$ref").and_then(|v| v.as_str()) {
                out.push(r.to_string());
            }
            for v in map.values() {
                out.extend(collect_refs(v));
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                out.extend(collect_refs(v));
            }
        }
        _ => {}
    }
    out
}

fn check_ref(
    def: &WorkflowDefinition,
    from: &str,
    reference: &str,
    node_map: &HashMap<&str, &str>,
) -> Result<(), String> {
    if let Some(rest) = reference.strip_prefix("workflow.input.") {
        if rest.is_empty() {
            return Err(format!("invalid reference '{reference}'"));
        }
        return Ok(());
    }
    if reference == "workflow.context.job_id" || reference == "workflow.context.trigger_type" {
        return Ok(());
    }
    if let Some(rest) = reference.strip_prefix("workflow.context.event.") {
        if rest.is_empty() {
            return Err(format!("invalid reference '{reference}'"));
        }
        return Ok(());
    }
    if let Some(rest) = reference.strip_prefix("nodes.") {
        // nodes.<id>.output.<name>
        let mut parts = rest.splitn(3, '.');
        let node_id = parts.next().unwrap_or("");
        let output_kw = parts.next().unwrap_or("");
        let output_name = parts.next().unwrap_or("");
        if node_id.is_empty() || output_kw != "output" || output_name.is_empty() {
            return Err(format!("invalid node reference '{reference}'"));
        }
        let src_type = node_map
            .get(node_id)
            .ok_or_else(|| format!("reference to unknown node '{node_id}'"))?;
        // Output-name check: typo guard for chaining (e.g. stdout vs stout).
        // trigger.input is dynamic (any declared input name is allowed).
        if *src_type != "trigger.input" {
            if let Some(known) = node_registry::known_data_outputs(src_type) {
                if !known.iter().any(|k| k == output_name) {
                    return Err(format!(
                        "reference '{reference}' is unknown output '{output_name}' for node '{node_id}' ({src_type}; expected one of: {})",
                        if known.is_empty() {
                            "(no outputs)".to_string()
                        } else {
                            known.join(", ")
                        }
                    ));
                }
            }
        }
        // Must precede in topological order (no forward/cycle refs).
        if let Some(order) = topo_order(def) {
            let pos = |id: &str| order.iter().position(|x| x == id);
            match (pos(from), pos(node_id)) {
                (Some(a), Some(b)) if b < a => Ok(()),
                _ => Err(format!("reference '{reference}' must point to a predecessor node")),
            }
        } else {
            Ok(())
        }
    } else {
        Err(format!("unsupported reference '{reference}'"))
    }
}

/// Kahn's topological order; None when a cycle exists.
pub fn topo_order(def: &WorkflowDefinition) -> Option<Vec<String>> {
    let mut indeg: HashMap<&str, usize> = def.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &def.edges {
        if indeg.contains_key(e.source.as_str()) && indeg.contains_key(e.target.as_str()) {
            *indeg.get_mut(e.target.as_str()).unwrap() += 1;
            adj.entry(e.source.as_str()).or_default().push(e.target.as_str());
        }
    }
    let mut queue: Vec<&str> = indeg.iter().filter(|(_, &d)| d == 0).map(|(&k, _)| k).collect();
    queue.sort_unstable();
    let mut order = Vec::new();
    while let Some(n) = queue.pop() {
        order.push(n.to_string());
        if let Some(nexts) = adj.get(n) {
            let mut sorted = nexts.clone();
            sorted.sort_unstable();
            for m in sorted {
                let d = indeg.get_mut(m).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push(m);
                }
            }
        }
    }
    if order.len() == def.nodes.len() {
        Some(order)
    } else {
        None
    }
}

fn find_cycle(def: &WorkflowDefinition) -> Option<Vec<String>> {
    if topo_order(def).is_some() {
        return None;
    }
    // Simple DFS to report one cycle.
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &def.edges {
        adj.entry(e.source.as_str()).or_default().push(e.target.as_str());
    }
    let mut stack: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut in_stack: HashSet<String> = HashSet::new();
    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        stack: &mut Vec<String>,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
    ) -> Option<Vec<String>> {
        visited.insert(node.to_string());
        in_stack.insert(node.to_string());
        stack.push(node.to_string());
        if let Some(nexts) = adj.get(node) {
            for m in nexts {
                if !visited.contains(*m) {
                    if let Some(c) = dfs(m, adj, stack, visited, in_stack) {
                        return Some(c);
                    }
                } else if in_stack.contains(*m) {
                    let pos = stack.iter().position(|x| x == m).unwrap_or(0);
                    let mut cycle: Vec<String> = stack[pos..].to_vec();
                    cycle.push(m.to_string());
                    return Some(cycle);
                }
            }
        }
        stack.pop();
        in_stack.remove(node);
        None
    }
    for n in &def.nodes {
        if !visited.contains(&n.id) {
            if let Some(c) = dfs(&n.id, &adj, &mut stack, &mut visited, &mut in_stack) {
                return Some(c);
            }
        }
    }
    Some(vec!["<cycle>".into()])
}

fn reachable_from(def: &WorkflowDefinition, starts: &[&str]) -> HashSet<String> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &def.edges {
        adj.entry(e.source.as_str()).or_default().push(e.target.as_str());
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = starts.iter().map(|s| s.to_string()).collect();
    while let Some(n) = stack.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(nexts) = adj.get(n.as_str()) {
            for m in nexts {
                stack.push(m.to_string());
            }
        }
    }
    seen
}

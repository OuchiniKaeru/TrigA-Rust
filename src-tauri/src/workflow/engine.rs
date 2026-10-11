use super::{
    context::RunContext,
    errors::is_retryable_message,
    graph::Graph,
    model::{mask_secrets, truncate_text, WorkflowDefinition},
    nodes,
};
use crate::{
    engine::retry,
    services::security::SecurityPolicy,
    AppError,
};
use chrono::Utc;
use sqlx::SqlitePool;
use std::{collections::{HashMap, HashSet}, time::Duration};
use uuid::Uuid;

pub struct NodeExecutionResult {
    pub outputs: HashMap<String, serde_json::Value>,
    /// For logic.condition: "true"/"false". For others: "next".
    pub taken_port: String,
}

/// Execute a single node with timeout/retry/cancel. Records nothing; caller persists.
pub async fn execute_node(
    pool: &SqlitePool,
    job_id: &str,
    _node_id: &str,
    node_type: &str,
    config: &serde_json::Value,
    ctx: &RunContext,
    default_timeout_secs: u64,
    default_max_attempts: u32,
    security: &SecurityPolicy,
) -> Result<(NodeExecutionResult, u32), AppError> {
    let max_attempts = nodes::common::node_max_attempts(config, default_max_attempts);
    let mut attempt: u32 = 1;
    loop {
        // Cooperative cancellation check before each attempt.
        if is_cancel_requested(pool, job_id).await? {
            return Err(AppError::Cancelled(format!("job {job_id} was cancelled")));
        }
        // Race the node future against cooperative cancellation so a long
        // external process does not block Cancel. Dropping the node future
        // drops the `kill_on_drop(true)` child handle, terminating the
        // subprocess instead of leaving it orphaned (spec 7.5).
        let node_fut = execute_node_once(node_type, config, ctx, default_timeout_secs, security);
        tokio::pin!(node_fut);
        let outcome: Result<NodeExecutionResult, AppError> = loop {
            tokio::select! {
                result = &mut node_fut => break result,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if is_cancel_requested(pool, job_id).await? {
                        break Err(AppError::Cancelled(format!("job {job_id} was cancelled")));
                    }
                }
            }
        };
        match outcome {
            Ok(result) => return Ok((result, attempt)),
            Err(e) => {
                let msg = e.to_string();
                let retryable = is_retryable_message(&msg)
                    && !matches!(e, AppError::Cancelled(_))
                    && !matches!(e, AppError::Validation(_))
                    && !matches!(e, AppError::Security(_));
                let timed_out = matches!(e, AppError::Timeout) || msg.contains("timed out");
                // Build a synthetic retry policy from node/workflow defaults.
                let policy = crate::core::models::RetryPolicy {
                    enabled: max_attempts > 1,
                    max_attempts,
                    delay_seconds: 10,
                    backoff: crate::core::models::BackoffStrategy::Fixed,
                    max_delay_seconds: 300,
                    retry_on_timeout: true,
                };
                // Respect per-config retry.enabled=false.
                let retry_enabled = config
                    .get("retry")
                    .and_then(|r| r.get("enabled"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                if !retryable
                    || !retry_enabled
                    || !retry::should_retry(&policy, attempt, timed_out)
                {
                    if matches!(e, AppError::Cancelled(_)) {
                        return Err(e);
                    }
                    if timed_out {
                        return Err(AppError::Timeout);
                    }
                    return Err(e);
                }
                tokio::time::sleep(retry::retry_delay(&policy, attempt)).await;
                attempt += 1;
                // Guard against infinite loop (max_attempts is total tries).
                if attempt > max_attempts {
                    return Err(e);
                }
            }
        }
    }
}

async fn execute_node_once(
    node_type: &str,
    config: &serde_json::Value,
    ctx: &RunContext,
    default_timeout_secs: u64,
    security: &SecurityPolicy,
) -> Result<NodeExecutionResult, AppError> {
    match node_type {
        "trigger.manual" => Ok(NodeExecutionResult {
            outputs: HashMap::new(),
            taken_port: "next".into(),
        }),
        "trigger.input" => Ok(NodeExecutionResult {
            // Expose runtime inputs as this node's outputs too, so both
            // `workflow.input.<name>` and `nodes.<input_id>.output.<name>` work.
            outputs: ctx.inputs.clone(),
            taken_port: "next".into(),
        }),
        "flow.end" => Ok(NodeExecutionResult {
            outputs: HashMap::new(),
            taken_port: String::new(),
        }),
        "command.exec" => {
            let outputs =
                nodes::command::execute(config, ctx, default_timeout_secs, security).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "python.script" => {
            let outputs =
                nodes::python::execute(config, ctx, default_timeout_secs, security).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "shell.exec" => {
            let outputs =
                nodes::shell::execute(config, ctx, default_timeout_secs, security).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "file.list" => {
            let fut = nodes::file::execute_list(config, ctx, security);
            let outputs = tokio::time::timeout(Duration::from_secs(default_timeout_secs.clamp(1, 86_400)), fut)
                .await
                .map_err(|_| AppError::Timeout)??;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "file.copy" => {
            let outputs = nodes::file::execute_copy(config, ctx, security).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "file.move" => {
            let outputs = nodes::file::execute_move(config, ctx, security).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "logic.condition" => {
            let port = nodes::condition::evaluate(config, ctx)?;
            let mut outputs = HashMap::new();
            outputs.insert("branch".into(), serde_json::Value::String(port.clone()));
            Ok(NodeExecutionResult {
                outputs,
                taken_port: port,
            })
        }
        "logic.switch" => {
            let (port, value) = nodes::switch::evaluate(config, ctx)?;
            let mut outputs = HashMap::new();
            outputs.insert("branch".into(), serde_json::Value::String(port.clone()));
            outputs.insert("value".into(), serde_json::Value::String(value));
            Ok(NodeExecutionResult {
                outputs,
                taken_port: port,
            })
        }
        "notification.log" => {
            let outputs = nodes::notification::execute(config, ctx)?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "http.request" => {
            let outputs =
                nodes::http::execute(config, ctx, default_timeout_secs, security).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "data.transform" => {
            let outputs = nodes::transform::execute(config, ctx)?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "flow.delay" => {
            let outputs = nodes::flow::execute_delay(config, ctx).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "file.wait_ready" => {
            let fut = nodes::file::execute_wait_ready(config, ctx, security);
            let outputs = tokio::time::timeout(Duration::from_secs(default_timeout_secs.clamp(1, 86_400)), fut)
                .await
                .map_err(|_| AppError::Timeout)??;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "notify.desktop" => {
            let outputs = nodes::notification::execute_desktop(config, ctx)?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        "trigger.url_changed" => {
            let outputs =
                nodes::url_watch::execute(config, ctx, default_timeout_secs, security).await?;
            Ok(NodeExecutionResult {
                outputs,
                taken_port: "next".into(),
            })
        }
        // Control nodes are driven by run_graph (subgraph fan-out), never here.
        "flow.for_each" | "flow.parallel" => Err(AppError::Validation(format!(
            "'{node_type}' must run via the graph executor"
        ))),
        other => Err(AppError::Validation(format!("unknown node type '{other}'"))),
    }
}

async fn is_cancel_requested(pool: &SqlitePool, job_id: &str) -> Result<bool, AppError> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT cancel_requested FROM execution_history WHERE job_id = ?")
            .bind(job_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    Ok(row.map(|r| r.0 != 0).unwrap_or(false))
}

/// Run the whole DAG sequentially in topological order with branch pruning.
/// Persists node runs + final job status. Returns final status string.
pub async fn run_graph(
    pool: &SqlitePool,
    job_id: &str,
    def: &WorkflowDefinition,
    ctx: &mut RunContext,
    default_timeout_secs: u64,
    default_max_attempts: u32,
    security: &SecurityPolicy,
) -> Result<String, AppError> {
    let graph = Graph::build(def);
    // Determine execution order: BFS from start nodes following taken branches.
    // We execute greedily: queue of ready nodes; condition nodes select one port.
    let mut executed: HashSet<String> = HashSet::new();
    let mut skipped: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = graph.start_nodes();
    // Track visit order for determinism.
    let mut final_status = "success".to_string();
    let mut first_error: Option<String> = None;

    // Pre-create pending rows for all nodes (spec: skipped branches recorded).
    for n in &def.nodes {
        upsert_node_run(
            pool,
            job_id,
            &n.id,
            &n.r#type,
            "pending",
            1,
            nodes::common::node_max_attempts(&n.config, default_max_attempts),
            None,
            None,
            None,
            None,
        )
        .await?;
    }

    while let Some(node_id) = queue.pop() {
        if executed.contains(&node_id) || skipped.contains(&node_id) {
            continue;
        }
        let node = def.node(&node_id).cloned();
        let Some(node) = node else { continue };
        // Cancellation check at node boundary.
        if is_cancel_requested(pool, job_id).await? {
            mark_remaining_cancelled(pool, job_id, def, &executed).await?;
            return Ok("cancelled".into());
        }
        // Control-flow nodes drive subgraphs instead of a single execution.
        if node.r#type == "flow.for_each" {
            match run_for_each_node(
                pool, job_id, def, &graph, &node_id, &node.config, ctx,
                default_timeout_secs, default_max_attempts, security,
                &mut executed, &mut skipped,
            )
            .await?
            {
                ControlOutcome::Continue { done_targets } => {
                    executed.insert(node_id.clone());
                    for next in done_targets {
                        if !executed.contains(&next) && !skipped.contains(&next) {
                            queue.push(next);
                        }
                    }
                    continue;
                }
                ControlOutcome::EndJob { status, message } => {
                    executed.insert(node_id.clone());
                    final_status = status;
                    first_error = Some(message);
                    break;
                }
            }
        }
        if node.r#type == "flow.parallel" {
            match run_parallel_node(
                pool, job_id, def, &graph, &node_id, &node.config, ctx,
                default_timeout_secs, default_max_attempts, security,
                &mut executed, &mut skipped,
            )
            .await?
            {
                ControlOutcome::Continue { done_targets } => {
                    executed.insert(node_id.clone());
                    for next in done_targets {
                        if !executed.contains(&next) && !skipped.contains(&next) {
                            queue.push(next);
                        }
                    }
                    continue;
                }
                ControlOutcome::EndJob { status, message } => {
                    executed.insert(node_id.clone());
                    final_status = status;
                    first_error = Some(message);
                    break;
                }
            }
        }
        let started_at = Utc::now();
        update_node_status(pool, job_id, &node_id, "running", None).await?;
        // Resolve + mask input for storage.
        let input_masked = match ctx.resolve(&node.config) {
            Ok(v) => mask_secrets(&v),
            Err(_) => mask_secrets(&node.config),
        };
        let input_json = serde_json::to_string(&input_masked).unwrap_or_default();
        set_node_input(pool, job_id, &node_id, &truncate_text(&input_json, 8000)).await?;

        let node_started = std::time::Instant::now();
        let result = execute_node(
            pool,
            job_id,
            &node_id,
            &node.r#type,
            &node.config,
            ctx,
            // Per-node timeout override wins (common::node_timeout handles it inside executors
            // for process nodes; for others default is used here).
            default_timeout_secs,
            default_max_attempts,
            security,
        )
        .await;
        let duration_ms = node_started.elapsed().as_millis() as i64;
        match result {
            Ok((exec_result, attempts)) => {
                ctx.outputs.insert(node_id.clone(), exec_result.outputs.clone());
                let output_masked = mask_secrets(&serde_json::Value::Object(
                    exec_result
                        .outputs
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ));
                let output_json =
                    serde_json::to_string(&output_masked).unwrap_or_default();
                finish_node_run(
                    pool,
                    job_id,
                    &node_id,
                    "success",
                    attempts,
                    started_at,
                    duration_ms,
                    Some(truncate_text(&output_json, 8000)),
                    None,
                )
                .await?;
                executed.insert(node_id.clone());
                // Enqueue successors via taken port only.
                let port = if exec_result.taken_port.is_empty() {
                    String::new()
                } else {
                    exec_result.taken_port.clone()
                };
                if !port.is_empty() {
                    for next in graph.next_via(&node_id, &port) {
                        if !executed.contains(&next) {
                            queue.push(next);
                        }
                    }
                    // For branch nodes, the non-taken branch subtrees become skipped.
                    if node.r#type == "logic.condition" {
                        let other = if port == "true" { "false" } else { "true" };
                        let pruned = graph.next_via(&node_id, other);
                        for root in pruned {
                            mark_subtree_skipped(pool, job_id, &graph, &root, &mut skipped, &executed).await?;
                        }
                    } else if node.r#type == "logic.switch" {
                        // Prune every outgoing port except the taken one.
                        if let Some(outgoing) = graph.outgoing.get(&node_id) {
                            let mut seen_ports = std::collections::HashSet::new();
                            for (p, _) in outgoing {
                                seen_ports.insert(p.clone());
                            }
                            for other in seen_ports {
                                if other == port {
                                    continue;
                                }
                                for root in graph.next_via(&node_id, &other) {
                                    mark_subtree_skipped(pool, job_id, &graph, &root, &mut skipped, &executed).await?;
                                }
                            }
                        }
                    }
                }
            }
            Err(AppError::Cancelled(msg)) => {
                finish_node_run(
                    pool,
                    job_id,
                    &node_id,
                    "cancelled",
                    1,
                    started_at,
                    duration_ms,
                    None,
                    Some(msg),
                )
                .await?;
                mark_remaining_cancelled(pool, job_id, def, &executed).await?;
                return Ok("cancelled".into());
            }
            Err(AppError::Timeout) => {
                finish_node_run(
                    pool,
                    job_id,
                    &node_id,
                    "timeout",
                    nodes::common::node_max_attempts(&node.config, default_max_attempts),
                    started_at,
                    duration_ms,
                    None,
                    Some("node timed out".into()),
                )
                .await?;
                executed.insert(node_id.clone());
                final_status = "timeout".into();
                first_error = Some(format!("node '{node_id}' timed out"));
                break;
            }
            Err(e) => {
                let msg = truncate_text(&e.to_string(), 4000);
                finish_node_run(
                    pool,
                    job_id,
                    &node_id,
                    "failed",
                    nodes::common::node_max_attempts(&node.config, default_max_attempts),
                    started_at,
                    duration_ms,
                    None,
                    Some(msg.clone()),
                )
                .await?;
                executed.insert(node_id.clone());
                final_status = "failed".into();
                first_error = Some(format!("node '{node_id}': {msg}"));
                break; // failure_policy stop
            }
        }
    }

    // Anything never executed and not explicitly skipped is unreachable → skipped.
    for n in &def.nodes {
        if !executed.contains(&n.id) && !skipped.contains(&n.id) {
            // If final_status is failed/timeout, downstream nodes are skipped by policy.
            update_node_status(pool, job_id, &n.id, "skipped", Some("not reached")).await?;
        }
    }

    if final_status != "success" {
        if let Some(err) = first_error {
            update_job_finished(pool, job_id, &final_status, Some(err)).await?;
        } else {
            update_job_finished(pool, job_id, &final_status, None).await?;
        }
    } else {
        update_job_finished(pool, job_id, "success", None).await?;
    }
    Ok(final_status)
}

/// Row-id based persistence for control-flow bodies (for_each iterations
/// use suffixed labels like `do_thing[0]` so each iteration keeps history).
async fn upsert_row(
    pool: &SqlitePool,
    row_id: &str,
    job_id: &str,
    node_label: &str,
    node_type: &str,
    status: &str,
    attempt: u32,
    max_attempts: u32,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO workflow_node_runs (id, job_id, node_id, node_type, status, attempt, max_attempts, input_json, output_json, error, log_ref) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL) ON CONFLICT(id) DO UPDATE SET status=excluded.status, attempt=excluded.attempt, max_attempts=excluded.max_attempts",
    )
    .bind(row_id)
    .bind(job_id)
    .bind(node_label)
    .bind(node_type)
    .bind(status)
    .bind(attempt as i64)
    .bind(max_attempts as i64)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| AppError::Storage(e.to_string()))
}

async fn update_row_status(
    pool: &SqlitePool,
    row_id: &str,
    status: &str,
    error: Option<&str>,
    started: bool,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    if started {
        sqlx::query("UPDATE workflow_node_runs SET status=?, started_at=? WHERE id=?")
            .bind(status)
            .bind(&now)
            .bind(row_id)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| AppError::Storage(e.to_string()))
    } else if let Some(err) = error {
        sqlx::query("UPDATE workflow_node_runs SET status=?, error=? WHERE id=?")
            .bind(status)
            .bind(err)
            .bind(row_id)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| AppError::Storage(e.to_string()))
    } else {
        sqlx::query("UPDATE workflow_node_runs SET status=? WHERE id=?")
            .bind(status)
            .bind(row_id)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| AppError::Storage(e.to_string()))
    }
}

async fn set_row_input(pool: &SqlitePool, row_id: &str, input_json: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE workflow_node_runs SET input_json=? WHERE id=?")
        .bind(input_json)
        .bind(row_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

#[allow(clippy::too_many_arguments)]
async fn finish_row(
    pool: &SqlitePool,
    row_id: &str,
    status: &str,
    attempt: u32,
    started_at: chrono::DateTime<Utc>,
    duration_ms: i64,
    output_json: Option<String>,
    error: Option<String>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE workflow_node_runs SET status=?, attempt=?, finished_at=?, duration_ms=?, output_json=COALESCE(?, output_json), error=COALESCE(?, error), started_at=COALESCE(started_at, ?) WHERE id=?")
        .bind(status)
        .bind(attempt as i64)
        .bind(&now)
        .bind(duration_ms)
        .bind(output_json)
        .bind(error)
        .bind(started_at.to_rfc3339())
        .bind(row_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

fn row_label(base: &str, suffix: Option<&str>) -> String {
    match suffix {
        Some(s) if !s.is_empty() => format!("{base}{s}"),
        _ => base.to_string(),
    }
}

fn row_id_for(job_id: &str, base: &str, suffix: Option<&str>) -> String {
    format!("{job_id}:{}", row_label(base, suffix))
}

/// Nodes reachable from `entries` without passing through `stop`.
fn reachable_excluding(
    graph: &Graph<'_>,
    entries: &[String],
    stop: &HashSet<String>,
) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack: Vec<String> = entries.to_vec();
    while let Some(n) = stack.pop() {
        if stop.contains(&n) || !seen.insert(n.clone()) {
            continue;
        }
        for next in graph.downstream(&n) {
            if !stop.contains(&next) {
                stack.push(next);
            }
        }
    }
    seen
}

/// Deterministic topological order restricted to `subset`.
fn topo_sort_subset(def: &WorkflowDefinition, subset: &HashSet<String>) -> Vec<String> {
    let mut indeg: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for id in subset {
        indeg.insert(id.clone(), 0);
    }
    for e in &def.edges {
        if subset.contains(&e.source) && subset.contains(&e.target) {
            if let Some(d) = indeg.get_mut(&e.target) {
                *d += 1;
            }
            adj.entry(e.source.clone()).or_default().push(e.target.clone());
        }
    }
    let mut queue: Vec<String> = indeg
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(k, _)| k.clone())
        .collect();
    queue.sort();
    let mut order = Vec::new();
    while let Some(n) = queue.pop() {
        order.push(n.clone());
        if let Some(nexts) = adj.get(&n) {
            let mut sorted = nexts.clone();
            sorted.sort();
            for m in sorted {
                if let Some(d) = indeg.get_mut(&m) {
                    *d -= 1;
                    if *d == 0 {
                        queue.push(m);
                    }
                }
            }
        }
    }
    order
}

/// Execute a control-flow body (no nested for_each/parallel in v1).
/// Persists rows with `id_suffix` (e.g. "[0]" per iteration, None for parallel).
/// Returns the base IDs executed/skipped inside the body.
#[allow(clippy::too_many_arguments)]
async fn run_body_subset(
    pool: &SqlitePool,
    job_id: &str,
    def: &WorkflowDefinition,
    graph: &Graph<'_>,
    ctx: &mut RunContext,
    order: &[String],
    subset: &HashSet<String>,
    default_timeout_secs: u64,
    default_max_attempts: u32,
    security: &SecurityPolicy,
    id_suffix: Option<&str>,
) -> Result<(HashSet<String>, HashSet<String>), AppError> {
    let mut executed: HashSet<String> = HashSet::new();
    let mut skipped: HashSet<String> = HashSet::new();
    for base in order {
        if executed.contains(base) || skipped.contains(base) {
            continue;
        }
        let node = def.node(base).cloned();
        let Some(node) = node else { continue };
        if node.r#type == "flow.for_each" || node.r#type == "flow.parallel" {
            return Err(AppError::Validation(format!(
                "nested '{}' inside a loop/parallel body is not supported in v1 (node '{base}')",
                node.r#type
            )));
        }
        if is_cancel_requested(pool, job_id).await? {
            return Err(AppError::Cancelled(format!("job {job_id} was cancelled")));
        }
        let label = row_label(base, id_suffix);
        let row_id = row_id_for(job_id, base, id_suffix);
        let started_at = Utc::now();
        upsert_row(
            pool,
            &row_id,
            job_id,
            &label,
            &node.r#type,
            "pending",
            1,
            nodes::common::node_max_attempts(&node.config, default_max_attempts),
        )
        .await?;
        update_row_status(pool, &row_id, "running", None, true).await?;
        let input_masked = match ctx.resolve(&node.config) {
            Ok(v) => mask_secrets(&v),
            Err(_) => mask_secrets(&node.config),
        };
        let input_json = serde_json::to_string(&input_masked).unwrap_or_default();
        set_row_input(pool, &row_id, &truncate_text(&input_json, 8000)).await?;

        let t0 = std::time::Instant::now();
        let result = execute_node(
            pool, job_id, base, &node.r#type, &node.config, ctx,
            default_timeout_secs, default_max_attempts, security,
        )
        .await;
        let duration_ms = t0.elapsed().as_millis() as i64;
        match result {
            Ok((exec_result, attempts)) => {
                ctx.outputs.insert(base.clone(), exec_result.outputs.clone());
                let output_masked = mask_secrets(&serde_json::Value::Object(
                    exec_result.outputs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                ));
                let output_json = serde_json::to_string(&output_masked).unwrap_or_default();
                finish_row(
                    pool, &row_id, "success", attempts, started_at, duration_ms,
                    Some(truncate_text(&output_json, 8000)), None,
                )
                .await?;
                executed.insert(base.clone());
                // Scoped branch pruning (condition/switch only).
                let port = exec_result.taken_port.clone();
                if !port.is_empty() {
                    if node.r#type == "logic.condition" {
                        let other = if port == "true" { "false" } else { "true" };
                        for root in graph.next_via(base, other) {
                            mark_subset_skipped(pool, job_id, graph, &root, subset, &mut skipped, &executed, id_suffix).await?;
                        }
                    } else if node.r#type == "logic.switch" {
                        if let Some(outgoing) = graph.outgoing.get(base) {
                            let mut ports = HashSet::new();
                            for (p, _) in outgoing {
                                ports.insert(p.clone());
                            }
                            for other in ports {
                                if other == port {
                                    continue;
                                }
                                for root in graph.next_via(base, &other) {
                                    mark_subset_skipped(pool, job_id, graph, &root, subset, &mut skipped, &executed, id_suffix).await?;
                                }
                            }
                        }
                    }
                }
            }
            Err(AppError::Cancelled(msg)) => {
                finish_row(pool, &row_id, "cancelled", 1, started_at, duration_ms, None, Some(msg.clone())).await?;
                return Err(AppError::Cancelled(msg));
            }
            Err(AppError::Timeout) => {
                finish_row(
                    pool, &row_id, "timeout",
                    nodes::common::node_max_attempts(&node.config, default_max_attempts),
                    started_at, duration_ms, None, Some("node timed out".into()),
                )
                .await?;
                executed.insert(base.clone());
                return Err(AppError::Timeout);
            }
            Err(e) => {
                let msg = truncate_text(&e.to_string(), 4000);
                finish_row(
                    pool, &row_id, "failed",
                    nodes::common::node_max_attempts(&node.config, default_max_attempts),
                    started_at, duration_ms, None, Some(msg.clone()),
                )
                .await?;
                executed.insert(base.clone());
                return Err(AppError::Action(format!("node '{base}': {msg}")));
            }
        }
    }
    Ok((executed, skipped))
}

async fn mark_subset_skipped(
    pool: &SqlitePool,
    job_id: &str,
    graph: &Graph<'_>,
    root: &str,
    subset: &HashSet<String>,
    skipped: &mut HashSet<String>,
    executed: &HashSet<String>,
    id_suffix: Option<&str>,
) -> Result<(), AppError> {
    let mut stack = vec![root.to_string()];
    while let Some(n) = stack.pop() {
        if !subset.contains(&n) || executed.contains(&n) || !skipped.insert(n.clone()) {
            continue;
        }
        let label = row_label(&n, id_suffix);
        let row_id = row_id_for(job_id, &n, id_suffix);
        // Ensure a row exists even if the node was never reached in this scope.
        // The real node type is recovered from the graph definition.
        let node_type = graph
            .def
            .node(&n)
            .map(|x| x.r#type.clone())
            .unwrap_or_else(|| "unknown".into());
        upsert_row(pool, &row_id, job_id, &label, &node_type, "pending", 1, 1).await.ok();
        update_row_status(pool, &row_id, "skipped", Some("branch not taken"), false).await?;
        for next in graph.downstream(&n) {
            stack.push(next);
        }
    }
    Ok(())
}

/// Outcome of a control node: continue with done targets, or end the job.
enum ControlOutcome {
    Continue { done_targets: Vec<String> },
    EndJob { status: String, message: String },
}

/// flow.for_each driver: iterate `items`, run the `next` body per item,
/// then continue at `done`. Per-iteration rows are suffixed `[i]`.
#[allow(clippy::too_many_arguments)]
async fn run_for_each_node(
    pool: &SqlitePool,
    job_id: &str,
    def: &WorkflowDefinition,
    graph: &Graph<'_>,
    node_id: &str,
    node_config: &serde_json::Value,
    ctx: &mut RunContext,
    default_timeout_secs: u64,
    default_max_attempts: u32,
    security: &SecurityPolicy,
    executed_main: &mut HashSet<String>,
    skipped_main: &mut HashSet<String>,
) -> Result<ControlOutcome, AppError> {
    let started_at = Utc::now();
    update_node_status(pool, job_id, node_id, "running", None).await?;
    let input_masked = match ctx.resolve(node_config) {
        Ok(v) => mask_secrets(&v),
        Err(_) => mask_secrets(node_config),
    };
    set_node_input(
        pool, job_id, node_id,
        &truncate_text(&serde_json::to_string(&input_masked).unwrap_or_default(), 8000),
    )
    .await?;
    let t0 = std::time::Instant::now();

    let items = match nodes::flow::resolve_items(node_config, ctx) {
        Ok(v) => v,
        Err(e) => {
            let msg = truncate_text(&e.to_string(), 4000);
            finish_node_run(pool, job_id, node_id, "failed",
                nodes::common::node_max_attempts(node_config, default_max_attempts),
                started_at, t0.elapsed().as_millis() as i64, None, Some(msg.clone())).await?;
            return Ok(ControlOutcome::EndJob { status: "failed".into(), message: format!("node '{node_id}': {msg}") });
        }
    };
    let entries = graph.next_via(node_id, "next");
    let done_targets = graph.next_via(node_id, "done");
    let stop: HashSet<String> = done_targets.iter().cloned().chain(std::iter::once(node_id.to_string())).collect();
    let subset = reachable_excluding(graph, &entries, &stop);
    let order = topo_sort_subset(def, &subset);
    let count = items.len();

    if count > 0 && !subset.is_empty() {
        for (idx, item) in items.iter().enumerate() {
            if is_cancel_requested(pool, job_id).await? {
                finish_node_run(pool, job_id, node_id, "cancelled", 1, started_at,
                    t0.elapsed().as_millis() as i64, None, Some("cancelled by user".into())).await?;
                mark_remaining_cancelled(pool, job_id, def, &HashSet::from([node_id.to_string()])).await.ok();
                return Ok(ControlOutcome::EndJob { status: "cancelled".into(), message: "cancelled by user".into() });
            }
            // Loop variables for this iteration.
            let mut loop_out = HashMap::new();
            loop_out.insert("item".into(), item.clone());
            loop_out.insert("index".into(), serde_json::json!(idx));
            loop_out.insert("count".into(), serde_json::json!(count));
            ctx.outputs.insert(node_id.to_string(), loop_out);
            let suffix = format!("[{idx}]");
            match run_body_subset(
                pool, job_id, def, graph, ctx, &order, &subset,
                default_timeout_secs, default_max_attempts, security, Some(&suffix),
            )
            .await
            {
                Ok(_) => {}
                Err(AppError::Cancelled(msg)) => {
                    finish_node_run(pool, job_id, node_id, "cancelled", 1, started_at,
                        t0.elapsed().as_millis() as i64, None, Some(msg.clone())).await?;
                    return Ok(ControlOutcome::EndJob { status: "cancelled".into(), message: msg });
                }
                Err(AppError::Timeout) => {
                    finish_node_run(pool, job_id, node_id, "timeout",
                        nodes::common::node_max_attempts(node_config, default_max_attempts),
                        started_at, t0.elapsed().as_millis() as i64, None,
                        Some(format!("node '{node_id}' iteration {idx} timed out"))).await?;
                    return Ok(ControlOutcome::EndJob { status: "timeout".into(), message: format!("node '{node_id}' timed out") });
                }
                Err(e) => {
                    let msg = truncate_text(&e.to_string(), 4000);
                    finish_node_run(pool, job_id, node_id, "failed",
                        nodes::common::node_max_attempts(node_config, default_max_attempts),
                        started_at, t0.elapsed().as_millis() as i64, None, Some(msg.clone())).await?;
                    return Ok(ControlOutcome::EndJob { status: "failed".into(), message: format!("node '{node_id}': {msg}") });
                }
            }
        }
    }
    // Final loop outputs + base-row summaries for body nodes.
    let mut out = HashMap::new();
    out.insert("count".into(), serde_json::json!(count));
    out.insert("items".into(), serde_json::Value::Array(items));
    ctx.outputs.insert(node_id.to_string(), out.clone());
    for base in &subset {
        executed_main.insert(base.clone());
        skipped_main.remove(base);
    }
    let output_json = serde_json::to_string(&mask_secrets(&serde_json::Value::Object(
        out.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    )))
    .unwrap_or_default();
    finish_node_run(pool, job_id, node_id, "success", 1, started_at,
        t0.elapsed().as_millis() as i64, Some(truncate_text(&output_json, 8000)), None).await?;
    // Summarize body base rows so no `pending` rows remain.
    for base in &subset {
        let node_type = def.node(base).map(|n| n.r#type.clone()).unwrap_or_else(|| "unknown".into());
        upsert_node_run(pool, job_id, base, &node_type, "success", 1,
            nodes::common::node_max_attempts(
                &def.node(base).map(|n| n.config.clone()).unwrap_or(serde_json::Value::Null),
                default_max_attempts,
            ),
            None,
            Some(format!("{{\"iterations\": {count}}}")),
            Some("loop summary; see indexed runs".into()), None).await.ok();
    }
    Ok(ControlOutcome::Continue { done_targets })
}

/// flow.parallel driver: run all branch subgraphs (v1: sequentially in port
/// order for deterministic SQLite persistence), then continue at `done`.
#[allow(clippy::too_many_arguments)]
async fn run_parallel_node(
    pool: &SqlitePool,
    job_id: &str,
    def: &WorkflowDefinition,
    graph: &Graph<'_>,
    node_id: &str,
    node_config: &serde_json::Value,
    ctx: &mut RunContext,
    default_timeout_secs: u64,
    default_max_attempts: u32,
    security: &SecurityPolicy,
    executed_main: &mut HashSet<String>,
    skipped_main: &mut HashSet<String>,
) -> Result<ControlOutcome, AppError> {
    let started_at = Utc::now();
    update_node_status(pool, job_id, node_id, "running", None).await?;
    let input_masked = match ctx.resolve(node_config) {
        Ok(v) => mask_secrets(&v),
        Err(_) => mask_secrets(node_config),
    };
    set_node_input(
        pool, job_id, node_id,
        &truncate_text(&serde_json::to_string(&input_masked).unwrap_or_default(), 8000),
    )
    .await?;
    let t0 = std::time::Instant::now();

    let (wait_for_all, max_branches) = match nodes::flow::parallel_policy(node_config) {
        Ok(v) => v,
        Err(e) => {
            let msg = truncate_text(&e.to_string(), 4000);
            finish_node_run(pool, job_id, node_id, "failed",
                nodes::common::node_max_attempts(node_config, default_max_attempts),
                started_at, t0.elapsed().as_millis() as i64, None, Some(msg.clone())).await?;
            return Ok(ControlOutcome::EndJob { status: "failed".into(), message: format!("node '{node_id}': {msg}") });
        }
    };
    let done_targets = graph.next_via(node_id, "done");
    let stop: HashSet<String> = done_targets.iter().cloned().chain(std::iter::once(node_id.to_string())).collect();
    // Branches = all successors except via `done`.
    let mut branch_entries: Vec<String> = Vec::new();
    if let Some(outgoing) = graph.outgoing.get(node_id) {
        let mut sorted = outgoing.clone();
        sorted.sort();
        for (port, target) in sorted {
            if port == "done" || stop.contains(&target) && done_targets.contains(&target) {
                continue;
            }
            if !branch_entries.contains(&target) {
                branch_entries.push(target);
            }
        }
    }
    if branch_entries.len() as u64 > max_branches {
        let msg = format!("flow.parallel: {} branches exceed max_branches={max_branches}", branch_entries.len());
        finish_node_run(pool, job_id, node_id, "failed",
            nodes::common::node_max_attempts(node_config, default_max_attempts),
            started_at, t0.elapsed().as_millis() as i64, None, Some(msg.clone())).await?;
        return Ok(ControlOutcome::EndJob { status: "failed".into(), message: format!("node '{node_id}': {msg}") });
    }
    if branch_entries.is_empty() {
        let mut out = HashMap::new();
        out.insert("branches".into(), serde_json::json!(0));
        ctx.outputs.insert(node_id.to_string(), out.clone());
        finish_node_run(pool, job_id, node_id, "success", 1, started_at,
            t0.elapsed().as_millis() as i64, Some(r#"{"branches": 0}"#.into()), None).await?;
        return Ok(ControlOutcome::Continue { done_targets });
    }
    let _ = wait_for_all; // v1 always waits for all branches, then fails if any failed.
    let mut first_failure: Option<String> = None;
    let mut completed_branches = 0u64;
    // Track body nodes globally so the final pass doesn't mark them skipped.
    let mut all_body: HashSet<String> = HashSet::new();
    for entry in &branch_entries {
        if is_cancel_requested(pool, job_id).await? {
            finish_node_run(pool, job_id, node_id, "cancelled", 1, started_at,
                t0.elapsed().as_millis() as i64, None, Some("cancelled by user".into())).await?;
            return Ok(ControlOutcome::EndJob { status: "cancelled".into(), message: "cancelled by user".into() });
        }
        let subset = reachable_excluding(graph, std::slice::from_ref(entry), &stop);
        // Skip branches already covered by an earlier branch (shared join).
        let fresh: HashSet<String> = subset.difference(&all_body).cloned().collect();
        if fresh.is_empty() {
            continue;
        }
        let order = topo_sort_subset(def, &fresh);
        match run_body_subset(
            pool, job_id, def, graph, ctx, &order, &fresh,
            default_timeout_secs, default_max_attempts, security, None,
        )
        .await
        {
            Ok((exec_b, skip_b)) => {
                for n in exec_b.union(&skip_b) {
                    all_body.insert(n.clone());
                }
                for n in exec_b {
                    executed_main.insert(n.clone());
                    skipped_main.remove(&n);
                }
                for n in skip_b {
                    if !executed_main.contains(&n) {
                        skipped_main.insert(n);
                    }
                }
                completed_branches += 1;
            }
            Err(AppError::Cancelled(msg)) => {
                finish_node_run(pool, job_id, node_id, "cancelled", 1, started_at,
                    t0.elapsed().as_millis() as i64, None, Some(msg.clone())).await?;
                return Ok(ControlOutcome::EndJob { status: "cancelled".into(), message: msg });
            }
            Err(AppError::Timeout) => {
                if first_failure.is_none() {
                    first_failure = Some(format!("node '{node_id}': a parallel branch timed out"));
                }
                completed_branches += 1;
            }
            Err(e) => {
                if first_failure.is_none() {
                    first_failure = Some(truncate_text(&e.to_string(), 4000));
                }
                completed_branches += 1;
            }
        }
    }
    if let Some(msg) = first_failure {
        finish_node_run(pool, job_id, node_id, "failed",
            nodes::common::node_max_attempts(node_config, default_max_attempts),
            started_at, t0.elapsed().as_millis() as i64, None, Some(msg.clone())).await?;
        return Ok(ControlOutcome::EndJob { status: "failed".into(), message: format!("node '{node_id}': {msg}") });
    }
    let mut out = HashMap::new();
    out.insert("branches".into(), serde_json::json!(completed_branches));
    ctx.outputs.insert(node_id.to_string(), out);
    finish_node_run(pool, job_id, node_id, "success", 1, started_at,
        t0.elapsed().as_millis() as i64,
        Some(format!(r#"{{"branches": {completed_branches}}}"#)), None).await?;
    Ok(ControlOutcome::Continue { done_targets })
}

async fn mark_subtree_skipped(
    pool: &SqlitePool,
    job_id: &str,
    graph: &Graph<'_>,
    root: &str,
    skipped: &mut HashSet<String>,
    executed: &HashSet<String>,
) -> Result<(), AppError> {
    let mut stack = vec![root.to_string()];
    while let Some(n) = stack.pop() {
        if executed.contains(&n) || !skipped.insert(n.clone()) {
            continue;
        }
        update_node_status(pool, job_id, &n, "skipped", Some("branch not taken")).await?;
        for next in graph.downstream(&n) {
            stack.push(next);
        }
    }
    Ok(())
}

async fn mark_remaining_cancelled(
    pool: &SqlitePool,
    job_id: &str,
    def: &WorkflowDefinition,
    executed: &HashSet<String>,
) -> Result<(), AppError> {
    for n in &def.nodes {
        if !executed.contains(&n.id) {
            update_node_status(pool, job_id, &n.id, "cancelled", Some("job cancelled")).await?;
        }
    }
    update_job_finished(pool, job_id, "cancelled", Some("cancelled by user".into())).await?;
    Ok(())
}

// --- persistence helpers (no open transaction across process waits) ---

async fn upsert_node_run(
    pool: &SqlitePool,
    job_id: &str,
    node_id: &str,
    node_type: &str,
    status: &str,
    attempt: u32,
    max_attempts: u32,
    input_json: Option<String>,
    output_json: Option<String>,
    error: Option<String>,
    log_ref: Option<String>,
) -> Result<(), AppError> {
    let id = format!("{job_id}:{node_id}");
    sqlx::query(
        "INSERT INTO workflow_node_runs (id, job_id, node_id, node_type, status, attempt, max_attempts, input_json, output_json, error, log_ref) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET status=excluded.status, attempt=excluded.attempt, max_attempts=excluded.max_attempts",
    )
    .bind(&id)
    .bind(job_id)
    .bind(node_id)
    .bind(node_type)
    .bind(status)
    .bind(attempt as i64)
    .bind(max_attempts as i64)
    .bind(input_json)
    .bind(output_json)
    .bind(error)
    .bind(log_ref)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| AppError::Storage(e.to_string()))
}

async fn update_node_status(
    pool: &SqlitePool,
    job_id: &str,
    node_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), AppError> {
    let id = format!("{job_id}:{node_id}");
    let now = Utc::now().to_rfc3339();
    if status == "running" {
        sqlx::query("UPDATE workflow_node_runs SET status=?, started_at=? WHERE id=?")
            .bind(status)
            .bind(&now)
            .bind(&id)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| AppError::Storage(e.to_string()))
    } else {
        if let Some(err) = error {
            sqlx::query("UPDATE workflow_node_runs SET status=?, error=? WHERE id=?")
                .bind(status)
                .bind(err)
                .bind(&id)
                .execute(pool)
                .await
                .map(|_| ())
                .map_err(|e| AppError::Storage(e.to_string()))
        } else {
            sqlx::query("UPDATE workflow_node_runs SET status=? WHERE id=?")
                .bind(status)
                .bind(&id)
                .execute(pool)
                .await
                .map(|_| ())
                .map_err(|e| AppError::Storage(e.to_string()))
        }
    }
}

async fn set_node_input(
    pool: &SqlitePool,
    job_id: &str,
    node_id: &str,
    input_json: &str,
) -> Result<(), AppError> {
    let id = format!("{job_id}:{node_id}");
    sqlx::query("UPDATE workflow_node_runs SET input_json=? WHERE id=?")
        .bind(input_json)
        .bind(&id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

#[allow(clippy::too_many_arguments)]
async fn finish_node_run(
    pool: &SqlitePool,
    job_id: &str,
    node_id: &str,
    status: &str,
    attempt: u32,
    started_at: chrono::DateTime<Utc>,
    duration_ms: i64,
    output_json: Option<String>,
    error: Option<String>,
) -> Result<(), AppError> {
    let id = format!("{job_id}:{node_id}");
    let now = Utc::now().to_rfc3339();
    let _ = Uuid::new_v4();
    sqlx::query("UPDATE workflow_node_runs SET status=?, attempt=?, finished_at=?, duration_ms=?, output_json=COALESCE(?, output_json), error=COALESCE(?, error), started_at=COALESCE(started_at, ?) WHERE id=?")
        .bind(status)
        .bind(attempt as i64)
        .bind(&now)
        .bind(duration_ms)
        .bind(output_json)
        .bind(error)
        .bind(started_at.to_rfc3339())
        .bind(&id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

async fn update_job_finished(
    pool: &SqlitePool,
    job_id: &str,
    status: &str,
    error: Option<String>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    // execution_history row keyed by job_id; attempt/duration updated best-effort.
    let started: Option<(String,)> =
        sqlx::query_as("SELECT started_at FROM execution_history WHERE job_id = ?")
            .bind(job_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
    let duration_ms: Option<i64> = started.and_then(|row| {
        chrono::DateTime::parse_from_rfc3339(&row.0)
            .ok()
            .map(|dt| (Utc::now() - dt.with_timezone(&Utc)).num_milliseconds())
    });
    sqlx::query("UPDATE execution_history SET status=?, error=COALESCE(?, error), finished_at=?, duration_ms=COALESCE(?, duration_ms) WHERE job_id=?")
        .bind(status)
        .bind(error)
        .bind(&now)
        .bind(duration_ms)
        .bind(job_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| AppError::Storage(e.to_string()))
}

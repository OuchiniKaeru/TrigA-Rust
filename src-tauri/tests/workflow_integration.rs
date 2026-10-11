use automation_manager::{
    engine::application_service::AppState,
    services::{security::SecurityPolicy, workflow_service},
    storage::{database, repository, workflow_repository, workflow_run_repository},
    workflow::{validator, yaml_codec},
};
use std::{collections::HashMap, sync::Arc, time::Duration};

async fn test_state() -> Arc<AppState> {
    let pool = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&pool).await.unwrap();
    Arc::new(AppState::new(pool, SecurityPolicy::default()))
}

fn minimal_yaml(id: &str) -> String {
    format!(
        r#"
schema_version: 1
workflow:
  id: "{id}"
  name: "demo {id}"
  description: ""
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
  retry_delay_seconds: 1
  retry_backoff: "fixed"
  max_retry_delay_seconds: 10
  failure_policy: "stop"
nodes:
  - id: "start"
    type: "trigger.manual"
    position: {{ x: 80, y: 160 }}
    config: {{}}
  - id: "run"
    type: "command.exec"
    position: {{ x: 320, y: 160 }}
    config:
      program: "echo"
      args: ["hello"]
  - id: "finish"
    type: "flow.end"
    position: {{ x: 560, y: 160 }}
    config: {{}}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "run"
    target_port: "in"
  - id: "e2"
    source: "run"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#
    )
}

async fn wait_for_job(state: &Arc<AppState>, job_id: &str) {
    for _ in 0..200 {
        let detail = workflow_run_repository::get_run_detail(&state.pool, job_id)
            .await
            .unwrap();
        if !matches!(detail.job.status.as_str(), "running" | "retrying" | "pending") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {job_id} did not finish in time");
}

#[tokio::test]
async fn yaml_roundtrip_and_validation() {
    let yaml = minimal_yaml("wf-roundtrip");
    let def = yaml_codec::parse(&yaml).unwrap();
    let back = yaml_codec::parse(&yaml_codec::serialize(&def).unwrap()).unwrap();
    assert_eq!(def, back);
    let report = validator::validate(&def).unwrap();
    assert!(report.valid, "errors: {:?}", report.errors);
}

#[tokio::test]
async fn rejects_duplicate_node_and_cycle_and_bad_ref() {
    let mut def = yaml_codec::parse(&minimal_yaml("wf-bad")).unwrap();
    // Duplicate node id.
    def.nodes.push(def.nodes[0].clone());
    let report = validator::validate(&def).unwrap();
    assert!(!report.valid);
    assert!(report.errors.iter().any(|e| e.contains("duplicate node")));

    // Cycle.
    let mut def = yaml_codec::parse(&minimal_yaml("wf-cycle")).unwrap();
    def.edges.push(automation_manager::workflow::model::WorkflowEdge {
        id: "e-cycle".into(),
        source: "finish".into(),
        source_port: "next".into(),
        target: "start".into(),
        target_port: "in".into(),
    });
    // finish has no "next" port either, so at least one error must appear.
    let report = validator::validate(&def).unwrap();
    assert!(!report.valid);

    // Unknown node type.
    let mut def = yaml_codec::parse(&minimal_yaml("wf-unknown")).unwrap();
    def.nodes[1].r#type = "http.fetch".into();
    let report = validator::validate(&def).unwrap();
    assert!(!report.valid);

    // Out-of-range policy.
    let mut def = yaml_codec::parse(&minimal_yaml("wf-policy")).unwrap();
    def.settings.default_max_attempts = 99;
    let report = validator::validate(&def).unwrap();
    assert!(!report.valid);
}

#[tokio::test]
async fn crud_version_conflict_and_restore() {
    let state = test_state().await;
    let rec = workflow_repository::create_from_yaml(&state.pool, &minimal_yaml("wf-crud"), None)
        .await
        .unwrap();
    assert_eq!(rec.version, 1);
    // Update.
    let yaml2 = minimal_yaml("wf-crud").replace("demo wf-crud", "demo v2");
    let updated = workflow_repository::update_from_yaml(
        &state.pool,
        "wf-crud",
        &yaml2,
        Some(1),
        Some("rename"),
    )
    .await
    .unwrap();
    assert_eq!(updated.version, 2);
    // Conflict.
    let conflict =
        workflow_repository::update_from_yaml(&state.pool, "wf-crud", &yaml2, Some(1), None).await;
    assert!(conflict.is_err());
    // Restore v1 as new version.
    let restored = workflow_repository::restore_version(&state.pool, "wf-crud", 1)
        .await
        .unwrap();
    assert_eq!(restored.version, 3);
    let versions = workflow_repository::list_versions(&state.pool, "wf-crud")
        .await
        .unwrap();
    assert_eq!(versions.len(), 3);
    // Duplicate creates new id.
    let dup = workflow_repository::duplicate(&state.pool, "wf-crud").await.unwrap();
    assert_ne!(dup.id, "wf-crud");
    // Logical delete.
    workflow_repository::delete(&state.pool, "wf-crud").await.unwrap();
    assert!(workflow_repository::get(&state.pool, "wf-crud").await.is_err());
    let _ = dup;
}

#[tokio::test]
async fn import_policies_and_no_execution_on_import() {
    let state = test_state().await;
    let yaml = minimal_yaml("wf-import");
    let rec = workflow_repository::import_yaml(&state.pool, &yaml, "cancel")
        .await
        .unwrap();
    assert_eq!(rec.id, "wf-import");
    // Import alone must not create history rows.
    let history = repository::list_history(&state.pool, 20).await.unwrap();
    assert!(history.is_empty());
    // Second import with cancel conflicts.
    assert!(workflow_repository::import_yaml(&state.pool, &yaml, "cancel")
        .await
        .is_err());
    // new_id creates a separate workflow.
    let rec2 = workflow_repository::import_yaml(&state.pool, &yaml, "new_id")
        .await
        .unwrap();
    assert_ne!(rec2.id, "wf-import");
    // update overwrites.
    let rec3 = workflow_repository::import_yaml(&state.pool, &yaml, "update")
        .await
        .unwrap();
    assert_eq!(rec3.id, "wf-import");
}

#[tokio::test]
async fn manual_command_run_succeeds_and_records_nodes() {
    let state = test_state().await;
    workflow_repository::create_from_yaml(&state.pool, &minimal_yaml("wf-run"), None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-run", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let detail = workflow_run_repository::get_run_detail(&state.pool, &job_id)
        .await
        .unwrap();
    assert_eq!(detail.job.status, "success");
    assert_eq!(detail.workflow_id.as_deref(), Some("wf-run"));
    assert!(detail.definition_snapshot_yaml.is_some());
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert_eq!(nodes.len(), 3);
    assert!(nodes.iter().all(|n| n.status == "success"));
    // Snapshot is fixed: editing the definition afterwards does not change history.
    let yaml2 = minimal_yaml("wf-run").replace("demo wf-run", "changed");
    workflow_repository::update_from_yaml(&state.pool, "wf-run", &yaml2, None, None)
        .await
        .unwrap();
    let detail2 = workflow_run_repository::get_run_detail(&state.pool, &job_id)
        .await
        .unwrap();
    assert!(detail2
        .definition_snapshot_yaml
        .unwrap()
        .contains("demo wf-run"));
}

#[tokio::test]
async fn condition_branches_and_skips_untaken_path() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-cond"
  name: "cond"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "check"
    type: "logic.condition"
    position: { x: 100, y: 0 }
    config:
      left: "a"
      operator: "eq"
      right: "a"
  - id: "on_true"
    type: "notification.log"
    position: { x: 200, y: 0 }
    config:
      message: "yes"
  - id: "on_false"
    type: "notification.log"
    position: { x: 200, y: 100 }
    config:
      message: "no"
  - id: "finish"
    type: "flow.end"
    position: { x: 300, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "check"
    target_port: "in"
  - id: "e2"
    source: "check"
    source_port: "true"
    target: "on_true"
    target_port: "in"
  - id: "e3"
    source: "check"
    source_port: "false"
    target: "on_false"
    target_port: "in"
  - id: "e4"
    source: "on_true"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-cond", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    let status = |id: &str| nodes.iter().find(|n| n.node_id == id).unwrap().status.clone();
    assert_eq!(status("on_true"), "success");
    assert_eq!(status("on_false"), "skipped");
}

#[tokio::test]
async fn failing_command_stops_and_reports_failed() {
    let state = test_state().await;
    let yaml = minimal_yaml("wf-fail").replace(
        "program: \"echo\"",
        "program: \"false\"",
    );
    // echo->false replacement keeps YAML valid (args still present).
    workflow_repository::create_from_yaml(&state.pool, &yaml, None)
        .await
        .unwrap();
    // Force failure: 'false' exits non-zero and is not retryable beyond max_attempts=1.
    let job_id = workflow_service::run_workflow(&state, "wf-fail", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let detail = workflow_run_repository::get_run_detail(&state.pool, &job_id)
        .await
        .unwrap();
    assert_eq!(detail.job.status, "failed");
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert_eq!(
        nodes.iter().find(|n| n.node_id == "run").unwrap().status,
        "failed"
    );
    assert_eq!(
        nodes.iter().find(|n| n.node_id == "finish").unwrap().status,
        "skipped"
    );
}

#[tokio::test]
async fn invalid_yaml_is_never_saved() {
    let state = test_state().await;
    workflow_repository::create_from_yaml(&state.pool, &minimal_yaml("wf-safe"), None)
        .await
        .unwrap();
    let bad = "schema_version: 999\nworkflow:\n  id: wf-safe\n  name: x\n  enabled: true\nnodes: []\nedges: []\n";
    assert!(workflow_repository::update_from_yaml(&state.pool, "wf-safe", bad, None, None)
        .await
        .is_err());
    let rec = workflow_repository::get(&state.pool, "wf-safe").await.unwrap();
    assert_eq!(rec.version, 1);
}

#[tokio::test]
async fn migration_is_idempotent_and_preserves_automations() {
    let state = test_state().await;
    let automation = automation_manager::core::models::AutomationDefinition::minimal(
        "legacy",
        automation_manager::core::models::TriggerDefinition::Manual,
        Default::default(),
    );
    repository::save_automation(&state.pool, &automation).await.unwrap();
    database::migrate(&state.pool).await.unwrap();
    database::migrate(&state.pool).await.unwrap();
    let got = repository::get_automation(&state.pool, &automation.id).await.unwrap();
    assert_eq!(got.name, "legacy");
}

#[tokio::test]
async fn no_double_run_without_link_and_fanout_with_link() {
    use automation_manager::core::models::{AutomationDefinition, ExecutionPolicy, TriggerDefinition};
    let state = test_state().await;
    workflow_repository::create_from_yaml(&state.pool, &minimal_yaml("wf-linked"), None)
        .await
        .unwrap();
    let automation = AutomationDefinition::minimal("legacy", TriggerDefinition::Manual, ExecutionPolicy::default());
    repository::save_automation(&state.pool, &automation).await.unwrap();
    // No link: trigger adapter fans out to zero workflows.
    let jobs = workflow_service::run_linked_workflows_for_automation(
        &state,
        &automation,
        "manual",
        &automation_manager::services::variables::EventContext::default(),
    )
    .await;
    assert!(jobs.is_empty());
    // Attach: now one workflow run is fanned out.
    workflow_repository::attach_automation_trigger(&state.pool, "wf-linked", &automation.id, true)
        .await
        .unwrap();
    let jobs = workflow_service::run_linked_workflows_for_automation(
        &state,
        &automation,
        "manual",
        &automation_manager::services::variables::EventContext::default(),
    )
    .await;
    assert_eq!(jobs.len(), 1);
    wait_for_job(&state, &jobs[0]).await;
}

#[tokio::test]
async fn disabled_workflow_manual_run_allowed_but_auto_rejected() {    let state = test_state().await;
    let yaml = minimal_yaml("wf-disabled").replace("enabled: true", "enabled: false");
    workflow_repository::create_from_yaml(&state.pool, &yaml, None)
        .await
        .unwrap();
    // Manual run works.
    let job_id = workflow_service::run_workflow(&state, "wf-disabled", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    // Linked (automatic) runs are rejected for disabled workflows.
    let automation = automation_manager::core::models::AutomationDefinition::minimal(
        "legacy2",
        automation_manager::core::models::TriggerDefinition::Manual,
        Default::default(),
    );
    repository::save_automation(&state.pool, &automation).await.unwrap();
    workflow_repository::attach_automation_trigger(&state.pool, "wf-disabled", &automation.id, true)
        .await
        .unwrap();
    let jobs = workflow_service::run_linked_workflows_for_automation(
        &state,
        &automation,
        "manual",
        &automation_manager::services::variables::EventContext::default(),
    )
    .await;
    assert!(jobs.is_empty());
}

#[tokio::test]
async fn rest_api_workflow_crud_run_and_schema() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;
    let state = test_state().await;
    let router = automation_manager::api::router(Arc::clone(&state));

    // Schema endpoints (spec 11).
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/schema/workflow")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/schema/node-types")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Create via REST (validated before save).
    let yaml = minimal_yaml("wf-rest");
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workflows")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "yaml": yaml })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Run via REST returns job_id immediately (spec 10.1).
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workflows/wf-rest/run")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let job_id = payload["job_id"].as_str().unwrap().to_string();
    wait_for_job(&state, &job_id).await;

    // Node detail endpoint.
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/workflow-runs/{job_id}/nodes"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // History default contract (20 max) still holds for legacy endpoint.
    let res = router
        .oneshot(
            Request::builder()
                .uri("/api/history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn edit_roundtrip_canonical_yaml_reparses_losslessly() {
    // Regression: the editor used to re-parse server canonical YAML
    // (unquoted scalars, multi-line mappings) on the client and silently
    // reset the workflow. Canonical YAML must re-parse to the same model.
    let state = test_state().await;
    let rec = workflow_repository::create_from_yaml(&state.pool, &minimal_yaml("wf-edit-rt"), None)
        .await
        .unwrap();
    // rec.definition_yaml is the canonical server form.
    let def = automation_manager::workflow::yaml_codec::parse(&rec.definition_yaml).unwrap();
    assert_eq!(def.nodes.len(), 3);
    assert_eq!(def.workflow.id, "wf-edit-rt");
    let run = def.node("run").unwrap();
    assert_eq!(run.r#type, "command.exec");
    assert_eq!(
        run.config.get("program").and_then(|v| v.as_str()),
        Some("echo")
    );
    // Simulate "add node + save": extend the parsed model and update.
    let mut def2 = def;
    def2.nodes.push(automation_manager::workflow::model::WorkflowNode {
        id: "extra-1".into(),
        r#type: "notification.log".into(),
        position: automation_manager::workflow::model::Position { x: 100.0, y: 100.0 },
        config: serde_json::json!({"message": "hi"}),
    });
    def2.edges.push(automation_manager::workflow::model::WorkflowEdge {
        id: "e-extra".into(),
        source: "run".into(),
        source_port: "next".into(),
        target: "extra-1".into(),
        target_port: "in".into(),
    });
    let yaml2 = automation_manager::workflow::yaml_codec::serialize(&def2).unwrap();
    let updated = workflow_repository::update_from_yaml(&state.pool, "wf-edit-rt", &yaml2, Some(1), None)
        .await
        .unwrap();
    assert_eq!(updated.version, 2);
    let after = workflow_repository::get(&state.pool, "wf-edit-rt").await.unwrap();
    let def3 = automation_manager::workflow::yaml_codec::parse(&after.definition_yaml).unwrap();
    assert_eq!(def3.nodes.len(), 4);
    assert!(def3.node("extra-1").is_some());
    // Original nodes survived (no reset-to-template).
    assert!(def3.node("start").is_some());
    assert!(def3.node("finish").is_some());
}

#[tokio::test]
async fn rest_api_parse_and_validate_yaml_endpoints() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;
    let state = test_state().await;
    let router = automation_manager::api::router(Arc::clone(&state));

    // Parse endpoint returns the definition JSON without persisting.
    let yaml = minimal_yaml("wf-parse");
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workflows/parse")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "yaml": yaml })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 512 * 1024).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["workflow"]["id"], serde_json::json!("wf-parse"));
    assert_eq!(payload["nodes"].as_array().unwrap().len(), 3);
    // Nothing persisted by parse.
    assert!(workflow_repository::get(&state.pool, "wf-parse").await.is_err());

    // Validate endpoint reports validity without persisting.
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workflows/validate-yaml")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "yaml": yaml })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 512 * 1024).await.unwrap();
    let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["valid"], serde_json::json!(true));

    // Invalid YAML is rejected with 400 and persists nothing.
    let bad = "schema_version: 999\nworkflow:\n  id: wf-parse\n  name: x\n  enabled: true\nnodes: []\nedges: []\n";
    let res = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workflows/validate-yaml")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "yaml": bad })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 512 * 1024).await.unwrap();
    let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["valid"], serde_json::json!(false));
    assert!(workflow_repository::get(&state.pool, "wf-parse").await.is_err());
}

#[tokio::test]
async fn trigger_input_defaults_required_and_refs() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-inputs"
  name: "inputs demo"
  enabled: true
inputs:
  target_dir:
    description: "target directory"
    default: "/tmp"
  mode:
    description: "mode flag"
    required: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.input"
    position: { x: 0, y: 0 }
    config: {}
  - id: "run"
    type: "command.exec"
    position: { x: 100, y: 0 }
    config:
      program: "echo"
      args:
        - value:
            $ref: "workflow.input.target_dir"
  - id: "finish"
    type: "flow.end"
    position: { x: 200, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "run"
    target_port: "in"
  - id: "e2"
    source: "run"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    // Missing required input -> validation error at run.
    let err = workflow_service::run_workflow(&state, "wf-inputs", HashMap::new(), "manual", HashMap::new())
        .await
        .expect_err("required input must be enforced");
    assert!(err.to_string().contains("mode"));
    // With required given, defaults apply and run succeeds.
    let mut inputs = HashMap::new();
    inputs.insert("mode".into(), serde_json::json!("fast"));
    let job_id = workflow_service::run_workflow(&state, "wf-inputs", inputs, "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert!(nodes.iter().all(|n| n.status == "success"));
    // trigger.input exposes inputs as its own outputs.
    let start = nodes.iter().find(|n| n.node_id == "start").unwrap();
    let out: serde_json::Value = serde_json::from_str(start.output_json.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(out.get("mode").and_then(|v| v.as_str()), Some("fast"));
}

#[tokio::test]
async fn node_output_chains_to_next_node() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-chain"
  name: "chain"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "first"
    type: "command.exec"
    position: { x: 100, y: 0 }
    config:
      program: "echo"
      args: ["hello-chain"]
  - id: "second"
    type: "command.exec"
    position: { x: 200, y: 0 }
    config:
      program: "echo"
      args:
        - value:
            $ref: "nodes.first.output.stdout"
  - id: "finish"
    type: "flow.end"
    position: { x: 300, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "first"
    target_port: "in"
  - id: "e2"
    source: "first"
    source_port: "next"
    target: "second"
    target_port: "in"
  - id: "e3"
    source: "second"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-chain", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert!(nodes.iter().all(|n| n.status == "success"));
    let second = nodes.iter().find(|n| n.node_id == "second").unwrap();
    assert!(second.input_json.as_deref().unwrap_or("").contains("hello-chain"));
}

#[tokio::test]
async fn invalid_output_ref_is_rejected() {
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-badref"
  name: "bad ref"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "run"
    type: "command.exec"
    position: { x: 100, y: 0 }
    config:
      program: "echo"
      args:
        - value:
            $ref: "nodes.start.output.stout"
  - id: "finish"
    type: "flow.end"
    position: { x: 200, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "run"
    target_port: "in"
  - id: "e2"
    source: "run"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    let def = yaml_codec::parse(yaml).unwrap();
    let report = validator::validate(&def).unwrap();
    assert!(!report.valid);
    assert!(report.errors.iter().any(|e| e.contains("stout")));
}

/// Local HTTP server for http.request / url_changed tests (no external net).
async fn start_test_http_server() -> String {
    use axum::{extract::Query, routing::get, Json};
    let app = axum::Router::new()
        .route(
            "/api/data",
            get(|| async {
                Json(serde_json::json!({"items": ["a", "b"], "total": 2}))
            }),
        )
        .route(
            "/echo",
            axum::routing::post(|body: String| async move {
                format!("got:{body}")
            }),
        )
        .route(
            "/page",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                let v = q.get("v").cloned().unwrap_or_else(|| "v1".into());
                format!("page content {v}")
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn http_request_get_and_post() {
    let base = start_test_http_server().await;
    let state = test_state().await;
    let yaml = format!(
        r#"
schema_version: 1
workflow:
  id: "wf-http"
  name: "http"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: {{ x: 0, y: 0 }}
    config: {{}}
  - id: "fetch"
    type: "http.request"
    position: {{ x: 100, y: 0 }}
    config:
      method: "GET"
      url: "{base}/api/data"
  - id: "pick"
    type: "data.transform"
    position: {{ x: 200, y: 0 }}
    config:
      op: "extract"
      value:
        $ref: "nodes.fetch.output.body_json"
      path: "total"
  - id: "finish"
    type: "flow.end"
    position: {{ x: 300, y: 0 }}
    config: {{}}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "fetch"
    target_port: "in"
  - id: "e2"
    source: "fetch"
    source_port: "next"
    target: "pick"
    target_port: "in"
  - id: "e3"
    source: "pick"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#
    );
    workflow_repository::create_from_yaml(&state.pool, &yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-http", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert!(nodes.iter().all(|n| n.status == "success"));
    let fetch = nodes.iter().find(|n| n.node_id == "fetch").unwrap();
    let out: serde_json::Value =
        serde_json::from_str(fetch.output_json.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(out.get("status_code").and_then(|v| v.as_i64()), Some(200));
    assert_eq!(out.get("is_success").and_then(|v| v.as_bool()), Some(true));
    let pick = nodes.iter().find(|n| n.node_id == "pick").unwrap();
    let pout: serde_json::Value =
        serde_json::from_str(pick.output_json.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(pout.get("result").and_then(|v| v.as_i64()), Some(2));
}

#[tokio::test]
async fn http_rejects_non_http_scheme() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-http-bad"
  name: "bad"
  enabled: true
settings:
  default_timeout_seconds: 10
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "fetch"
    type: "http.request"
    position: { x: 100, y: 0 }
    config:
      url: "file:///etc/passwd"
  - id: "finish"
    type: "flow.end"
    position: { x: 200, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "fetch"
    target_port: "in"
  - id: "e2"
    source: "fetch"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-http-bad", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let detail = workflow_run_repository::get_run_detail(&state.pool, &job_id)
        .await
        .unwrap();
    assert_eq!(detail.job.status, "failed");
}

#[tokio::test]
async fn data_transform_ops_chain() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-transform"
  name: "transform"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "shape"
    type: "data.transform"
    position: { x: 100, y: 0 }
    config:
      operations:
        - op: "template"
          template: "hello {{who}}"
          vars:
            who: "triga"
        - op: "upper"
        - op: "replace"
          search: "HELLO"
          replace: "HI"
          all: false
  - id: "finish"
    type: "flow.end"
    position: { x: 200, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "shape"
    target_port: "in"
  - id: "e2"
    source: "shape"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-transform", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    let shape = nodes.iter().find(|n| n.node_id == "shape").unwrap();
    assert_eq!(shape.status, "success");
    let out: serde_json::Value =
        serde_json::from_str(shape.output_json.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        out.get("result").and_then(|v| v.as_str()),
        Some("HI TRIGA")
    );
}

#[tokio::test]
async fn delay_waits_and_reports() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-delay"
  name: "delay"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "wait"
    type: "flow.delay"
    position: { x: 100, y: 0 }
    config:
      seconds: 0
      milliseconds: 300
  - id: "finish"
    type: "flow.end"
    position: { x: 200, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "wait"
    target_port: "in"
  - id: "e2"
    source: "wait"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let t0 = std::time::Instant::now();
    let job_id = workflow_service::run_workflow(&state, "wf-delay", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    assert!(t0.elapsed() >= Duration::from_millis(250));
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert!(nodes.iter().all(|n| n.status == "success"));
}

#[tokio::test]
async fn wait_ready_stable_file_succeeds_and_missing_times_out() {
    let state = test_state().await;
    let dir = std::env::temp_dir().join(format!("triga-wait-ready-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ready = dir.join("ready.csv");
    std::fs::write(&ready, b"id,value\n1,2\n").unwrap();
    let ready_str = ready.to_string_lossy().replace('\\', "/");
    let missing = dir.join("never-appears.csv").to_string_lossy().replace('\\', "/");

    let yaml_ok = format!(
        r#"
schema_version: 1
workflow:
  id: "wf-wait-ok"
  name: "wait ok"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: {{ x: 0, y: 0 }}
    config: {{}}
  - id: "wait"
    type: "file.wait_ready"
    position: {{ x: 100, y: 0 }}
    config:
      path: "{ready_str}"
      stable_seconds: 1
      timeout_seconds: 10
  - id: "finish"
    type: "flow.end"
    position: {{ x: 200, y: 0 }}
    config: {{}}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "wait"
    target_port: "in"
  - id: "e2"
    source: "wait"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#
    );
    workflow_repository::create_from_yaml(&state.pool, &yaml_ok, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-wait-ok", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert!(nodes.iter().all(|n| n.status == "success"));

    let yaml_missing = format!(
        r#"
schema_version: 1
workflow:
  id: "wf-wait-missing"
  name: "wait missing"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: {{ x: 0, y: 0 }}
    config: {{}}
  - id: "wait"
    type: "file.wait_ready"
    position: {{ x: 100, y: 0 }}
    config:
      path: "{missing}"
      stable_seconds: 1
      timeout_seconds: 2
  - id: "finish"
    type: "flow.end"
    position: {{ x: 200, y: 0 }}
    config: {{}}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "wait"
    target_port: "in"
  - id: "e2"
    source: "wait"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#
    );
    workflow_repository::create_from_yaml(&state.pool, &yaml_missing, None)
        .await
        .unwrap();
    let job2 = workflow_service::run_workflow(&state, "wf-wait-missing", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job2).await;
    let detail = workflow_run_repository::get_run_detail(&state.pool, &job2)
        .await
        .unwrap();
    assert_eq!(detail.job.status, "timeout");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn desktop_notify_is_best_effort_success() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-notify"
  name: "notify"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "ping"
    type: "notify.desktop"
    position: { x: 100, y: 0 }
    config:
      title: "TrigA test"
      message: "hello"
  - id: "finish"
    type: "flow.end"
    position: { x: 200, y: 0 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "ping"
    target_port: "in"
  - id: "e2"
    source: "ping"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-notify", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    assert!(nodes.iter().all(|n| n.status == "success"));
}

#[tokio::test]
async fn url_changed_hash_and_expected() {
    let base = start_test_http_server().await;
    let state = test_state().await;
    let yaml = format!(
        r#"
schema_version: 1
workflow:
  id: "wf-url"
  name: "url"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
execution:
  deduplication:
    enabled: false
nodes:
  - id: "watch"
    type: "trigger.url_changed"
    position: {{ x: 0, y: 0 }}
    config:
      url: "{base}/page?v=v1"
  - id: "finish"
    type: "flow.end"
    position: {{ x: 100, y: 0 }}
    config: {{}}
edges:
  - id: "e1"
    source: "watch"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#
    );
    workflow_repository::create_from_yaml(&state.pool, &yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-url", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    let watch = nodes.iter().find(|n| n.node_id == "watch").unwrap();
    assert_eq!(watch.status, "success");
    let out: serde_json::Value =
        serde_json::from_str(watch.output_json.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(out.get("changed").and_then(|v| v.as_bool()), Some(true));
    let hash = out.get("hash").and_then(|v| v.as_str()).unwrap().to_string();
    // Same content + expected_hash => changed=false.
    let yaml2 = format!(
        r#"
schema_version: 1
workflow:
  id: "wf-url2"
  name: "url2"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "watch"
    type: "trigger.url_changed"
    position: {{ x: 0, y: 0 }}
    config:
      url: "{base}/page?v=v1"
      expected_hash: "{hash}"
  - id: "finish"
    type: "flow.end"
    position: {{ x: 100, y: 0 }}
    config: {{}}
edges:
  - id: "e1"
    source: "watch"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#
    );
    workflow_repository::create_from_yaml(&state.pool, &yaml2, None)
        .await
        .unwrap();
    let job2 = workflow_service::run_workflow(&state, "wf-url2", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job2).await;
    let nodes2 = workflow_run_repository::list_node_runs(&state.pool, &job2)
        .await
        .unwrap();
    let watch2 = nodes2.iter().find(|n| n.node_id == "watch").unwrap();
    let out2: serde_json::Value =
        serde_json::from_str(watch2.output_json.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(out2.get("changed").and_then(|v| v.as_bool()), Some(false));
}

#[tokio::test]
async fn for_each_runs_body_per_item() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-foreach"
  name: "foreach"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "loop"
    type: "flow.for_each"
    position: { x: 100, y: 0 }
    config:
      items: ["a", "b", "c"]
  - id: "do_thing"
    type: "notification.log"
    position: { x: 200, y: 0 }
    config:
      message:
        $ref: "nodes.loop.output.item"
  - id: "finish"
    type: "flow.end"
    position: { x: 300, y: 0 }
    config: {}
edges:
  - id: "e0"
    source: "start"
    source_port: "next"
    target: "loop"
    target_port: "in"
  - id: "e1"
    source: "loop"
    source_port: "next"
    target: "do_thing"
    target_port: "in"
  - id: "e2"
    source: "loop"
    source_port: "done"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-foreach", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let detail = workflow_run_repository::get_run_detail(&state.pool, &job_id)
        .await
        .unwrap();
    assert_eq!(detail.job.status, "success");
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    let ids: Vec<&str> = nodes.iter().map(|n| n.node_id.as_str()).collect();
    assert!(ids.contains(&"do_thing[0]"));
    assert!(ids.contains(&"do_thing[1]"));
    assert!(ids.contains(&"do_thing[2]"));
    let first = nodes.iter().find(|n| n.node_id == "do_thing[0]").unwrap();
    assert!(first.output_json.as_deref().unwrap_or("").contains('a'));
    let loop_node = nodes.iter().find(|n| n.node_id == "loop").unwrap();
    assert_eq!(loop_node.status, "success");
}

#[tokio::test]
async fn parallel_runs_all_branches_then_done() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-parallel"
  name: "parallel"
  enabled: true
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 0, y: 0 }
    config: {}
  - id: "fan"
    type: "flow.parallel"
    position: { x: 100, y: 0 }
    config: {}
  - id: "a"
    type: "notification.log"
    position: { x: 200, y: 0 }
    config:
      message: "A"
  - id: "b"
    type: "notification.log"
    position: { x: 200, y: 100 }
    config:
      message: "B"
  - id: "finish"
    type: "flow.end"
    position: { x: 300, y: 50 }
    config: {}
edges:
  - id: "e0"
    source: "start"
    source_port: "next"
    target: "fan"
    target_port: "in"
  - id: "e1"
    source: "fan"
    source_port: "branch1"
    target: "a"
    target_port: "in"
  - id: "e2"
    source: "fan"
    source_port: "branch2"
    target: "b"
    target_port: "in"
  - id: "e3"
    source: "fan"
    source_port: "done"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    let job_id = workflow_service::run_workflow(&state, "wf-parallel", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let detail = workflow_run_repository::get_run_detail(&state.pool, &job_id)
        .await
        .unwrap();
    assert_eq!(detail.job.status, "success");
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    let status = |id: &str| nodes.iter().find(|n| n.node_id == id).unwrap().status.clone();
    assert_eq!(status("a"), "success");
    assert_eq!(status("b"), "success");
    assert_eq!(status("finish"), "success");
}

#[tokio::test]
async fn switch_routes_to_matching_port_and_skips_others() {
    let state = test_state().await;
    let yaml = r#"
schema_version: 1
workflow:
  id: "wf-switch"
  name: "switch demo"
  enabled: true
inputs:
  mode:
    default: "b"
settings:
  default_timeout_seconds: 30
  default_max_attempts: 1
execution:
  deduplication:
    enabled: false
nodes:
  - id: "start"
    type: "trigger.input"
    position: { x: 0, y: 0 }
    config: {}
  - id: "sw"
    type: "logic.switch"
    position: { x: 100, y: 0 }
    config:
      value:
        $ref: "workflow.input.mode"
      cases:
        - equals: "a"
          port: "case1"
        - equals: "b"
          port: "case2"
      default_port: "default"
  - id: "on_a"
    type: "notification.log"
    position: { x: 200, y: 0 }
    config:
      message: "got a"
  - id: "on_b"
    type: "notification.log"
    position: { x: 200, y: 100 }
    config:
      message: "got b"
  - id: "on_default"
    type: "notification.log"
    position: { x: 200, y: 200 }
    config:
      message: "got default"
  - id: "finish"
    type: "flow.end"
    position: { x: 300, y: 100 }
    config: {}
edges:
  - id: "e0"
    source: "start"
    source_port: "next"
    target: "sw"
    target_port: "in"
  - id: "e1"
    source: "sw"
    source_port: "case1"
    target: "on_a"
    target_port: "in"
  - id: "e2"
    source: "sw"
    source_port: "case2"
    target: "on_b"
    target_port: "in"
  - id: "e3"
    source: "sw"
    source_port: "default"
    target: "on_default"
    target_port: "in"
  - id: "e4"
    source: "on_b"
    source_port: "next"
    target: "finish"
    target_port: "in"
"#;
    workflow_repository::create_from_yaml(&state.pool, yaml, None)
        .await
        .unwrap();
    // Default mode=b -> case2 runs.
    let job_id = workflow_service::run_workflow(&state, "wf-switch", HashMap::new(), "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job_id).await;
    let nodes = workflow_run_repository::list_node_runs(&state.pool, &job_id)
        .await
        .unwrap();
    let status = |id: &str| nodes.iter().find(|n| n.node_id == id).unwrap().status.clone();
    assert_eq!(status("on_b"), "success");
    assert_eq!(status("on_a"), "skipped");
    assert_eq!(status("on_default"), "skipped");
    // Explicit mode=a -> case1 runs.
    let mut inputs = HashMap::new();
    inputs.insert("mode".into(), serde_json::json!("a"));
    let job2 = workflow_service::run_workflow(&state, "wf-switch", inputs, "manual", HashMap::new())
        .await
        .unwrap();
    wait_for_job(&state, &job2).await;
    let nodes2 = workflow_run_repository::list_node_runs(&state.pool, &job2)
        .await
        .unwrap();
    let status2 = |id: &str| nodes2.iter().find(|n| n.node_id == id).unwrap().status.clone();
    assert_eq!(status2("on_a"), "success");
    assert_eq!(status2("on_b"), "skipped");
}

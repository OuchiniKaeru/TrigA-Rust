use automation_manager::{
    core::models::{AutomationDefinition, ExecutionPolicy, TriggerDefinition, ActionDefinition},
    engine::application_service::{self, AppState},
    services::security::SecurityPolicy,
    storage::{database, repository},
    core::validation::validate_automation,
};
use std::sync::Arc;

async fn mem_state() -> Arc<AppState> {
    let pool = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&pool).await.unwrap();
    Arc::new(AppState::new(pool, SecurityPolicy::default()))
}

#[tokio::test]
async fn interval_validation_and_overlap_skip() {
    let state = mem_state().await;
    // interval with 0 must be rejected
    let bad = AutomationDefinition::minimal(
        "bad",
        TriggerDefinition::Interval { interval_seconds: 0, start_immediately: false },
        ExecutionPolicy::default(),
    );
    assert!(validate_automation(&bad).is_err());

    // valid interval, overlap default is skip
    let mut auto = AutomationDefinition::minimal(
        "interval-job",
        TriggerDefinition::Interval { interval_seconds: 600, start_immediately: true },
        ExecutionPolicy::default(),
    );
    assert_eq!(auto.execution.concurrency.overlap_policy, automation_manager::core::models::OverlapPolicy::Skip);
    // long-running action to hold the running flag
    auto.actions = vec![ActionDefinition::Command { executable: "sleep".into(), args: vec!["2".into()] }];
    auto.execution.timeout_seconds = 10;
    auto.execution.deduplication.enabled = false;
    repository::save_automation(&state.pool, &auto).await.unwrap();

    // Simulate running flag held -> second tick must be skipped
    state.scheduler.check_dedup("warm", std::time::Duration::from_secs(0)).await;
    // mark running manually by starting a first execution in background
    let s1 = Arc::clone(&state);
    let a1 = auto.clone();
    let h = tokio::spawn(async move {
        let ctx = automation_manager::services::variables::EventContext::for_trigger(&a1.id, "", None);
        application_service::run_automation_with_context(&s1, &a1, "interval", ctx).await.unwrap()
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let ctx2 = automation_manager::services::variables::EventContext::for_trigger(&auto.id, "", None);
    let _job2 = application_service::run_automation_with_context(&state, &auto, "interval", ctx2).await.unwrap();
    let _ = h.await.unwrap();
    let history = repository::list_history(&state.pool, 20).await.unwrap();
    assert!(history.iter().any(|h| h.status == "skipped"), "expected a skipped overlap, got {:?}", history.iter().map(|h| &h.status).collect::<Vec<_>>());
}

#[tokio::test]
async fn scheduled_cron_and_timezone_validation() {
    let good = AutomationDefinition::minimal(
        "morning",
        TriggerDefinition::Scheduled { schedule: "0 9 * * *".into(), timezone: Some("Asia/Tokyo".into()) },
        ExecutionPolicy::default(),
    );
    assert!(validate_automation(&good).is_ok());
    let bad_tz = AutomationDefinition::minimal(
        "bad-tz",
        TriggerDefinition::Scheduled { schedule: "0 9 * * *".into(), timezone: Some("Mars/Olympus".into()) },
        ExecutionPolicy::default(),
    );
    assert!(validate_automation(&bad_tz).is_err());
    assert!(automation_manager::triggers::scheduler::parse_schedule("0 9 * * *").is_some());
    assert!(automation_manager::triggers::scheduler::parse_schedule("09:00").is_some());
}

#[tokio::test]
async fn folder_created_and_shell_validation() {
    let folder = AutomationDefinition::minimal(
        "folders",
        TriggerDefinition::FolderCreated { path: "/tmp/input".into() },
        ExecutionPolicy::default(),
    );
    assert!(validate_automation(&folder).is_ok());
    let shell = AutomationDefinition {
        actions: vec![ActionDefinition::Shell { command: "echo hi".into() }],
        ..AutomationDefinition::minimal("s", TriggerDefinition::Manual, ExecutionPolicy::default())
    };
    assert!(validate_automation(&shell).is_ok());
}

#[tokio::test]
async fn condition_mismatch_records_skipped() {
    let state = mem_state().await;
    let mut auto = AutomationDefinition::minimal("cond", TriggerDefinition::Manual, ExecutionPolicy::default());
    auto.conditions = vec![automation_manager::core::models::ConditionDefinition::Extension { value: "csv".into() }];
    auto.execution.deduplication.enabled = false;
    let mut ctx = automation_manager::services::variables::EventContext::for_trigger(&auto.id, "", None);
    ctx.extension = "txt".into();
    let _ = application_service::run_automation_with_context(&state, &auto, "manual", ctx).await.unwrap();
    let history = repository::list_history(&state.pool, 20).await.unwrap();
    assert_eq!(history[0].status, "skipped");
}

//! Regression: file triggers must record execution history for every way a
//! file can appear or change (direct create, move/rename-in, modify).
use automation_manager::{
    core::models::{ActionDefinition, AutomationDefinition, ExecutionPolicy, TriggerDefinition},
    engine::application_service::AppState,
    services::security::SecurityPolicy,
    storage::{database, repository},
};
use std::sync::Arc;

async fn mem_state() -> Arc<AppState> {
    let pool = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&pool).await.unwrap();
    Arc::new(AppState::new(pool, SecurityPolicy::default()))
}

async fn watch_automation(
    state: &Arc<AppState>,
    name: &str,
    trigger: TriggerDefinition,
) -> AutomationDefinition {
    let mut auto =
        AutomationDefinition::minimal(name, trigger, ExecutionPolicy::default());
    auto.execution.deduplication.enabled = false;
    auto.actions = vec![ActionDefinition::Command {
        executable: "true".into(),
        args: vec![],
    }];
    repository::save_automation(&state.pool, &auto)
        .await
        .unwrap();
    auto
}

async fn wait_for_history(
    state: &Arc<AppState>,
    want: usize,
) -> Vec<repository::HistoryRecord> {
    for _ in 0..50 {
        let history = repository::list_history(&state.pool, 20).await.unwrap();
        if history.len() >= want {
            return history;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    repository::list_history(&state.pool, 20).await.unwrap()
}

#[tokio::test]
async fn moved_in_file_records_file_created_history() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("triga-mv-{stamp}"));
    let outside = std::env::temp_dir().join(format!("triga-out-{stamp}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    let state = mem_state().await;
    watch_automation(
        &state,
        "mv-watch",
        TriggerDefinition::FileCreated {
            path: dir.to_string_lossy().into_owned(),
        },
    )
    .await;
    automation_manager::triggers::watcher::start(Arc::clone(&state))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let src = outside.join("payload.txt");
    std::fs::write(&src, "data").unwrap();
    std::fs::rename(&src, dir.join("payload.txt")).unwrap();

    let history = wait_for_history(&state, 1).await;
    assert_eq!(history.len(), 1, "move-in must record exactly one history row");
    assert_eq!(history[0].trigger_type, "file_created");
    assert_eq!(history[0].status, "success");

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[tokio::test]
async fn direct_create_and_modify_record_history() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("triga-cm-{stamp}"));
    std::fs::create_dir_all(&dir).unwrap();
    let watch = dir.to_string_lossy().into_owned();

    let state = mem_state().await;
    watch_automation(
        &state,
        "create-watch",
        TriggerDefinition::FileCreated { path: watch.clone() },
    )
    .await;
    watch_automation(
        &state,
        "modify-watch",
        TriggerDefinition::FileModified { path: watch },
    )
    .await;
    automation_manager::triggers::watcher::start(Arc::clone(&state))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let target = dir.join("note.txt");
    std::fs::write(&target, "v1").unwrap();
    let history = wait_for_history(&state, 2).await;
    assert!(
        history.len() >= 2,
        "create should fire created+modified, got {history:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn rename_reports_are_coalesced_without_skipped_rows() {
    // One logical appearance via temp + in-dir rename must not produce
    // duplicate/skipped history rows for the same destination, even with
    // the default deduplication policy enabled.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("triga-coal-{stamp}"));
    std::fs::create_dir_all(&dir).unwrap();

    let pool = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&pool).await.unwrap();
    let state = Arc::new(AppState::new(pool, SecurityPolicy::default()));
    let mut auto = AutomationDefinition::minimal(
        "coal-watch",
        TriggerDefinition::FileCreated {
            path: dir.to_string_lossy().into_owned(),
        },
        ExecutionPolicy::default(),
    );
    auto.actions = vec![ActionDefinition::Command {
        executable: "true".into(),
        args: vec![],
    }];
    repository::save_automation(&state.pool, &auto)
        .await
        .unwrap();
    automation_manager::triggers::watcher::start(Arc::clone(&state))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let tmp = dir.join(".tmp-abc");
    std::fs::write(&tmp, "new content").unwrap();
    std::fs::rename(&tmp, dir.join("doc.txt")).unwrap();

    let history = wait_for_history(&state, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let history = repository::list_history(&state.pool, 20).await.unwrap();
    let skipped = history
        .iter()
        .filter(|h| h.status == "skipped")
        .count();
    assert_eq!(
        skipped,
        0,
        "rename To/Both double-report must be coalesced, got {history:?}"
    );
    assert!(
        history.iter().any(|h| h.status == "success"),
        "destination must still fire, got {history:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn atomic_save_fires_exactly_once_on_final_path() {
    // Temp + in-dir rename is one logical appearance: the transient temp
    // path (already renamed away) must not fire its own job.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("triga-single-{stamp}"));
    std::fs::create_dir_all(&dir).unwrap();

    let pool = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&pool).await.unwrap();
    let state = Arc::new(AppState::new(pool, SecurityPolicy::default()));
    let mut auto = AutomationDefinition::minimal(
        "single-watch",
        TriggerDefinition::FileCreated {
            path: dir.to_string_lossy().into_owned(),
        },
        ExecutionPolicy::default(),
    );
    auto.actions = vec![ActionDefinition::Command {
        executable: "true".into(),
        args: vec![],
    }];
    repository::save_automation(&state.pool, &auto)
        .await
        .unwrap();
    automation_manager::triggers::watcher::start(Arc::clone(&state))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let tmp = dir.join(".tmp-abc");
    std::fs::write(&tmp, "new content").unwrap();
    std::fs::rename(&tmp, dir.join("doc.txt")).unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let history = repository::list_history(&state.pool, 20).await.unwrap();
    assert_eq!(history.len(), 1, "one appearance must yield one row, got {history:?}");
    assert_eq!(history[0].status, "success");
    assert!(
        history[0].result.as_deref().is_some_and(|p| p.ends_with("doc.txt")),
        "history must record the final path, got {history:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

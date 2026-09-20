use std::path::Path;
use std::sync::Arc;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::{
    core::models::TriggerDefinition,
    engine::application_service::{self, AppState},
    services::variables::EventContext,
    storage::repository,
};

use super::{classify_events, FileEvent, FileTriggerKind};

/// Coalescing window for OS file events. A single logical filesystem
/// occurrence (e.g. an in-directory rename) is reported by notify as several
/// channel messages (`Modify(Name(From))`, `Modify(Name(To))`,
/// `Modify(Name(Both))` for one `mv`). Collecting a short fixed window after
/// the first arrival lets us collapse reports of the same entry into one job
/// instead of recording spurious duplicate/skipped history rows.
/// The window only runs after an event arrived, so idle waiting stays
/// event-driven (spec #95: no polling).
const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(150);

pub async fn start(state: Arc<AppState>) -> Result<(), crate::AppError> {
    let automations = repository::list_automations(&state.pool).await?;
    let mut roots: Vec<String> = automations
        .iter()
        .filter(|automation| automation.enabled)
        .filter_map(|automation| match &automation.trigger {
            TriggerDefinition::FolderCreated { path }
            | TriggerDefinition::FileCreated { path }
            | TriggerDefinition::FileModified { path } => Some(path.clone()),
            TriggerDefinition::Manual
            | TriggerDefinition::Interval { .. }
            | TriggerDefinition::Scheduled { .. } => None,
        })
        .collect();
    roots.sort();
    roots.dedup();

    if roots.is_empty() {
        return Ok(());
    }

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let mut watcher = RecommendedWatcher::new(
        move |result| {
            let _ = sender.send(result);
        },
        Config::default(),
    )
    .map_err(|error| crate::AppError::Trigger(error.to_string()))?;

    for root in roots {
        let path = Path::new(&root);
        if path.exists() {
            watcher
                .watch(path, RecursiveMode::Recursive)
                .map_err(|error| crate::AppError::Trigger(error.to_string()))?;
        }
    }
    let task_state = Arc::clone(&state);
    let handle = tokio::spawn(async move {
        let _watcher = watcher;
        while let Some(first) = receiver.recv().await {
            // Micro-batch: one logical occurrence arrives as several channel
            // messages (rename = From + To + Both). Drain a short fixed
            // window, then collapse identical reports so each entry fires
            // exactly one job per automation.
            let mut raw = vec![first];
            let deadline = tokio::time::Instant::now() + COALESCE_WINDOW;
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match tokio::time::timeout(remaining, receiver.recv()).await {
                    Ok(Some(next)) => raw.push(next),
                    _ => break,
                }
            }
            let mut file_events: Vec<FileEvent> = Vec::new();
            for result in &raw {
                let Ok(event) = result else { continue };
                // Classify with the full path list so rename pairs
                // (Both: [from, to]) resolve to the destination entry.
                file_events.extend(classify_events(&event.kind, &event.paths));
            }
            dedupe_events(&mut file_events);
            // Drop creation reports for entries that no longer exist: a temp
            // file renamed away within the batch window (atomic save,
            // browser/office downloads) must not fire its own job. Only the
            // surviving destination fires. Modification reports are left
            // untouched.
            file_events.retain(|event| match event.kind {
                FileTriggerKind::CreatedFile | FileTriggerKind::CreatedFolder => {
                    event.path.exists()
                }
                FileTriggerKind::Modified => true,
            });
            if file_events.is_empty() {
                continue;
            }
            let Ok(automations) = repository::list_automations(&task_state.pool).await else {
                continue;
            };
            for file_event in &file_events {
                for automation in &automations {
                    if !automation.enabled {
                        continue;
                    }
                    if super::matches_automation(automation, file_event) {
                        let trigger_type = automation.trigger.trigger_type();
                        let context = build_context(&automation.id, &file_event.path);
                        let _ = application_service::run_automation_with_context(
                            &task_state,
                            automation,
                            trigger_type,
                            context,
                        )
                        .await;
                    }
                }
            }
        }
    });
    state.scheduler.replace_watcher(handle).await;
    Ok(())
}

/// Collapse reports of the same entry (same trigger kind + same path),
/// preserving first-seen order. `Modify(Name(To))` and `Modify(Name(Both))`
/// for one rename both resolve to the destination, so the second report is
/// dropped here instead of becoming a duplicate/skipped job.
fn dedupe_events(events: &mut Vec<FileEvent>) {
    use std::collections::HashSet;
    let mut seen: HashSet<(String, std::path::PathBuf)> = HashSet::new();
    events.retain(|event| {
        seen.insert((
            format!("{:?}", event.kind),
            event.path.clone(),
        ))
    });
}

fn build_context(automation_id: &str, path: &Path) -> EventContext {
    let path_text = path.to_string_lossy().into_owned();
    let is_dir = path.is_dir();
    let file_path = if is_dir { String::new() } else { path_text.clone() };
    let folder_path = if is_dir {
        path_text.clone()
    } else {
        path.parent()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = std::path::Path::new(&file_path)
        .extension()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_name_without_ext = file_name
        .strip_suffix(&format!(".{extension}"))
        .unwrap_or(&file_name)
        .to_string();
    let folder_name = std::path::Path::new(&folder_path)
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let now = chrono::Utc::now();
    EventContext {
        file_path,
        file_name,
        file_name_without_ext,
        folder_path,
        folder_name,
        extension,
        timestamp: now.to_rfc3339(),
        date: now.format("%Y-%m-%d").to_string(),
        time: now.format("%H:%M:%S").to_string(),
        automation_id: automation_id.to_string(),
        job_id: String::new(),
        attempt: 1,
    }
}

#[allow(dead_code)]
fn trigger_for_kind(kind: &FileTriggerKind) -> &'static str {
    match kind {
        FileTriggerKind::CreatedFile => "file_created",
        FileTriggerKind::CreatedFolder => "folder_created",
        FileTriggerKind::Modified => "file_modified",
    }
}

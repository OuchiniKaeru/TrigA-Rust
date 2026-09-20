use crate::core::models::{AutomationDefinition, TriggerDefinition};
use notify::EventKind;

pub mod watcher;
pub mod scheduler;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTriggerKind {
    CreatedFile,
    CreatedFolder,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    pub kind: FileTriggerKind,
    pub path: PathBuf,
    pub is_dir: bool,
}

pub fn map_event(kind: &EventKind, path: &Path) -> Option<FileEvent> {
    classify_events(kind, std::slice::from_ref(&path.to_path_buf()))
        .into_iter()
        .next()
}

/// Classify a raw notify event (with all its paths) into file trigger events.
///
/// Classification prefers the notify event kind over filesystem probes:
/// - `Create(File)` → file creation, `Create(Folder)` → folder creation.
/// - `Modify(Name(To))` → an entry appeared at the path (e.g. `mv` into the
///   watched dir, atomic-save rename). Treated as a creation so
///   `file_created` / `folder_created` automations fire and record history.
/// - `Modify(Name(Both))` carries `[from, to]`; only the destination is an
///   appearance. The source is a removal and is ignored.
/// - `Modify(Name(From))` → removal from the watched dir. Ignored.
/// - Other `Modify` → content/metadata modification.
/// - `Access`, `Remove`, `Other` → ignored (no job, no history).
pub fn classify_events(kind: &EventKind, paths: &[PathBuf]) -> Vec<FileEvent> {
    use notify::event::{CreateKind, ModifyKind, RenameMode};
    match kind {
        EventKind::Create(CreateKind::File) => paths
            .iter()
            .map(|path| file_event(FileTriggerKind::CreatedFile, path))
            .collect(),
        EventKind::Create(CreateKind::Folder) => paths
            .iter()
            .map(|path| file_event(FileTriggerKind::CreatedFolder, path))
            .collect(),
        EventKind::Create(_) => paths
            .iter()
            .map(|path| {
                if path.is_dir() {
                    file_event(FileTriggerKind::CreatedFolder, path)
                } else {
                    file_event(FileTriggerKind::CreatedFile, path)
                }
            })
            .collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => paths
            .iter()
            .map(|path| {
                if path.is_dir() {
                    file_event(FileTriggerKind::CreatedFolder, path)
                } else {
                    file_event(FileTriggerKind::CreatedFile, path)
                }
            })
            .collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => paths
            .last()
            .map(|path| {
                vec![if path.is_dir() {
                    file_event(FileTriggerKind::CreatedFolder, path)
                } else {
                    file_event(FileTriggerKind::CreatedFile, path)
                }]
            })
            .unwrap_or_default(),
        EventKind::Modify(ModifyKind::Name(_)) => Vec::new(),
        EventKind::Modify(_) => paths
            .iter()
            .map(|path| file_event(FileTriggerKind::Modified, path))
            .collect(),
        _ => Vec::new(),
    }
}

fn file_event(kind: FileTriggerKind, path: &PathBuf) -> FileEvent {
    let is_dir = path.is_dir();
    FileEvent {
        kind,
        path: path.clone(),
        is_dir,
    }
}

pub fn matches_automation(automation: &AutomationDefinition, event: &FileEvent) -> bool {
    if !automation.enabled {
        return false;
    }
    match (&automation.trigger, &event.kind) {
        (TriggerDefinition::FileCreated { path }, FileTriggerKind::CreatedFile)
        | (TriggerDefinition::FileModified { path }, FileTriggerKind::Modified)
        | (TriggerDefinition::FolderCreated { path }, FileTriggerKind::CreatedFolder) => {
            event.path.starts_with(path)
        }
        // A folder_created trigger should also fire when a directory entry
        // appears but `is_dir` could not be resolved yet (e.g. rapid tests):
        // fall back to path prefix only when the event was a creation.
        (TriggerDefinition::FolderCreated { path }, FileTriggerKind::CreatedFile)
            if event.is_dir =>
        {
            event.path.starts_with(path)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_create_and_modify_events() {
        let path = Path::new("/tmp/input.csv");
        // A non-existent path is treated as a file creation.
        assert_eq!(
            map_event(&EventKind::Create(notify::event::CreateKind::File), path)
                .unwrap()
                .kind,
            FileTriggerKind::CreatedFile
        );
        assert_eq!(
            map_event(
                &EventKind::Modify(notify::event::ModifyKind::Data(
                    notify::event::DataChange::Content
                )),
                path
            )
            .unwrap()
            .kind,
            FileTriggerKind::Modified
        );
        assert!(map_event(&EventKind::Remove(notify::event::RemoveKind::File), path).is_none());
    }

    #[test]
    fn matches_only_enabled_automation_with_same_trigger_and_root() {
        let automation = AutomationDefinition::minimal(
            "watch",
            TriggerDefinition::FileCreated {
                path: "/tmp/input".into(),
            },
            Default::default(),
        );
        let event = FileEvent {
            kind: FileTriggerKind::CreatedFile,
            path: "/tmp/input/new.csv".into(),
            is_dir: false,
        };
        assert!(matches_automation(&automation, &event));
    }

    #[test]
    fn folder_trigger_matches_directory_creation() {
        let automation = AutomationDefinition::minimal(
            "folders",
            TriggerDefinition::FolderCreated {
                path: "/tmp/input".into(),
            },
            Default::default(),
        );
        let event = FileEvent {
            kind: FileTriggerKind::CreatedFolder,
            path: "/tmp/input/project_a".into(),
            is_dir: true,
        };
        assert!(matches_automation(&automation, &event));
    }

    #[test]
    fn rename_into_watch_dir_matches_file_created() {
        use notify::event::{ModifyKind, RenameMode};
        // `mv outside/payload.txt watchdir/` arrives as Modify(Name(To)).
        let events = classify_events(
            &EventKind::Modify(ModifyKind::Name(RenameMode::To)),
            &[PathBuf::from("/tmp/input/payload.txt")],
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, FileTriggerKind::CreatedFile);

        let automation = AutomationDefinition::minimal(
            "watch",
            TriggerDefinition::FileCreated {
                path: "/tmp/input".into(),
            },
            Default::default(),
        );
        assert!(matches_automation(&automation, &events[0]));
    }

    #[test]
    fn rename_from_watch_dir_is_ignored() {
        use notify::event::{ModifyKind, RenameMode};
        let events = classify_events(
            &EventKind::Modify(ModifyKind::Name(RenameMode::From)),
            &[PathBuf::from("/tmp/input/gone.txt")],
        );
        assert!(events.is_empty());
    }
}

use super::models::{AutomationDefinition, TriggerDefinition};
use crate::AppError;

pub fn validate_automation(automation: &AutomationDefinition) -> Result<(), AppError> {
    if automation.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }

    let execution = &automation.execution;
    if execution.timeout_seconds == 0 || execution.timeout_seconds > 86_400 {
        return Err(AppError::Validation(
            "timeout_seconds must be between 1 and 86400".into(),
        ));
    }

    let retry = &execution.retry;
    if retry.max_attempts == 0 || retry.max_attempts > 10 {
        return Err(AppError::Validation(
            "max_attempts must be between 1 and 10".into(),
        ));
    }
    if retry.delay_seconds > retry.max_delay_seconds {
        return Err(AppError::Validation(
            "delay_seconds must not exceed max_delay_seconds".into(),
        ));
    }

    if execution.concurrency.max_workers == 0 || execution.concurrency.max_workers > 32 {
        return Err(AppError::Validation(
            "max_workers must be between 1 and 32".into(),
        ));
    }

    if execution.deduplication.window_seconds > 86_400 {
        return Err(AppError::Validation(
            "deduplication window is too large".into(),
        ));
    }

    if automation.actions.is_empty() {
        return Err(AppError::Validation(
            "at least one action is required".into(),
        ));
    }

    match &automation.trigger {
        TriggerDefinition::Interval { interval_seconds, .. } => {
            if *interval_seconds == 0 {
                return Err(AppError::Validation(
                    "interval_seconds must be greater than 0".into(),
                ));
            }
        }
        TriggerDefinition::Scheduled { schedule, timezone } => {
            if !valid_schedule(schedule) {
                return Err(AppError::Validation(
                    "scheduled schedule must be cron (e.g. \"0 9 * * *\") or HH:MM".into(),
                ));
            }
            if let Some(tz) = timezone {
                if !tz.trim().is_empty() && tz.parse::<chrono_tz::Tz>().is_err() {
                    return Err(AppError::Validation("unknown timezone".into()));
                }
            }
        }
        TriggerDefinition::FolderCreated { path }
        | TriggerDefinition::FileCreated { path }
        | TriggerDefinition::FileModified { path } => {
            if path.trim().is_empty() {
                return Err(AppError::Validation("trigger path must not be empty".into()));
            }
        }
        TriggerDefinition::Manual => {}
    }

    for action in &automation.actions {
        match action {
            crate::core::models::ActionDefinition::Shell { command }
                if command.trim().is_empty() =>
            {
                return Err(AppError::Validation("shell command must not be empty".into()));
            }
            crate::core::models::ActionDefinition::Command { executable, .. }
                if executable.trim().is_empty() =>
            {
                return Err(AppError::Validation("executable must not be empty".into()));
            }
            crate::core::models::ActionDefinition::Python { script, .. }
                if script.trim().is_empty() =>
            {
                return Err(AppError::Validation("python script must not be empty".into()));
            }
            _ => {}
        }
    }

    Ok(())
}

fn valid_schedule(value: &str) -> bool {
    if valid_time(value) {
        return true;
    }
    // Accept 5-field cron ("0 9 * * *") or 6-field cron with seconds.
    let fields: Vec<&str> = value.split_whitespace().collect();
    if fields.len() == 5 {
        return format!("0 {}", value).parse::<cron::Schedule>().is_ok();
    }
    value.parse::<cron::Schedule>().is_ok()
}

fn valid_time(value: &str) -> bool {
    let mut parts = value.split(':');
    let hour = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minute = parts.next().and_then(|part| part.parse::<u32>().ok());
    parts.next().is_none() && hour.is_some_and(|value| value < 24) && minute.is_some_and(|value| value < 60)
}

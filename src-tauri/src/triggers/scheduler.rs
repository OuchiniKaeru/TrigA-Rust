use std::{str::FromStr, sync::Arc, time::Duration};

use chrono::{DateTime, Local, NaiveTime, Utc};
use cron::Schedule;

use crate::{
    core::models::{AutomationDefinition, TriggerDefinition},
    engine::application_service::{self, AppState},
    services::variables::EventContext,
};

/// Register all enabled interval/scheduled automations at boot.
pub async fn start(state: Arc<AppState>) -> Result<(), crate::AppError> {
    let automations = crate::storage::repository::list_automations(&state.pool).await?;
    for automation in automations {
        start_automation(Arc::clone(&state), automation).await;
    }
    Ok(())
}

/// (Re)register a single automation. Aborts any previous task for the id,
/// then spawns a new event-driven loop when the trigger is interval/scheduled
/// and the automation is enabled. Disabled automations are removed (spec #64).
pub async fn start_automation(state: Arc<AppState>, automation: AutomationDefinition) {
    state.scheduler.remove_task(&automation.id).await;
    if !automation.enabled {
        return;
    }
    match &automation.trigger {
        TriggerDefinition::Interval {
            interval_seconds,
            start_immediately,
        } => {
            if *interval_seconds == 0 {
                return;
            }
            let task_state = Arc::clone(&state);
            let task_auto = automation.clone();
            let immediate = *start_immediately;
            let period = Duration::from_secs(*interval_seconds);
            let handle = tokio::spawn(async move {
                if immediate {
                    run_tick(&task_state, &task_auto, "interval").await;
                }
                // Event-driven ticker: no polling. Missed ticks are skipped
                // (spec #66) so a slow action never causes catch-up bursts.
                let mut ticker = tokio::time::interval(period);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                // Consume the immediate first tick.
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    run_tick(&task_state, &task_auto, "interval").await;
                }
            });
            state.scheduler.insert_task(automation.id, handle).await;
        }
        TriggerDefinition::Scheduled { schedule, timezone } => {
            let task_state = Arc::clone(&state);
            let task_auto = automation.clone();
            let schedule_text = schedule.clone();
            let timezone_text = timezone.clone();
            let handle = tokio::spawn(async move {
                loop {
                    let wait = next_scheduled_wait(&schedule_text, timezone_text.as_deref());
                    let Some(duration) = wait else { return };
                    tokio::time::sleep(duration).await;
                    // Misfire policy: skip (spec #25). Because the next wait
                    // is always recomputed from "now", sleeping through
                    // multiple schedule points resumes from the next future
                    // point instead of running catch-up jobs.
                    run_tick(&task_state, &task_auto, "scheduled").await;
                }
            });
            state.scheduler.insert_task(automation.id, handle).await;
        }
        _ => {}
    }
}

pub async fn stop_automation(state: &AppState, automation_id: &str) {
    state.scheduler.remove_task(automation_id).await;
}

async fn run_tick(state: &Arc<AppState>, automation: &AutomationDefinition, trigger_type: &str) {
    // Refresh the definition so edits/disable take effect without restart.
    let current = match crate::storage::repository::get_automation(&state.pool, &automation.id).await
    {
        Ok(value) if value.enabled => value,
        _ => {
            state.scheduler.remove_task(&automation.id).await;
            return;
        }
    };
    let context = EventContext::for_trigger(&current.id, "", None);
    let _ = application_service::run_automation_with_context(
        state,
        &current,
        trigger_type,
        context,
    )
    .await;
}

/// Parse a schedule string into a cron `Schedule`.
/// Accepts 5-field cron ("0 9 * * *"), 6-field cron with seconds,
/// and legacy daily "HH:MM" (treated as "M H * * *").
pub fn parse_schedule(text: &str) -> Option<Schedule> {
    let trimmed = text.trim();
    if is_daily_time(trimmed) {
        let mut parts = trimmed.split(':');
        let hour: u32 = parts.next()?.parse().ok()?;
        let minute: u32 = parts.next()?.parse().ok()?;
        return Schedule::from_str(&format!("0 {minute} {hour} * * *")).ok();
    }
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if fields.len() == 5 {
        return Schedule::from_str(&format!("0 {trimmed}")).ok();
    }
    Schedule::from_str(trimmed).ok()
}

fn is_daily_time(text: &str) -> bool {
    let mut parts = text.split(':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(m), None) => {
            h.parse::<u32>().is_ok_and(|v| v < 24) && m.parse::<u32>().is_ok_and(|v| v < 60)
        }
        _ => false,
    }
}

/// Duration until the next scheduled occurrence, resolved in the requested
/// timezone (spec #65) or the OS local timezone when absent.
fn next_scheduled_wait(schedule: &str, timezone: Option<&str>) -> Option<Duration> {
    let cron_schedule = parse_schedule(schedule)?;
    let now_utc = Utc::now();
    if let Some(tz_name) = timezone.filter(|v| !v.trim().is_empty()) {
        if let Ok(tz) = tz_name.parse::<chrono_tz::Tz>() {
            let now_tz = now_utc.with_timezone(&tz);
            let next_tz = cron_schedule.after(&now_tz).next()?;
            let next_utc = next_tz.with_timezone(&Utc);
            return (next_utc - now_utc).to_std().ok();
        }
        return None;
    }
    let now_local = Local::now();
    let next_local: DateTime<Local> = cron_schedule.after(&now_local).next()?;
    (next_local.with_timezone(&Utc) - now_utc).to_std().ok()
}

#[allow(dead_code)]
fn next_daily_occurrence(now: DateTime<Utc>, time: NaiveTime) -> DateTime<Utc> {
    let today = now.date_naive().and_time(time).and_utc();
    if today > now {
        today
    } else {
        today + chrono::Duration::days(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn daily_time_rolls_to_tomorrow_after_time_passes() {
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 10, 0, 0).unwrap();
        let next =
            next_daily_occurrence(now, NaiveTime::from_hms_opt(9, 0, 0).unwrap());
        assert_eq!(next.date_naive().to_string(), "2026-09-20");
    }

    #[test]
    fn parses_five_field_cron_and_daily_time() {
        assert!(parse_schedule("0 9 * * *").is_some());
        assert!(parse_schedule("09:00").is_some());
        assert!(parse_schedule("not a schedule").is_none());
    }

    #[test]
    fn cron_iterates_forward_in_timezone() {
        let schedule = parse_schedule("0 9 * * *").unwrap();
        let tz: chrono_tz::Tz = "Asia/Tokyo".parse().unwrap();
        let now = tz.with_ymd_and_hms(2026, 9, 19, 8, 0, 0).unwrap();
        let next = schedule.after(&now).next().unwrap();
        assert_eq!(next.hour(), 9);
    }

    #[test]
    fn rejects_unknown_timezone() {
        assert!(next_scheduled_wait("0 9 * * *", Some("Mars/Olympus")).is_none());
    }
}

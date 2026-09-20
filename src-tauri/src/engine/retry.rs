use crate::core::models::{BackoffStrategy, RetryPolicy};
use std::time::Duration;

pub fn retry_delay(policy: &RetryPolicy, failed_attempt: u32) -> Duration {
    let multiplier = match policy.backoff {
        BackoffStrategy::Fixed => 1,
        BackoffStrategy::Exponential => 2u64.saturating_pow(failed_attempt.saturating_sub(1)),
    };
    Duration::from_secs(
        policy
            .delay_seconds
            .saturating_mul(multiplier)
            .min(policy.max_delay_seconds),
    )
}

pub fn should_retry(policy: &RetryPolicy, attempt: u32, timed_out: bool) -> bool {
    policy.enabled && attempt < policy.max_attempts && (!timed_out || policy.retry_on_timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_backoff_is_capped() {
        let policy = RetryPolicy {
            enabled: true,
            max_attempts: 4,
            delay_seconds: 10,
            backoff: BackoffStrategy::Exponential,
            max_delay_seconds: 25,
            retry_on_timeout: true,
        };
        assert_eq!(retry_delay(&policy, 1), Duration::from_secs(10));
        assert_eq!(retry_delay(&policy, 2), Duration::from_secs(20));
        assert_eq!(retry_delay(&policy, 3), Duration::from_secs(25));
    }

    #[test]
    fn max_attempts_is_total_attempt_count() {
        let policy = RetryPolicy {
            enabled: true,
            max_attempts: 3,
            ..RetryPolicy::default()
        };
        assert!(should_retry(&policy, 1, false));
        assert!(should_retry(&policy, 2, false));
        assert!(!should_retry(&policy, 3, false));
    }
}

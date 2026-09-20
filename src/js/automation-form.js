export function buildAutomationDefinition(values, id) {
  const triggerType = values.trigger || 'manual';
  const args = String(values.args || '').trim().split(/\s+/).filter(Boolean);
  const timeoutSeconds = Number(values.timeoutSeconds || 300);
  const retryEnabled = Boolean(values.retryEnabled);
  const maxAttempts = Number(values.maxAttempts || 1);
  const delaySeconds = Number(values.delaySeconds || 0);
  const overlap = values.overlap === 'queue' ? 'queue' : 'skip';

  let trigger;
  if (triggerType === 'manual') {
    trigger = 'manual';
  } else if (triggerType === 'interval') {
    trigger = {
      interval: {
        interval_seconds: Number(values.intervalSeconds || 3600),
        start_immediately: Boolean(values.startImmediately),
      },
    };
  } else if (triggerType === 'scheduled') {
    // Accept cron ("0 9 * * *") or legacy HH:MM. Timezone is optional;
    // empty means OS local timezone (spec #65).
    const schedule = String(values.schedule || values.scheduledTime || '0 9 * * *').trim() || '0 9 * * *';
    const timezone = String(values.timezone || '').trim();
    trigger = {
      scheduled: timezone ? { schedule, timezone } : { schedule },
    };
  } else {
    trigger = {
      [triggerType]: { path: values.watchPath || './' },
    };
  }

  return {
    id,
    name: values.name,
    description: values.description || '',
    enabled: values.enabled !== false,
    trigger,
    conditions: [],
    actions: [{ type: 'command', executable: values.executable, args }],
    execution: {
      timeout_seconds: timeoutSeconds,
      retry: {
        enabled: retryEnabled,
        max_attempts: retryEnabled ? maxAttempts : 1,
        delay_seconds: retryEnabled ? delaySeconds : 0,
        backoff: values.backoff || 'fixed',
        max_delay_seconds: 300,
        retry_on_timeout: retryEnabled && Boolean(values.retryOnTimeout),
      },
      concurrency: { max_workers: 1, overlap_policy: overlap },
      deduplication: { enabled: true, window_seconds: 5 },
    },
  };
}

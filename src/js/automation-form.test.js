import test from 'node:test';
import assert from 'node:assert/strict';
import { buildAutomationDefinition } from './automation-form.js';

test('serializes execution policy controls', () => {
  const automation = buildAutomationDefinition({
    name: 'Retry job',
    trigger: 'manual',
    executable: 'python3',
    args: 'worker.py {job_id}',
    timeoutSeconds: '120',
    retryEnabled: true,
    maxAttempts: '3',
    delaySeconds: '10',
    backoff: 'exponential',
    retryOnTimeout: true,
  }, 'automation-1');

  assert.deepEqual(automation.execution, {
    timeout_seconds: 120,
    retry: {
      enabled: true,
      max_attempts: 3,
      delay_seconds: 10,
      backoff: 'exponential',
      max_delay_seconds: 300,
      retry_on_timeout: true,
    },
    concurrency: { max_workers: 1, overlap_policy: 'skip' },
    deduplication: { enabled: true, window_seconds: 5 },
  });
});

test('builds interval and scheduled triggers per spec', () => {
  const interval = buildAutomationDefinition({
    name: 'Every hour',
    trigger: 'interval',
    executable: 'python3',
    intervalSeconds: '600',
    startImmediately: true,
  }, 'interval-1');
  assert.deepEqual(interval.trigger, {
    interval: { interval_seconds: 600, start_immediately: true },
  });
  assert.equal(interval.execution.concurrency.overlap_policy, 'skip');

  const scheduled = buildAutomationDefinition({
    name: 'Morning',
    trigger: 'scheduled',
    executable: 'python3',
    schedule: '0 9 * * *',
    timezone: 'Asia/Tokyo',
  }, 'scheduled-1');
  assert.deepEqual(scheduled.trigger, {
    scheduled: { schedule: '0 9 * * *', timezone: 'Asia/Tokyo' },
  });

  const folder = buildAutomationDefinition({
    name: 'New folder',
    trigger: 'folder_created',
    executable: 'python3',
    watchPath: '/tmp/input',
  }, 'folder-1');
  assert.deepEqual(folder.trigger, { folder_created: { path: '/tmp/input' } });
});

test('disables retry values when retry is off', () => {
  const automation = buildAutomationDefinition({
    name: 'Simple job',
    executable: 'python3',
    retryEnabled: false,
    maxAttempts: '8',
    delaySeconds: '50',
    retryOnTimeout: true,
  }, 'automation-2');

  assert.equal(automation.execution.retry.max_attempts, 1);
  assert.equal(automation.execution.retry.delay_seconds, 0);
  assert.equal(automation.execution.retry.retry_on_timeout, false);
});

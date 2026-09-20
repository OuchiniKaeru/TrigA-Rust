---
name: triga-skill
description: External AI agents use this skill to manage **TrigA** (Automation Manager) through its loopback REST API. With it you can create automations from natural language, inspect status, run jobs, and read execution history.
---

## Endpoint

```text
http://127.0.0.1:47821/api
```

- Loopback only (`127.0.0.1`). No auth token in V1 — never expose the port.
- All bodies and responses are JSON.
- History endpoints return at most 20 records, newest first (`started_at DESC`).

## Quick capability check

```bash
curl -s http://127.0.0.1:47821/api/schema/automation
```

```json
{
  "triggers": ["folder_created", "file_created", "file_modified", "scheduled", "interval", "manual"],
  "conditions": ["filename", "folder_name", "extension", "path", "exists"],
  "actions": ["command", "python", "shell", "copy", "move"],
  "execution": {
    "timeout_seconds": true,
    "retry": true,
    "concurrency": true,
    "deduplication": true
  }
}
```

Always GET the schema first when generating definitions, and only use the listed trigger/condition/action names.

## Automations

### List

```bash
curl -s http://127.0.0.1:47821/api/automations
```

### Get one

```bash
curl -s http://127.0.0.1:47821/api/automations/{id}
```

### Create

```bash
curl -s -X POST http://127.0.0.1:47821/api/automations \
  -H 'Content-Type: application/json' \
  -d @automation.json
```

### Update (replace whole definition, keep the same id)

```bash
curl -s -X PUT http://127.0.0.1:47821/api/automations/{id} \
  -H 'Content-Type: application/json' \
  -d @automation.json
```

### Delete

```bash
curl -s -X DELETE http://127.0.0.1:47821/api/automations/{id}
```

### Enable / Disable

```bash
curl -s -X POST http://127.0.0.1:47821/api/automations/{id}/enable
curl -s -X POST http://127.0.0.1:47821/api/automations/{id}/disable
```

`enabled: false` stops **automatic** triggers only. Manual runs still work (useful for testing).

### Run now (manual trigger)

```bash
curl -s -X POST http://127.0.0.1:47821/api/automations/{id}/run
# => {"job_id": "..."}
```

## Jobs and history

```bash
curl -s 'http://127.0.0.1:47821/api/history?limit=20'
curl -s http://127.0.0.1:47821/api/history/{id}
curl -s http://127.0.0.1:47821/api/jobs/{id}
curl -s -X POST http://127.0.0.1:47821/api/jobs/{id}/cancel
```

Job `status` is one of `success`, `failed`, `timeout`, `skipped`, `cancelled`. `attempt` counts total tries (`max_attempts` is the total, not retries).

## Automation definition reference

Minimal shape (fields `id`, `name`, `enabled`, `trigger`, `actions`, `execution` are required):

```json
{
  "id": "csv-inbox",
  "name": "Process incoming CSV",
  "description": "Run Python on new CSV files, then archive them",
  "enabled": true,
  "trigger": { "file_created": { "path": "/home/user/work/input" } },
  "conditions": [{ "type": "extension", "value": "csv" }],
  "actions": [
    { "type": "python", "script": "/home/user/work/process.py", "args": ["{file_path}"] },
    { "type": "move", "source": "{file_path}", "destination": "/home/user/work/done/{file_name}" }
  ],
  "execution": {
    "timeout_seconds": 300,
    "retry": {
      "enabled": true,
      "max_attempts": 3,
      "delay_seconds": 10,
      "backoff": "fixed",
      "max_delay_seconds": 300,
      "retry_on_timeout": true
    },
    "concurrency": { "max_workers": 1, "overlap_policy": "skip" },
    "deduplication": { "enabled": true, "window_seconds": 5 }
  }
}
```

### Triggers

```json
"manual"
{ "folder_created": { "path": "/watch/dir" } }
{ "file_created":   { "path": "/watch/dir" } }
{ "file_modified":  { "path": "/watch/dir" } }
{ "interval":  { "interval_seconds": 3600, "start_immediately": false } }
{ "scheduled": { "schedule": "0 9 * * *", "timezone": "Asia/Tokyo" } }
```

- `scheduled.schedule` accepts 5-field cron (`0 9 * * *`) or daily `HH:MM`. Empty `timezone` means OS local time.
- `interval` default overlap is `skip`: a tick is skipped while the previous job still runs.

### Conditions (all must match; empty = always run)

```json
{ "type": "filename",    "pattern": "report_*.csv" }
{ "type": "folder_name", "pattern": "project_*" }
{ "type": "extension",   "value": "csv" }
{ "type": "path",        "pattern": "/data/inbox/*" }
{ "type": "exists",      "expected": true }
```

`*` is a wildcard. `extension` may omit the leading dot.

### Actions (run in order, stop on first failure)

```json
{ "type": "python",  "script": "/path/to/job.py", "args": ["{file_path}"] }
{ "type": "command", "executable": "python3", "args": ["--version"] }
{ "type": "shell",   "command": "echo processed {file_name}" }
{ "type": "copy",    "source": "{file_path}", "destination": "/backup/{file_name}" }
{ "type": "move",    "source": "{file_path}", "destination": "/done/{file_name}" }
```

- No shell interpolation in `command` args (each arg is passed separately).
- `shell` runs via the OS interpreter (`sh -c` / `cmd /C`) and needs its interpreter allowlisted.

### Variables (expanded in action fields)

`{file_path}` `{file_name}` `{file_name_without_ext}` `{folder_path}` `{folder_name}` `{extension}` `{timestamp}` `{date}` `{time}` `{automation_id}` `{job_id}` `{attempt}`

### Execution policy bounds (rejected outside these)

- `timeout_seconds`: 1–86400
- `retry.max_attempts`: 1–10 (total attempts)
- `concurrency.max_workers`: 1–32
- `retry.delay_seconds` must not exceed `retry.max_delay_seconds`

## Natural-language recipe

User: 「InputフォルダにCSVが入ったらPythonで処理してCompletedへ移動して」

1. Pick trigger `file_created` on the Input folder + condition `extension: csv`.
2. Actions: `python` with `{file_path}`, then `move` to the Completed folder.
3. `POST /api/automations`, then `POST /api/automations/{id}/run` for a test (works even while disabled).
4. `GET /api/history?limit=20` to confirm `success`.

## Error handling

Failures return JSON: `{"error": "<type>: <message>"}` with HTTP 400 (validation/security) or 404 (unknown id). Fix the definition and retry — never retry blindly on 400.

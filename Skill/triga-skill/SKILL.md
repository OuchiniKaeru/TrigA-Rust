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

## Workflows (new execution format; legacy automations keep working)

Workflows chain command/python/shell/file/condition/log nodes as a serial DAG.
SQLite is the source of truth; YAML is the import/export format (never auto-executes on import).

```bash
curl -s http://127.0.0.1:47821/api/schema/workflow
curl -s http://127.0.0.1:47821/api/schema/node-types
curl -s http://127.0.0.1:47821/api/workflows
curl -s http://127.0.0.1:47821/api/workflows/{id}
curl -s http://127.0.0.1:47821/api/workflows/{id}/export
curl -s http://127.0.0.1:47821/api/workflow-runs/{job_id}
curl -s http://127.0.0.1:47821/api/workflow-runs/{job_id}/nodes
```

Create / update (validated before save; invalid YAML is rejected and never overwrites):

```bash
curl -s -X POST http://127.0.0.1:47821/api/workflows \
  -H 'Content-Type: application/json' -d '{"yaml": "<workflow YAML>"}'
curl -s -X PUT http://127.0.0.1:47821/api/workflows/{id} \
  -H 'Content-Type: application/json' -d '{"yaml": "<workflow YAML>", "expected_version": 1}'
curl -s -X POST http://127.0.0.1:47821/api/workflows/import \
  -H 'Content-Type: application/json' -d '{"yaml": "<workflow YAML>", "policy": "update|new_id|cancel"}'
```

Run / cancel / versions (run returns `{"job_id"}` immediately; `enabled:false` still allows manual runs):

```bash
curl -s -X POST http://127.0.0.1:47821/api/workflows/{id}/run \
  -H 'Content-Type: application/json' -d '{"inputs": {"target_dir": "/tmp"}}'
curl -s -X POST http://127.0.0.1:47821/api/workflow-runs/{job_id}/cancel
curl -s http://127.0.0.1:47821/api/workflows/{id}/versions
curl -s -X POST http://127.0.0.1:47821/api/workflows/{id}/versions/{version}/restore
curl -s -X POST http://127.0.0.1:47821/api/workflows/{id}/duplicate
curl -s -X POST http://127.0.0.1:47821/api/workflows/{id}/enable
curl -s -X POST http://127.0.0.1:47821/api/workflows/{id}/disable
```

Minimal workflow YAML (refs are typed `$ref`, never `${...}` interpolation):

```yaml
schema_version: 1
workflow:
  id: "wf-demo"
  name: "Demo"
  enabled: true
settings:
  default_timeout_seconds: 300
  default_max_attempts: 3
nodes:
  - id: "start"
    type: "trigger.manual"
    position: { x: 80, y: 160 }
    config: {}
  - id: "run"
    type: "command.exec"
    position: { x: 320, y: 160 }
    config:
      program: "echo"
      args: ["hello"]
  - id: "finish"
    type: "flow.end"
    position: { x: 560, y: 160 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "run"
    target_port: "in"
  - id: "e2"
    source: "run"
    source_port: "next"
    target: "finish"
    target_port: "in"
```

Node types: `trigger.manual`, `trigger.input`, `trigger.url_changed`, `flow.end`, `command.exec`, `python.script`, `shell.exec` (dangerous, explicit only), `file.list`, `file.copy`, `file.move`, `file.wait_ready`, `logic.condition` (ports `true`/`false`), `logic.switch` (multi-branch), `http.request`, `data.transform`, `flow.delay`, `flow.for_each` (ports `next`/`done`), `flow.parallel` (branches + `done`), `notify.desktop`, `notification.log`.
Refs: `workflow.input.<name>`, `workflow.context.job_id`, `workflow.context.trigger_type`, `workflow.context.event.*`, `nodes.<id>.output.<name>`.
Rules: DAG only (cycles rejected), `max_attempts` is total tries (1–10), `timeout_seconds` 1–86400, `failure_policy: stop`, never store secrets in YAML, import alone never executes.
Link an existing automation trigger to a workflow with `POST /api/workflows/{id}/triggers {"automation_id": "..."}` (explicit fan-out; unlinked triggers never double-run).

### trigger.input (runtime inputs)

Declare inputs under top-level `inputs:`, start with a `trigger.input` node, read via `workflow.input.<name>` (or `nodes.<input_id>.output.<name>`).

```yaml
inputs:
  target_dir:
    description: "Target folder"
    default: "/tmp"
  mode:
    description: "Mode"
    required: true
nodes:
  - id: "start"
    type: "trigger.input"
    position: { x: 80, y: 160 }
    config: {}
  - id: "list_files"
    type: "file.list"
    position: { x: 320, y: 160 }
    config:
      path: { $ref: "workflow.input.target_dir" }
```

Run with inputs (defaults apply; missing `required` is rejected):

```bash
curl -s -X POST http://127.0.0.1:47821/api/workflows/{id}/run \
  -H 'Content-Type: application/json' -d '{"inputs": {"target_dir": "/data/inbox", "mode": "fast"}}'
```

### Running a workflow from an existing automation

Reuse a file/schedule/interval/manual trigger without duplicating its definition:

```bash
curl -s -X POST http://127.0.0.1:47821/api/workflows/{workflow_id}/triggers \
  -H 'Content-Type: application/json' -d '{"automation_id": "{automation_id}"}'
curl -s http://127.0.0.1:47821/api/workflows/{workflow_id}/triggers
curl -s -X DELETE http://127.0.0.1:47821/api/workflow-triggers/{link_id}
```

No link = legacy automation runs alone (no double-run). Linked = legacy action + all linked workflows share the same trigger event. `enabled:false` workflows never auto-start (manual run still works). Event values are available as `workflow.context.event.*` (e.g. `file_path`); a declared input with the same name (e.g. `file_path`) is auto-filled on linked runs.

### Passing node outputs to the next node

Reference outputs with `{"$ref": "nodes.<id>.output.<name>"}`:

| Node | Outputs |
|---|---|
| `command.exec` / `python.script` / `shell.exec` | `stdout`, `stderr`, `exit_code` |
| `file.list` | `files`, `path` |
| `file.copy` / `file.move` | `source`, `destination` |
| `logic.condition` | `branch` |
| `logic.switch` | `branch`, `value` |
| `notification.log` | `message`, `level` |
| `trigger.input` | declared input names |

```yaml
- id: "process"
  type: "command.exec"
  config:
    program: "echo"
    args:
      - value: { $ref: "nodes.list_files.output.files" }
- id: "notify"
  type: "notification.log"
  config:
    message: { $ref: "nodes.process.output.stdout" }
```

Unknown output names and forward/circular refs are rejected at validate/save/run time. Node I/O JSON is visible in `GET /api/workflow-runs/{job_id}/nodes`.

### Branch nodes

`logic.condition` = 2-way (`true`/`false` ports, `operator`: `eq/ne/gt/gte/lt/lte/contains/starts_with/ends_with/exists`). `logic.switch` = N-way by value equality:

```yaml
- id: "sw"
  type: "logic.switch"
  config:
    value: { $ref: "workflow.input.mode" }
    cases:
      - equals: "a"
        port: "case1"
      - equals: "b"
        port: "case2"
    default_port: "default"
```

Edges use `source_port: case1 | case2 | default`. First matching case runs, else `default_port`; other paths become `skipped`.

### http.request (external Web API)

`method` GET/POST/PUT/DELETE/PATCH/HEAD, `url` (http/https only), `headers`, `query`, `body`/`body_json`, `timeout_seconds`, `max_response_bytes` (default 1MB, max 10MB). Outputs: `status_code`, `headers`, `body`, `body_json`, `is_success`, `url`, `elapsed_ms`. Object/array bodies are sent as JSON. Timeouts/retries follow the workflow execution policy.

### data.transform (no-code data shaping)

Single `{op, ...}` or `{operations: [...]}` (max 32 steps; omitted per-step `value` chains the previous result). Ops: `replace` {search, replace, all}, `template` {template with `{{key}}`, vars}, `extract` {path like `a.b.0.c`}, `filter` {array, where: {field?, equals|contains|starts_with|ends_with|gt|lt}}, `map` {array, field}, `join` {separator}, `split` {separator, limit}, `trim`/`upper`/`lower`, `date_format` {output_format strftime, optional input_format}. Output: `result`, `steps`. Example: extract one field from `nodes.fetch.output.body_json` for command args.

### flow.delay (sleep)

`{seconds: 0..86400, milliseconds?}` (total max 24h). Async sleep (no worker-thread block), cancellable. Outputs: `waited_ms`, `requested_ms`. Use for post-file grace periods or API rate spacing.

### file.wait_ready (write-completion wait)

`{path, stable_seconds: 1..300 (default 2), timeout_seconds: 1..86400 (default 60), check_interval_ms?}`. Waits until size/mtime stay unchanged for the stable window; times out otherwise. Outputs: `path`, `size`, `stable_seconds`, `waited_ms`. Place after `file_created` triggers before parsing half-written CSVs.

### notify.desktop (OS notification, best-effort)

`{title?, message (required), level: info|warning|error|success}`. Outputs: `delivered`, `title`, `message`. Headless environments return `delivered:false` with success status (pair with `notification.log` for guaranteed history).

### trigger.url_changed (web-update origin)

`{url (required, http/https), method: GET|HEAD, headers?, timeout_seconds?, max_response_bytes?, json_path?, expected_hash?}`. Fetches the URL, hashes content (or the `json_path` sub-value) with sha256. Outputs: `url`, `status_code`, `body`, `hash`, `changed`, `bytes`. No `expected_hash` = first run reports `changed:true`. Recommended polling: link an `interval` automation → this node → `logic.condition` on `nodes.watch.output.changed == true`.

### flow.for_each (loop over arrays)

`{items: array|$ref, max_items: 1..1000 (default 100)}`. Ports: `next` (body, runs once per item) / `done` (after loop). In-body refs: `nodes.<loop>.output.item|index|count`. Outputs: `count`, `items`. Per-iteration rows are recorded as `<node>[i]`. Empty arrays skip the body. Body failure stops the loop (`failure_policy: stop`). No nested for_each/parallel in v1 bodies.

**Pitfall**: do not route an edge back from any body node into the `for_each` node's `next` port — control flow returns to `for_each` automatically after each body iteration. Connecting a body node's output to `for_each.next` creates a graph cycle that fails validation with "graph contains a cycle" or "unknown target port 'next'" on the for_each node.

**Pitfall**: `data.transform` nodes cannot reference their own output. When chaining multiple transform operations (e.g., filter then map), use separate `data.transform` nodes where the second references the first node's `output.result`.

### flow.parallel (fan-out / join)

Branches = every outgoing port except `done`; `done` targets run after all branches. `max_branches: 1..32`. Output: `branches`. Any branch failure fails the node. v1 runs branches sequentially in port order (same results as concurrent; SQLite writes stay serialized). Keep branch node sets disjoint.

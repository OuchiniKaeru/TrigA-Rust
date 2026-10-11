/* TrigA Workflow UI: list / node editor / YAML / run view (Vanilla JS). */
const NODE_CATALOG = [
  { type: 'trigger.manual', label: 'Manual', category: 'Trigger', hint: 'Manual start (no inputs)' },
  { type: 'trigger.input', label: 'Input', category: 'Trigger', hint: 'Start with runtime inputs (workflow.input.*)' },
  { type: 'flow.end', label: 'End', category: 'Flow', hint: 'Workflow end' },
  { type: 'command.exec', label: 'Command', category: 'Command', hint: 'Outputs: stdout, stderr, exit_code' },
  { type: 'python.script', label: 'Python', category: 'Command', hint: 'Outputs: stdout, stderr, exit_code' },
  { type: 'shell.exec', label: 'Shell ⚠', category: 'Command', hint: 'Outputs: stdout, stderr, exit_code' },
  { type: 'file.list', label: 'List files', category: 'File', hint: 'Outputs: files, path' },
  { type: 'file.copy', label: 'Copy', category: 'File', hint: 'Outputs: source, destination' },
  { type: 'file.move', label: 'Move', category: 'File', hint: 'Outputs: source, destination' },
  { type: 'logic.condition', label: 'Condition (2-branch)', category: 'Logic', hint: 'Ports: true / false. Output: branch' },
  { type: 'logic.switch', label: 'Switch (N-branch)', category: 'Logic', hint: 'Ports: cases[] + default. Outputs: branch, value' },
  { type: 'http.request', label: 'HTTP Request', category: 'Web', hint: 'GET/POST/PUT/DELETE. Outputs: status_code, body, is_success…' },
  { type: 'trigger.url_changed', label: 'URL Changed', category: 'Trigger', hint: 'Fetch URL, hash it. Outputs: hash, changed, body' },
  { type: 'data.transform', label: 'Transform', category: 'Data', hint: 'replace/template/extract/filter/map. Output: result' },
  { type: 'flow.for_each', label: 'For Each (loop)', category: 'Flow', hint: 'Ports: next (body) / done. Loop vars: item/index/count' },
  { type: 'flow.parallel', label: 'Parallel (fan-out)', category: 'Flow', hint: 'Every port except done is a branch; done = join' },
  { type: 'flow.delay', label: 'Delay', category: 'Flow', hint: 'Wait seconds. Outputs: waited_ms' },
  { type: 'file.wait_ready', label: 'Wait File Ready', category: 'File', hint: 'Wait until file stable. Outputs: path, size' },
  { type: 'notify.desktop', label: 'Desktop Notify', category: 'Notify', hint: 'OS notification (best-effort)' },
  { type: 'notification.log', label: 'Log', category: 'Notify', hint: 'Outputs: message, level' },
];

const NODE_OUTPUTS = {
  'command.exec': ['stdout', 'stderr', 'exit_code'],
  'python.script': ['stdout', 'stderr', 'exit_code'],
  'shell.exec': ['stdout', 'stderr', 'exit_code'],
  'file.list': ['files', 'path'],
  'file.copy': ['source', 'destination'],
  'file.move': ['source', 'destination'],
  'file.wait_ready': ['path', 'size', 'stable_seconds', 'waited_ms'],
  'logic.condition': ['branch'],
  'logic.switch': ['branch', 'value'],
  'http.request': ['status_code', 'headers', 'body', 'body_json', 'is_success', 'url', 'elapsed_ms'],
  'trigger.url_changed': ['url', 'status_code', 'body', 'hash', 'changed', 'bytes'],
  'data.transform': ['result', 'steps'],
  'flow.delay': ['waited_ms', 'requested_ms'],
  'flow.for_each': ['count', 'items', 'item', 'index'],
  'flow.parallel': ['branches'],
  'notify.desktop': ['delivered', 'title', 'message'],
  'notification.log': ['message', 'level'],
};

function defaultConfig(type) {
  switch (type) {
    case 'command.exec': return { program: 'echo', args: ['hello'] };
    case 'python.script': return { interpreter: 'python3', script: 'process.py', args: [] };
    case 'shell.exec': return { command: 'echo hello' };
    case 'file.list': return { path: './', recursive: false };
    case 'file.copy': return { source: './a.txt', destination: './b.txt' };
    case 'file.move': return { source: './a.txt', destination: './b.txt' };
    case 'logic.condition': return { left: '', operator: 'eq', right: '' };
    case 'logic.switch': return { value: { $ref: 'workflow.input.mode' }, cases: [{ equals: 'a', port: 'case1' }, { equals: 'b', port: 'case2' }], default_port: 'default' };
    case 'http.request': return { method: 'GET', url: 'https://example.com/api', timeout_seconds: 30, max_response_bytes: 1048576 };
    case 'trigger.url_changed': return { url: 'https://example.com/', method: 'GET', timeout_seconds: 30 };
    case 'data.transform': return { operations: [{ op: 'extract', value: { $ref: 'nodes.fetch.output.body_json' }, path: 'data' }] };
    case 'flow.for_each': return { items: { $ref: 'nodes.list_files.output.files' }, max_items: 100 };
    case 'flow.parallel': return { wait_for_all: true, max_branches: 8 };
    case 'flow.delay': return { seconds: 5 };
    case 'file.wait_ready': return { path: './data.csv', stable_seconds: 2, timeout_seconds: 60 };
    case 'notify.desktop': return { title: 'TrigA', message: 'Workflow finished', level: 'info' };
    case 'trigger.input': return {};
    case 'notification.log': return { message: 'done', level: 'info' };
    default: return {};
  }
}

function newWorkflowTemplate() {
  return {
    schema_version: 1,
    workflow: { id: crypto.randomUUID(), name: 'New workflow', description: '', enabled: true },
    settings: { default_timeout_seconds: 300, default_max_attempts: 3, retry_delay_seconds: 10, retry_backoff: 'fixed', max_retry_delay_seconds: 300, retry_on_timeout: true, failure_policy: 'stop' },
    nodes: [
      { id: 'start', type: 'trigger.manual', position: { x: 80, y: 160 }, config: {} },
      { id: 'finish', type: 'flow.end', position: { x: 480, y: 160 }, config: {} },
    ],
    edges: [{ id: 'e1', source: 'start', source_port: 'next', target: 'finish', target_port: 'in' }],
  };
}

export function initWorkflows({ request, t, getLang }) {
  const state = {
    list: [],
    editingId: null,
    def: null,
    dirty: false,
    selected: null,
    connectFrom: null,
    undo: [],
    redo: [],
    validation: null,
    runJobId: null,
    runTimer: null,
    nodeTypes: [],
    ioOpen: new Set(),
  };

  const $ = (s) => document.querySelector(s);
  const canvas = () => $('#wf-canvas');

  async function refreshList() {
    try {
      const q = $('#wf-search')?.value?.trim();
      const enabledFilter = $('#wf-enabled-filter')?.value;
      let url = '/workflows';
      const params = [];
      if (enabledFilter === 'enabled') params.push('enabled=true');
      if (enabledFilter === 'disabled') params.push('enabled=false');
      if (params.length) url += '?' + params.join('&');
      let items = await request(url);
      if (q) {
        const needle = q.toLowerCase();
        items = items.filter((w) => (w.name + ' ' + (w.description || '') + ' ' + w.id).toLowerCase().includes(needle));
      }
      state.list = items;
      renderList();
    } catch (e) { console.error(e); }
  }

  function renderList() {
    const el = $('#workflow-list');
    if (!el) return;
    $('#workflow-count').textContent = t('wf.total', { n: state.list.length });
    if (!state.list.length) {
      el.innerHTML = `<div class="empty">${t('wf.empty')}</div>`;
      return;
    }
    el.innerHTML = state.list.map((w) => `
      <div class="automation-row">
        <div><strong>${escapeHtml(w.name)}</strong><small>${escapeHtml(w.description || t('list.nodesc'))} · v${w.version}</small></div>
        <div>${w.enabled ? '<span class="status enabled">enabled</span>' : '<span class="status disabled">disabled</span>'}</div>
        <div class="actions">
          <button data-wf-run="${w.id}">${t('btn.run')}</button>
          <button data-wf-edit="${w.id}">${t('btn.edit')}</button>
          <button data-wf-toggle="${w.id}" data-enabled="${w.enabled}">${w.enabled ? t('btn.disable') : t('btn.enable')}</button>
          <button data-wf-dup="${w.id}">⎘</button>
          <button data-wf-export="${w.id}">⇩</button>
          <button data-wf-delete="${w.id}">${t('btn.delete')}</button>
        </div>
      </div>`).join('');
  }

  function pushUndo() {
    state.undo.push(JSON.stringify(state.def));
    if (state.undo.length > 50) state.undo.shift();
    state.redo = [];
  }

  function uniqueNodeId(base) {
    const ids = new Set((state.def?.nodes || []).map((n) => n.id));
    if (!ids.has(base)) return base;
    let i = 2;
    while (ids.has(`${base}-${i}`)) i++;
    return `${base}-${i}`;
  }

  /// Fetch a faithful JSON definition for YAML text via the server
  /// (never parse workflow YAML on the client).
  async function fetchDefinition(yamlText) {
    return request('/workflows/parse', { method: 'POST', body: JSON.stringify({ yaml: yamlText }) });
  }

  function markDirty() {
    state.dirty = true;
    updateEditorHeader();
  }

  function updateEditorHeader() {
    const title = $('#wf-editor-title');
    if (title && state.def) {
      title.textContent = `${state.def.workflow?.name || '(unnamed)'}${state.dirty ? ' ●' : ''}`;
    }
  }

  function renderPalette() {
    const el = $('#wf-palette');
    if (!el) return;
    const cats = {};
    for (const n of NODE_CATALOG) {
      (cats[n.category] = cats[n.category] || []).push(n);
    }
    el.innerHTML = Object.entries(cats).map(([cat, items]) => `
      <div class="wf-pal-cat"><h4>${cat}</h4>
      ${items.map((n) => `<button class="wf-pal-item" data-pal="${n.type}" title="${escapeHtml(n.hint || n.type)}">${escapeHtml(n.label)}<small>${n.type}</small></button>`).join('')}
      </div>`).join('') + `
      <div class="wf-pal-cat"><h4>Refs</h4>
        <p class="muted" style="font-size:11px;line-height:1.5">Inputs: <code>workflow.input.&lt;name&gt;</code><br>Outputs: <code>nodes.&lt;id&gt;.output.&lt;name&gt;</code><br>Event: <code>workflow.context.event.file_path</code> etc.<br>Use <code>{"$ref": "..."}</code> in config.</p>
      </div>`;
  }

  function availableRefs() {
    const refs = [];
    for (const [name] of Object.entries(state.def?.inputs || {})) {
      refs.push(`workflow.input.${name}`);
    }
    for (const n of state.def?.nodes || []) {
      const outs = NODE_OUTPUTS[n.type];
      if (n.type === 'trigger.input') {
        for (const [name] of Object.entries(state.def?.inputs || {})) {
          refs.push(`nodes.${n.id}.output.${name}`);
        }
      } else if (outs) {
        for (const o of outs) refs.push(`nodes.${n.id}.output.${o}`);
      }
    }
    refs.push('workflow.context.job_id', 'workflow.context.trigger_type', 'workflow.context.event.file_path', 'workflow.context.event.file_name');
    return refs;
  }

  async function refreshTriggersPanel() {
    const el = $('#wf-triggers');
    if (!el || !state.editingId) return;
    try {
      const links = await request(`/workflows/${state.editingId}/triggers`);
      const autos = await request('/automations').catch(() => []);
      state.automationsCache = autos;
      el.innerHTML = `
        <div class="wf-trigger-list">${links.length ? links.map((l) => `
          <div class="wf-trigger-row"><span>${escapeHtml(l.automation_id || l.trigger_id || l.id)}</span><small>${escapeHtml(l.trigger_type)}</small>
          <button data-unlink="${l.id}">Unlink</button></div>`).join('') : '<span class="muted">No linked automations. Link one to auto-run this workflow on its trigger.</span>'}
        </div>
        <div class="wf-trigger-add">
          <select id="wf-link-auto">${autos.map((a) => `<option value="${a.id}">${escapeHtml(a.name)} (${escapeHtml(typeof a.trigger === 'string' ? a.trigger : Object.keys(a.trigger || {})[0] || 'manual')})</option>`).join('')}</select>
          <button id="wf-link-btn">Link automation</button>
        </div>
        <p class="muted" style="font-size:11px">Link = existing automation's trigger fans out to this workflow (same file/schedule event). Unlinked triggers never double-run. Disabled workflows never auto-start (manual run still works).</p>`;
      $('#wf-link-btn')?.addEventListener('click', async () => {
        const aid = $('#wf-link-auto')?.value;
        if (!aid) return;
        try {
          await request(`/workflows/${state.editingId}/triggers`, { method: 'POST', body: JSON.stringify({ automation_id: aid }) });
          await refreshTriggersPanel();
        } catch (e) { alert(e.message); }
      });
      el.querySelectorAll('[data-unlink]').forEach((b) => b.addEventListener('click', async () => {
        try {
          await fetch('http://127.0.0.1:47821/api/workflow-triggers/' + b.dataset.unlink, { method: 'DELETE' }).then((r) => { if (!r.ok) throw new Error('unlink failed'); });
          await refreshTriggersPanel();
        } catch (e) { alert(e.message); }
      }));
    } catch (e) { el.innerHTML = `<span class="wf-error">${escapeHtml(e.message)}</span>`; }
  }

  function collectRunInputs() {
    const declared = state.def?.inputs || {};
    if (!Object.keys(declared).length) return {};
    // Build defaults first so the prompt shows them.
    const defaults = {};
    for (const [k, v] of Object.entries(declared)) {
      defaults[k] = v?.default ?? '';
    }
    const text = prompt('Workflow inputs (JSON object):', JSON.stringify(defaults));
    if (text === null) return null; // cancelled
    try {
      const parsed = JSON.parse(text);
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) return parsed;
      alert('Inputs must be a JSON object.');
      return null;
    } catch (e) {
      alert('Invalid JSON: ' + e.message);
      return null;
    }
  }

  function nodeStatusClass(nodeId) {
    const run = state.lastRunNodes?.find((n) => n.node_id === nodeId);
    return run ? ` wf-status-${run.status}` : '';
  }

  function renderCanvas() {
    const c = canvas();
    if (!c || !state.def) return;
    c.innerHTML = '';
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', 'wf-edges');
    c.appendChild(svg);
    for (const e of state.def.edges || []) {
      const s = (state.def.nodes || []).find((n) => n.id === e.source);
      const tgt = (state.def.nodes || []).find((n) => n.id === e.target);
      if (!s || !tgt) continue;
      const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
      const x1 = (s.position?.x || 0) + 150, y1 = (s.position?.y || 0) + 30;
      const x2 = (tgt.position?.x || 0), y2 = (tgt.position?.y || 0) + 30;
      line.setAttribute('x1', x1); line.setAttribute('y1', y1);
      line.setAttribute('x2', x2); line.setAttribute('y2', y2);
      line.setAttribute('class', 'wf-edge' + (e.source_port === 'true' ? ' edge-true' : e.source_port === 'false' ? ' edge-false' : ''));
      line.dataset.edge = e.id;
      line.innerHTML = `<title>${e.id} (${e.source_port} → ${e.target_port}) — click to delete</title>`;
      svg.appendChild(line);
    }
    // Resize svg to canvas scroll size
    svg.setAttribute('width', Math.max(c.scrollWidth, 1200));
    svg.setAttribute('height', Math.max(c.scrollHeight, 600));
    for (const n of state.def.nodes || []) {
      const div = document.createElement('div');
      div.className = 'wf-node' + (state.selected === n.id ? ' selected' : '') + (state.connectFrom === n.id ? ' connecting' : '') + nodeStatusClass(n.id);
      div.dataset.node = n.id;
      div.style.left = (n.position?.x || 0) + 'px';
      div.style.top = (n.position?.y || 0) + 'px';
      div.innerHTML = `<b>${escapeHtml(n.id)}</b><span>${escapeHtml(n.type)}</span>`;
      c.appendChild(div);
    }
  }

  function renderProps() {
    const el = $('#wf-props');
    if (!el) return;
    const node = (state.def?.nodes || []).find((n) => n.id === state.selected);
    // Workflow-level fields
    const wf = state.def?.workflow || {};
    let html = `<div class="wf-props-section"><h4>${t('wf.workflow')}</h4>
      <label>Name<input id="wf-name" value="${escapeHtml(wf.name || '')}"></label>
      <label>Description<textarea id="wf-desc">${escapeHtml(wf.description || '')}</textarea></label>
      <label class="check-field"><input id="wf-enabled" type="checkbox" ${wf.enabled === false ? '' : 'checked'}><span>Enabled</span></label>
    </div>`;
    if (!node) {
      html += `<div class="wf-props-section"><p class="muted">${t('wf.selectNode')}</p>
        <label>Default timeout (s)<input id="wf-timeout" type="number" min="1" max="86400" value="${state.def?.settings?.default_timeout_seconds ?? 300}"></label>
        <label>Default max attempts<input id="wf-attempts" type="number" min="1" max="10" value="${state.def?.settings?.default_max_attempts ?? 1}"></label>
        <label>Inputs (JSON, top-level <code>inputs:</code>)<textarea id="wf-inputs" rows="6" placeholder='{"target_dir": {"description": "...", "default": "/tmp", "required": false}}'>${escapeHtml(JSON.stringify(state.def?.inputs ?? {}, null, 2))}</textarea></label>
        <div id="wf-inputs-error" class="wf-error"></div>
        <p class="muted" style="font-size:11px">trigger.input exposes these as <code>workflow.input.&lt;name&gt;</code>. Required inputs without defaults must be given at run time.</p>
      </div>
      <div class="wf-props-section"><h4>Automation triggers</h4><div id="wf-triggers"><span class="muted">Loading…</span></div></div>
      <div class="wf-props-section"><h4>Available refs</h4><p class="muted" style="font-size:11px">${availableRefs().map((r) => `<code>${escapeHtml(r)}</code>`).join('<br>')}</p></div>`;
    } else {
      const outs = node.type === 'trigger.input'
        ? Object.keys(state.def?.inputs || {})
        : (NODE_OUTPUTS[node.type] || []);
      html += `<div class="wf-props-section"><h4>${escapeHtml(node.id)} <small>${escapeHtml(node.type)}</small></h4>
        <div class="wf-props-actions">
          <button id="wf-connect-btn">${state.connectFrom === node.id ? t('wf.cancelConnect') : t('wf.connect')}</button>
          <button id="wf-dup-node-btn">${t('wf.duplicate')}</button>
          <button id="wf-del-node-btn" class="danger">${t('btn.delete')}</button>
        </div>
        <label>Config (JSON)<textarea id="wf-node-config" rows="10">${escapeHtml(JSON.stringify(node.config ?? {}, null, 2))}</textarea></label>
        <div id="wf-config-error" class="wf-error"></div>
        ${outs.length ? `<p class="muted" style="font-size:11px">This node outputs:<br>${outs.map((o) => `<code>nodes.${escapeHtml(node.id)}.output.${escapeHtml(o)}</code>`).join('<br>')}</p>` : ''}
        <p class="muted" style="font-size:11px">Reference outputs with <code>{"$ref": "nodes.&lt;id&gt;.output.&lt;name&gt;"}</code>. Example:<br><code>"path": {"$ref": "nodes.list_files.output.files"}</code></p>
      </div>`;
    }
    el.innerHTML = html;
    $('#wf-name')?.addEventListener('change', (e) => { pushUndo(); state.def.workflow.name = e.target.value; markDirty(); updateEditorHeader(); });
    $('#wf-desc')?.addEventListener('change', (e) => { pushUndo(); state.def.workflow.description = e.target.value; markDirty(); });
    $('#wf-enabled')?.addEventListener('change', (e) => { pushUndo(); state.def.workflow.enabled = e.target.checked; markDirty(); });
    $('#wf-timeout')?.addEventListener('change', (e) => { pushUndo(); state.def.settings.default_timeout_seconds = Number(e.target.value); markDirty(); });
    $('#wf-attempts')?.addEventListener('change', (e) => { pushUndo(); state.def.settings.default_max_attempts = Number(e.target.value); markDirty(); });
    $('#wf-inputs')?.addEventListener('change', (e) => {
      try {
        const v = JSON.parse(e.target.value);
        if (v && typeof v === 'object' && !Array.isArray(v)) {
          pushUndo();
          state.def.inputs = v;
          $('#wf-inputs-error').textContent = '';
          markDirty();
        } else {
          $('#wf-inputs-error').textContent = 'Inputs must be a JSON object.';
        }
      } catch (err) { $('#wf-inputs-error').textContent = 'Invalid JSON: ' + err.message; }
    });
    if ($('#wf-triggers')) refreshTriggersPanel();
    $('#wf-node-config')?.addEventListener('change', (e) => {
      try {
        const v = JSON.parse(e.target.value);
        pushUndo();
        node.config = v;
        $('#wf-config-error').textContent = '';
        markDirty();
      } catch (err) { $('#wf-config-error').textContent = 'Invalid JSON: ' + err.message; }
    });
    $('#wf-connect-btn')?.addEventListener('click', () => {
      state.connectFrom = state.connectFrom === node.id ? null : node.id;
      renderCanvas(); renderProps();
    });
    $('#wf-del-node-btn')?.addEventListener('click', () => {
      pushUndo();
      state.def.nodes = state.def.nodes.filter((n) => n.id !== node.id);
      state.def.edges = state.def.edges.filter((e) => e.source !== node.id && e.target !== node.id);
      state.selected = null;
      markDirty(); renderCanvas(); renderProps();
    });
    $('#wf-dup-node-btn')?.addEventListener('click', () => {
      pushUndo();
      const newId = uniqueNodeId(node.id + '-copy');
      const copy = JSON.parse(JSON.stringify(node));
      copy.id = newId;
      copy.position = { x: (node.position?.x || 0) + 40, y: (node.position?.y || 0) + 40 };
      state.def.nodes.push(copy);
      state.selected = newId;
      markDirty(); renderCanvas(); renderProps();
    });
  }

  function renderValidation() {
    const el = $('#wf-validation');
    if (!el) return;
    if (!state.validation) { el.innerHTML = `<span class="muted">${t('wf.notValidated')}</span>`; return; }
    const v = state.validation;
    el.innerHTML = `
      <div class="wf-validation ${v.valid ? 'ok' : 'bad'}">${v.valid ? t('wf.valid') : t('wf.invalid')}</div>
      ${(v.errors || []).map((e) => `<div class="wf-error">• ${escapeHtml(e)}</div>`).join('')}
      ${(v.warnings || []).map((w) => `<div class="wf-warn">• ${escapeHtml(typeof w === 'string' ? w : w.message)}</div>`).join('')}`;
  }

  function showView(view) {
    document.querySelectorAll('.view').forEach((item) => item.classList.toggle('active', item.id === view));
    document.querySelectorAll('.nav').forEach((item) => item.classList.toggle('active', item.dataset.view === view));
    const titles = { workflows: t('wf.title'), 'workflow-editor': t('wf.editor'), dashboard: t('nav.dashboard'), automations: t('nav.automations'), history: t('panel.history'), settings: t('settings.title') };
    const h = document.querySelector('#page-title');
    if (h) h.textContent = titles[view] || view;
  }

  function escapeHtml(s) {
    return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  }

  function defToYamlJson() {
    return state.def;
  }

  async function saveEditor() {
    if (!state.def || !state.editingId) return;
    try {
      const yaml = toYaml(state.def);
      const rec = await request(`/workflows/${state.editingId}`, { method: 'PUT', body: JSON.stringify({ yaml, expected_version: state.editingVersion }) });
      state.editingVersion = rec.version;
      // Refresh from the canonical server definition so the editor never drifts.
      state.def = await fetchDefinition(rec.definition_yaml);
      state.dirty = false;
      updateEditorHeader();
      renderCanvas(); renderProps();
      await refreshList();
      alert(t('wf.saved'));
    } catch (e) {
      if (/conflict/i.test(e.message)) {
        alert(t('wf.conflict') + '\n' + e.message);
      } else {
        alert(e.message);
      }
    }
  }

  function toYaml(def) {
    // Minimal YAML emitter for the workflow shape (avoids adding js-yaml dependency).
    const esc = (s) => JSON.stringify(String(s));
    const lines = [];
    lines.push(`schema_version: ${def.schema_version ?? 1}`);
    lines.push(`workflow:`);
    lines.push(`  id: ${esc(def.workflow?.id || '')}`);
    lines.push(`  name: ${esc(def.workflow?.name || '')}`);
    lines.push(`  description: ${esc(def.workflow?.description || '')}`);
    lines.push(`  enabled: ${def.workflow?.enabled === false ? 'false' : 'true'}`);
    if (def.inputs && Object.keys(def.inputs).length) {
      lines.push(`inputs:`);
      for (const [k, v] of Object.entries(def.inputs)) lines.push(`  ${k}: ${JSON.stringify(v)}`);
    }
    lines.push(`settings:`);
    const s = def.settings || {};
    for (const [k, v] of Object.entries(s)) lines.push(`  ${k}: ${JSON.stringify(v)}`);
    if (def.triggers?.length) {
      lines.push(`triggers:`);
      for (const trg of def.triggers) {
        lines.push(`  - type: ${esc(trg.type || 'manual')}`);
        for (const [k, v] of Object.entries(trg)) {
          if (k === 'type') continue;
          if (v === undefined || v === null) continue;
          lines.push(`    ${k}: ${JSON.stringify(v)}`);
        }
      }
    }
    lines.push(`nodes:`);
    for (const n of def.nodes || []) {
      lines.push(`  - id: ${esc(n.id)}`);
      lines.push(`    type: ${esc(n.type)}`);
      lines.push(`    position: { x: ${n.position?.x ?? 0}, y: ${n.position?.y ?? 0} }`);
      lines.push(`    config: ${JSON.stringify(n.config ?? {})}`);
    }
    lines.push(`edges:`);
    for (const e of def.edges || []) {
      lines.push(`  - id: ${esc(e.id)}`);
      lines.push(`    source: ${esc(e.source)}`);
      lines.push(`    source_port: ${esc(e.source_port || 'next')}`);
      lines.push(`    target: ${esc(e.target)}`);
      lines.push(`    target_port: ${esc(e.target_port || 'in')}`);
    }
    return lines.join('\n') + '\n';
  }

  async function validateEditor() {
    if (!state.def) return;
    try {
      // Validate the exact editor content (Validate-before-Apply, no persistence).
      const yaml = toYaml(state.def);
      state.validation = await request('/workflows/validate-yaml', { method: 'POST', body: JSON.stringify({ yaml }) });
      renderValidation();
    } catch (e) { alert(e.message); }
  }

  async function runEditor() {
    if (!state.editingId) return;
    if (state.dirty && !confirm(t('wf.runDirty'))) return;
    const inputs = collectRunInputs();
    if (inputs === null) return; // user cancelled
    try {
      const res = await request(`/workflows/${state.editingId}/run`, { method: 'POST', body: JSON.stringify({ inputs }) });
      state.runJobId = res.job_id;
      alert(t('msg.jobCreated', { id: res.job_id }));
      openRunView(res.job_id);
    } catch (e) { alert(e.message); }
  }

  async function openRunView(jobId) {
    state.runJobId = jobId;
    // Fresh open/close state per run: a new job starts with all details closed.
    state.ioOpen = new Set();
    $('#run-dialog').showModal();
    await refreshRunView();
    clearInterval(state.runTimer);
    state.runTimer = setInterval(async () => {
      if (!$('#run-dialog').open) { clearInterval(state.runTimer); return; }
      await refreshRunView();
    }, 2000);
  }

  function rememberOpenDetails() {
    // Merge the DOM state into state.ioOpen before any re-render wipes it,
    // so user-opened input/output sections stay open across live refreshes.
    document.querySelectorAll('#run-detail details[data-io]').forEach((el) => {
      const id = el.getAttribute('data-io');
      if (el.open) state.ioOpen.add(id);
      else state.ioOpen.delete(id);
    });
  }

  function applyOpenDetails() {
    document.querySelectorAll('#run-detail details[data-io]').forEach((el) => {
      if (state.ioOpen.has(el.getAttribute('data-io'))) el.open = true;
    });
  }

  async function refreshRunView() {
    try {
      rememberOpenDetails();
      const detail = await request(`/workflow-runs/${state.runJobId}`);
      const nodes = await request(`/workflow-runs/${state.runJobId}/nodes`).catch(() => detail.nodes || []);
      state.lastRunNodes = nodes;
      $('#run-detail').innerHTML = `
        <div class="run-head"><b>${escapeHtml(detail.job?.automation_name || '')}</b>
        <span class="status ${detail.job?.status}">${escapeHtml(detail.job?.status || '')}</span>
        <small>job ${escapeHtml(state.runJobId)} · trigger ${escapeHtml(detail.job?.trigger_type || '')}</small>
        <span class="run-io-toggle"><button id="run-io-expand">${t('wf.expandAll')}</button><button id="run-io-collapse">${t('wf.collapseAll')}</button></span></div>
        <table><thead><tr><th>Node</th><th>Type</th><th>Status</th><th>Attempt</th><th>Duration</th><th>Error</th></tr></thead>
        <tbody>${nodes.map((n) => `<tr><td>${escapeHtml(n.node_id)}</td><td>${escapeHtml(n.node_type)}</td><td><span class="status ${n.status}">${escapeHtml(n.status)}</span></td><td>${n.attempt}/${n.max_attempts}</td><td>${n.duration_ms ?? '-'}</td><td>${escapeHtml(n.error || '')}</td></tr>
        <tr class="wf-io-row"><td colspan="6"><details data-io="${escapeHtml(n.node_id)}"><summary>input / output</summary>
          <div><b>in:</b> <code>${escapeHtml(n.input_json || '-')}</code></div>
          <div><b>out:</b> <code>${escapeHtml(n.output_json || '-')}</code></div>
        </details></td></tr>`).join('')}</tbody></table>
        <p class="muted" style="font-size:11px">Chain outputs with <code>{"$ref": "nodes.&lt;id&gt;.output.&lt;name&gt;"}</code> (e.g. stdout, files, branch). Switch/condition non-taken paths show as <code>skipped</code>.</p>
        ${detail.job?.status === 'running' || detail.job?.status === 'retrying' ? `<button id="run-cancel-btn" class="danger">${t('btn.cancel')}</button>` : ''}`;
      applyOpenDetails();
      // Terminal states never change: stop the live refresh so open
      // sections are never re-rendered away afterwards.
      if (!['running', 'retrying', 'pending'].includes(detail.job?.status)) {
        clearInterval(state.runTimer);
        state.runTimer = null;
      }
      $('#run-cancel-btn')?.addEventListener('click', async () => {
        await request(`/workflow-runs/${state.runJobId}/cancel`, { method: 'POST' });
        await refreshRunView();
      });
      $('#run-io-expand')?.addEventListener('click', () => {
        document.querySelectorAll('#run-detail details[data-io]').forEach((el) => {
          el.open = true;
          state.ioOpen.add(el.getAttribute('data-io'));
        });
      });
      $('#run-io-collapse')?.addEventListener('click', () => {
        state.ioOpen.clear();
        document.querySelectorAll('#run-detail details[data-io]').forEach((el) => {
          el.open = false;
        });
      });
      renderCanvas();
    } catch (e) { $('#run-detail').innerHTML = `<div class="wf-error">${escapeHtml(e.message)}</div>`; }
  }

  function bindEditor() {
    document.addEventListener('click', async (e) => {
      const pal = e.target.closest('[data-pal]');
      if (pal && state.def) {
        pushUndo();
        const type = pal.dataset.pal;
        const id = uniqueNodeId(type.split('.')[1] + '-' + Math.random().toString(36).slice(2, 6));
        state.def.nodes.push({ id, type, position: { x: 120 + Math.random() * 300, y: 120 + Math.random() * 200 }, config: defaultConfig(type) });
        state.selected = id;
        markDirty(); renderCanvas(); renderProps();
        return;
      }
      const nodeEl = e.target.closest('.wf-node');
      if (nodeEl && canvas()?.contains(nodeEl)) {
        const id = nodeEl.dataset.node;
        if (state.connectFrom && state.connectFrom !== id) {
          pushUndo();
          const from = state.def.nodes.find((n) => n.id === state.connectFrom);
          let port = 'next';
          if (from?.type === 'logic.condition') {
            port = confirm('Use TRUE port? (Cancel = FALSE)') ? 'true' : 'false';
          } else if (from?.type === 'logic.switch') {
            const cases = Array.isArray(from.config?.cases) ? from.config.cases.map((c) => c.port).filter(Boolean) : [];
            const defPort = from.config?.default_port || 'default';
            const all = [...cases, defPort];
            const picked = prompt(`Switch port? (cases: ${cases.join(', ') || '-'}, default: ${defPort})`, all[0] || 'default');
            port = (picked || 'default').trim() || 'default';
          } else if (from?.type === 'flow.for_each') {
            port = confirm('Loop BODY port (next)? (Cancel = DONE port)') ? 'next' : 'done';
          } else if (from?.type === 'flow.parallel') {
            const picked = prompt('Parallel port? (each unique port = one branch; "done" = join continuation)', 'branch1');
            port = (picked || 'branch1').trim() || 'branch1';
          }
          state.def.edges.push({ id: crypto.randomUUID(), source: state.connectFrom, source_port: port, target: id, target_port: 'in' });
          state.connectFrom = null;
          markDirty(); renderCanvas(); renderProps();
        } else {
          state.selected = id;
          renderCanvas(); renderProps();
        }
        return;
      }
      const edge = e.target.closest('.wf-edge');
      if (edge && edge.tagName === 'line') {
        if (confirm(t('wf.deleteEdge'))) {
          pushUndo();
          state.def.edges = state.def.edges.filter((x) => x.id !== edge.dataset.edge);
          markDirty(); renderCanvas();
        }
        return;
      }
    });

    // Drag nodes
    let drag = null;
    document.addEventListener('pointerdown', (e) => {
      const nodeEl = e.target.closest('.wf-node');
      if (nodeEl && canvas()?.contains(nodeEl)) {
        drag = { id: nodeEl.dataset.node, dx: e.clientX - nodeEl.offsetLeft, dy: e.clientY - nodeEl.offsetTop, moved: false };
      }
    });
    document.addEventListener('pointermove', (e) => {
      if (!drag || !state.def) return;
      const node = state.def.nodes.find((n) => n.id === drag.id);
      if (!node) return;
      const rect = canvas().getBoundingClientRect();
      node.position = { x: Math.max(0, e.clientX - rect.left - 75), y: Math.max(0, e.clientY - rect.top - 20) };
      drag.moved = true;
      const el = canvas().querySelector(`[data-node="${CSS.escape(drag.id)}"]`);
      if (el) { el.style.left = node.position.x + 'px'; el.style.top = node.position.y + 'px'; }
    });
    document.addEventListener('pointerup', () => {
      if (drag?.moved) { pushUndoMove(); markDirty(); renderCanvas(); }
      drag = null;
    });
    function pushUndoMove() { /* position already mutated; keep simple history */ }

    $('#wf-save-btn')?.addEventListener('click', saveEditor);
    $('#wf-undo-btn')?.addEventListener('click', () => {
      const prev = state.undo.pop();
      if (!prev) return;
      state.redo.push(JSON.stringify(state.def));
      state.def = JSON.parse(prev);
      state.dirty = true; renderCanvas(); renderProps(); updateEditorHeader();
    });
    $('#wf-redo-btn')?.addEventListener('click', () => {
      const next = state.redo.pop();
      if (!next) return;
      state.undo.push(JSON.stringify(state.def));
      state.def = JSON.parse(next);
      state.dirty = true; renderCanvas(); renderProps(); updateEditorHeader();
    });
    $('#wf-validate-btn')?.addEventListener('click', validateEditor);
    $('#wf-run-btn')?.addEventListener('click', runEditor);
    $('#wf-cancel-run-btn')?.addEventListener('click', async () => {
      if (!state.runJobId) return;
      await request(`/workflow-runs/${state.runJobId}/cancel`, { method: 'POST' }).catch((e) => alert(e.message));
    });
    $('#wf-yaml-btn')?.addEventListener('click', openYamlDialog);
    $('#wf-back-btn')?.addEventListener('click', () => {
      if (state.dirty && !confirm(t('wf.unsaved'))) return;
      showView('workflows');
    });
    $('#wf-new-btn')?.addEventListener('click', async () => {
      const tpl = newWorkflowTemplate();
      try {
        const rec = await request('/workflows', { method: 'POST', body: JSON.stringify({ yaml: toYaml(tpl) }) });
        await refreshList();
        await openEditorWithYaml(rec, rec.definition_yaml);
      } catch (e) { alert(e.message); }
    });
    $('#wf-search')?.addEventListener('input', refreshList);
    $('#wf-enabled-filter')?.addEventListener('change', refreshList);

    document.addEventListener('click', async (e) => {
      const run = e.target.closest('[data-wf-run]');
      const edit = e.target.closest('[data-wf-edit]');
      const toggle = e.target.closest('[data-wf-toggle]');
      const del = e.target.closest('[data-wf-delete]');
      const dup = e.target.closest('[data-wf-dup]');
      const exp = e.target.closest('[data-wf-export]');
      try {
        if (run) {
          if (!confirm(t('wf.confirmRun'))) return;
          // Fetch the definition to know whether inputs are needed.
          let inputs = {};
          try {
            const yaml = await request(`/workflows/${run.dataset.wfRun}/export`, { raw: true });
            const def = await request('/workflows/parse', { method: 'POST', body: JSON.stringify({ yaml: typeof yaml === 'string' ? yaml : '' }) });
            const declared = def?.inputs || {};
            if (Object.keys(declared).length) {
              const defaults = {};
              for (const [k, v] of Object.entries(declared)) defaults[k] = v?.default ?? '';
              const text = prompt('Workflow inputs (JSON object):', JSON.stringify(defaults));
              if (text === null) return;
              try {
                inputs = JSON.parse(text);
              } catch {
                alert('Invalid JSON.');
                return;
              }
            }
          } catch { inputs = {}; }
          const res = await request(`/workflows/${run.dataset.wfRun}/run`, { method: 'POST', body: JSON.stringify({ inputs }) });
          alert(t('msg.jobCreated', { id: res.job_id }));
          openRunView(res.job_id);
        }
        if (edit) {
          const rec = await request(`/workflows/${edit.dataset.wfEdit}`);
          const yaml = await request(`/workflows/${edit.dataset.wfEdit}/export`, { raw: true });
          await openEditorWithYaml(rec, typeof yaml === 'string' ? yaml : rec.definition_yaml);
        }
        if (toggle) {
          const action = toggle.dataset.enabled === 'true' ? 'disable' : 'enable';
          await request(`/workflows/${toggle.dataset.wfToggle}/${action}`, { method: 'POST' });
          await refreshList();
        }
        if (del && confirm(t('wf.confirmDelete'))) {
          await request(`/workflows/${del.dataset.wfDelete}`, { method: 'DELETE' });
          await refreshList();
        }
        if (dup) {
          await request(`/workflows/${dup.dataset.wfDup}/duplicate`, { method: 'POST' });
          await refreshList();
        }
        if (exp) {
          const yaml = await request(`/workflows/${exp.dataset.wfExport}/export`, { raw: true });
          downloadText(`${exp.dataset.wfExport}.yaml`, typeof yaml === 'string' ? yaml : String(yaml));
        }
      } catch (err) { alert(err.message); }
    });

    window.addEventListener('beforeunload', (e) => {
      if (state.dirty && document.querySelector('#workflow-editor.active')) {
        e.preventDefault();
      }
    });
  }

  async function openEditorWithYaml(rec, yamlText) {
    // The server is the single source of truth for YAML parsing:
    // convert the stored/canonical YAML to JSON via /parse.
    let def;
    try {
      def = await fetchDefinition(yamlText);
    } catch (e) {
      alert(t('wf.parseError') + '\n' + e.message);
      return;
    }
    state.def = def;
    state.editingId = rec.id;
    state.editingVersion = rec.version;
    state.dirty = false;
    state.selected = null;
    state.connectFrom = null;
    state.undo = [];
    state.redo = [];
    state.validation = null;
    state.lastRunNodes = null;
    showView('workflow-editor');
    renderPalette(); renderCanvas(); renderProps(); renderValidation(); updateEditorHeader();
    $('#wf-yaml-text').value = yamlText;
  }

  function openYamlDialog() {
    if (!state.def) return;
    $('#wf-yaml-text').value = toYaml(state.def);
    $('#yaml-validation').innerHTML = '';
    $('#yaml-dialog').showModal();
  }

  function bindYamlDialog() {
    $('#wf-yaml-validate')?.addEventListener('click', async () => {
      const yaml = $('#wf-yaml-text').value;
      try {
        const report = await request('/workflows/validate-yaml', { method: 'POST', body: JSON.stringify({ yaml }) });
        const el = $('#yaml-validation');
        if (report.valid) {
          const warns = (report.warnings || []).map((w) => `<div class="wf-warn">• ${escapeHtml(typeof w === 'string' ? w : w.message)}</div>`).join('');
          el.innerHTML = `<div class="wf-validation ok">${t('wf.valid')}</div>${warns}`;
          $('#wf-yaml-apply').disabled = false;
        } else {
          el.innerHTML = `<div class="wf-validation bad">${t('wf.invalid')}</div>` +
            (report.errors || []).map((e) => `<div class="wf-error">• ${escapeHtml(e)}</div>`).join('');
          $('#wf-yaml-apply').disabled = true;
        }
      } catch (e) {
        $('#yaml-validation').innerHTML = `<div class="wf-error">${escapeHtml(e.message)}</div>`;
        $('#wf-yaml-apply').disabled = true;
      }
    });
    $('#wf-yaml-apply')?.addEventListener('click', async () => {
      const yaml = $('#wf-yaml-text').value;
      try {
        const rec = await request(`/workflows/${state.editingId}`, { method: 'PUT', body: JSON.stringify({ yaml, expected_version: state.editingVersion }) });
        state.editingVersion = rec.version;
        state.def = await fetchDefinition(rec.definition_yaml);
        state.dirty = false;
        updateEditorHeader(); renderCanvas(); renderProps();
        $('#yaml-dialog').close();
        await refreshList();
      } catch (e) { alert(e.message); }
    });
    $('#wf-yaml-export')?.addEventListener('click', async () => {
      const yaml = await request(`/workflows/${state.editingId}/export`, { raw: true });
      $('#wf-yaml-text').value = typeof yaml === 'string' ? yaml : String(yaml);
    });
    $('#wf-yaml-import-file')?.addEventListener('change', async (e) => {
      const file = e.target.files[0];
      if (!file) return;
      const text = await file.text();
      $('#wf-yaml-text').value = text;
    });
  }

  function downloadText(name, text) {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: 'text/yaml' }));
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  }

  return { refreshList, bindEditor, bindYamlDialog, showView, defToYamlJson };
}

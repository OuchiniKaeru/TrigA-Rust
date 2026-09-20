import { bindDialogControls } from './modal.js';
import { buildAutomationDefinition } from './automation-form.js';
import { THEMES, loadTheme, applyTheme } from './theme.js';
import {
  LANGUAGES,
  loadLanguage,
  saveLanguage,
  translate,
  applyStaticStrings,
} from './i18n.js';

const API = 'http://127.0.0.1:47821/api';
const $ = (selector) => document.querySelector(selector);
let automations = [], history = [], editingId = null;
let lang = loadLanguage();

const t = (key, vars) => translate(lang, key, vars);

const THEME_LABELS = {
  en: {
    'light-blue': 'Light Blue',
    light: 'Light',
    dark: 'Dark',
    'light-orange': 'Light Orange',
    'light-pink': 'Light Pink',
    'dark-tokyo': 'Dark Tokyo',
  },
  ja: {
    'light-blue': 'ライトブルー',
    light: 'ライト',
    dark: 'ダーク',
    'light-orange': 'ライトオレンジ',
    'light-pink': 'ライトピンク',
    'dark-tokyo': 'ダークトーキョー',
  },
};

const LANG_LABELS = { en: 'English', ja: '日本語' };

const status = (value) => `<span class="status ${value}">${value}</span>`;

function pageTitle(view) {
  if (view === 'settings') return t('settings.title');
  const key = `nav.${view}`;
  const text = t(key);
  if (text === key) return view[0].toUpperCase() + view.slice(1);
  return text;
}

function show(view) {
  document.querySelectorAll('.view').forEach((item) => item.classList.toggle('active', item.id === view));
  document.querySelectorAll('.nav').forEach((item) => item.classList.toggle('active', item.dataset.view === view));
  $('#page-title').textContent = pageTitle(view);
}

async function request(path, options) {
  const response = await fetch(API + path, { headers: { 'Content-Type': 'application/json' }, ...options });
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new Error(body.error || 'Request failed');
  }
  if (response.status === 204) return null;
  return response.json();
}

function renderHistory(target, values) {
  if (!values.length) {
    target.innerHTML = `<div class="empty">${t('empty.executions')}</div>`;
    return;
  }
  target.innerHTML = `<table><thead><tr><th>${t('table.automation')}</th><th>${t('table.trigger')}</th><th>${t('table.status')}</th><th>${t('table.target')}</th><th>${t('table.started')}</th></tr></thead><tbody>${values.slice(0, 20).map((item) => `<tr><td>${item.automation_name}</td><td>${item.trigger_type}</td><td>${status(item.status)}</td><td>${item.result ? item.result.split('/').pop() : '-'}</td><td>${new Date(item.started_at).toLocaleString(lang === 'ja' ? 'ja-JP' : 'en-US')}</td></tr>`).join('')}</tbody></table>`;
}

function triggerLabel(trigger) {
  if (typeof trigger === 'string') return trigger;
  const keys = Object.keys(trigger || {});
  return keys[0] || 'manual';
}

function renderAutomations() {
  $('#automation-count').textContent = t('list.total', { n: automations.length });
  $('#stat-total').textContent = automations.length;
  $('#stat-enabled').textContent = automations.filter((item) => item.enabled).length;
  $('#automation-list').innerHTML = automations.length ? automations.map((item) => `<div class="automation-row"><div><strong>${item.name}</strong><small>${item.description || t('list.nodesc')}</small></div><div>${triggerLabel(item.trigger)}</div><div>${item.enabled ? status('enabled') : status('disabled')}</div><div class="actions"><button data-run="${item.id}">${t('btn.run')}</button><button data-edit="${item.id}">${t('btn.edit')}</button><button data-toggle="${item.id}" data-enabled="${item.enabled}">${item.enabled ? t('btn.disable') : t('btn.enable')}</button><button data-delete="${item.id}">${t('btn.delete')}</button></div></div>`).join('') : `<div class="empty">${t('empty.automations')}</div>`;
}

function openEditor(automation) {
  const form = $('#automation-form');
  editingId = automation?.id || null;
  form.reset();
  $('#automation-dialog h2').textContent = editingId ? t('dialog.edit') : t('dialog.new');
  $('#automation-form [type="submit"]').textContent = editingId ? t('btn.save') : t('btn.create');
  if (!automation) return $('#automation-dialog').showModal();
  const raw = typeof automation.trigger === 'string' ? { manual: {} } : automation.trigger;
  const type = Object.keys(raw)[0];
  const detail = raw[type] || {};
  const retry = automation.execution.retry;
  form.elements.name.value = automation.name;
  form.elements.description.value = automation.description;
  form.elements.trigger.value = type;
  if (form.elements.watchPath && detail.path) form.elements.watchPath.value = detail.path;
  if (form.elements.intervalSeconds && detail.interval_seconds) form.elements.intervalSeconds.value = detail.interval_seconds;
  if (form.elements.startImmediately) form.elements.startImmediately.checked = Boolean(detail.start_immediately);
  if (form.elements.schedule && detail.schedule) form.elements.schedule.value = detail.schedule;
  if (form.elements.timezone) form.elements.timezone.value = detail.timezone || '';
  if (form.elements.overlap && automation.execution?.concurrency?.overlap_policy) form.elements.overlap.value = automation.execution.concurrency.overlap_policy;
  form.elements.executable.value = automation.actions[0].executable || '';
  form.elements.args.value = (automation.actions[0].args || []).join(' ');
  form.elements.timeoutSeconds.value = automation.execution.timeout_seconds;
  form.elements.retryEnabled.checked = retry.enabled;
  form.elements.maxAttempts.value = retry.max_attempts;
  form.elements.delaySeconds.value = retry.delay_seconds;
  form.elements.backoff.value = retry.backoff;
  form.elements.retryOnTimeout.checked = retry.retry_on_timeout;
  $('#automation-dialog').showModal();
}

async function refresh() {
  try {
    automations = await request('/automations');
    history = await request('/history');
    renderAutomations();
    renderHistory($('#history-list'), history);
    renderHistory($('#recent-history'), history.slice(0, 5));
    $('#stat-success').textContent = history.filter((item) => item.status === 'success').length;
    $('#stat-failed').textContent = history.filter((item) => item.status === 'failed').length;
  } catch (error) {
    console.error(error);
  }
}

function themeLabel(name) {
  return (THEME_LABELS[lang] || THEME_LABELS.en)[name] || name;
}

function renderSettings() {
  const themeSelect = $('#theme-select');
  const currentTheme = document.documentElement.dataset.theme || 'light-blue';
  themeSelect.innerHTML = THEMES.map((name) => `<option value="${name}"${name === currentTheme ? ' selected' : ''}>${themeLabel(name)}</option>`).join('');
  const langSelect = $('#lang-select');
  langSelect.innerHTML = LANGUAGES.map((code) => `<option value="${code}"${code === lang ? ' selected' : ''}>${LANG_LABELS[code]}</option>`).join('');
}

function setLanguage(next) {
  lang = saveLanguage(next);
  applyStaticStrings(lang);
  renderSettings();
  refresh();
  const active = document.querySelector('.view.active');
  if (active) show(active.id);
}

loadTheme();
applyStaticStrings(lang);
renderSettings();
$('#theme-select').onchange = (event) => applyTheme(event.target.value);
$('#lang-select').onchange = (event) => setLanguage(event.target.value);

$('#new-automation').onclick = () => openEditor();
document.querySelectorAll('.nav,.text-button').forEach((button) => button.onclick = () => show(button.dataset.view));
document.querySelectorAll('[data-dialog-cancel]').forEach((button) => bindDialogControls($('#automation-dialog'), button));

$('#automation-form').onsubmit = async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const data = new FormData(form);
  const values = Object.fromEntries(data.entries());
  values.retryEnabled = data.get('retryEnabled') === 'on';
  values.retryOnTimeout = data.get('retryOnTimeout') === 'on';
  values.startImmediately = data.get('startImmediately') === 'on';
  values.enabled = editingId ? automations.find((item) => item.id === editingId)?.enabled !== false : true;
  const automation = buildAutomationDefinition(values, editingId || crypto.randomUUID());
  try {
    await request(editingId ? `/automations/${editingId}` : '/automations', { method: editingId ? 'PUT' : 'POST', body: JSON.stringify(automation) });
    $('#automation-dialog').close();
    form.reset();
    editingId = null;
    $('#automation-dialog h2').textContent = t('dialog.new');
    await refresh();
    show('automations');
  } catch (error) {
    alert(error.message);
  }
};

document.addEventListener('click', async (event) => {
  const run = event.target.closest('[data-run]');
  const edit = event.target.closest('[data-edit]');
  const toggle = event.target.closest('[data-toggle]');
  const del = event.target.closest('[data-delete]');
  try {
    if (run) {
      const result = await request(`/automations/${run.dataset.run}/run`, { method: 'POST' });
      alert(t('msg.jobCreated', { id: result.job_id }));
      await refresh();
    }
    if (edit) {
      openEditor(automations.find((item) => item.id === edit.dataset.edit));
    }
    if (toggle) {
      const action = toggle.dataset.enabled === 'true' ? 'disable' : 'enable';
      await request(`/automations/${toggle.dataset.toggle}/${action}`, { method: 'POST' });
      await refresh();
    }
    if (del && confirm(t('msg.confirmDelete'))) {
      await request(`/automations/${del.dataset.delete}`, { method: 'DELETE' });
      await refresh();
    }
  } catch (error) {
    alert(error.message);
  }
});

refresh();
// Background triggers (file/schedule/interval) record history without user
// interaction, so refresh while the tab is visible. 5s is a low-frequency
// UI poll only; the engine itself stays event-driven (spec #17/#95).
setInterval(() => { if (document.visibilityState === 'visible') refresh(); }, 5000);

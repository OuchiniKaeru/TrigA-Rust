export const LANGUAGES = ['en', 'ja'];
export const DEFAULT_LANGUAGE = 'en';

export const STRINGS = {
  en: {
    'brand.subtitle': 'Automation Manager',
    'nav.dashboard': 'Dashboard',
    'nav.automations': 'Automations',
    'nav.workflows': 'Workflows',
    'nav.history': 'History',
    'nav.settings': 'Settings',
    'engine.online': 'Engine online',
    'header.eyebrow': 'LOCAL AUTOMATION CONTROL',
    'action.new': '＋ New automation',
    'stats.automations': 'Automations',
    'stats.enabled': 'Enabled',
    'stats.success': 'Success runs',
    'stats.failed': 'Failed runs',
    'panel.recent': 'Recent executions',
    'panel.viewAll': 'View all',
    'panel.automations': 'Automations',
    'panel.history': 'Execution history',
    'history.latest': 'Latest 20',
    'table.automation': 'Automation',
    'table.trigger': 'Trigger',
    'table.status': 'Status',
    'table.target': 'Target',
    'table.started': 'Started',
    'empty.executions': 'No executions yet.',
    'empty.automations': 'Create your first automation.',
    'list.total': '{n} total',
    'list.nodesc': 'No description',
    'btn.run': 'Run',
    'btn.edit': 'Edit',
    'btn.enable': 'Enable',
    'btn.disable': 'Disable',
    'btn.delete': 'Delete',
    'btn.cancel': 'Cancel',
    'btn.create': 'Create automation',
    'btn.save': 'Save changes',
    'dialog.new': 'New automation',
    'dialog.edit': 'Edit automation',
    'dialog.eyebrow': 'AUTOMATION DEFINITION',
    'field.name': 'Name',
    'field.description': 'Description',
    'field.trigger': 'Trigger',
    'field.watchPath': 'Watch path',
    'field.interval': 'Interval seconds',
    'field.startImmediately': 'Start interval immediately',
    'field.schedule': 'Schedule (cron "0 9 * * *" or HH:MM)',
    'field.timezone': 'Timezone (e.g. Asia/Tokyo, empty=local)',
    'field.overlap': 'Overlap policy',
    'field.executable': 'Command executable',
    'field.args': 'Arguments',
    'field.policy': 'Execution policy',
    'field.timeout': 'Timeout (seconds)',
    'field.retry': 'Enable retry',
    'field.maxAttempts': 'Max attempts',
    'field.delay': 'Retry delay (seconds)',
    'field.backoff': 'Backoff',
    'field.retryOnTimeout': 'Retry on timeout',
    'trigger.manual': 'Manual',
    'trigger.folder_created': 'Folder created',
    'trigger.file_created': 'File created',
    'trigger.file_modified': 'File modified',
    'trigger.interval': 'Interval',
    'trigger.scheduled': 'Scheduled (cron)',
    'overlap.skip': 'Skip (default)',
    'overlap.queue': 'Queue',
    'settings.title': 'Settings',
    'settings.appearance': 'Appearance',
    'settings.appearanceBody': 'Choose a color theme and display language. Preferences are saved in this app.',
    'settings.theme': 'Theme',
    'settings.language': 'Language',
    'settings.api': 'Local API',
    'settings.apiBody': 'External AI agents and CLI tools can manage TrigA through the loopback REST API.',
    'settings.endpoint': 'Endpoint',
    'settings.about': 'About',
    'settings.aboutBody': 'Bug reports and questions are easier to handle with the version number.',
    'settings.version': 'Version',
    'msg.jobCreated': 'Job created: {id}',
    'msg.confirmDelete': 'Delete this automation?',
    'wf.title': 'Workflows',
    'wf.editor': 'Workflow editor',
    'wf.workflow': 'Workflow',
    'wf.empty': 'Create your first workflow.',
    'wf.total': '{n} total',
    'wf.selectNode': 'Select a node to edit its config.',
    'wf.connect': 'Connect →',
    'wf.cancelConnect': 'Cancel connect',
    'wf.duplicate': 'Duplicate node',
    'wf.notValidated': 'Not validated yet',
    'wf.valid': 'Valid ✓',
    'wf.invalid': 'Invalid ✗',
    'wf.saved': 'Workflow saved.',
    'wf.runDirty': 'You have unsaved changes. Run the saved version anyway?',
    'wf.unsaved': 'You have unsaved changes. Leave anyway?',
    'wf.confirmRun': 'Run this workflow now? Disabled workflows can still be run manually.',
    'wf.confirmDelete': 'Delete this workflow?',
    'wf.deleteEdge': 'Delete this connection?',
    'wf.conflict': 'Version conflict. Reload the workflow and reapply your changes.',
    'wf.parseError': 'Failed to load workflow definition.',
    'wf.expandAll': 'Expand all',
    'wf.collapseAll': 'Collapse all',
  },
  ja: {
    'brand.subtitle': 'Automation Manager',
    'nav.dashboard': 'ダッシュボード',
    'nav.automations': '自動化',
    'nav.workflows': 'ワークフロー',
    'nav.history': '履歴',
    'nav.settings': '設定',
    'engine.online': 'エンジン稼働中',
    'header.eyebrow': 'ローカル自動化コントロール',
    'action.new': '＋ 新規作成',
    'stats.automations': '自動化数',
    'stats.enabled': '有効',
    'stats.success': '成功',
    'stats.failed': '失敗',
    'panel.recent': '最近の実行',
    'panel.viewAll': 'すべて見る',
    'panel.automations': '自動化一覧',
    'panel.history': '実行履歴',
    'history.latest': '最新20件',
    'table.automation': '自動化',
    'table.trigger': 'トリガー',
    'table.status': '状態',
    'table.target': '対象',
    'table.started': '開始時刻',
    'empty.executions': '実行履歴がありません。',
    'empty.automations': '最初の自動化を作成しましょう。',
    'list.total': '全{n}件',
    'list.nodesc': '説明なし',
    'btn.run': '実行',
    'btn.edit': '編集',
    'btn.enable': '有効化',
    'btn.disable': '無効化',
    'btn.delete': '削除',
    'btn.cancel': 'キャンセル',
    'btn.create': '自動化を作成',
    'btn.save': '変更を保存',
    'dialog.new': '新規自動化',
    'dialog.edit': '自動化の編集',
    'dialog.eyebrow': '自動化の定義',
    'field.name': '名前',
    'field.description': '説明',
    'field.trigger': 'トリガー',
    'field.watchPath': '監視パス',
    'field.interval': '間隔（秒）',
    'field.startImmediately': '有効化直後に1回実行する',
    'field.schedule': 'スケジュール（cron「0 9 * * *」または HH:MM）',
    'field.timezone': 'タイムゾーン（例 Asia/Tokyo、空欄=ローカル）',
    'field.overlap': '重複ポリシー',
    'field.executable': '実行コマンド',
    'field.args': '引数',
    'field.policy': '実行ポリシー',
    'field.timeout': 'タイムアウト（秒）',
    'field.retry': 'リトライを有効化',
    'field.maxAttempts': '最大試行回数',
    'field.delay': 'リトライ間隔（秒）',
    'field.backoff': 'バックオフ',
    'field.retryOnTimeout': 'タイムアウト時もリトライ',
    'trigger.manual': '手動',
    'trigger.folder_created': 'フォルダ作成',
    'trigger.file_created': 'ファイル作成',
    'trigger.file_modified': 'ファイル変更',
    'trigger.interval': '間隔実行',
    'trigger.scheduled': '定期実行（cron）',
    'overlap.skip': 'スキップ（既定）',
    'overlap.queue': 'キュー',
    'settings.title': '設定',
    'settings.appearance': '外観',
    'settings.appearanceBody': 'カラーテーマと言語を選べます。設定はこのアプリ内に保存されます。',
    'settings.theme': 'テーマ',
    'settings.language': '言語',
    'settings.api': 'ローカルAPI',
    'settings.apiBody': '外部AIエージェントやCLIから、ループバックREST APIでTrigAを操作できます。',
    'settings.endpoint': 'エンドポイント',
    'settings.about': 'このアプリについて',
    'settings.aboutBody': '不具合報告や質問の際はバージョン番号があると対応しやすくなります。',
    'settings.version': 'バージョン',
    'msg.jobCreated': 'ジョブを作成しました: {id}',
    'msg.confirmDelete': 'この自動化を削除しますか？',
    'wf.title': 'ワークフロー',
    'wf.editor': 'ワークフロー編集',
    'wf.workflow': 'ワークフロー',
    'wf.empty': '最初のワークフローを作成しましょう。',
    'wf.total': '全{n}件',
    'wf.selectNode': 'ノードを選択して設定を編集します。',
    'wf.connect': '接続 →',
    'wf.cancelConnect': '接続を取消',
    'wf.duplicate': 'ノードを複製',
    'wf.notValidated': '未検証',
    'wf.valid': '有効 ✓',
    'wf.invalid': '無効 ✗',
    'wf.saved': 'ワークフローを保存しました。',
    'wf.runDirty': '未保存の変更があります。保存済みバージョンを実行しますか？',
    'wf.unsaved': '未保存の変更があります。移動しますか？',
    'wf.confirmRun': 'このワークフローを今実行しますか？無効でも手動実行できます。',
    'wf.confirmDelete': 'このワークフローを削除しますか？',
    'wf.deleteEdge': 'この接続を削除しますか？',
    'wf.conflict': 'バージョン競合が発生しました。再読込して変更を適用し直してください。',
    'wf.parseError': 'ワークフロー定義の読込に失敗しました。',
    'wf.expandAll': 'すべて開く',
    'wf.collapseAll': 'すべて閉じる',
  },
};

export function normalizeLanguage(value) {
  return LANGUAGES.includes(value) ? value : DEFAULT_LANGUAGE;
}

export function translate(lang, key, vars) {
  const table = STRINGS[normalizeLanguage(lang)] || STRINGS.en;
  let text = table[key] ?? STRINGS.en[key] ?? key;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      text = text.replace(`{${name}}`, String(value));
    }
  }
  return text;
}

export function applyStaticStrings(lang) {
  document.querySelectorAll('[data-i18n]').forEach((node) => {
    node.textContent = translate(lang, node.dataset.i18n);
  });
  document.documentElement.lang = normalizeLanguage(lang);
}

export function loadLanguage() {
  let saved = null;
  try {
    saved = localStorage.getItem('triga-lang');
  } catch {
    saved = null;
  }
  return normalizeLanguage(saved);
}

export function saveLanguage(lang) {
  const value = normalizeLanguage(lang);
  try {
    localStorage.setItem('triga-lang', value);
  } catch {
    // Private mode etc: language still applies for this session.
  }
  return value;
}

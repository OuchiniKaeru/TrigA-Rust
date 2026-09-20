import test from 'node:test';
import assert from 'node:assert/strict';
import { translate, normalizeLanguage, DEFAULT_LANGUAGE } from './i18n.js';

test('default language is English', () => {
  assert.equal(DEFAULT_LANGUAGE, 'en');
});

test('translates main labels to Japanese', () => {
  assert.equal(translate('ja', 'nav.dashboard'), 'ダッシュボード');
  assert.equal(translate('ja', 'nav.settings'), '設定');
  assert.equal(translate('ja', 'btn.run'), '実行');
  assert.equal(translate('ja', 'settings.theme'), 'テーマ');
});

test('keeps English strings intact', () => {
  assert.equal(translate('en', 'nav.dashboard'), 'Dashboard');
  assert.equal(translate('en', 'settings.theme'), 'Theme');
});

test('interpolates variables and falls back safely', () => {
  assert.equal(translate('ja', 'list.total', { n: 3 }), '全3件');
  assert.equal(translate('en', 'list.total', { n: 3 }), '3 total');
  assert.equal(translate('xx', 'nav.history'), 'History');
  assert.equal(translate('en', 'missing.key'), 'missing.key');
});

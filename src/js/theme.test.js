import test from 'node:test';
import assert from 'node:assert/strict';
import { THEMES, DEFAULT_THEME, normalizeTheme } from './theme.js';

test('default theme is bright light-blue', () => {
  assert.equal(DEFAULT_THEME, 'light-blue');
  assert.ok(THEMES.includes('light-blue'));
});

test('offers light, dark, and accent themes', () => {
  for (const name of ['light', 'dark', 'light-orange', 'light-pink', 'dark-tokyo']) {
    assert.ok(THEMES.includes(name), `missing theme: ${name}`);
  }
});

test('unknown theme falls back to default', () => {
  assert.equal(normalizeTheme('neon-zebra'), DEFAULT_THEME);
  assert.equal(normalizeTheme('dark'), 'dark');
});

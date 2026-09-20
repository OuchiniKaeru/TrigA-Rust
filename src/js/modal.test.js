import test from 'node:test';
import assert from 'node:assert/strict';
import { bindDialogControls } from './modal.js';

test('cancel button closes the dialog without submitting', () => {
  const dialog = { closeCalls: 0, close() { this.closeCalls += 1; } };
  const cancel = { onclick: null };
  bindDialogControls(dialog, cancel);

  cancel.onclick();

  assert.equal(dialog.closeCalls, 1);
});

test('Escape closes the dialog', () => {
  const dialog = { closeCalls: 0, close() { this.closeCalls += 1; } };
  const cancel = { onclick: null };
  const originalDocument = globalThis.document;
  const listeners = {};
  globalThis.document = { addEventListener(type, listener) { listeners[type] = listener; } };

  bindDialogControls(dialog, cancel);
  listeners.keydown({ key: 'Escape', preventDefault() {} });

  assert.equal(dialog.closeCalls, 1);
  globalThis.document = originalDocument;
});

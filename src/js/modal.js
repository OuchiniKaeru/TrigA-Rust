export function bindDialogControls(dialog, cancelButton) {
  cancelButton.onclick = () => dialog.close();
  if (dialog.__automationEscapeBound) return;
  dialog.__automationEscapeBound = true;
  const eventTarget = dialog.ownerDocument ?? globalThis.document;
  if (!eventTarget) return;
  eventTarget.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      dialog.close();
    }
  });
}

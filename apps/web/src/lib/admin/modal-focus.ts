// Shared keyboard and backdrop behavior for the two administration editors.
export function modalFocus(backdrop: HTMLElement, close: () => void) {
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const dialog = backdrop.querySelector<HTMLElement>('[role="dialog"]');
  const focusable = () => Array.from(dialog?.querySelectorAll<HTMLElement>(
    'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])'
  ) ?? []).filter(element => element.getClientRects().length > 0);
  focusable()[0]?.focus();
  const click = (event: MouseEvent) => { if (event.target === backdrop) close(); };
  const keydown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); return; }
    if (event.key !== 'Tab') return;
    const items = focusable();
    if (!items.length) { event.preventDefault(); return; }
    const first = items[0], last = items[items.length - 1], active = document.activeElement;
    if (event.shiftKey && (active === first || !dialog?.contains(active))) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && (active === last || !dialog?.contains(active))) { event.preventDefault(); first.focus(); }
  };
  const focusin = (event: FocusEvent) => { if (dialog && event.target instanceof Node && !dialog.contains(event.target)) focusable()[0]?.focus(); };
  backdrop.addEventListener('click', click);
  document.addEventListener('keydown', keydown);
  document.addEventListener('focusin', focusin);
  return { destroy() { backdrop.removeEventListener('click', click); document.removeEventListener('keydown', keydown); document.removeEventListener('focusin', focusin); if (previous?.isConnected) previous.focus(); } };
}

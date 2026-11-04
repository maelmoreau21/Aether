/** Navigation clavier du panneau de menus (motif ARIA « menu »). */

export const MENU_ITEM_SELECTOR = '[role="menuitemradio"]:not(:disabled), [role="menuitem"]:not(:disabled)';

/** Index à focaliser après ↑ ↓ Début Fin ; `null` pour les autres touches. */
export function nextMenuIndex(key: string, index: number, count: number): number | null {
  if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(key)) return null;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  if (index === -1) return key === 'ArrowUp' ? count - 1 : 0;
  return (index + (key === 'ArrowUp' ? -1 : 1) + count) % count;
}

export function menuItems(panel: HTMLElement | undefined): HTMLButtonElement[] {
  return panel ? Array.from(panel.querySelectorAll<HTMLButtonElement>(MENU_ITEM_SELECTOR)) : [];
}

/** Roving tabindex : seul `target` reste atteignable au Tab, puis il prend le focus. */
export function focusMenuItem(items: HTMLButtonElement[], target: HTMLButtonElement): void {
  for (const item of items) item.tabIndex = -1;
  target.tabIndex = 0;
  target.focus();
}

import { exitTvApp, isDirection, registerMediaKeys, tvActive, tvKey, type Direction } from './keys';
import { nearestBox } from './spatial';

/**
 * Remote-control navigation of the TV layout: arrows move the focus to the nearest control on screen,
 * OK activates it, Back closes the open layer or goes back in the history (and leaves the TV application
 * on the home screen). Components that already handle an arrow call `preventDefault()` and keep it; a
 * container marked `data-tv-nav="self"` (the video player) handles every key itself.
 */
const FOCUSABLE = 'a[href], button, input:not([type="hidden"]), select, textarea, summary, [tabindex], [contenteditable="true"]';
const SELF = '[data-tv-nav="self"]';
const TEXT_INPUT = /^(?:text|search|email|url|tel|password|number|date|time|datetime-local|month|week)$/;

/**
 * Bars pinned to the top or the bottom of the screen (sticky header, mini-player) are their own layers:
 * the page scrolls under them, so their position on screen says nothing about their place in the page.
 * Up reaches the top bar only once nothing is left above in the page, and Down the bottom bar likewise.
 */
type Layer = 'top' | 'page' | 'bottom';
type Candidate = { element: HTMLElement; box: DOMRect; layer: Layer };

function candidate(element: Element): Candidate | null {
  if (!(element instanceof HTMLElement)) return null;
  if (element.matches(':disabled') || element.tabIndex < 0 || element.closest('[inert], [hidden], [aria-hidden="true"]')) return null;
  if (element.tagName === 'SUMMARY' && element.parentElement?.tagName !== 'DETAILS') return null;
  const box = element.getBoundingClientRect();
  if (box.width < 1 || box.height < 1) return null;
  const style = getComputedStyle(element);
  if (style.visibility === 'hidden' || style.pointerEvents === 'none') return null;
  let layer: Layer = 'page';
  for (let node: HTMLElement | null = element; node && node !== document.body; node = node.parentElement) {
    const nodeStyle = node === element ? style : getComputedStyle(node);
    if (nodeStyle.opacity === '0') return null;
    if (layer !== 'page' || (nodeStyle.position !== 'fixed' && nodeStyle.position !== 'sticky')) continue;
    // A pinned control off screen (a skip link waiting for Tab) can never be scrolled into view.
    if (nodeStyle.position === 'fixed' && (box.bottom <= 0 || box.top >= innerHeight || box.right <= 0 || box.left >= innerWidth)) return null;
    const bar = node.getBoundingClientRect();
    // Only wide, short bars: a sticky side column belongs to the page.
    if (bar.width >= innerWidth / 2 && bar.height <= innerHeight * 0.4) layer = bar.top + bar.height / 2 < innerHeight / 2 ? 'top' : 'bottom';
  }
  return { element, box, layer };
}

/** The layer arrows move in: the top modal dialog, or the page. */
export function navigationScope(): HTMLElement {
  const dialogs = [...document.querySelectorAll<HTMLDialogElement>('dialog[open]')];
  const modal = dialogs.reverse().find(dialog => { try { return dialog.matches(':modal'); } catch { return true; } });
  return modal ?? document.body;
}

function candidates(scope: ParentNode): Candidate[] {
  return [...scope.querySelectorAll(FOCUSABLE)].map(candidate).filter((item): item is Candidate => item !== null);
}

export function focusables(scope: ParentNode = navigationScope()): HTMLElement[] {
  return candidates(scope).map(item => item.element);
}

function reveal(element: HTMLElement) {
  element.focus({ preventScroll: true });
  // Rows scroll only as far as needed; the page keeps the focused row near the middle of the screen.
  element.scrollIntoView({ block: 'center', inline: 'nearest' });
}

/** First control worth focusing when nothing is: the first visible one in the main content. */
export function focusFirst(scope: HTMLElement = navigationScope()): HTMLElement | null {
  const main = scope === document.body ? document.querySelector<HTMLElement>('main') ?? scope : scope;
  const all = focusables(main);
  const visible = all.find(element => { const rect = element.getBoundingClientRect(); return rect.bottom > 0 && rect.top < innerHeight; });
  const target = visible ?? all[0] ?? focusables(scope)[0] ?? null;
  if (target) reveal(target);
  return target;
}

/** Moves the focus one step; returns the element reached, or null at the edge of the layer. */
export function moveFocus(direction: Direction, scope: HTMLElement = navigationScope()): HTMLElement | null {
  const current = document.activeElement instanceof HTMLElement && scope.contains(document.activeElement) && document.activeElement !== scope ? document.activeElement : null;
  if (!current) return focusFirst(scope);
  const all = candidates(scope);
  const layer = all.find(item => item.element === current)?.layer ?? 'page';
  const others = all.filter(item => item.element !== current && !item.element.contains(current));
  const from = current.getBoundingClientRect();
  for (const wanted of layerOrder(layer, direction)) {
    const pool = others.filter(item => item.layer === wanted);
    const index = nearestBox(from, pool.map(item => item.box), direction);
    if (index >= 0) { reveal(pool[index].element); return pool[index].element; }
  }
  return null;
}

function layerOrder(layer: Layer, direction: Direction): Layer[] {
  if (layer === 'top') return direction === 'down' ? ['page', 'bottom'] : ['top'];
  if (layer === 'bottom') return direction === 'up' ? ['page', 'top'] : ['bottom'];
  return direction === 'up' ? ['page', 'top'] : direction === 'down' ? ['page', 'bottom'] : ['page'];
}

function editable(element: Element | null): boolean {
  if (!(element instanceof HTMLElement)) return false;
  if (element.isContentEditable || element.tagName === 'TEXTAREA') return true;
  return element instanceof HTMLInputElement && TEXT_INPUT.test(element.type);
}

/** Whether the element keeps this arrow for itself (caret, slider, list box). */
function ownsArrow(element: Element | null, direction: Direction): boolean {
  if (!(element instanceof HTMLElement)) return false;
  const sideways = direction === 'left' || direction === 'right';
  if (editable(element)) {
    if (element.tagName === 'TEXTAREA' || element.isContentEditable) return true;
    // Left and right move the caret until it reaches the end of the text.
    const input = element as HTMLInputElement;
    if (!sideways || input.selectionStart === null) return false;
    return direction === 'left' ? input.selectionStart > 0 : (input.selectionEnd ?? 0) < input.value.length;
  }
  if (element instanceof HTMLInputElement && element.type === 'range') return sideways;
  if (element.getAttribute('role') === 'slider') return sideways;
  return false;
}

/** An open layer Back should close before going back in the history. */
function openLayer(): boolean {
  if (document.querySelector('dialog[open], [aria-haspopup]:not([aria-haspopup="false"])[aria-expanded="true"]')) return true;
  const active = document.activeElement;
  return Boolean(active?.closest('details[open]'));
}

function atHome(): boolean {
  const path = location.pathname.replace(/\/+$/, '');
  return /\/app$/.test(path) || path === '' || /\/login$/.test(path);
}

function back() {
  const active = document.activeElement instanceof HTMLElement ? document.activeElement : document.body;
  if (openLayer()) {
    const escape = new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true, cancelable: true });
    active.dispatchEvent(escape);
    const dialog = [...document.querySelectorAll<HTMLDialogElement>('dialog[open]')].pop();
    if (dialog && !escape.defaultPrevented) {
      const request = (dialog as HTMLDialogElement & { requestClose?: () => void }).requestClose;
      if (request) request.call(dialog);
      else if (dialog.dispatchEvent(new Event('cancel', { cancelable: true }))) dialog.close();
    }
    return;
  }
  if (editable(active)) { active.blur(); return; }
  if (!atHome()) { history.back(); return; }
  exitTvApp();
}

function keydown(event: KeyboardEvent) {
  if (!tvActive() || event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
  const target = event.target instanceof Element ? event.target : null;
  if (target?.closest(SELF) || navigationScope().closest(SELF)) return;
  const key = event.key === 'Backspace' && !editable(target) ? 'back' : tvKey(event);
  if (isDirection(key)) {
    if (ownsArrow(target, key)) return;
    event.preventDefault();
    moveFocus(key);
  } else if (key === 'back') {
    // Escape inside an input already reached its component; the TV keys never do.
    if (event.key === 'Escape') { if (!openLayer()) { event.preventDefault(); back(); } return; }
    event.preventDefault();
    back();
  } else if (key === 'enter' && target instanceof HTMLInputElement && (target.type === 'checkbox' || target.type === 'radio')) {
    // OK is Enter, which never toggles a box on its own.
    event.preventDefault();
    target.click();
  }
}

/** Installs the remote-control navigation; returns the cleanup. Inactive outside the TV layout. */
export function installTvNavigation(): () => void {
  if (tvActive()) registerMediaKeys();
  window.addEventListener('keydown', keydown);
  return () => window.removeEventListener('keydown', keydown);
}

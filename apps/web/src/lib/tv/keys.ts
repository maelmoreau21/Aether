/**
 * Remote-control keys of Samsung Tizen and LG webOS TVs, read from `key` first and then from the
 * legacy `keyCode`: TV browsers often report `Unidentified` for the remote's dedicated keys.
 */
export type TvKey =
  | 'up' | 'down' | 'left' | 'right' | 'enter' | 'back'
  | 'playPause' | 'play' | 'pause' | 'stop' | 'forward' | 'rewind' | 'next' | 'previous';

export type Direction = 'up' | 'down' | 'left' | 'right';

const BY_KEY: Record<string, TvKey> = {
  ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right', Enter: 'enter',
  Escape: 'back', GoBack: 'back', BrowserBack: 'back', XF86Back: 'back',
  MediaPlayPause: 'playPause', MediaPlay: 'play', MediaPause: 'pause', MediaStop: 'stop',
  MediaFastForward: 'forward', MediaRewind: 'rewind', MediaTrackNext: 'next', MediaTrackPrevious: 'previous'
};

// Tizen (10009 back, 10252 play/pause, 10232/10233 previous/next) and webOS (461 back); 412–417 and 19 are
// shared by both platforms and HbbTV.
const BY_CODE: Record<number, TvKey> = {
  37: 'left', 38: 'up', 39: 'right', 40: 'down', 13: 'enter',
  10009: 'back', 461: 'back',
  10252: 'playPause', 179: 'playPause', 415: 'play', 19: 'pause', 413: 'stop',
  417: 'forward', 412: 'rewind', 10233: 'next', 176: 'next', 10232: 'previous', 177: 'previous'
};

/** Keys Tizen only delivers once registered (`tizen.tvinputdevice`). */
export const TIZEN_MEDIA_KEYS = ['MediaPlayPause', 'MediaPlay', 'MediaPause', 'MediaStop', 'MediaFastForward', 'MediaRewind', 'MediaTrackNext', 'MediaTrackPrevious'] as const;

export function tvKey(event: Pick<KeyboardEvent, 'key' | 'keyCode'>): TvKey | null {
  const named = BY_KEY[event.key];
  if (named) return named;
  if (event.key && event.key !== 'Unidentified' && event.key.length === 1) return null;
  return BY_CODE[event.keyCode] ?? null;
}

export function isDirection(key: TvKey | null): key is Direction {
  return key === 'up' || key === 'down' || key === 'left' || key === 'right';
}

/** True while the TV layout (`data-tv` on `<html>`) is active. */
export function tvActive(): boolean {
  return typeof document !== 'undefined' && document.documentElement.dataset.tv === 'true';
}

type TizenGlobal = { tvinputdevice?: { registerKeyBatch?: (keys: string[]) => void; registerKey?: (key: string) => void }; application?: { getCurrentApplication?: () => { exit?: () => void } } };
function tizen(): TizenGlobal | undefined {
  return typeof window === 'undefined' ? undefined : (window as unknown as { tizen?: TizenGlobal }).tizen;
}

/** Asks Tizen for the media keys; elsewhere they already reach the page. */
export function registerMediaKeys() {
  const input = tizen()?.tvinputdevice;
  if (!input) return;
  try {
    if (input.registerKeyBatch) input.registerKeyBatch([...TIZEN_MEDIA_KEYS]);
    else for (const key of TIZEN_MEDIA_KEYS) input.registerKey?.(key);
  } catch {
    // Registration fails on keys the model lacks: the remaining ones still work.
  }
}

/** Leaves the TV application (Back on the home screen). Returns false in an ordinary browser. */
export function exitTvApp(): boolean {
  const app = tizen()?.application?.getCurrentApplication?.();
  if (app?.exit) { app.exit(); return true; }
  if (typeof navigator !== 'undefined' && /Web0S|webOS/i.test(navigator.userAgent)) { window.close(); return true; }
  return false;
}

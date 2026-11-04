import { writable, derived } from 'svelte/store';
import { savePreferences } from '../preferences';

export type ThemeMode = 'light' | 'dark' | 'system';

const STORAGE_KEY = 'media_theme';

function getSystemTheme(): 'light' | 'dark' {
  if (typeof window === 'undefined') return 'dark';
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

function getStoredTheme(): ThemeMode {
  if (typeof window === 'undefined') return 'system';
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'light' || stored === 'dark' || stored === 'system') {
      return stored;
    }
  } catch {
    // LocalStorage inaccessible
  }
  return 'system';
}

export const themeMode = writable<ThemeMode>(getStoredTheme());
/** True when the TV layout is active (data-tv on <html>). */
export const tvLayout = writable(false);

export const effectiveTheme = derived(themeMode, ($mode) => {
  if ($mode === 'system') {
    return getSystemTheme();
  }
  return $mode;
});

export function setTheme(mode: ThemeMode, persist = true) {
  themeMode.set(mode);
  if (persist) savePreferences({ theme: mode });
  if (typeof window === 'undefined') return;

  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // LocalStorage inaccessible
  }

  const effective = mode === 'system' ? getSystemTheme() : mode;
  document.documentElement.setAttribute('data-theme', effective);
}

export function initTheme() {
  if (typeof window === 'undefined') return;

  const currentMode = getStoredTheme();
  themeMode.set(currentMode);

  const effective = currentMode === 'system' ? getSystemTheme() : currentMode;
  document.documentElement.setAttribute('data-theme', effective);
  tvLayout.set(document.documentElement.dataset.tv === 'true');

  // Watch for OS theme changes when in 'system' mode
  const mediaQuery = window.matchMedia('(prefers-color-scheme: light)');
  const listener = () => {
    themeMode.update((current) => {
      if (current === 'system') {
        document.documentElement.setAttribute('data-theme', getSystemTheme());
      }
      return current;
    });
  };

  if (typeof mediaQuery.addEventListener === 'function') {
    mediaQuery.addEventListener('change', listener);
  } else if (typeof mediaQuery.addListener === 'function') {
    mediaQuery.addListener(listener);
  }
}

/** Per-device TV layout (10-foot UI). `auto` follows the device detection in static/theme-init.js. */
export type TvMode = 'auto' | 'on' | 'off';
const TV_STORAGE_KEY = 'media_tv';
const TV_USER_AGENT = /SMART-TV|SmartTV|Tizen|Web0S|webOS|HbbTV|NetCast|BRAVIA|Android TV|GoogleTV|AFT[A-Z]|CrKey/i;

export function getTvMode(): TvMode {
  if (typeof window === 'undefined') return 'auto';
  try {
    const stored = localStorage.getItem(TV_STORAGE_KEY);
    if (stored === 'on' || stored === 'off') return stored;
  } catch {
    // LocalStorage inaccessible
  }
  return 'auto';
}

export function setTvMode(mode: TvMode) {
  if (typeof window === 'undefined') return;
  try {
    if (mode === 'auto') localStorage.removeItem(TV_STORAGE_KEY);
    else localStorage.setItem(TV_STORAGE_KEY, mode);
  } catch {
    // LocalStorage inaccessible: the choice still applies to this page.
  }
  const enabled = mode === 'on' || (mode === 'auto' && TV_USER_AGENT.test(navigator.userAgent));
  if (enabled) document.documentElement.dataset.tv = 'true';
  else delete document.documentElement.dataset.tv;
  tvLayout.set(enabled);
}

import { get, writable } from 'svelte/store';
import { session } from '../auth/session';
import { preferenceState, savePreferences } from '../preferences';

/** Extensions with their own administration page; every other one opens its extension page. */
export const pinnableExtensions = ['media-requests', 'media-reports'] as const;
export type PinnableExtension = string;
const pins = writable<string[]>([]);
const names = writable<Record<string, string>>({});
export const extensionPins = { subscribe: pins.subscribe };
export const extensionPinNames = { subscribe: names.subscribe };
let accountId: string | undefined;

/** Any installed extension can be pinned to the administration menu. */
export function isPinnableExtension(id: string): boolean {
  return /^[a-z0-9][a-z0-9._-]{0,63}$/.test(id);
}
export function hasOwnPage(id: string): boolean {
  return pinnableExtensions.some(candidate => candidate === id);
}

export function readExtensionPins(raw: string | null): string[] {
  try {
    const value: unknown = JSON.parse(raw ?? '[]');
    return Array.isArray(value) ? [...new Set(value.filter((id): id is string => typeof id === 'string' && isPinnableExtension(id)))].slice(0, 32) : [];
  } catch { return []; }
}
function readNames(raw: string | null): Record<string, string> {
  try {
    const value: unknown = JSON.parse(raw ?? '{}');
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    return Object.fromEntries(Object.entries(value).filter(([id, name]) => isPinnableExtension(id) && typeof name === 'string').map(([id, name]) => [id, (name as string).slice(0, 80)]));
  } catch { return {}; }
}

function sanitizePins(value: unknown): string[] {
  return Array.isArray(value) ? [...new Set(value.filter((id): id is string => typeof id === 'string' && isPinnableExtension(id)))].slice(0, 32) : [];
}
function sanitizeNames(value: unknown): Record<string, string> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
  return Object.fromEntries(
    Object.entries(value)
      .filter(([id, name]) => isPinnableExtension(id) && typeof name === 'string' && name.length > 0)
      .slice(0, 32)
      .map(([id, name]) => [id, (name as string).slice(0, 80)])
  );
}
function mirrorPins(id: string, next: string[], nextNames: Record<string, string>) {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(`media_admin_extension_pins_${id}`, JSON.stringify(next));
    localStorage.setItem(`media_admin_extension_pin_names_${id}`, JSON.stringify(nextNames));
  } catch { /* Keep the choice for this session if storage is blocked. */ }
}

session.subscribe(current => {
  accountId = current?.user.role === 'administrator' ? current.user.id : undefined;
  let saved: string[] = [], savedNames: Record<string, string> = {};
  if (accountId) {
    const prefs = get(preferenceState);
    if (prefs.adminExtensionPins !== undefined) {
      saved = sanitizePins(prefs.adminExtensionPins);
      savedNames = sanitizeNames(prefs.adminExtensionPinNames);
    } else if (typeof localStorage !== 'undefined') {
      try {
        saved = readExtensionPins(localStorage.getItem(`media_admin_extension_pins_${accountId}`));
        savedNames = readNames(localStorage.getItem(`media_admin_extension_pin_names_${accountId}`));
      } catch { /* Browser storage is optional. */ }
    }
  }
  pins.set(saved); names.set(savedNames);
});

preferenceState.subscribe(prefs => {
  if (!accountId || prefs.adminExtensionPins === undefined) return;
  const saved = sanitizePins(prefs.adminExtensionPins);
  const savedNames = sanitizeNames(prefs.adminExtensionPinNames);
  pins.set(saved); names.set(savedNames);
  mirrorPins(accountId, saved, savedNames);
});

export function toggleExtensionPin(id: string, name?: string): void {
  if (!accountId || !isPinnableExtension(id)) return;
  const current = get(pins);
  const next = current.includes(id) ? current.filter(pin => pin !== id) : [...current, id].slice(0, 32);
  const nextNames = { ...get(names) };
  if (next.includes(id) && name) nextNames[id] = name.slice(0, 80); else delete nextNames[id];
  pins.set(next); names.set(nextNames);
  mirrorPins(accountId, next, nextNames);
  void savePreferences({ adminExtensionPins: next, adminExtensionPinNames: nextNames });
}

import { get, writable } from 'svelte/store';
import { session } from '../auth/session';

export const pinnableExtensions = ['media-requests', 'media-reports'] as const;
export type PinnableExtension = typeof pinnableExtensions[number];
const pins = writable<PinnableExtension[]>([]);
export const extensionPins = { subscribe: pins.subscribe };
let accountId: string | undefined;

export function isPinnableExtension(id: string): id is PinnableExtension {
  return pinnableExtensions.some(candidate => candidate === id);
}

export function readExtensionPins(raw: string | null): PinnableExtension[] {
  try {
    const value: unknown = JSON.parse(raw ?? '[]');
    return Array.isArray(value) ? [...new Set(value.filter((id): id is PinnableExtension => typeof id === 'string' && isPinnableExtension(id)))] : [];
  } catch { return []; }
}

session.subscribe(current => {
  accountId = current?.user.role === 'administrator' ? current.user.id : undefined;
  let saved: PinnableExtension[] = [];
  if (accountId && typeof localStorage !== 'undefined') {
    try { saved = readExtensionPins(localStorage.getItem(`media_admin_extension_pins_${accountId}`)); } catch { /* Browser storage is optional. */ }
  }
  pins.set(saved);
});

export function toggleExtensionPin(id: string): void {
  if (!accountId || !isPinnableExtension(id)) return;
  const current = get(pins);
  const next = current.includes(id) ? current.filter(pin => pin !== id) : [...current, id];
  pins.set(next);
  try { localStorage.setItem(`media_admin_extension_pins_${accountId}`, JSON.stringify(next)); } catch { /* Keep the choice for this session if storage is blocked. */ }
}

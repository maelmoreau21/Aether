import { afterEach, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { session, type Session } from '../auth/session';
import { extensionPins, readExtensionPins, toggleExtensionPin } from './extension-pins';

const account = (id: string, role: 'administrator' | 'user' = 'administrator'): Session => ({ user: { id, login: id, role }, csrfToken: 'private' });
afterEach(() => { session.set(null); vi.unstubAllGlobals(); });

it('ignores corrupt, invalid and duplicate pin values; any extension id is pinnable', () => {
  expect(readExtensionPins('invalid')).toEqual([]);
  expect(readExtensionPins('{}')).toEqual([]);
  expect(readExtensionPins('["media-reports",null,"Bad id!","tmdb","media-reports"]')).toEqual(['media-reports', 'tmdb']);
});

it('persists pins per administrator and never leaks them to another account', () => {
  const saved = new Map<string, string>();
  vi.stubGlobal('localStorage', { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => saved.set(key, value) });
  session.set(account('a')); toggleExtensionPin('media-requests');
  expect(get(extensionPins)).toEqual(['media-requests']);
  session.set(account('b')); expect(get(extensionPins)).toEqual([]);
  toggleExtensionPin('media-reports');
  session.set(account('a')); expect(get(extensionPins)).toEqual(['media-requests']);
  expect([...saved.values()].join()).not.toContain('private');
  session.set(account('a', 'user')); toggleExtensionPin('media-reports'); expect(get(extensionPins)).toEqual([]);
  session.set(null); expect(get(extensionPins)).toEqual([]);
});

it('keeps pinning usable when browser storage is blocked', () => {
  vi.stubGlobal('localStorage', { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } });
  session.set(account('a')); toggleExtensionPin('media-reports');
  expect(get(extensionPins)).toEqual(['media-reports']);
  toggleExtensionPin('media-reports'); expect(get(extensionPins)).toEqual([]);
  toggleExtensionPin('Bad id!'); expect(get(extensionPins)).toEqual([]);
  toggleExtensionPin('fanart-tv', 'Fanart.tv'); expect(get(extensionPins)).toEqual(['fanart-tv']);
});

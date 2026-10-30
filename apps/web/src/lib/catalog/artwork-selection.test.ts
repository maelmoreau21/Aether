import { beforeEach, expect, test, vi } from 'vitest';
import { selectArtwork } from './artwork-selection';
import { invalidateSession, session, type Session } from '../auth/session';
import type { Work } from './api';
import { setArtworkRotationPolicy, type ArtworkRotationPolicy } from './artwork-rotation-policy.svelte';

const current: Session = { user: { id: 'viewer', login: 'viewer', role: 'user' }, csrfToken: 'session-one' };
const work: Work = { id: 'film', title: 'Film', kind: 'film', year: null, description: '', libraryId: 'library', synthetic: false, artwork: { backdrop: '/fallback' }, posterUrl: '/poster' };
const variant = (id: string, kind = 'backdrop', language: string | null = null, origin = 'provider', position = 0) => ({ id, kind, language, origin, position, url: `/images/${id}` });
const policy: ArtworkRotationPolicy = { enabled: true, enableMovies: true, enableSeries: true, rotationMode: 'random', minHoursBetweenSwitches: 0, poolSize: 4, preferredLanguage: 'fr', fallbackLanguage: 'en', includeUnknownLanguage: true, allowAnyLanguageFallback: true };

beforeEach(() => {
  const stored = new Map<string, string>();
  vi.stubGlobal('localStorage', { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => stored.set(key, value) });
  const visit = new Map<string, string>();
  vi.stubGlobal('sessionStorage', { getItem: (key: string) => visit.get(key) ?? null, setItem: (key: string, value: string) => visit.set(key, value), clear: () => visit.clear() });
  invalidateSession();
  setArtworkRotationPolicy(policy);
});

test('preserves legacy fallbacks and handles missing works', () => {
  expect(selectArtwork(work, current)).toEqual({ backdrop: '/fallback', poster: '/poster' });
  expect(selectArtwork(null, current)).toEqual({});
});

test('stays stable across API ordering, navigation and reload; next login avoids the previous image', () => {
  const images = [variant('a', 'poster'), variant('b', 'poster'), variant('c', 'poster')];
  const first = selectArtwork({ ...work, imageVariants: images }, current).poster;
  expect(selectArtwork({ ...work, imageVariants: images.toReversed() }, current).poster).toBe(first);
  session.set(current); // Clear in-memory history as a reload would; recover browser history.
  setArtworkRotationPolicy(policy);
  expect(selectArtwork({ ...work, imageVariants: images }, current).poster).toBe(first);
  const next = { ...current, csrfToken: 'session-two' };
  session.set(next);
  setArtworkRotationPolicy(policy);
  expect(selectArtwork({ ...work, imageVariants: images }, next).poster).not.toBe(first);
  expect(localStorage.getItem('media_artwork_choices_viewer')).not.toContain(current.csrfToken);
});

test('manual overrides remain fixed and the chosen language takes priority over neutral images', () => {
  const images = [variant('neutral'), variant('english', 'backdrop', 'en'), variant('french', 'backdrop', 'fr')];
  expect(selectArtwork({ ...work, imageVariants: images }, current).backdrop).toBe('/images/french');
  expect(selectArtwork({ ...work, imageVariants: images }, current, ['de', 'en']).backdrop).toBe('/images/english');
  expect(selectArtwork({ ...work, imageVariants: [...images, variant('manual', 'backdrop', 'de', 'manual')] }, current).backdrop).toBe('/images/manual');
  expect(selectArtwork({ ...work, imageVariants: [...images, variant('legacy', 'backdrop', null, 'legacy')] }, current).backdrop).toBe('/images/legacy');
});

test('rotates every role independently and retains a remaining image when a variant disappears', () => {
  const images = ['poster', 'backdrop', 'logo', 'thumb', 'illustration'].flatMap(kind => [variant(`${kind}-a`, kind), variant(`${kind}-b`, kind)]);
  const selected = selectArtwork({ ...work, imageVariants: images }, current);
  expect(Object.keys(selected)).toEqual(expect.arrayContaining(['poster', 'backdrop', 'logo', 'thumb', 'illustration']));
  expect(selectArtwork({ ...work, imageVariants: [variant('only', 'poster')] }, current).poster).toBe('/images/only');
});

test('tolerates malformed or blocked browser storage', () => {
  vi.stubGlobal('localStorage', { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } });
  expect(selectArtwork({ ...work, imageVariants: [variant('a')] }, current).backdrop).toBe('/images/a');
});

test('keeps account histories separate and bounds retained choices', () => {
  const images = [variant('a', 'poster'), variant('b', 'poster')];
  for (let i = 0; i < 520; i++) selectArtwork({ ...work, id: `film-${i}`, imageVariants: images }, current);
  expect(Object.keys(JSON.parse(localStorage.getItem('media_artwork_choices_viewer')!))).toHaveLength(512);
  const other = { ...current, user: { ...current.user, id: 'other' } };
  selectArtwork({ ...work, imageVariants: images }, other);
  expect(Object.keys(JSON.parse(localStorage.getItem('media_artwork_choices_other')!))).toEqual(['film:poster']);
});

test('new browsing visits rotate while the authentication cookie remains the same', () => {
  const item = { ...work, imageVariants: [variant('a', 'poster'), variant('b', 'poster')] };
  const first = selectArtwork(item, current).poster;
  sessionStorage.clear();
  session.set(current);
  setArtworkRotationPolicy(policy);
  expect(selectArtwork(item, current).poster).not.toBe(first);
});

test('disabled rotation leaves posters fixed and does not write rotation history', () => {
  setArtworkRotationPolicy({ ...policy, enabled: false });
  const item = { ...work, imageVariants: [variant('b', 'poster'), variant('a', 'poster')] };
  expect(selectArtwork(item, current).poster).toBe('/images/a');
  expect(localStorage.getItem('media_artwork_choices_viewer')).toBeNull();
});

test('server poster choice is shared unless a manual image or personal rotation takes priority', () => {
  const images = [variant('a', 'poster', 'fr'), variant('server', 'poster', 'en', 'provider', -1)];
  setArtworkRotationPolicy({ ...policy, enabled: false });
  expect(selectArtwork({ ...work, imageVariants: images }, current).poster).toBe('/images/server');
  expect(selectArtwork({ ...work, imageVariants: [...images, variant('manual', 'poster', null, 'manual')] }, current).poster).toBe('/images/manual');
  setArtworkRotationPolicy({ ...policy, preferredLanguage: 'fr', allowAnyLanguageFallback: false });
  expect(selectArtwork({ ...work, imageVariants: images }, current).poster).toBe('/images/a');
});

test('media type, pool size and sequential setting control rotation', () => {
  setArtworkRotationPolicy({ ...policy, rotationMode: 'sequential', poolSize: 2 });
  const item = { ...work, imageVariants: [variant('a', 'poster'), variant('b', 'poster'), variant('c', 'poster')] };
  const first = selectArtwork(item, current).poster;
  expect(['/images/a', '/images/b']).toContain(first);
  session.set({ ...current, csrfToken: 'next' }); setArtworkRotationPolicy({ ...policy, rotationMode: 'sequential', poolSize: 2 });
  expect(selectArtwork(item, { ...current, csrfToken: 'next' }).poster).toBe(first === '/images/a' ? '/images/b' : '/images/a');
});

test('rotates at the configured interval within the same visit', () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-27T10:00:00Z'));
  setArtworkRotationPolicy({ ...policy, rotationMode: 'sequential', minHoursBetweenSwitches: 1 });
  const item = { ...work, imageVariants: [variant('a', 'poster'), variant('b', 'poster')] };
  const first = selectArtwork(item, current).poster;
  vi.advanceTimersByTime(3_600_000);
  expect(selectArtwork(item, current).poster).not.toBe(first);
  vi.useRealTimers();
});

test('minimum interval holds the previous poster and film switch excludes series', () => {
  setArtworkRotationPolicy({ ...policy, minHoursBetweenSwitches: 24, enableSeries: false });
  const item = { ...work, imageVariants: [variant('a', 'poster'), variant('b', 'poster')] };
  const first = selectArtwork(item, current).poster;
  session.set({ ...current, csrfToken: 'later' });
  setArtworkRotationPolicy({ ...policy, minHoursBetweenSwitches: 24, enableSeries: false });
  expect(selectArtwork(item, { ...current, csrfToken: 'later' }).poster).toBe(first);
  expect(selectArtwork({ ...item, kind: 'series' as const }, current).poster).toBe('/images/a');
});

test('language restrictions reject unavailable images and keep the fallback', () => {
  setArtworkRotationPolicy({ ...policy, includeUnknownLanguage: false, allowAnyLanguageFallback: false });
  const item = { ...work, imageVariants: [variant('de', 'poster', 'de'), variant('unknown', 'poster')] };
  expect(selectArtwork(item, current).poster).toBe('/poster');
  expect(selectArtwork({ ...item, imageVariants: [...item.imageVariants, variant('english', 'poster', 'en')] }, current).poster).toBe('/images/english');
});


test('an explicitly empty library selection disables rotation for every library', () => {
  const item = { ...work, imageVariants: [variant('a', 'poster'), variant('b', 'poster')] };
  setArtworkRotationPolicy({ ...policy, enabledLibraries: [] });
  expect(selectArtwork(item, current).poster).toBe('/images/a');
  expect(localStorage.getItem('media_artwork_choices_viewer')).toBeNull();
  setArtworkRotationPolicy({ ...policy, enabledLibraries: ['another-library'] });
  expect(selectArtwork(item, current).poster).toBe('/images/a');
  setArtworkRotationPolicy({ ...policy, enabledLibraries: ['library'] });
  selectArtwork(item, current);
  expect(localStorage.getItem('media_artwork_choices_viewer')).not.toBeNull();
});

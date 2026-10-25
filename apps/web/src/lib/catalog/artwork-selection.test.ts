import { beforeEach, expect, test, vi } from 'vitest';
import { selectArtwork } from './artwork-selection';
import { session, type Session } from '../auth/session';
import type { Work } from './api';

const current: Session = { user: { id: 'viewer', login: 'viewer', role: 'user' }, csrfToken: 'session-one' };
const work: Work = { id: 'film', title: 'Film', kind: 'film', year: null, description: '', libraryId: 'library', synthetic: false, artwork: { backdrop: '/fallback' }, posterUrl: '/poster' };
const variant = (id: string, kind = 'backdrop', language: string | null = null, origin = 'provider', position = 0) => ({ id, kind, language, origin, position, url: `/images/${id}` });

beforeEach(() => {
  const stored = new Map<string, string>();
  vi.stubGlobal('localStorage', { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => stored.set(key, value) });
  const visit = new Map<string, string>();
  vi.stubGlobal('sessionStorage', { getItem: (key: string) => visit.get(key) ?? null, setItem: (key: string, value: string) => visit.set(key, value), clear: () => visit.clear() });
  session.set(null);
});

test('preserves legacy fallbacks and handles missing works', () => {
  expect(selectArtwork(work, current)).toEqual({ backdrop: '/fallback', poster: '/poster' });
  expect(selectArtwork(null, current)).toEqual({});
});

test('stays stable across API ordering, navigation and reload; next login avoids the previous image', () => {
  const images = [variant('a'), variant('b'), variant('c')];
  const first = selectArtwork({ ...work, imageVariants: images }, current).backdrop;
  expect(selectArtwork({ ...work, imageVariants: images.toReversed() }, current).backdrop).toBe(first);
  session.set(current); // Clear in-memory history as a reload would; recover browser history.
  expect(selectArtwork({ ...work, imageVariants: images }, current).backdrop).toBe(first);
  const next = { ...current, csrfToken: 'session-two' };
  session.set(next);
  expect(selectArtwork({ ...work, imageVariants: images }, next).backdrop).not.toBe(first);
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
  const images = [variant('a'), variant('b')];
  for (let i = 0; i < 520; i++) selectArtwork({ ...work, id: `film-${i}`, imageVariants: images }, current);
  expect(Object.keys(JSON.parse(localStorage.getItem('media_artwork_choices_viewer')!))).toHaveLength(512);
  const other = { ...current, user: { ...current.user, id: 'other' } };
  selectArtwork({ ...work, imageVariants: images }, other);
  expect(Object.keys(JSON.parse(localStorage.getItem('media_artwork_choices_other')!))).toEqual(['film:backdrop']);
});

test('new browsing visits rotate while the authentication cookie remains the same', () => {
  const item = { ...work, imageVariants: [variant('a'), variant('b')] };
  const first = selectArtwork(item, current).backdrop;
  sessionStorage.clear();
  session.set(current);
  expect(selectArtwork(item, current).backdrop).not.toBe(first);
});

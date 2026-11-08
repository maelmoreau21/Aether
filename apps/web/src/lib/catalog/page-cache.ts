import { writable } from 'svelte/store';
import { onSessionGenerationChange } from '../auth/session';
import { request, type CatalogPage } from './api';
import { languageScope, scopedKey, unscopedKey } from './cache-scope';

// Session-only metadata. Never stores playback credentials or media bytes. Every entry is keyed by
// the reader's account and language chain (see cache-scope.ts): a page rendered for someone else, or
// in another language, can never be found under the current reader's key.
const FRESH_MS = 30_000;
const MAX_BYTES = 4 * 1024 * 1024;
const MAX_PAGES = 48;
const STORAGE_PREFIX = 'media_catalog_cache_v1_';

function storageKey(key: string): string {
  return `${STORAGE_PREFIX}${encodeURIComponent(key)}`;
}

type Entry = { data: CatalogPage; at: number; bytes: number; library: string };
const pages = new Map<string, Entry>();
const pending = new Map<string, { controller: AbortController; promise: Promise<CatalogPage> }>();
export const catalogRevision = writable(0);
let revision = 0;
const scrolls = new Map<string, number>();

export function saveCatalogScroll(key: string, y: number) {
  scrolls.delete(key); scrolls.set(key, y);
  if (scrolls.size > MAX_PAGES) scrolls.delete(scrolls.keys().next().value!);
}
export function catalogScroll(key: string) { return scrolls.get(key); }
let epoch = 0;
let bytes = 0;

export function invalidateCatalog(library?: string, notify = true): void {
  epoch++;
  if (notify) catalogRevision.set(++revision);
  for (const job of pending.values()) job.controller.abort();
  pending.clear();
  for (const [key, entry] of pages) {
    if (!library || !entry.library || entry.library === library) {
      bytes -= entry.bytes;
      pages.delete(key);
      if (typeof sessionStorage !== 'undefined') {
        try { sessionStorage.removeItem(storageKey(key)); } catch { /* ignore */ }
      }
    }
  }
  if (!library && typeof sessionStorage !== 'undefined') {
    try {
      const toRemove: string[] = [];
      for (let i = 0; i < sessionStorage.length; i++) {
        const k = sessionStorage.key(i);
        if (k?.startsWith(STORAGE_PREFIX)) toRemove.push(k);
      }
      for (const k of toRemove) sessionStorage.removeItem(k);
    } catch { /* ignore */ }
  }
}
onSessionGenerationChange(() => { scrolls.clear(); invalidateCatalog(undefined, false); });
if (typeof window !== 'undefined') window.addEventListener('media-access-denied', () => invalidateCatalog(undefined, false));
// Pages are rendered in the account's metadata language: a language change makes them stale.
if (typeof window !== 'undefined') window.addEventListener('media-locale-saved', () => invalidateCatalog());
// A change of language chain (interface language, metadata language or fallbacks) makes the pages that
// are on screen stale: mounted views reload through `catalogRevision`. Entries of the previous chain
// stay unreachable under their own scope.
let lastScope: string | undefined;
languageScope.subscribe(value => {
  if (lastScope !== undefined && value !== lastScope) invalidateCatalog();
  lastScope = value;
});
// Progress affects latest shelves, not the rest of the inventory. Do not
// trigger a catalog reload while the player is open.
export function invalidateLatestCatalog(): void {
  const latest = (key: string) => new URL(unscopedKey(key), 'http://localhost').searchParams.get('scope') === 'latest';
  for (const [key, entry] of pages) {
    if (!latest(key)) continue;
    bytes -= entry.bytes;
    pages.delete(key);
  }
  if (typeof sessionStorage !== 'undefined') {
    try {
      const keys = Array.from({ length: sessionStorage.length }, (_, i) => sessionStorage.key(i));
      for (const key of keys) if (key?.startsWith(STORAGE_PREFIX) && latest(decodeURIComponent(key.slice(STORAGE_PREFIX.length)))) sessionStorage.removeItem(key);
    } catch { /* Optional browser storage. */ }
  }
  // Latest requests in flight can finish after a progress update. Cancel those
  // requests so their stale result cannot be stored again.
  for (const [key, job] of pending) if (latest(key)) { job.controller.abort(); pending.delete(key); }
}
if (typeof window !== 'undefined') window.addEventListener('media-progress-updated', invalidateLatestCatalog);

export function cachedCatalog(rawKey: string): CatalogPage | undefined {
  const key = scopedKey(rawKey);
  const entry = pages.get(key);
  if (entry) {
    pages.delete(key);
    pages.set(key, entry);
    return entry.data;
  }
  if (typeof sessionStorage !== 'undefined') {
    try {
      const raw = sessionStorage.getItem(storageKey(key));
      if (raw) {
        const parsed = JSON.parse(raw) as Entry;
        if (parsed?.data && typeof parsed.at === 'number') {
          pages.set(key, parsed);
          bytes += parsed.bytes || 0;
          return parsed.data;
        }
      }
    } catch {
      // Storage unavailable or parse error
    }
  }
  return undefined;
}
export function catalogCacheStats() { return { pages: pages.size, jsonBytes: bytes, pending: pending.size }; }

export function loadCatalog(rawKey: string, url: string): Promise<CatalogPage> {
  const key = scopedKey(rawKey);
  const entry = pages.get(key);
  if (entry && Date.now() - entry.at < FRESH_MS) return Promise.resolve(entry.data);
  if (!entry && typeof sessionStorage !== 'undefined') {
    try {
      const raw = sessionStorage.getItem(storageKey(key));
      if (raw) {
        const parsed = JSON.parse(raw) as Entry;
        if (parsed?.data && Date.now() - parsed.at < FRESH_MS) {
          pages.set(key, parsed);
          bytes += parsed.bytes || 0;
          return Promise.resolve(parsed.data);
        }
      }
    } catch {
      // Storage unavailable
    }
  }
  const existing = pending.get(key);
  if (existing) return existing.promise;
  if (pending.size >= MAX_PAGES) {
    const first = pending.keys().next().value!;
    pending.get(first)!.controller.abort(); pending.delete(first);
  }
  const controller = new AbortController();
  const generation = epoch;
  const promise = request<CatalogPage>(url, controller.signal).then(data => {
    if (generation !== epoch || controller.signal.aborted) throw new DOMException('Cache invalidated', 'AbortError');
    const size = new TextEncoder().encode(JSON.stringify(data)).byteLength;
    const old = pages.get(key);
    if (old) {
      bytes -= old.bytes;
      pages.delete(key);
      if (typeof sessionStorage !== 'undefined') {
        try { sessionStorage.removeItem(storageKey(key)); } catch { /* ignore */ }
      }
    }
    if (size <= MAX_BYTES) {
      const library = new URL(url, 'http://local').searchParams.get('library') ?? '';
      const newEntry = { data, at: Date.now(), bytes: size, library };
      pages.set(key, newEntry);
      bytes += size;
      if (typeof sessionStorage !== 'undefined') {
        try { sessionStorage.setItem(storageKey(key), JSON.stringify(newEntry)); } catch { /* ignore */ }
      }
      while (bytes > MAX_BYTES || pages.size > MAX_PAGES) {
        const first = pages.keys().next().value!;
        bytes -= pages.get(first)!.bytes;
        pages.delete(first);
        if (typeof sessionStorage !== 'undefined') {
          try { sessionStorage.removeItem(storageKey(first)); } catch { /* ignore */ }
        }
      }
    }
    return data;
  }).finally(() => { if (pending.get(key)?.controller === controller) pending.delete(key); });
  pending.set(key, { controller, promise });
  return promise;
}

/** Warm only the first bounded page on navigation intent, using the exact display key. */
export function preloadCatalog(section: 'home' | 'films' | 'series', library?: string): void {
  const kinds = section === 'home' ? ['film', 'series'] : [section === 'films' ? 'film' : 'series'];
  for (const kind of kinds) {
    const url = `/api/v1/catalog?${new URLSearchParams({ ...(library ? {library} : {}), ...(section === 'home' ? { scope: 'latest' } : {}), kind, q: '', sort: section === 'home' ? 'added-desc' : 'title-asc', offset: '0', limit: '24', count: 'false' })}`;
    void loadCatalog(url, url).catch(() => {});
  }
}

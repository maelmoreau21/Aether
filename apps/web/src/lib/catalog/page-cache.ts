import { writable } from 'svelte/store';
import { onSessionGenerationChange } from '../auth/session';
import { request, type CatalogPage } from './api';

// Session-only metadata. Never stores playback credentials or media bytes.
const FRESH_MS = 30_000;
const MAX_BYTES = 4 * 1024 * 1024;
const MAX_PAGES = 48;
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
    }
  }
}
onSessionGenerationChange(() => { scrolls.clear(); invalidateCatalog(undefined, false); });
if (typeof window !== 'undefined') window.addEventListener('media-access-denied', () => invalidateCatalog(undefined, false));

export function cachedCatalog(key: string): CatalogPage | undefined {
  const entry = pages.get(key);
  if (!entry) return;
  pages.delete(key); pages.set(key, entry);
  return entry.data;
}
export function catalogCacheStats() { return { pages: pages.size, jsonBytes: bytes, pending: pending.size }; }

export function loadCatalog(key: string, url: string): Promise<CatalogPage> {
  const entry = pages.get(key);
  if (entry && Date.now() - entry.at < FRESH_MS) return Promise.resolve(entry.data);
  const existing = pending.get(key);
  if (existing) return existing.promise;
  if (pending.size >= MAX_PAGES) {
    const first = pending.keys().next().value!;
    pending.get(first)!.controller.abort(); pending.delete(first);
  }
  const controller = new AbortController();
  const generation = epoch;
  const promise = request<CatalogPage>(url, controller.signal).then(data => {
    if (generation !== epoch) throw new DOMException('Cache invalidated', 'AbortError');
    const size = new TextEncoder().encode(JSON.stringify(data)).byteLength;
    const old = pages.get(key);
    if (old) { bytes -= old.bytes; pages.delete(key); }
    if (size <= MAX_BYTES) {
      pages.set(key, { data, at: Date.now(), bytes: size, library: new URL(url, 'http://local').searchParams.get('library') ?? '' });
      bytes += size;
      while (bytes > MAX_BYTES || pages.size > MAX_PAGES) {
        const first = pages.keys().next().value!;
        bytes -= pages.get(first)!.bytes; pages.delete(first);
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
    const url = `/api/v1/catalog?${new URLSearchParams({ ...(library ? {library} : {}), kind, q: '', sort: section === 'home' ? 'added-desc' : 'title-asc', offset: '0', limit: '24' })}`;
    void loadCatalog(url, url).catch(() => {});
  }
}

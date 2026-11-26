import { get, writable } from 'svelte/store';
import { onSessionGenerationChange, session } from '../auth/session';
import { CatalogRequestError, request, type CatalogPage } from './api';
import { languageScope, scopedKey, unscopedKey, withMetadataLocale } from './cache-scope';
import { shelfSeed } from './discover';
import { newHeroSeed } from './hero-picks';

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
type PendingJob = {
  controller: AbortController;
  promise: Promise<CatalogPage>;
  waiters: Set<symbol>;
};
const pages = new Map<string, Entry>();
const pending = new Map<string, PendingJob>();
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
let pendingHomeSeed: { seed: number; library: string } | null = null;

export function invalidateCatalog(library?: string, notify = true): void {
  epoch++;
  if (!library) pendingHomeSeed = null;
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

function readEntry(key: string): Entry | undefined {
  const entry = pages.get(key);
  if (entry) {
    pages.delete(key);
    pages.set(key, entry);
    return entry;
  }
  if (typeof sessionStorage !== 'undefined') {
    try {
      const raw = sessionStorage.getItem(storageKey(key));
      if (raw) {
        const parsed = JSON.parse(raw) as Entry;
        if (parsed?.data && typeof parsed.at === 'number') {
          pages.set(key, parsed);
          bytes += parsed.bytes || 0;
          return parsed;
        }
      }
    } catch {
      // Storage unavailable or parse error
    }
  }
  return undefined;
}

function storeEntry(key: string, url: string, data: CatalogPage): void {
  const parsedUrl = new URL(url, 'http://local');
  const isSearchQuery = Boolean(parsedUrl.searchParams.get('q')?.trim());
  const library = parsedUrl.searchParams.get('library') ?? '';
  const newEntry: Entry = { data, at: Date.now(), bytes: 0, library };
  const serialized = JSON.stringify(newEntry);
  const size = new TextEncoder().encode(serialized).byteLength;
  newEntry.bytes = size;
  const old = pages.get(key);
  if (old) {
    bytes -= old.bytes;
    pages.delete(key);
    if (typeof sessionStorage !== 'undefined') {
      try { sessionStorage.removeItem(storageKey(key)); } catch { /* ignore */ }
    }
  }
  if (size <= MAX_BYTES) {
    pages.set(key, newEntry);
    bytes += size;
    if (!isSearchQuery && typeof sessionStorage !== 'undefined') {
      try { sessionStorage.setItem(storageKey(key), serialized); } catch { /* ignore */ }
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
}

export function cachedCatalog(rawKey: string): CatalogPage | undefined {
  return readEntry(scopedKey(rawKey))?.data;
}

export function isCatalogFresh(rawKey: string): boolean {
  const entry = readEntry(scopedKey(rawKey));
  return Boolean(entry && Date.now() - entry.at < FRESH_MS);
}
export function catalogCacheStats() { return { pages: pages.size, jsonBytes: bytes, pending: pending.size }; }

const RETRY_DELAY_MS = 1500;

/** Failures a second request can fix: a timeout, a server error, a network drop. */
export function isTransientCatalogError(cause: unknown): boolean {
  if (cause instanceof CatalogRequestError) return cause.kind === 'timeout' || (cause.kind === 'http' && (cause.status ?? 0) >= 500);
  return cause instanceof TypeError;
}

function abortableDelay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) { reject(signal.reason); return; }
    const timer = setTimeout(() => { signal.removeEventListener('abort', stop); resolve(); }, ms);
    const stop = () => { clearTimeout(timer); reject(signal.reason); };
    signal.addEventListener('abort', stop, { once: true });
  });
}

function attachCallerSignal(key: string, job: PendingJob, signal?: AbortSignal): Promise<CatalogPage> {
  const token = Symbol('waiter');
  job.waiters.add(token);
  if (!signal) {
    return job.promise.finally(() => { job.waiters.delete(token); });
  }
  if (signal.aborted) {
    job.waiters.delete(token);
    if (job.waiters.size === 0 && pending.get(key) === job) {
      pending.delete(key);
      job.controller.abort(signal.reason);
    }
    return Promise.reject(signal.reason ?? new DOMException('Aborted', 'AbortError'));
  }
  return new Promise<CatalogPage>((resolve, reject) => {
    const onAbort = () => {
      job.waiters.delete(token);
      if (job.waiters.size === 0 && pending.get(key) === job) {
        pending.delete(key);
        job.controller.abort(signal.reason);
      }
      reject(signal.reason ?? new DOMException('Aborted', 'AbortError'));
    };
    signal.addEventListener('abort', onAbort, { once: true });
    job.promise.then(
      value => {
        signal.removeEventListener('abort', onAbort);
        job.waiters.delete(token);
        resolve(value);
      },
      err => {
        signal.removeEventListener('abort', onAbort);
        job.waiters.delete(token);
        reject(err);
      }
    );
  });
}

export function loadCatalog(rawKey: string, url: string, signal?: AbortSignal): Promise<CatalogPage> {
  if (signal?.aborted) {
    return Promise.reject(signal.reason ?? new DOMException('Aborted', 'AbortError'));
  }
  const key = scopedKey(rawKey);
  const entry = readEntry(key);
  if (entry && Date.now() - entry.at < FRESH_MS) return Promise.resolve(entry.data);
  const existing = pending.get(key);
  if (existing) return attachCallerSignal(key, existing, signal);
  if (pending.size >= MAX_PAGES) {
    const first = pending.keys().next().value!;
    pending.get(first)!.controller.abort(); pending.delete(first);
  }
  const isSearchQuery = Boolean(new URL(url, 'http://local').searchParams.get('q')?.trim());
  const controller = new AbortController();
  const generation = epoch;
  const fetchPage = () => request<CatalogPage>(url, controller.signal);
  const promise = fetchPage().catch(async cause => {
    // A busy server (slow answer, 5xx, dropped connection) gets one more try before a shelf is
    // reported as unavailable; interactive searches fail fast and access answers are final.
    if (isSearchQuery || controller.signal.aborted || generation !== epoch || !isTransientCatalogError(cause)) throw cause;
    await abortableDelay(RETRY_DELAY_MS, controller.signal);
    return fetchPage();
  }).then(data => {
    if (generation !== epoch || controller.signal.aborted) throw new DOMException('Cache invalidated', 'AbortError');
    storeEntry(key, url, data);
    return data;
  }).finally(() => { if (pending.get(key)?.controller === controller) pending.delete(key); });
  const job: PendingJob = { controller, promise, waiters: new Set() };
  pending.set(key, job);
  return attachCallerSignal(key, job, signal);
}

/** Warm only the first bounded page on navigation intent, using the exact display key. */
export function preloadCatalog(section: 'home' | 'films' | 'series', library?: string): void {
  const kinds = section === 'home' ? ['film', 'series'] : [section === 'films' ? 'film' : 'series'];
  for (const kind of kinds) {
    const url = `/api/v1/catalog?${new URLSearchParams({ ...(library ? {library} : {}), ...(section === 'home' ? { scope: 'latest' } : {}), kind, q: '', sort: section === 'home' ? 'added-desc' : 'title-asc', offset: '0', limit: '24', count: 'false' })}`;
    void loadCatalog(url, url).catch(() => {});
  }
}

export interface CatalogRouteScope {
  section: 'home' | 'films' | 'series';
  library?: string;
  query?: string;
  sort?: string;
  offset?: number;
  filters?: Record<string, string>;
}

const FILTER_PARAMS = ['genre', 'year', 'quality', 'codec', 'language'] as const;

export function catalogRouteScope(
  routeId: string | null | undefined,
  pathname: string,
  searchParams: Pick<URLSearchParams, 'get' | 'has'>
): CatalogRouteScope | null {
  let section: 'home' | 'films' | 'series' | null = null;
  if (routeId === '/') section = 'home';
  else if (routeId === '/films') section = 'films';
  else if (routeId === '/series') section = 'series';
  else {
    const normalized = pathname.replace(/\/+$/, '');
    if (normalized === '' || normalized === '/app') section = 'home';
    else if (normalized.endsWith('/films')) section = 'films';
    else if (normalized.endsWith('/series')) section = 'series';
  }
  if (!section) return null;
  const library = searchParams.get('library') ?? undefined;
  if (section === 'home') {
    return library ? { section, library } : { section };
  }
  if (searchParams.has('work')) return null;
  const filters: Record<string, string> = {};
  for (const key of FILTER_PARAMS) {
    const value = searchParams.get(key);
    if (value) filters[key] = value;
  }
  const offsetRaw = Number(searchParams.get('offset') ?? 0);
  const offset = Number.isFinite(offsetRaw) && offsetRaw > 0 ? Math.floor(offsetRaw) : 0;
  return {
    section,
    ...(library ? { library } : {}),
    query: searchParams.get('q') ?? '',
    sort: searchParams.get('sort') ?? 'title-asc',
    offset,
    filters
  };
}

export function homeSampleUrls(seed: number, library?: string): { tonight: string; rated: string } {
  const base = (key: string, params: Record<string, string>) =>
    `/api/v1/catalog?${new URLSearchParams({
      ...(library ? { library } : {}),
      q: '',
      count: 'false',
      sort: 'random',
      offset: '0',
      seed: String(shelfSeed(String(seed), key)),
      ...params
    })}`;
  return {
    tonight: base('tonight', { unwatched: 'true', limit: '24' }),
    rated: base('rated', { minRating: '6', limit: '24' })
  };
}

/** Reuses the pre-warmed visit seed once for the matching library scope, or draws a fresh seed. */
export function takeHomeVisitSeed(library?: string): number {
  if (pendingHomeSeed && pendingHomeSeed.library === (library ?? '')) {
    const seed = pendingHomeSeed.seed;
    pendingHomeSeed = null;
    return seed;
  }
  return newHeroSeed();
}

type StartupFlightResult = { ok: true; data: CatalogPage } | { ok: false };
type StartupFlight = {
  rawKey: string;
  effectiveUrl: string;
  controller: AbortController;
  settled?: StartupFlightResult;
  promise: Promise<StartupFlightResult>;
};

export type StartupPreloadTicket = {
  mode: 'direct' | 'speculative';
  scope: CatalogRouteScope;
  homeSeed?: number;
  flights: StartupFlight[];
  discarded: boolean;
  committed: boolean;
};

function catalogUrlsForScope(scope: CatalogRouteScope, homeSeed?: number): string[] {
  if (scope.section === 'home') {
    const latest = (['film', 'series'] as const).map(
      kind =>
        `/api/v1/catalog?${new URLSearchParams({
          ...(scope.library ? { library: scope.library } : {}),
          scope: 'latest',
          kind,
          q: '',
          sort: 'added-desc',
          offset: '0',
          limit: '24',
          count: 'false'
        })}`
    );
    if (homeSeed === undefined) return latest;
    const samples = homeSampleUrls(homeSeed, scope.library);
    return [...latest, samples.tonight, samples.rated];
  }
  const kind = scope.section === 'films' ? 'film' : 'series';
  const url = `/api/v1/catalog?${new URLSearchParams({
    ...(scope.filters ?? {}),
    ...(scope.library ? { library: scope.library } : {}),
    kind,
    q: scope.query ?? '',
    sort: scope.sort ?? 'title-asc',
    offset: String(scope.offset ?? 0),
    limit: '24',
    count: 'false'
  })}`;
  return [url];
}

/**
 * Starts initial catalog fetches immediately in parallel with `readSession()`.
 * When the session is still unverified (`null`), responses stay detached from the reader cache
 * until `commitStartupPreload` runs after authentication succeeds.
 */
export function preloadStartupCatalog(scope: CatalogRouteScope): StartupPreloadTicket {
  const homeSeed = scope.section === 'home' ? newHeroSeed() : undefined;
  if (get(session)) {
    if (scope.section === 'home' && homeSeed !== undefined) {
      pendingHomeSeed = { seed: homeSeed, library: scope.library ?? '' };
    }
    for (const url of catalogUrlsForScope(scope, homeSeed)) {
      void loadCatalog(url, url).catch(() => {});
    }
    return { mode: 'direct', scope, homeSeed, flights: [], discarded: false, committed: true };
  }

  const urls = catalogUrlsForScope(scope, homeSeed);
  const flights: StartupFlight[] = urls.map(rawKey => {
    const controller = new AbortController();
    const effectiveUrl = withMetadataLocale(rawKey);
    const flight: StartupFlight = {
      rawKey,
      effectiveUrl,
      controller,
      promise: Promise.resolve({ ok: false })
    };
    const signal = AbortSignal.any([controller.signal, AbortSignal.timeout(10000)]);
    flight.promise = fetch(effectiveUrl, { credentials: 'same-origin', signal })
      .then(async (response): Promise<StartupFlightResult> => {
        if (!response.ok || controller.signal.aborted) return { ok: false };
        const data = (await response.json()) as CatalogPage;
        if (controller.signal.aborted) return { ok: false };
        return { ok: true, data };
      })
      .catch((): StartupFlightResult => ({ ok: false }))
      .then(result => {
        flight.settled = result;
        return result;
      });
    return flight;
  });

  return { mode: 'speculative', scope, homeSeed, flights, discarded: false, committed: false };
}

export function discardStartupPreload(ticket: StartupPreloadTicket | null | undefined): void {
  if (!ticket || ticket.discarded || ticket.committed) return;
  ticket.discarded = true;
  for (const flight of ticket.flights) flight.controller.abort();
}

export function commitStartupPreload(ticket: StartupPreloadTicket | null | undefined): void {
  if (!ticket || ticket.discarded || ticket.committed || ticket.mode !== 'speculative') return;
  if (!get(session)) {
    discardStartupPreload(ticket);
    return;
  }
  ticket.committed = true;
  if (ticket.scope.section === 'home' && ticket.homeSeed !== undefined) {
    pendingHomeSeed = { seed: ticket.homeSeed, library: ticket.scope.library ?? '' };
  }
  const commitEpoch = epoch;
  for (const flight of ticket.flights) {
    const key = scopedKey(flight.rawKey);
    if (isCatalogFresh(flight.rawKey) || pending.has(key)) continue;
    if (flight.settled) {
      if (flight.settled.ok && withMetadataLocale(flight.rawKey) === flight.effectiveUrl) {
        storeEntry(key, flight.rawKey, flight.settled.data);
      } else {
        void loadCatalog(flight.rawKey, flight.rawKey).catch(() => {});
      }
      continue;
    }
    const adoptionController = new AbortController();
    adoptionController.signal.addEventListener('abort', () => flight.controller.abort(), { once: true });
    const promise = flight.promise
      .then(async result => {
        if (commitEpoch !== epoch || adoptionController.signal.aborted) {
          throw new DOMException('Cache invalidated', 'AbortError');
        }
        if (result.ok && withMetadataLocale(flight.rawKey) === flight.effectiveUrl) {
          storeEntry(key, flight.rawKey, result.data);
          return result.data;
        }
        const fresh = await request<CatalogPage>(flight.rawKey, adoptionController.signal);
        if (commitEpoch !== epoch || adoptionController.signal.aborted) {
          throw new DOMException('Cache invalidated', 'AbortError');
        }
        storeEntry(key, flight.rawKey, fresh);
        return fresh;
      })
      .finally(() => {
        if (pending.get(key)?.controller === adoptionController) pending.delete(key);
      });
    pending.set(key, { controller: adoptionController, promise, waiters: new Set() });
  }
}

import type { Media } from '../player/types';
import { getSessionGeneration, invalidateSession, onSessionGenerationChange } from '../auth/session';
import { goto } from '$app/navigation';
import { tr } from '../i18n';
import { cacheScope, withMetadataLocale } from './cache-scope';
import { resolve } from '$app/paths';
export interface CatalogSource {
  id: string;
  assetIds: string[];
  playbackMediaId: string;
  nodeId?: string;
  nodeStatus?: 'connected' | 'disconnected' | 'revoked';
  connectionMode?: string;
  score?: number;
}
export interface Edition { id: string; label: string; versions: { id: string; label: string; sources: CatalogSource[] }[] }
export interface Work {
  tags?: string[]; minimumAge?: number | null; globalMediaId?: string | null;
  id: string; kind: 'film' | 'series' | 'episode'; title: string; year: number | null;
  seasonNumber?: number;
  episodesPage?: { season: number | null; offset: number; limit: number; nextOffset: number; hasMore: boolean };
  description: string; libraryId: string; synthetic: boolean; number?: number;
  posterUrl?: string | null;
  artwork?: Record<string, string | null>;
  imageVariants?: { id: string; kind: string; url: string; origin: string; position: number; language?: string | null; provider?: string | null; pinned?: boolean; contentHash?: string | null }[];
  addedAt?: number | null;
  nodeId?: string;
  nodeStatus?: 'connected' | 'disconnected' | 'revoked';
  available?: boolean;
  originalLanguage?: string | null; originalTitle?: string | null; plot?: string | null; outline?: string | null;
  premiered?: string | null; genres?: string[]; rating?: number | null; mpaa?: string | null;
  actors?: { name: string; role?: string; imageUrl?: string | null; thumbUrl?: string | null; thumb_url?: string | null }[]; directors?: string[]; writers?: string[];
  crewCredits?: { name: string; role?: string; imageUrl?: string | null; providerId?: string | null }[];
  ratings?: { provider:string;value:number;max:number }[];
  externalIds?: Record<string, string>; metadataProvenance?: string | null;
  seasonCount?: number; episodeCount?: number; editions?: Edition[];
  seasons?: { id: string; number: number; title: string; episodeCount?: number; episodes: Work[] }[];
  translations?: Record<string, { title?: string; overview?: string; plot?: string; tagline?: string }>;
}

export function localizedTitle(work: Work | null | undefined, locale?: string): string {
  if (!work) return '';
  const lang = (locale ?? 'fr').toLowerCase();
  const shortLang = lang.split('-')[0];
  const entry = work.translations?.[lang] ?? work.translations?.[shortLang];
  return entry?.title || work.title || '';
}

export function localizedPlot(work: Work | null | undefined, locale?: string): string {
  if (!work) return '';
  const lang = (locale ?? 'fr').toLowerCase();
  const shortLang = lang.split('-')[0];
  const entry = work.translations?.[lang] ?? work.translations?.[shortLang];
  return entry?.overview || entry?.plot || work.description || work.plot || '';
}

export function localizedTagline(work: Work | null | undefined, locale?: string): string {
  if (!work) return '';
  const lang = (locale ?? 'fr').toLowerCase();
  const shortLang = lang.split('-')[0];
  const entry = work.translations?.[lang] ?? work.translations?.[shortLang];
  return entry?.tagline || work.outline || '';
}
export interface CatalogPage { items: Work[]; total: number | null; hasMore?: boolean; offset: number; limit: number; notice: string }
export interface FetchDescriptorOptions { revalidate?: boolean }

const MAX_DESCRIPTOR_CACHE_SIZE = 128;
const descriptorCache = new Map<string, Media>();
/** Descriptors are cached per account and language chain: another reader never sees them. */
const descriptorKey = (id: string): string => `${cacheScope()}\n${id}`;
let cacheSessionGeneration = getSessionGeneration();
let cacheGeneration = 0;
let demoFetchPromise: Promise<Media[]> | null = null;
let demoFetchSessionGeneration = -1;
let demoFetchCacheGeneration = -1;

function syncCacheWithSession(): void {
  const currentSessionGen = getSessionGeneration();
  if (cacheSessionGeneration !== currentSessionGen) {
    descriptorCache.clear();
    demoFetchPromise = null;
    cacheSessionGeneration = currentSessionGen;
    cacheGeneration += 1;
  }
}

if (typeof window !== 'undefined') {
  window.addEventListener('media-locale-saved', () => {
    descriptorCache.clear();
    demoFetchPromise = null;
    cacheGeneration += 1;
  });
}

onSessionGenerationChange(newSessionGen => {
  descriptorCache.clear();
  demoFetchPromise = null;
  cacheSessionGeneration = newSessionGen;
  cacheGeneration += 1;
});

function setCachedDescriptor(id: string, media: Media): void {
  syncCacheWithSession();
  const key = descriptorKey(id);
  if (descriptorCache.has(key)) {
    descriptorCache.delete(key);
  } else if (descriptorCache.size >= MAX_DESCRIPTOR_CACHE_SIZE) {
    const oldestKey = descriptorCache.keys().next().value;
    if (oldestKey !== undefined) {
      descriptorCache.delete(oldestKey);
    }
  }
  descriptorCache.set(key, media);
}

async function fetchWithAbort(url: string, signal: AbortSignal): Promise<Response> {
  if (signal.aborted) {
    throw signal.reason ?? new DOMException('Aborted', 'AbortError');
  }
  let onAbort: (() => void) | undefined;
  const abortPromise = new Promise<never>((_, reject) => {
    onAbort = () => reject(signal.reason ?? new DOMException('Aborted', 'AbortError'));
    signal.addEventListener('abort', onAbort, { once: true });
  });
  try {
    return await Promise.race([fetch(url, { signal }), abortPromise]);
  } finally {
    if (onAbort) {
      signal.removeEventListener('abort', onAbort);
    }
  }
}

export async function request<T>(url: string, signal: AbortSignal, timeoutMs=10000): Promise<T> {
  syncCacheWithSession();
  const requestSessionGen = getSessionGeneration();
  const combinedSignal = AbortSignal.any([signal, AbortSignal.timeout(timeoutMs)]);
  let response:Response;
  try { response=await fetchWithAbort(withMetadataLocale(url), combinedSignal); }
  catch (cause) {
    if(!signal.aborted && combinedSignal.aborted)throw new Error(tr('catalog_the_server_is_taking_too_long'));
    throw cause;
  }
  if (response.status === 401) {
    if (invalidateSession(requestSessionGen)) {
      clearDescriptorCache();
      try {
        await goto(resolve('login/'), { replaceState: true });
      } catch {
        // Ignore router navigation error outside active SvelteKit client context
      }
    }
    throw new Error(tr('catalog_your_session_has_expired_sign_in'));
  }
  if (response.status === 403) {
    if (requestSessionGen === getSessionGeneration()) {
      clearDescriptorCache();
      if (typeof window !== 'undefined') window.dispatchEvent(new Event('media-access-denied'));
    }
    throw new Error(tr('catalog_your_account_does_not_have_access'));
  }
  if (!response.ok) throw new Error(response.status === 404 ? tr('this_media_item_was_not_found') : tr('the_catalogue_is_unavailable_http', { status: response.status }));
  if (requestSessionGen !== getSessionGeneration()) {
    throw new Error(tr('the_session_changed_during_the_request'));
  }
  let data:T;
  try{data=await response.json() as T;}
  catch(cause){if(!signal.aborted&&combinedSignal.aborted)throw new Error(tr('catalog_the_server_is_taking_too_long'));throw cause;}
  if (combinedSignal.aborted) {
    if (!signal.aborted) throw new Error(tr('catalog_the_server_is_taking_too_long'));
    throw combinedSignal.reason ?? new DOMException('Aborted', 'AbortError');
  }
  if (requestSessionGen !== getSessionGeneration()) {
    throw new Error(tr('the_session_changed_during_the_request'));
  }
  return data;
}
export function playbackIds(work: Work): string[] {
  return [...new Set((work.editions ?? []).flatMap(edition => edition.versions.flatMap(version => version.sources.map(source => source.playbackMediaId))))];
}
export function sourceLabel(media: Media): string {
  const mime = media.sources[0]?.mime;
  return mime === 'video/mp4' ? 'MP4' : mime === 'video/x-matroska' ? 'MKV' : media.title;
}

export function clearDescriptorCache(): void {
  descriptorCache.clear();
  demoFetchPromise = null;
  cacheSessionGeneration = getSessionGeneration();
  cacheGeneration += 1;
}

export function getCachedMedia(id: string): Media | undefined {
  syncCacheWithSession();
  return descriptorCache.get(descriptorKey(id));
}

export function cacheMedia(media: Media): void {
  setCachedDescriptor(media.id, media);
}

async function fetchDescriptor(
  id: string,
  signal?: AbortSignal,
  options?: FetchDescriptorOptions
): Promise<Media> {
  if (signal?.aborted) {
    throw signal.reason ?? new DOMException('Aborted', 'AbortError');
  }
  syncCacheWithSession();
  const reqSessionGen = getSessionGeneration();
  const reqCacheGen = cacheGeneration;
  const revalidate = options?.revalidate ?? false;
  if (!revalidate) {
    const cached = descriptorCache.get(descriptorKey(id));
    if (cached) return cached;
  }

  const sig = signal ?? new AbortController().signal;

  if (id.startsWith('demo-')) {
    try {
      if (
        !demoFetchPromise ||
        revalidate ||
        demoFetchSessionGeneration !== reqSessionGen ||
        demoFetchCacheGeneration !== reqCacheGen
      ) {
        demoFetchSessionGeneration = reqSessionGen;
        demoFetchCacheGeneration = reqCacheGen;
        const currentPromise = request<{ media: Media[] }>('/api/v1/demo/media', sig)
          .then(r => {
            if (
              !sig.aborted &&
              reqSessionGen === getSessionGeneration() &&
              reqCacheGen === cacheGeneration
            ) {
              for (const item of r.media) {
                setCachedDescriptor(item.id, item);
              }
            }
            return r.media;
          })
          .finally(() => {
            if (demoFetchPromise === currentPromise) {
              demoFetchPromise = null;
            }
          });
        demoFetchPromise = currentPromise;
      }
      const list = await demoFetchPromise;
      if (sig.aborted) {
        throw sig.reason ?? new DOMException('Aborted', 'AbortError');
      }
      if (reqSessionGen !== getSessionGeneration()) {
        throw new Error(tr('the_session_changed_during_the_request'));
      }
      const found =
        (reqCacheGen === cacheGeneration ? descriptorCache.get(descriptorKey(id)) : undefined) ??
        list.find(item => item.id === id);
      if (found) return found;
      throw new Error(tr('demo_media_not_found'));
    } catch (err) {
      if (
        !sig.aborted &&
        reqSessionGen === getSessionGeneration() &&
        reqCacheGen === cacheGeneration
      ) {
        descriptorCache.delete(descriptorKey(id));
      }
      throw err;
    }
  }

  try {
    const item = await request<Media>(`/api/v1/media/${encodeURIComponent(id)}`, sig, 6000);
    if (sig.aborted) {
      throw sig.reason ?? new DOMException('Aborted', 'AbortError');
    }
    if (reqSessionGen !== getSessionGeneration()) {
      throw new Error(tr('the_session_changed_during_the_request'));
    }
    if (reqCacheGen === cacheGeneration) {
      setCachedDescriptor(item.id, item);
      if (item.requestedId && item.requestedId !== item.id) {
        setCachedDescriptor(item.requestedId, item);
      }
    }
    return item;
  } catch (err) {
    if (
      !sig.aborted &&
      reqSessionGen === getSessionGeneration() &&
      reqCacheGen === cacheGeneration
    ) {
      descriptorCache.delete(descriptorKey(id));
    }
    throw err;
  }
}

// Share only work in flight. A playback click always requests fresh authorization.
const descriptorRequests = new Map<string, { controller: AbortController; promise: Promise<Media>; consumers: number }>();
onSessionGenerationChange(() => {
  // Existing callers receive the original HTTP/session error; late data is rejected above.
  descriptorRequests.clear();
});
export function fetchMediaDescriptor(id: string, signal?: AbortSignal, options?: FetchDescriptorOptions): Promise<Media> {
  if (signal?.aborted) return Promise.reject(signal.reason ?? new DOMException('Aborted', 'AbortError'));
  const cached = getCachedMedia(id);
  if (cached && !options?.revalidate) return Promise.resolve(cached);
  const key = `${getSessionGeneration()}:${cacheGeneration}:${id}`;
  let job = descriptorRequests.get(key);
  if (!job || job.controller.signal.aborted) {
    const controller = new AbortController();
    const promise = fetchDescriptor(id, controller.signal, options).finally(() => {
      if (descriptorRequests.get(key)?.controller === controller) descriptorRequests.delete(key);
    });
    job = { controller, promise, consumers: 0 };
    descriptorRequests.set(key, job);
  }
  const shared = job;
  shared.consumers++;
  return new Promise<Media>((resolve, reject) => {
    let done = false;
    const finish = (error?: unknown, media?: Media) => {
      if (done) return;
      done = true;
      signal?.removeEventListener('abort', abort);
      if (--shared.consumers === 0) shared.controller.abort();
      if (error !== undefined) reject(error); else resolve(media!);
    };
    const abort = () => finish(signal?.reason ?? new DOMException('Aborted', 'AbortError'));
    signal?.addEventListener('abort', abort, { once: true });
    shared.promise.then(media => finish(undefined, media), error => finish(error));
  });
}

export async function revalidateMediaDescriptor(id: string, signal?: AbortSignal): Promise<Media> {
  return fetchMediaDescriptor(id, signal, { revalidate: true });
}

export async function fetchMediaDescriptors(
  ids: string[],
  signal?: AbortSignal,
  options?: FetchDescriptorOptions
): Promise<Media[]> {
  return Promise.all(ids.map(id => fetchMediaDescriptor(id, signal, options)));
}

export const preloadMediaDescriptor = fetchMediaDescriptor;
export const preloadMediaDescriptors = fetchMediaDescriptors;

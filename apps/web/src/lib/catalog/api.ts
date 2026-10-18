import type { Media } from '../player/types';
import { getSessionGeneration, invalidateSession, onSessionGenerationChange } from '../auth/session';
import { goto } from '$app/navigation';
import { resolve } from '$app/paths';
export interface CatalogSource { id: string; assetIds: string[]; playbackMediaId: string }
export interface Edition { id: string; label: string; versions: { id: string; label: string; sources: CatalogSource[] }[] }
export interface Work {
  id: string; kind: 'film' | 'series' | 'episode'; title: string; year: number | null;
  description: string; libraryId: string; synthetic: boolean; number?: number;
  posterUrl?: string | null;
  nodeId?: string;
  nodeStatus?: 'connected' | 'disconnected' | 'revoked';
  available?: boolean;
  originalTitle?: string | null; plot?: string | null; outline?: string | null;
  premiered?: string | null; genres?: string[]; rating?: number | null; mpaa?: string | null;
  externalIds?: Record<string, string>; metadataProvenance?: string | null;
  seasonCount?: number; episodeCount?: number; editions?: Edition[];
  seasons?: { id: string; number: number; title: string; episodes: Work[] }[];
}
export interface CatalogPage { items: Work[]; total: number; offset: number; limit: number; notice: string }
export interface FetchDescriptorOptions { revalidate?: boolean }

const MAX_DESCRIPTOR_CACHE_SIZE = 128;
const descriptorCache = new Map<string, Media>();
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

onSessionGenerationChange(newSessionGen => {
  descriptorCache.clear();
  demoFetchPromise = null;
  cacheSessionGeneration = newSessionGen;
  cacheGeneration += 1;
});

function setCachedDescriptor(id: string, media: Media): void {
  syncCacheWithSession();
  if (descriptorCache.has(id)) {
    descriptorCache.delete(id);
  } else if (descriptorCache.size >= MAX_DESCRIPTOR_CACHE_SIZE) {
    const oldestKey = descriptorCache.keys().next().value;
    if (oldestKey !== undefined) {
      descriptorCache.delete(oldestKey);
    }
  }
  descriptorCache.set(id, media);
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

export async function request<T>(url: string, signal: AbortSignal): Promise<T> {
  syncCacheWithSession();
  const requestSessionGen = getSessionGeneration();
  const combinedSignal = AbortSignal.any([signal, AbortSignal.timeout(10000)]);
  const response = await fetchWithAbort(url, combinedSignal);
  if (response.status === 401) {
    if (invalidateSession(requestSessionGen)) {
      clearDescriptorCache();
      try {
        await goto(resolve('login/'), { replaceState: true });
      } catch {
        // Ignore router navigation error outside active SvelteKit client context
      }
    }
    throw new Error('Votre session a expiré. Connectez-vous à nouveau.');
  }
  if (response.status === 403) {
    if (requestSessionGen === getSessionGeneration()) {
      clearDescriptorCache();
    }
    throw new Error('Votre compte n’a pas accès à cette ressource.');
  }
  if (!response.ok) throw new Error(response.status === 404 ? 'Ce média est introuvable.' : `Le catalogue est indisponible (HTTP ${response.status}).`);
  if (requestSessionGen !== getSessionGeneration()) {
    throw new Error('La session a changé pendant la requête.');
  }
  const data = (await response.json()) as T;
  if (combinedSignal.aborted) {
    throw combinedSignal.reason ?? new DOMException('Aborted', 'AbortError');
  }
  if (requestSessionGen !== getSessionGeneration()) {
    throw new Error('La session a changé pendant la requête.');
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
  return descriptorCache.get(id);
}

export function cacheMedia(media: Media): void {
  setCachedDescriptor(media.id, media);
}

export async function fetchMediaDescriptor(
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
    const cached = descriptorCache.get(id);
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
        throw new Error('La session a changé pendant la requête.');
      }
      const found =
        (reqCacheGen === cacheGeneration ? descriptorCache.get(id) : undefined) ??
        list.find(item => item.id === id);
      if (found) return found;
      throw new Error('Média de démonstration introuvable.');
    } catch (err) {
      if (
        !sig.aborted &&
        reqSessionGen === getSessionGeneration() &&
        reqCacheGen === cacheGeneration
      ) {
        descriptorCache.delete(id);
      }
      throw err;
    }
  }

  try {
    const item = await request<Media>(`/api/v1/media/${encodeURIComponent(id)}`, sig);
    if (sig.aborted) {
      throw sig.reason ?? new DOMException('Aborted', 'AbortError');
    }
    if (reqSessionGen !== getSessionGeneration()) {
      throw new Error('La session a changé pendant la requête.');
    }
    if (reqCacheGen === cacheGeneration) {
      setCachedDescriptor(item.id, item);
    }
    return item;
  } catch (err) {
    if (
      !sig.aborted &&
      reqSessionGen === getSessionGeneration() &&
      reqCacheGen === cacheGeneration
    ) {
      descriptorCache.delete(id);
    }
    throw err;
  }
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

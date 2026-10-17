import type { Media } from '../player/types';
import { session } from '../auth/session';
import { goto } from '$app/navigation';
import { resolve } from '$app/paths';
export interface CatalogSource { id: string; assetIds: string[]; playbackMediaId: string }
export interface Edition { id: string; label: string; versions: { id: string; label: string; sources: CatalogSource[] }[] }
export interface Work {
  id: string; kind: 'film' | 'series' | 'episode'; title: string; year: number | null;
  description: string; libraryId: string; synthetic: boolean; number?: number;
  posterUrl?: string | null;
  originalTitle?: string | null; plot?: string | null; outline?: string | null;
  premiered?: string | null; genres?: string[]; rating?: number | null; mpaa?: string | null;
  externalIds?: Record<string, string>; metadataProvenance?: string | null;
  seasonCount?: number; episodeCount?: number; editions?: Edition[];
  seasons?: { id: string; number: number; title: string; episodes: Work[] }[];
}
export interface CatalogPage { items: Work[]; total: number; offset: number; limit: number; notice: string }
export async function request<T>(url: string, signal: AbortSignal): Promise<T> {
  const response = await fetch(url, { signal: AbortSignal.any([signal, AbortSignal.timeout(10000)]) });
  if (response.status === 401) {
    session.set(null);
    await goto(resolve('login/'), { replaceState:true });
    throw new Error('Votre session a expiré. Connectez-vous à nouveau.');
  }
  if (response.status === 403) throw new Error('Votre compte n’a pas accès à cette ressource.');
  if (!response.ok) throw new Error(response.status === 404 ? 'Ce média est introuvable.' : `Le catalogue est indisponible (HTTP ${response.status}).`);
  return response.json() as Promise<T>;
}
export function playbackIds(work: Work): string[] {
  return [...new Set((work.editions ?? []).flatMap(edition => edition.versions.flatMap(version => version.sources.map(source => source.playbackMediaId))))];
}
export function sourceLabel(media: Media): string {
  const mime = media.sources[0]?.mime;
  return mime === 'video/mp4' ? 'MP4' : mime === 'video/x-matroska' ? 'MKV' : media.title;
}

const descriptorCache = new Map<string, Media>();
let demoFetchPromise: Promise<Media[]> | null = null;

export function clearDescriptorCache(): void {
  descriptorCache.clear();
  demoFetchPromise = null;
}

export function getCachedMedia(id: string): Media | undefined {
  return descriptorCache.get(id);
}

export function cacheMedia(media: Media): void {
  descriptorCache.set(media.id, media);
}

export async function fetchMediaDescriptor(id: string, signal?: AbortSignal): Promise<Media> {
  const cached = descriptorCache.get(id);
  if (cached) return cached;

  if (id.startsWith('demo-')) {
    if (!demoFetchPromise) {
      const dummySignal = new AbortController().signal;
      demoFetchPromise = request<{ media: Media[] }>('/api/v1/demo/media', signal ?? dummySignal)
        .then(r => {
          for (const item of r.media) {
            descriptorCache.set(item.id, item);
          }
          return r.media;
        })
        .finally(() => {
          demoFetchPromise = null;
        });
    }
    await demoFetchPromise;
    const found = descriptorCache.get(id);
    if (found) return found;
    throw new Error('Média de démonstration introuvable.');
  }

  const sig = signal ?? new AbortController().signal;
  const item = await request<Media>(`/api/v1/media/${encodeURIComponent(id)}`, sig);
  descriptorCache.set(item.id, item);
  return item;
}

export async function fetchMediaDescriptors(ids: string[], signal?: AbortSignal): Promise<Media[]> {
  return Promise.all(ids.map(id => fetchMediaDescriptor(id, signal)));
}

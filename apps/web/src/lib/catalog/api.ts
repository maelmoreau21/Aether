import type { Media } from '../player/types';
export interface CatalogSource { id: string; assetIds: string[]; playbackMediaId: string }
export interface Edition { id: string; label: string; versions: { id: string; label: string; sources: CatalogSource[] }[] }
export interface Work {
  id: string; kind: 'film' | 'series' | 'episode'; title: string; year: number | null;
  description: string; libraryId: string; synthetic: boolean; number?: number;
  seasonCount?: number; episodeCount?: number; editions?: Edition[];
  seasons?: { id: string; number: number; title: string; episodes: Work[] }[];
}
export interface CatalogPage { items: Work[]; total: number; offset: number; limit: number; notice: string }
export async function request<T>(url: string, signal: AbortSignal): Promise<T> {
  const response = await fetch(url, { signal: AbortSignal.any([signal, AbortSignal.timeout(10000)]) });
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

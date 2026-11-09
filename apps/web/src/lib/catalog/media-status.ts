// What was generated for one file (preview thumbnails, chapters, intro/credit segments), as the
// media tools of a detail page read it from `/api/v1/admin/assets/{asset}/media-status`.
import type { TranslationKey } from '../i18n/messages';

export type TrickplayStatus = 'generated' | 'missing' | 'running' | 'failed' | 'disabled';
export type ChapterImagesStatus = 'done' | 'running' | 'failed' | 'none' | 'disabled';
export type MediaStatus = {
  assetId: string;
  trickplay: { enabled: boolean; status: TrickplayStatus; width: number; intervalSeconds: number; thumbnailCount: number | null; canPurge: boolean };
  chapters: { count: number; imagesEnabled: boolean; images: ChapterImagesStatus };
  segments: { detectionEnabled: boolean; automatic: number; manual: number; locked: number };
};

/** Label of the preview thumbnails' state. */
export function trickplayLabel(status: MediaStatus['trickplay']): { key: TranslationKey; params?: Record<string, number> } {
  switch (status.status) {
    case 'generated': return { key: 'media_trickplay_generated', params: { width: status.width } };
    case 'running': return { key: 'media_trickplay_running' };
    case 'failed': return { key: 'media_trickplay_failed' };
    case 'disabled': return { key: 'media_trickplay_disabled' };
    default: return { key: 'media_trickplay_missing' };
  }
}

/** Label of the segments' state: a correction locks them, else the detections, else nothing. */
export function segmentsLabel(segments: MediaStatus['segments']): { key: TranslationKey; params?: Record<string, number> } {
  if (segments.locked > 0) return { key: 'media_segments_locked', params: { count: segments.locked } };
  if (!segments.detectionEnabled && segments.automatic === 0) return { key: 'media_segments_disabled' };
  if (segments.automatic > 0) return { key: 'media_segments_auto', params: { count: segments.automatic } };
  return { key: 'media_segments_none' };
}

/** Label of the chapters' state. */
export function chaptersLabel(chapters: MediaStatus['chapters']): { key: TranslationKey; params?: Record<string, number> } {
  return chapters.count > 0 ? { key: 'media_chapters_count', params: { count: chapters.count } } : { key: 'media_chapters_none' };
}

/** A warm-up is still waiting or running: the panel refreshes until it is done. */
export function mediaStatusActive(status: MediaStatus): boolean {
  return status.trickplay.status === 'running' || status.chapters.images === 'running';
}

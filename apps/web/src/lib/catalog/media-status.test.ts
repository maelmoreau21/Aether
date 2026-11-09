import { describe, expect, it } from 'vitest';
import { chaptersLabel, mediaStatusActive, segmentsLabel, trickplayLabel, type MediaStatus } from './media-status';

const status = (patch: Partial<MediaStatus> = {}): MediaStatus => ({
  assetId: 'a1',
  trickplay: { enabled: true, status: 'generated', width: 320, intervalSeconds: 10, thumbnailCount: 120, canPurge: true },
  chapters: { count: 8, imagesEnabled: true, images: 'done' },
  segments: { detectionEnabled: true, automatic: 2, manual: 0, locked: 0 },
  ...patch
});

describe('media status labels', () => {
  it('names the generated thumbnails with their width, and every other state', () => {
    expect(trickplayLabel(status().trickplay)).toEqual({ key: 'media_trickplay_generated', params: { width: 320 } });
    for (const [state, key] of [['missing', 'media_trickplay_missing'], ['running', 'media_trickplay_running'], ['failed', 'media_trickplay_failed'], ['disabled', 'media_trickplay_disabled']] as const) {
      expect(trickplayLabel({ ...status().trickplay, status: state }).key).toBe(key);
    }
  });
  it('shows a locked correction before the detections it replaces', () => {
    expect(segmentsLabel({ detectionEnabled: true, automatic: 2, manual: 1, locked: 1 })).toEqual({ key: 'media_segments_locked', params: { count: 1 } });
    expect(segmentsLabel(status().segments)).toEqual({ key: 'media_segments_auto', params: { count: 2 } });
    expect(segmentsLabel({ detectionEnabled: true, automatic: 0, manual: 0, locked: 0 }).key).toBe('media_segments_none');
    expect(segmentsLabel({ detectionEnabled: false, automatic: 0, manual: 0, locked: 0 }).key).toBe('media_segments_disabled');
  });
  it('counts the chapters', () => {
    expect(chaptersLabel(status().chapters)).toEqual({ key: 'media_chapters_count', params: { count: 8 } });
    expect(chaptersLabel({ count: 0, imagesEnabled: true, images: 'none' }).key).toBe('media_chapters_none');
  });
  it('keeps refreshing while thumbnails or chapter images are being prepared', () => {
    expect(mediaStatusActive(status())).toBe(false);
    expect(mediaStatusActive(status({ trickplay: { ...status().trickplay, status: 'running' } }))).toBe(true);
    expect(mediaStatusActive(status({ chapters: { count: 3, imagesEnabled: true, images: 'running' } }))).toBe(true);
  });
});

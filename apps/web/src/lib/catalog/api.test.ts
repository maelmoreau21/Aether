import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  clearDescriptorCache,
  fetchMediaDescriptor,
  fetchMediaDescriptors,
  getCachedMedia,
  playbackIds,
  sourceLabel,
  type Work
} from './api';
import type { Media } from '../player/types';

const sampleMedia: Media = {
  id: 'media-1',
  title: 'Épisode 1',
  sources: [{ id: 's1', audioTrackId: 'a1', language: 'fr', url: '/api/v1/assets/media-1/content', mime: 'video/x-matroska' }],
  subtitles: []
};

const sampleMedia2: Media = {
  id: 'media-2',
  title: 'Épisode 2',
  sources: [{ id: 's2', audioTrackId: 'a2', language: 'fr', url: '/api/v1/assets/media-2/content', mime: 'video/mp4' }],
  subtitles: []
};

afterEach(() => {
  vi.unstubAllGlobals();
  clearDescriptorCache();
});

describe('media descriptor cache and deferred loading', () => {
  it('caches descriptors and does not re-fetch from network', async () => {
    const fetchMock = vi.fn().mockResolvedValue(Response.json(sampleMedia));
    vi.stubGlobal('fetch', fetchMock);

    expect(getCachedMedia('media-1')).toBeUndefined();
    const result1 = await fetchMediaDescriptor('media-1');
    expect(result1).toEqual(sampleMedia);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    expect(getCachedMedia('media-1')).toEqual(sampleMedia);

    // Second call should come from cache
    const result2 = await fetchMediaDescriptor('media-1');
    expect(result2).toEqual(sampleMedia);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('supports batch prefetching without duplicating cached entries', async () => {
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url.includes('media-1')) return Promise.resolve(Response.json(sampleMedia));
      if (url.includes('media-2')) return Promise.resolve(Response.json(sampleMedia2));
      return Promise.reject(new Error('not found'));
    });
    vi.stubGlobal('fetch', fetchMock);

    const results = await fetchMediaDescriptors(['media-1', 'media-2']);
    expect(results).toHaveLength(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);

    // Pre-cached: zero additional network calls
    const cachedAgain = await fetchMediaDescriptors(['media-1', 'media-2']);
    expect(cachedAgain).toEqual(results);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('handles abort signal when switching episodes quickly', async () => {
    const controller = new AbortController();
    controller.abort();

    const fetchMock = vi.fn().mockImplementation((_url: string, opts?: RequestInit) => {
      if (opts?.signal?.aborted) {
        return Promise.reject(new DOMException('Aborted', 'AbortError'));
      }
      return Promise.resolve(Response.json(sampleMedia));
    });
    vi.stubGlobal('fetch', fetchMock);

    await expect(fetchMediaDescriptor('media-1', controller.signal)).rejects.toThrow();
  });

  it('handles 403 Forbidden when permissions are revoked', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 403 })));
    await expect(fetchMediaDescriptor('media-forbidden')).rejects.toThrow('Votre compte n’a pas accès à cette ressource.');
  });

  it('computes correct source labels from media mime type', () => {
    expect(sourceLabel(sampleMedia)).toBe('MKV');
    expect(sourceLabel(sampleMedia2)).toBe('MP4');
  });

  it('extracts playback ids accurately from Work structure', () => {
    const work: Work = {
      id: 'ep-1',
      kind: 'episode',
      title: 'Episode 1',
      year: 2026,
      description: '',
      libraryId: 'lib-1',
      synthetic: false,
      editions: [
        {
          id: 'ed-1',
          label: 'Locale',
          versions: [
            { id: 'v1', label: '1080p', sources: [{ id: 's1', assetIds: ['a1'], playbackMediaId: 'a1' }] },
            { id: 'v2', label: '4k', sources: [{ id: 's2', assetIds: ['a2'], playbackMediaId: 'a2' }] }
          ]
        }
      ]
    };
    expect(playbackIds(work)).toEqual(['a1', 'a2']);
  });
});

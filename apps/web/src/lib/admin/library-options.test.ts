import { describe, expect, it } from 'vitest';
import { defaultLibraryOptions, recommendedLibraryOptions } from './types';

describe('library processing defaults', () => {
  it('prepares previews and keeps media files read-only', () => {
    const options = defaultLibraryOptions();
    expect(options).toMatchObject({ trickplayEnabled: true, trickplayDuringScan: true,
      trickplayWidth: 320, trickplayIntervalSeconds: 10, trickplayQuality: 7,
      trickplayColumns: 10, trickplayRows: 10,
      realtimeMonitoring: true, automaticCollections: true, introDetectionEnabled: true,
      chapterImagesEnabled: true, chapterImagesDuringScan: false, onlineRefreshIntervalDays: 30,
      saveNfoToMedia: false, saveImagesToMedia: false, trickplaySaveToMedia: false,
      chapterImagesSaveToMedia: false, saveSubtitlesToMedia: false });
  });

  it('applies the preset without changing language, provider restrictions or subtitle policy', () => {
    const current = { ...defaultLibraryOptions(), preferredMetadataLanguage: 'ja', metadataCountryCode: 'JP',
      realtimeMonitoring: false, trickplayEnabled: false, trickplayWidth: 640, trickplayColumns: 5, trickplayRows: 5,
      metadataProviders: [{ id: 'provider-a', enabled: false }], embeddedSubtitlePolicy: 'allow_text' as const,
      subtitleDownloadEnabled: true, subtitleRequireHash: false };
    const next = recommendedLibraryOptions(current);
    expect(next).toMatchObject({ preferredMetadataLanguage: 'ja', metadataCountryCode: 'JP',
      metadataProviders: current.metadataProviders, embeddedSubtitlePolicy: 'allow_text',
      subtitleDownloadEnabled: true, subtitleRequireHash: false, trickplayEnabled: true,
      trickplayWidth: 320, trickplayColumns: 10, trickplayRows: 10, realtimeMonitoring: true });
    expect(current.trickplayEnabled).toBe(false);
    expect(current.trickplayWidth).toBe(640);
    expect(current.trickplayColumns).toBe(5);
  });

  it('does not share provider lists between new libraries', () => {
    const first = defaultLibraryOptions();
    first.metadataProviders.push({ id: 'provider', enabled: true });
    expect(defaultLibraryOptions().metadataProviders).toEqual([]);
  });
});

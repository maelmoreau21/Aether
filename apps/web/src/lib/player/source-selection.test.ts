import { describe, expect, it } from 'vitest';
import { audioLanguage, audioSelectionNotice, chooseSource, isAudioDescription, playbackMime } from './source-selection';
import type { Media } from './types';

const media: Media = { id: 'dual', title: 'Synthetic', subtitles: [], sources: [
  { id: 'original', audioTrackId: 'internal-default', language: 'und', url: '/original', mime: 'video/mp4', tracks: [
    { index: 0, codec_type: 'video', codec_name: 'av1', pix_fmt: 'yuv420p10le' },
    { index: 1, codec_type: 'audio', codec_name: 'opus', channels: 6 },
    { index: 2, codec_type: 'audio', codec_name: 'opus', channels: 6 }
  ] },
  ...['fra', 'eng'].map((language, i) => ({ id: language, audioTrackId: `internal-${i + 1}`, language,
    url: '/prepared/video', mime: 'video/mp4', externalAudioUrl: `/prepared/audio-${i + 1}`, prepare: true }))
] };

describe('AV1/Opus source selection', () => {
  it('never invents an incomplete AV1 codecs parameter', () => {
    for (const source of media.sources) expect(playbackMime(media, source)).toBe('video/mp4');
  });
  it('keeps a concrete French track when a capability hint rejects prepared video', () => {
    expect(chooseSource(media, undefined, 'fr', source => source.id === 'original')?.audioTrackId).toBe('internal-1');
  });
  it('restores English and explicit FR/EN/FR choices on reopening', () => {
    expect(chooseSource(media, undefined, 'en-US')?.audioTrackId).toBe('internal-2');
    for (const id of ['internal-1', 'internal-2', 'internal-1'])
      expect(chooseSource(media, id, 'en', () => false)?.audioTrackId).toBe(id);
    expect(chooseSource(media, 'stale-id', 'eng')?.audioTrackId).toBe('internal-2');
  });
  it('normalizes ISO and regional language tags', () => {
    expect(['fra', 'fre', 'fr-FR', 'FR_fr'].map(audioLanguage)).toEqual(['fr', 'fr', 'fr', 'fr']);
    expect(['eng', 'en-GB'].map(audioLanguage)).toEqual(['en', 'en']);
  });
  it('uses an explicitly marked default audio only when requested, with manual choices first', () => {
    const withDefault: Media = { ...media, sources: media.sources.map((source, i) => i === 2
      ? { ...source, tracks: [{ index: 2, codec_type: 'audio', disposition: { default: 1 } }] }
      : source) };
    expect(chooseSource(withDefault, undefined, 'fr')?.audioTrackId).toBe('internal-1');
    expect(chooseSource(withDefault, undefined, 'fr', () => true, true)?.audioTrackId).toBe('internal-2');
    expect(chooseSource(withDefault, 'internal-1', 'en', () => true, true)?.audioTrackId).toBe('internal-1');
    expect(chooseSource(media, undefined, 'fr', () => true, true)?.audioTrackId).toBe('internal-1');
  });
  it('does not confuse another audio default flag in a full probe with the selected track', () => {
    const tracks = [{ index: 1, codec_type: 'audio', disposition: { default: 0 } }, { index: 2, codec_type: 'audio', disposition: { default: 1 } }];
    const withProbe = { ...media, sources: media.sources.map(source => ({ ...source, tracks })) };
    expect(chooseSource(withProbe, undefined, 'fr', () => true, true)?.audioTrackId).toBe('internal-2');
  });
  it('migrates a saved ambiguous original to the preferred indexed language', () => {
    expect(chooseSource(media, 'internal-default', 'fra')?.audioTrackId).toBe('internal-1');
    expect(chooseSource(media, 'internal-default', 'eng')?.audioTrackId).toBe('internal-2');
  });
  it('distinguishes an unavailable language from a manual choice or unknown original', () => {
    expect(audioSelectionNotice(media, media.sources[1], 'fr')).toBe('');
    expect(audioSelectionNotice(media, media.sources[2], 'fr')).toBe('');
    expect(audioSelectionNotice(media, media.sources[1], 'de')).toContain('indisponible');
    expect(audioSelectionNotice({...media,sources:[media.sources[0]]},media.sources[0],'fr')).toContain('ne peut pas être confirmée');
  });
});

describe('audio description', () => {
  const described: Media = { id: 'ad', title: 'AD', subtitles: [], sources: [
    { id: 'main', audioTrackId: 'internal-1', language: 'fr', url: '/v', mime: 'video/mp4', tracks: [{ index: 1, codec_type: 'audio', codec_name: 'aac' }] },
    { id: 'ad', audioTrackId: 'internal-2', language: 'fr', url: '/v', mime: 'video/mp4', tracks: [{ index: 2, codec_type: 'audio', codec_name: 'aac', tags: { title: 'Audiodescription' } }] },
    { id: 'flagged', audioTrackId: 'internal-3', language: 'en', url: '/v', mime: 'video/mp4', tracks: [{ index: 3, codec_type: 'audio', codec_name: 'aac', disposition: { visual_impaired: 1 } }] }
  ] };
  it('recognises flags and titles', () => {
    expect(described.sources.map(isAudioDescription)).toEqual([false, true, true]);
  });
  it('is never chosen by default, and first when the account prefers it', () => {
    expect(chooseSource(described, undefined, 'fr')?.id).toBe('main');
    expect(chooseSource(described, undefined, 'fr', () => true, false, true)?.id).toBe('ad');
    expect(chooseSource(described, 'internal-1', 'fr', () => true, false, true)?.id).toBe('main');
  });
});

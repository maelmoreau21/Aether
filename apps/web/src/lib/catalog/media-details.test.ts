import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  audioSummary, fetchAssetDetails, formatBitrate, formatBytes, formatDuration, groupTracks,
  nodeStatusLabel, trackRoles, versionLabel, videoSummary, type AssetDetails
} from './media-details';

const details = (overrides: Partial<AssetDetails> = {}): AssetDetails => ({
  assetId: 'a1', workId: 'w1', available: true, pathsVisible: false,
  node: { id: 'local', name: 'NAS salon', kind: 'local', status: 'connected' },
  library: { id: 'l1', name: 'Films', kind: 'films' },
  file: { size: 1_610_612_736, mime: 'video/x-matroska' },
  container: { formatName: 'matroska,webm', durationSeconds: 5400, bitRate: 4_500_000 },
  tracks: [
    { index: 0, codecType: 'video', codecName: 'av1', width: 3840, height: 2160, pixFmt: 'yuv420p10le', frameRate: 23.976, colorTransfer: 'smpte2084' },
    { index: 1, codecType: 'audio', codecName: 'opus', channels: 6, channelLayout: '5.1', sampleRate: '48000', language: 'fra', disposition: { default: true } },
    { index: 2, codecType: 'subtitle', codecName: 'subrip', language: 'fra', disposition: { forced: true, hearingImpaired: true } },
    { index: 3, codecType: 'attachment', codecName: 'ttf' }
  ],
  externalAudio: [], externalSubtitles: [], chapterCount: 0,
  ...overrides
});

afterEach(() => vi.unstubAllGlobals());

describe('media details formatting', () => {
  it('formats sizes, bit rates and durations in French', () => {
    expect(formatBytes(1_610_612_736)).toBe('1,5 Go');
    expect(formatBytes(512)).toBe('512 o');
    expect(formatBytes(0)).toBe('—');
    expect(formatBitrate(4_500_000)).toBe('4,5 Mb/s');
    expect(formatBitrate(256_000)).toBe('256 kb/s');
    expect(formatBitrate(undefined)).toBeNull();
    expect(formatDuration(5400)).toBe('1 h 30 min');
    expect(formatDuration(95)).toBe('1 min 35 s');
  });

  it('describes video, audio and roles of each track', () => {
    const { video, audio, subtitle, other } = groupTracks(details().tracks);
    expect(other).toHaveLength(1);
    expect(videoSummary(video[0])).toEqual(['3840 × 2160 (4K Ultra HD)', '23,976 i/s', '10 bits', 'HDR10 (PQ)']);
    expect(audioSummary(audio[0])).toEqual(['5.1 · 6 canaux', '48 kHz']);
    expect(trackRoles(subtitle[0].disposition)).toEqual(['Forcés', 'Malentendants (SDH)']);
  });

  it('labels versions and Node status without inventing data', () => {
    expect(versionLabel(details(), 0)).toBe('Version 1 · 4K Ultra HD · AV1 · MKV');
    expect(versionLabel(undefined, 2)).toBe('Version 3');
    expect(nodeStatusLabel('connected').tone).toBe('online');
    expect(nodeStatusLabel('revoked').label).toBe('Révoqué');
    expect(nodeStatusLabel(undefined).tone).toBe('offline');
  });

  it('requests the opaque asset identifier only and maps refusals', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify(details()), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    const result = await fetchAssetDetails('a/1', new AbortController().signal);
    expect(result.node.name).toBe('NAS salon');
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/assets/a%2F1/details', expect.anything());
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 403 })));
    await expect(fetchAssetDetails('a1', new AbortController().signal)).rejects.toThrow('accès');
  });
});

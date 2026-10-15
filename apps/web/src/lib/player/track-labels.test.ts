import { describe, expect, it } from 'vitest';
import { audioLabel, videoLabel } from './track-labels';
import type { Source } from './types';
const source: Source = { id: 'one', audioTrackId: 'fr', language: 'fr', url: '/known', mime: 'video/mp4' };
describe('track labels use available metadata', () => {
  it('shows the known audio codec and layout', () => {
    expect(audioLabel({ ...source, tracks: [{ codec_type: 'audio', codec_name: 'opus', channels: 6, channel_layout: '5.1' }] })).toBe('français · OPUS · 5.1');
  });
  it('never invents an unknown channel layout or codec', () => {
    expect(audioLabel({ ...source, tracks: [{ codec_type: 'audio', channels: 8 }] })).toBe('français · 8 canaux');
    expect(audioLabel(source)).toBe('français');
  });
  it('shows only known video properties', () => {
    expect(videoLabel({ codec_name: 'av1', width: 320, height: 180, pix_fmt: 'yuv420p10le' })).toBe('AV1 · 320 × 180 · 10 bits');
    expect(videoLabel({})).toBe('Informations indisponibles');
  });
});

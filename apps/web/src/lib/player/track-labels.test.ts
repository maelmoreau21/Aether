import { describe, expect, it } from 'vitest';
import { audioLabel, videoLabel, videoResolutionBadge, containerLabel, colorProfileLabel } from './track-labels';
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

  it('formats video resolution badges', () => {
    expect(videoResolutionBadge(1920, 1080)).toBe('1080p Full HD');
    expect(videoResolutionBadge(3840, 2160)).toBe('4K Ultra HD');
    expect(videoResolutionBadge(1280, 720)).toBe('720p HD');
    expect(videoResolutionBadge(640, 480)).toBe('SD');
    expect(videoResolutionBadge(undefined, undefined)).toBeNull();
  });

  it('formats container labels and color profiles', () => {
    expect(containerLabel('video/mp4')).toBe('MPEG-4 (.mp4)');
    expect(containerLabel('video/x-matroska')).toBe('Matroska (.mkv)');
    expect(colorProfileLabel('yuv420p10le')).toBe('10 bits (Plage étendue)');
    expect(colorProfileLabel('yuv420p')).toBe('8 bits standard');
    expect(colorProfileLabel(undefined)).toBeNull();
  });
});

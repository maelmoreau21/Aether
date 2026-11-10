import { describe, expect, it } from 'vitest';
import type { Media, Source } from './types';
import {
  FLAG_COPY_AUDIO, FLAG_NIGHT, FLAG_SURROUND, FLAG_TONEMAP, StallMonitor, audioIndex, lowerCeiling, maxBitrate,
  needsToneMapping, planTranscode, qualityChoice, qualityPreference, rung, transcodeReason, type Environment, type TranscodeOptions
} from './transcode';

const offer = { url: '/api/v1/assets/a/prepared/transcode-', videoIndex: 0, videoCodec: 'hevc', width: 3840, height: 2160, bitrateKbps: 60000, durationSeconds: 7200, dynamicRange: 'hdr10', copyVideoMime: null };
const tracks = [
  { index: 0, codec_type: 'video', codec_name: 'hevc', width: 3840, height: 2160 },
  { index: 1, codec_type: 'audio', codec_name: 'eac3', channels: 6, disposition: { default: 1 } },
  { index: 2, codec_type: 'audio', codec_name: 'dts', channels: 6 }
];
const original: Source = { id: 'a', audioTrackId: 'internal-default', language: 'und', url: '/content', mime: 'video/x-matroska', tracks };
const second: Source = { id: 'a-transcode-2', audioTrackId: 'internal-2', language: 'en', url: '/content', mime: 'video/x-matroska', tracks: [tracks[2]], transcodeOnly: true };
const media: Media = { id: 'a', title: 'Film', sources: [original, second], subtitles: [], transcode: offer };
const options: TranscodeOptions = { quality: 'auto', maxKbps: 0, toneMapping: 'auto', audioChannels: 'stereo' };
const sdr: Environment = { mse: () => true, canPlay: () => '', hdrDisplay: false };

describe('fallback transcoding plan', () => {
  it('plays directly unless something requires the server', () => {
    expect(transcodeReason(media, original, options, sdr, false)).toBeUndefined();
    expect(transcodeReason(media, second, options, sdr, false)).toBe('audio');
    expect(transcodeReason(media, original, options, sdr, true)).toBe('audio');
    expect(transcodeReason(media, original, { ...options, quality: 720 }, sdr, false)).toBe('quality');
    expect(transcodeReason(media, original, { ...options, quality: 2160 }, sdr, false)).toBeUndefined();
    expect(transcodeReason(media, original, { ...options, maxKbps: 20000 }, sdr, false)).toBe('bitrate');
    expect(transcodeReason(media, original, { ...options, toneMapping: 'always' }, sdr, false)).toBe('hdr');
    expect(transcodeReason(media, original, { ...options, burnSubtitle: 5 }, sdr, false)).toBe('subtitle');
    expect(transcodeReason({ ...media, transcode: null }, second, options, sdr, false)).toBeUndefined();
  });

  it('builds the exact resource the Node parses', () => {
    const plan = planTranscode(media, original, 'quality', { ...options, quality: 1080 }, sdr)!;
    expect(plan.prefix).toBe('/api/v1/assets/a/prepared/transcode-v0a1sxh1080b8000f1-');
    expect(plan.flags & FLAG_TONEMAP).toBeTruthy();
    expect(plan.mime).toBe('video/mp4; codecs="avc1.640029,mp4a.40.2"');
    const hdr = planTranscode(media, original, 'quality', { ...options, quality: 1080 }, { ...sdr, hdrDisplay: true })!;
    expect(hdr.flags & FLAG_TONEMAP).toBe(0);
  });

  it('picks the audio track of the source and its channel and passthrough choices', () => {
    expect(audioIndex(media, original)).toBe(1);
    expect(audioIndex(media, second)).toBe(2);
    expect(audioIndex(media, { ...original, audioTrackId: 'external-0' })).toBeUndefined();
    const surround = planTranscode(media, second, 'audio', { ...options, audioChannels: 'surround' }, sdr)!;
    expect(surround.audio).toBe(2);
    expect(surround.flags & FLAG_SURROUND).toBeTruthy();
    const passthrough = planTranscode(media, original, 'quality', { ...options, quality: 720, audioChannels: 'passthrough' }, { ...sdr, canPlay: mime => mime.includes('ec-3') ? 'probably' : '' })!;
    expect(passthrough.flags & FLAG_COPY_AUDIO).toBeTruthy();
    expect(passthrough.mime).toContain('ec-3');
    const night = planTranscode(media, original, 'quality', { ...options, quality: 720, serverAudio: 'night' }, sdr)!;
    expect(night.flags & FLAG_NIGHT).toBeTruthy();
  });

  it('copies a decodable original picture when only the audio needs converting', () => {
    const av1 = { ...media, transcode: { ...offer, dynamicRange: null, copyVideoMime: 'video/mp4; codecs="av01.0.08M.10"' } };
    const plan = planTranscode(av1, second, 'audio', options, sdr)!;
    expect(plan.height).toBe(0);
    expect(plan.subtitle).toBeUndefined();
    expect(plan.mime).toBe('video/mp4; codecs="av01.0.08M.10,mp4a.40.2"');
    // Without MSE support for the copy, the picture is transcoded instead.
    const encoded = planTranscode(av1, second, 'audio', options, { ...sdr, mse: mime => !mime.includes('av01') })!;
    expect(encoded.height).toBe(2160);
    expect(planTranscode(av1, second, 'audio', options, { ...sdr, mse: () => false })).toBeUndefined();
  });

  it('follows the server audio and video transcoding switches', () => {
    const noVideo = { ...media, transcode: { ...offer, video: false } };
    expect(planTranscode(noVideo, original, 'quality', { ...options, quality: 1080 }, sdr)).toBeUndefined();
    const noAudio = { ...media, transcode: { ...offer, audio: false } };
    // The original track is copied when fragmented MP4 takes it, never encoded.
    const copied = planTranscode(noAudio, original, 'quality', { ...options, quality: 1080, serverAudio: 'night' }, sdr)!;
    expect(copied.flags & FLAG_COPY_AUDIO).toBeTruthy();
    expect(copied.flags & FLAG_NIGHT).toBe(0);
    expect(copied.mime).toBe('video/mp4; codecs="avc1.640029,ec-3"');
    expect(planTranscode(noAudio, second, 'audio', options, sdr)).toBeUndefined();
  });

  it('fits the ladder to the source height and every ceiling', () => {
    expect(rung(offer, options)).toEqual({ height: 2160, kbps: 20000 });
    expect(rung(offer, { ...options, maxKbps: 4256 })).toEqual({ height: 720, kbps: 4000 });
    expect(rung(offer, { ...options, maxKbps: 3500 })).toEqual({ height: 720, kbps: 3244 });
    expect(rung(offer, { ...options, maxKbps: 1000 })).toEqual({ height: 360, kbps: 744 });
    expect(rung({ ...offer, height: 800 }, { ...options, quality: 1080 }).height).toBe(1080);
    expect(rung({ ...offer, height: 800 }, options).height).toBe(1080);
    expect(rung(offer, { ...options, maxKbps: 20000, adaptiveKbps: 1756 }).height).toBe(480);
  });

  it('steps down one rung at a time and stops at the bottom', () => {
    expect(lowerCeiling({ kbps: 8000 })).toBe(4256);
    expect(lowerCeiling({ kbps: 3244 })).toBe(1756);
    expect(lowerCeiling({ kbps: 800 })).toBeUndefined();
  });

  it('does not tone map Dolby Vision profile 5 or on HDR screens', () => {
    expect(needsToneMapping(offer, options, sdr)).toBe(true);
    expect(needsToneMapping({ ...offer, dynamicRange: 'dolby_vision_5' }, options, sdr)).toBe(false);
    expect(needsToneMapping({ ...offer, dynamicRange: 'dolby_vision_8' }, options, sdr)).toBe(true);
    expect(needsToneMapping(offer, { toneMapping: 'never' }, sdr)).toBe(false);
    expect(needsToneMapping({ ...offer, dynamicRange: null }, options, sdr)).toBe(false);
  });

  it('reads stored preferences strictly', () => {
    expect(qualityChoice('720')).toBe(720);
    expect(qualityChoice('original')).toBe('original');
    expect(qualityChoice('999')).toBe('auto');
    expect(qualityPreference(1080)).toBe('1080');
    expect(maxBitrate(4000)).toBe(4000);
    expect(maxBitrate(100)).toBe(0);
    expect(maxBitrate('4000')).toBe(0);
  });
});

describe('stall monitor', () => {
  it('asks for a lower quality after three stalls in a minute or one long stall', () => {
    const monitor = new StallMonitor();
    expect(monitor.waiting(0)).toBe(false); monitor.playing(500);
    expect(monitor.waiting(10000)).toBe(false); monitor.playing(10500);
    expect(monitor.waiting(20000)).toBe(true);
    monitor.reset();
    expect(monitor.waiting(100000)).toBe(false);
    expect(monitor.stuck(103000)).toBe(false);
    expect(monitor.stuck(104000)).toBe(true);
    expect(monitor.playing(104500)).toBe(true);
    // Old stalls leave the window.
    monitor.reset();
    monitor.waiting(0); monitor.playing(100); monitor.waiting(30000); monitor.playing(30100);
    expect(monitor.waiting(70000)).toBe(false);
  });
});

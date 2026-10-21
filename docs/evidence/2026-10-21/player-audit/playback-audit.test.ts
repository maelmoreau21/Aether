import { afterEach, describe, expect, it, vi } from 'vitest';
import { NativePlayer } from '../../../../apps/web/src/lib/player/native';
import { ExternalAudio } from '../../../../apps/web/src/lib/player/external-audio';
import type { Media } from '../../../../apps/web/src/lib/player/types';

class Element extends EventTarget {
  currentTime = 0; duration = 120; paused = true; seeking = false;
  volume = .4; playbackRate = 1; muted = false; readyState = 4;
  dataset = {}; preload = ''; hidden = false; src = ''; failLoad = false;
  removed = false; loads = 0;
  canPlayType() { return 'probably'; }
  insertAdjacentElement() {} remove() { this.removed = true; }
  removeAttribute() { this.src = ''; }
  querySelectorAll() { return []; }
  play() { this.paused = false; return Promise.resolve(); }
  pause() { this.paused = true; this.dispatchEvent(new Event('pause')); }
  load() {
    this.loads++;
    queueMicrotask(() => {
      this.dispatchEvent(new Event(this.failLoad ? 'error' : 'loadedmetadata'));
      if (!this.failLoad) this.dispatchEvent(new Event('seeked'));
    });
  }
}
function setup() {
  const audios: Element[] = [];
  vi.stubGlobal('Audio', class { constructor() { const a = new Element(); audios.push(a); return a; } });
  const video = new Element(); const error = vi.fn();
  const player = new NativePlayer(video as unknown as HTMLVideoElement, error);
  return { video, error, player, audios };
}
const media: Media = { id: 'audit', title: 'Synthetic test', subtitles: [],
  sources: ['fr', 'en'].map(language => ({ id: language, audioTrackId: language, language,
    url: '/prepared/video', mime: 'video/mp4', externalAudioUrl: `/prepared/${language}` })) };
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe('regressions for the playback audit fixes', () => {
  it('prepares cold components without conflict against the single-worker contract', async () => {
    let occupied = false;
    const fetcher = vi.fn(async () => {
      if (occupied) return { ok: false, status: 503 };
      occupied = true;
      await new Promise(resolve => setTimeout(resolve, 20));
      occupied = false; return { ok: true, status: 200 };
    });
    vi.stubGlobal('fetch', fetcher);
    const { player, video } = setup();
    try {
      await player.load({ ...media, sources: media.sources.map(s => ({ ...s, prepare: true })) });
      expect(fetcher).toHaveBeenCalledTimes(2); expect(video.loads).toBe(1);
    } finally { player.destroy(); }
  });
  it('keeps the previous audio and removes a failed candidate during a same-video switch', async () => {
    const { player, video, audios } = setup();
    try {
      await player.load(media); video.currentTime = 25;
      const change = player.selectAudio('en');
      audios[1].failLoad = true;
      await expect(change).rejects.toThrow('illisible');
      expect(audios[0].removed).toBe(false);
      expect(audios[1].removed).toBe(true);
      const audioInstances = audios.length;
      // The formerly active track is still valid, so reselecting it needs no new load.
      await player.selectAudio('fr'); expect(audios.length).toBe(audioInstances);
      expect(audios[0].src).toBe('/prepared/fr'); expect(video.currentTime).toBe(25);
      await player.play(); expect(audios[0].paused).toBe(false); expect(video.muted).toBe(true);
    } finally { player.destroy(); }
  });
  it('pauses video and raises one terminal error when the audio alone stalls', async () => {
    vi.useFakeTimers();
    const { video, error, audios, player } = setup();
    const bridge = new ExternalAudio(video as unknown as HTMLVideoElement, 0, error);
    try {
      await bridge.load('/audio'); await video.play(); await bridge.play();
      audios[0].dispatchEvent(new Event('waiting'));
      video.currentTime = 4;
      await vi.advanceTimersByTimeAsync(11000);
      expect(video.paused).toBe(true); expect(error).toHaveBeenCalledTimes(1);
    } finally { bridge.destroy(); player.destroy(); }
  });
  it('realigns the audio immediately at an explicit seek boundary', async () => {
    const { video, error, audios, player } = setup();
    const bridge = new ExternalAudio(video as unknown as HTMLVideoElement, 0, error);
    try {
      await bridge.load('/audio'); video.currentTime = 10; audios[0].currentTime = 9.86;
      video.dispatchEvent(new Event('seeked'));
      expect(audios[0].currentTime).toBe(10);
    } finally { bridge.destroy(); player.destroy(); }
  });
  it('selects a concrete French track rather than an ambiguous multi-audio original', async () => {
    const { video, player } = setup();
    try {
      await player.load({ ...media, sources: [
        { id: 'original', audioTrackId: 'internal-default', language: 'fr', url: '/original', mime: 'video/mp4',
          tracks: [
            { index: 1, codec_type: 'audio', codec_name: 'opus', tags: { language: 'fra' }, disposition: { default: 0 } },
            { index: 2, codec_type: 'audio', codec_name: 'opus', tags: { language: 'eng' }, disposition: { default: 1 } }
          ] },
        { ...media.sources[0], id: 'explicit-fr', audioTrackId: 'internal-1' }
      ] });
      expect(video.src).toBe('/prepared/video'); expect(video.loads).toBe(1);
    } finally { player.destroy(); }
  });
});

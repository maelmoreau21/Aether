import { describe, it, expect, vi, afterEach } from 'vitest';
import { NativePlayer } from './native';
import type { Media } from './types';
class Video extends EventTarget {
  currentTime = 0; duration = 12; paused = true; volume = 0.4; playbackRate = 1.5;
  src = ''; loads = 0; auto = true;
  pause() { this.paused = true; }
  play() { this.paused = false; return Promise.resolve(); }
  querySelectorAll() { return []; }
  removeAttribute() { this.src = ''; }
  load() {
    this.loads++;
    if (this.auto) queueMicrotask(() => { this.dispatchEvent(new Event('loadedmetadata')); queueMicrotask(() => this.dispatchEvent(new Event('seeked'))); });
  }
}
const media: Media = { id: 'demo', title: 'Test', subtitles: [], sources: ['fr', 'en'].map(language => ({ id: language, audioTrackId: language, language, mime: 'video/mp4', url: `/${language}` })) };
function setup() { const video = new Video(); const error = vi.fn(); return { video, error, player: new NativePlayer(video as unknown as HTMLVideoElement, error) }; }
afterEach(() => vi.useRealTimers());
describe('native source replacement', () => {
  it('preserves paused position, speed and volume, and never silently substitutes a missing language', async () => {
    const { video, player } = setup();
    await player.load(media); video.currentTime = 4.25;
    await player.selectAudio('en');
    expect(video.src).toBe('/en'); expect(video.paused).toBe(true); expect(video.currentTime).toBe(4.25);
    expect(video.volume).toBe(0.4); expect(video.playbackRate).toBe(1.5);
    await expect(player.selectAudio('de')).rejects.toThrow('absente'); expect(video.src).toBe('/en'); expect(video.loads).toBe(2);
  });
  it('opens an indexed source with unknown language without claiming French audio', async () => {
    const { video, player } = setup();
    await player.load({ ...media, sources: [{ ...media.sources[0], language: 'und', url: '/indexed' }] });
    expect(video.src).toBe('/indexed');
  });
  it('resumes only an active presentation and refuses concurrent replacements', async () => {
    const { video, player } = setup(); await player.load(media); await player.play(); video.currentTime = 5;
    const change = player.selectAudio('en');
    await expect(player.selectAudio('fr')).rejects.toThrow('indisponible'); await change;
    expect(video.paused).toBe(false); expect(video.currentTime).toBe(5);
  });
  it('terminates a stalled load after 10s without retry or fallback', async () => {
    vi.useFakeTimers(); const { video, player } = setup(); video.auto = false;
    const result = expect(player.load(media)).rejects.toThrow('expiré');
    await vi.advanceTimersByTimeAsync(10000); await result;
    expect(video.loads).toBe(1); expect(video.paused).toBe(true);
    await vi.advanceTimersByTimeAsync(60000); expect(video.loads).toBe(1);
  });
  it('turns a playback stall into one terminal error without reloading', async () => {
    vi.useFakeTimers(); const { video, player, error } = setup();
    await player.load(media); await player.play(); video.dispatchEvent(new Event('waiting'));
    await vi.advanceTimersByTimeAsync(10000);
    expect(video.paused).toBe(true); expect(error).toHaveBeenCalledTimes(1); expect(video.loads).toBe(1);
    await vi.advanceTimersByTimeAsync(60000); expect(error).toHaveBeenCalledTimes(1);
  });
  it('cancels pending work on teardown', async () => {
    const { video, player } = setup(); video.auto = false;
    const result = expect(player.load(media)).rejects.toThrow('fermé'); player.destroy(); await result;
  });
});

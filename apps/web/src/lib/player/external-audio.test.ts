import { afterEach, describe, expect, it, vi } from 'vitest';
import { ExternalAudio } from './external-audio';
class Element extends EventTarget {
  currentTime = 0; duration = 12; paused = true; seeking = false; volume = .4; playbackRate = 1; muted = false;
  readyState = 4; dataset = {}; preload = ''; hidden = false; src = ''; auto = true;
  canPlayType() { return 'probably'; }
  insertAdjacentElement() {} remove() {} removeAttribute() {}
  play() { this.paused=false; return Promise.resolve(); }
  pause() { this.paused=true; }
  load() { if (this.auto) queueMicrotask(()=>this.dispatchEvent(new Event('loadedmetadata'))); }
}
afterEach(()=>{vi.unstubAllGlobals();vi.useRealTimers();});
function setup() {
  const video = new Element(); const audio = new Element(); const error=vi.fn();
  vi.stubGlobal('Audio', class { constructor() { return audio; } });
  const bridge = new ExternalAudio(video as unknown as HTMLVideoElement, 0, error);
  return { video, audio, error, bridge };
}
describe('independent audio timeline',()=>{
  it('applies signed offsets without playing delayed audio before its start',async()=>{
    vi.useFakeTimers(); const {video,audio,bridge}=setup(); await bridge.load('/audio');
    bridge.setOffset(2); await video.play(); await bridge.play(); expect(audio.paused).toBe(true);
    video.currentTime=3; await vi.advanceTimersByTimeAsync(100); expect(audio.currentTime).toBe(1); expect(audio.paused).toBe(false);
    bridge.setOffset(-1); expect(audio.currentTime).toBe(4);
    bridge.setOffset(NaN); expect(audio.currentTime).toBe(4);
    bridge.destroy(); expect(vi.getTimerCount()).toBe(0);
  });
  it('follows seeking, pauses, speed and volume without playing internal audio',async()=>{
    vi.useFakeTimers();const {video,audio,bridge}=setup();await bridge.load('/authorized/audio');
    expect(video.muted).toBe(true);video.currentTime=4;video.dispatchEvent(new Event('seeked'));
    expect(audio.currentTime).toBe(4);await video.play();await bridge.play();expect(audio.paused).toBe(false);
    video.volume=.2;video.playbackRate=1.5;video.dispatchEvent(new Event('ratechange'));expect(audio.volume).toBe(.2);expect(audio.playbackRate).toBe(1.5);
    video.pause();video.dispatchEvent(new Event('pause'));expect(audio.paused).toBe(true);
    bridge.destroy();expect(video.muted).toBe(false);expect(vi.getTimerCount()).toBe(0);
  });
  it('corrects drift, and stops with an explicit error when external audio fails',async()=>{
    vi.useFakeTimers();const {video,audio,error,bridge}=setup();await bridge.load('/audio');
    video.currentTime=6;audio.currentTime=4;await vi.advanceTimersByTimeAsync(100);expect(audio.currentTime).toBe(6);
    await video.play();audio.dispatchEvent(new Event('error'));expect(video.paused).toBe(true);expect(error).toHaveBeenCalledTimes(1);
    audio.dispatchEvent(new Event('error'));expect(error).toHaveBeenCalledTimes(1);bridge.destroy();
  });
  it('bounds pending metadata loads and supports cancellation',async()=>{
    vi.useFakeTimers();const {audio,bridge}=setup();audio.auto=false;
    const load=expect(bridge.load('/audio')).rejects.toThrow('expiré');await vi.advanceTimersByTimeAsync(10000);await load;bridge.destroy();
  });
  it('resumes after audio buffering but respects a manual pause during recovery',async()=>{
    vi.useFakeTimers(); const {video,audio,bridge,error}=setup(); await bridge.load('/audio');
    await video.play(); await bridge.play(); audio.dispatchEvent(new Event('waiting'));
    expect(video.paused).toBe(true);
    audio.dispatchEvent(new Event('canplay')); await vi.advanceTimersByTimeAsync(0);
    expect(video.paused).toBe(false); expect(audio.paused).toBe(false);
    audio.dispatchEvent(new Event('waiting')); audio.dispatchEvent(new Event('canplay'));
    bridge.pause(); video.pause(); await vi.advanceTimersByTimeAsync(11000);
    expect(video.paused).toBe(true); expect(error).not.toHaveBeenCalled(); bridge.destroy();
  });
});

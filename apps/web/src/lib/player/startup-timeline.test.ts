import { describe, expect, it, vi } from 'vitest';
import { StartupTimeline, observeClockAdvance, observeFirstFrame } from './startup-timeline';

describe('startup timeline', () => {
  it('reports first frame and first sound as separate stages, and fires once both exist', () => {
    let now = 1000;
    const report = vi.fn();
    const timeline = new StartupTimeline(() => now, report);
    now = 1040; timeline.mark('metadata');
    now = 1100; timeline.mark('buffered');
    now = 1130; timeline.mark('first-frame');
    expect(report).not.toHaveBeenCalled();
    now = 1190; timeline.mark('first-audio', 'audio-element');
    expect(report).toHaveBeenCalledOnce();
    const summary = report.mock.calls[0][0];
    expect(summary.stages).toMatchObject({ metadata: 40, buffered: 100, 'first-frame': 130, 'first-audio': 190 });
    expect(summary.audioMinusFrameMs).toBe(60);
    expect(summary.audioSignal).toBe('audio-element');
    now = 1500; timeline.mark('first-frame');
    expect(timeline.summary().stages['first-frame']).toBe(130);
    expect(report).toHaveBeenCalledOnce();
  });
  it('restarts cleanly for a new source', () => {
    let now = 0;
    const report = vi.fn();
    const timeline = new StartupTimeline(() => now, report);
    now = 10; timeline.mark('first-frame'); now = 20; timeline.mark('first-audio', 'media-clock');
    now = 100; timeline.begin();
    expect(timeline.summary().stages).toEqual({});
    now = 130; timeline.mark('first-frame'); now = 135; timeline.mark('first-audio', 'media-clock');
    expect(report).toHaveBeenCalledTimes(2);
    expect(report.mock.calls[1][0].stages['first-frame']).toBe(30);
  });
  it('does not claim a gap when the sound was not observed', () => {
    const timeline = new StartupTimeline(() => 0);
    timeline.mark('first-frame');
    expect(timeline.summary().audioMinusFrameMs).toBeUndefined();
  });
});

describe('frame and clock observers', () => {
  it('uses per-frame callbacks and waits for the media clock to move', () => {
    const callbacks: Array<(now: number, metadata: { mediaTime: number }) => void> = [];
    const video = Object.assign(new EventTarget(), {
      currentTime: 5,
      requestVideoFrameCallback: (cb: (now: number, metadata: { mediaTime: number }) => void) => callbacks.push(cb),
      cancelVideoFrameCallback: vi.fn()
    }) as unknown as HTMLVideoElement;
    const frame = vi.fn(); const advance = vi.fn();
    observeFirstFrame(video, frame);
    observeClockAdvance(video, 5, advance);
    callbacks.splice(0).forEach(cb => cb(0, { mediaTime: 5 }));
    expect(frame).toHaveBeenCalledWith(5);
    expect(advance).not.toHaveBeenCalled();
    callbacks.splice(0).forEach(cb => cb(0, { mediaTime: 5.05 }));
    expect(advance).toHaveBeenCalledOnce();
  });
  it('falls back to playing/timeupdate without requestVideoFrameCallback and can be cancelled', () => {
    const video = Object.assign(new EventTarget(), { currentTime: 2 }) as unknown as HTMLVideoElement;
    const frame = vi.fn(); const advance = vi.fn();
    const stopFrame = observeFirstFrame(video, frame);
    const stopClock = observeClockAdvance(video, 2, advance);
    video.dispatchEvent(new Event('playing'));
    expect(frame).toHaveBeenCalledOnce();
    (video as unknown as { currentTime: number }).currentTime = 2.3;
    video.dispatchEvent(new Event('timeupdate'));
    expect(advance).toHaveBeenCalledOnce();
    stopFrame(); stopClock();
  });
});

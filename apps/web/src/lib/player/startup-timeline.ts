/**
 * Startup stages of one `NativePlayer.load()` + first `play()`, measured in the
 * browser so the first picture and the first sound are reported separately.
 *
 * Stage meaning (each is the first time it happens after `begin`):
 * - `prepared`      – preparation HEAD answered (Node-side remux/copy finished); absent on Direct Play
 * - `metadata`      – `loadedmetadata`: network + container index cost
 * - `video-ready`   – source accepted at the requested position (includes saved-position seek)
 * - `audio-ready`   – selected external/segmented audio loaded (MSE init + first window)
 * - `buffered`      – startup reserve reached (decodable data at the position)
 * - `play-resolved` – `video.play()` resolved
 * - `first-frame`   – first frame handed to the compositor (`requestVideoFrameCallback`)
 * - `first-audio`   – audio started; see `audioSignal` for how strong that evidence is
 *
 * `audioSignal`:
 * - `audio-element`: a separate audio element's `play()` resolved (external or MSE track);
 *   the element plays, no PCM was inspected.
 * - `media-clock`: the track lives in the video element, which exposes no audio event; the
 *   media clock advanced past the start position. This is a proxy, not a measurement of sound.
 */
export type StartupStage =
  | 'prepared' | 'metadata' | 'video-ready' | 'audio-ready' | 'buffered'
  | 'play-resolved' | 'first-frame' | 'first-audio';
export type AudioSignal = 'audio-element' | 'media-clock';

export interface StartupSummary {
  /** Milliseconds from `begin` for every stage reached. */
  stages: Partial<Record<StartupStage, number>>;
  audioSignal?: AudioSignal;
  /** `first-audio` minus `first-frame`: positive when the picture precedes the sound. */
  audioMinusFrameMs?: number;
}

export class StartupTimeline {
  private origin: number;
  private marks: Partial<Record<StartupStage, number>> = {};
  private signal?: AudioSignal;
  private reported = false;

  constructor(private now: () => number = () => performance.now(), private report?: (summary: StartupSummary) => void) {
    this.origin = now();
  }

  /** Restarts the origin, e.g. when a new source replaces the previous one. */
  begin() {
    this.origin = this.now();
    this.marks = {};
    this.signal = undefined;
    this.reported = false;
  }

  mark(stage: StartupStage, signal?: AudioSignal) {
    if (this.marks[stage] !== undefined) return;
    this.marks[stage] = Math.max(0, this.now() - this.origin);
    if (stage === 'first-audio') this.signal = signal;
    if (!this.reported && this.marks['first-frame'] !== undefined && this.marks['first-audio'] !== undefined) {
      this.reported = true;
      this.report?.(this.summary());
    }
  }

  summary(): StartupSummary {
    const stages: StartupSummary['stages'] = {};
    for (const [stage, value] of Object.entries(this.marks) as [StartupStage, number][]) stages[stage] = Math.round(value * 10) / 10;
    const frame = this.marks['first-frame'];
    const audio = this.marks['first-audio'];
    return {
      stages,
      audioSignal: this.signal,
      audioMinusFrameMs: frame !== undefined && audio !== undefined ? Math.round((audio - frame) * 10) / 10 : undefined
    };
  }
}

/**
 * Calls `onFrame` for the first presented frame of `video`, if the browser has
 * `requestVideoFrameCallback`; returns a cancel function. Without the API the
 * `playing` event is used instead (later by up to one display refresh).
 */
export function observeFirstFrame(video: HTMLVideoElement, onFrame: (mediaTime: number) => void): () => void {
  const target = video as HTMLVideoElement & {
    requestVideoFrameCallback?: (cb: (now: number, metadata: { mediaTime: number }) => void) => number;
    cancelVideoFrameCallback?: (handle: number) => void;
  };
  if (typeof target.requestVideoFrameCallback === 'function') {
    const handle = target.requestVideoFrameCallback((_now, metadata) => onFrame(metadata.mediaTime));
    return () => target.cancelVideoFrameCallback?.(handle);
  }
  const playing = () => onFrame(video.currentTime);
  video.addEventListener('playing', playing, { once: true });
  return () => video.removeEventListener('playing', playing);
}

/**
 * Calls `onAdvance` once the media clock moved at least `minimum` seconds past
 * `from`. Uses per-frame callbacks when available (frame-accurate); `timeupdate`
 * only fires about every 250 ms, so the fallback is coarse.
 */
export function observeClockAdvance(video: HTMLVideoElement, from: number, onAdvance: () => void, minimum = 0.03): () => void {
  const target = video as HTMLVideoElement & {
    requestVideoFrameCallback?: (cb: (now: number, metadata: { mediaTime: number }) => void) => number;
    cancelVideoFrameCallback?: (handle: number) => void;
  };
  if (typeof target.requestVideoFrameCallback === 'function') {
    let handle = 0;
    let cancelled = false;
    const step = (_now: number, metadata: { mediaTime: number }) => {
      if (cancelled) return;
      if (metadata.mediaTime >= from + minimum) onAdvance();
      else handle = target.requestVideoFrameCallback!(step);
    };
    handle = target.requestVideoFrameCallback(step);
    return () => { cancelled = true; target.cancelVideoFrameCallback?.(handle); };
  }
  const check = () => {
    if (video.currentTime >= from + minimum) { video.removeEventListener('timeupdate', check); onAdvance(); }
  };
  video.addEventListener('timeupdate', check);
  check();
  return () => video.removeEventListener('timeupdate', check);
}

/**
 * Night mode and loudness levelling in the browser, applied to the element playing
 * the sound (the video itself, or a separate audio track).
 *
 * Off by default: no Web Audio graph exists until the viewer turns one of these on,
 * so production audio keeps its decoded channels untouched (see `external-audio.ts`).
 * Once on, the graph keeps every channel (`discrete`, destination opened to the
 * device's maximum) and only changes levels. An element routed through Web Audio is
 * silent if its source turns cross-origin, so callers keep enhanced playback on
 * same-origin URLs (`requiresSameOrigin`).
 */
export type AudioEnhancement = 'off' | 'normalize' | 'night';
export const AUDIO_ENHANCEMENTS: readonly AudioEnhancement[] = ['off', 'normalize', 'night'];

export function audioEnhancement(value: unknown): AudioEnhancement {
  return AUDIO_ENHANCEMENTS.find(item => item === value) ?? 'off';
}

/** Compressor settings for each mode; `off` passes the signal through unchanged. */
export function enhancementSettings(mode: AudioEnhancement) {
  if (mode === 'night') return { threshold: -30, knee: 12, ratio: 8, attack: 0.005, release: 0.2, gain: 2.2 };
  if (mode === 'normalize') return { threshold: -24, knee: 20, ratio: 3, attack: 0.02, release: 0.4, gain: 1.4 };
  return { threshold: 0, knee: 0, ratio: 1, attack: 0.003, release: 0.25, gain: 1 };
}

type Chain = { source: MediaElementAudioSourceNode; compressor: DynamicsCompressorNode; gain: GainNode };

export class AudioEnhancer {
  private context?: AudioContext;
  private chains = new WeakMap<HTMLMediaElement, Chain>();
  private elements = new Set<HTMLMediaElement>();
  private mode: AudioEnhancement = 'off';

  static supported() { return typeof AudioContext !== 'undefined'; }

  /** True once this element's sound goes through Web Audio (it can never leave it). */
  requiresSameOrigin(element: HTMLMediaElement) { return this.chains.has(element); }

  current() { return this.mode; }

  /** Same-origin or MediaSource sources only: anything else would play silence. */
  static safeSource(element: HTMLMediaElement) {
    const url = element.currentSrc || element.src;
    if (!url || url.startsWith('blob:')) return true;
    try { return new URL(url, location.href).origin === location.origin; } catch { return false; }
  }

  /** Applies `mode` to `element`; returns false when this element cannot be processed. */
  apply(element: HTMLMediaElement, mode: AudioEnhancement): boolean {
    this.mode = mode;
    if (mode === 'off' && !this.chains.has(element)) return true;
    if (!AudioEnhancer.supported()) return mode === 'off';
    let chain = this.chains.get(element);
    if (!chain) {
      if (!AudioEnhancer.safeSource(element)) return false;
      this.context ??= new AudioContext({ latencyHint: 'playback' });
      const context = this.context;
      const destination = context.destination;
      destination.channelCount = Math.max(2, destination.maxChannelCount);
      destination.channelInterpretation = 'discrete';
      const source = context.createMediaElementSource(element);
      const compressor = context.createDynamicsCompressor();
      const gain = context.createGain();
      for (const node of [compressor, gain]) { node.channelCountMode = 'max'; node.channelInterpretation = 'discrete'; }
      source.connect(compressor).connect(gain).connect(destination);
      chain = { source, compressor, gain };
      this.chains.set(element, chain);
      this.elements.add(element);
    }
    const settings = enhancementSettings(mode);
    const now = this.context!.currentTime;
    chain.compressor.threshold.setValueAtTime(settings.threshold, now);
    chain.compressor.knee.setValueAtTime(settings.knee, now);
    chain.compressor.ratio.setValueAtTime(settings.ratio, now);
    chain.compressor.attack.setValueAtTime(settings.attack, now);
    chain.compressor.release.setValueAtTime(settings.release, now);
    chain.gain.gain.setTargetAtTime(settings.gain, now, 0.05);
    // Autoplay policies start contexts suspended; the play gesture resumes it.
    if (this.context!.state === 'suspended') void this.context!.resume().catch(() => {});
    return true;
  }

  /** Re-applies the current mode to every element still attached to the page. */
  refresh() {
    for (const element of this.elements) {
      if (!element.isConnected) { this.elements.delete(element); continue; }
      this.apply(element, this.mode);
    }
  }

  resume() { if (this.context?.state === 'suspended') void this.context.resume().catch(() => {}); }
}

/**
 * One enhancer for the page. An element handed to `createMediaElementSource` stays bound to
 * that context for its whole life, and the player reuses its video element between sessions:
 * closing the context would mute it for good, so this one is never closed.
 */
export const pageAudio = new AudioEnhancer();

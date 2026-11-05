import { get } from 'svelte/store';
import { session, getSessionGeneration, onSessionGenerationChange } from '../auth/session';

export type Origin = 'manual' | 'automatic' | 'sync';
export type EventKind = 'start' | 'heartbeat' | 'pause' | 'resume' | 'seek' | 'buffering' | 'buffered' | 'audio_change' | 'subtitle_change' | 'speed_change' | 'source_change' | 'skip_intro' | 'skip_credits' | 'skip_recap' | 'skip_preview' | 'stop' | 'ended' | 'error';
export interface TrackInfo { id: string; language: string; codec: string }
export interface Snapshot { position: number; duration: number; paused: boolean; speed: number }
export interface PlaybackInfo { method: 'direct' | 'remux' | 'separate_audio' | 'prepared' | 'unknown'; audio?: TrackInfo; mediaId?: string }
interface Sample {
  sequence: number; kind: EventKind; elapsedMs: number; position: number; duration: number;
  playing: boolean; buffering: boolean; rate: number; origin: Origin;
  fromPosition?: number; previousTrack?: TrackInfo; track?: TrackInfo;
  method?: PlaybackInfo['method']; errorCode?: string;
  sourceMediaId?: string; stopReason?: 'closed' | 'next' | 'previous' | 'queue_change';
}
interface Options {
  mediaId: string; music: boolean; state?: () => Snapshot | undefined;
  origin?: () => Origin; info?: () => PlaybackInfo | undefined;
}

/** Bounded, memory-only delivery. Every asynchronous operation is fenced to its account. */
export class PlaybackCollector {
  private generation = getSessionGeneration();
  private epoch = performance.now();
  private id = crypto.randomUUID();
  private sequence = 0;
  private queue: Sample[] = [];
  private enabled = false;
  private created = false;
  private closed = false;
  private abandoned = false;
  private buffering = true;
  private flushing = false;
  private starting = false;
  private lastContact = performance.now();
  private audio?: TrackInfo;
  private subtitle?: TrackInfo;
  private listeners: (() => void)[] = [];
  private timer: ReturnType<typeof setInterval>;
  private unwatch: () => void;

  constructor(private element: HTMLMediaElement, private options: Options) {
    this.listen('playing', () => { this.buffering = false; this.event('resume'); });
    this.listen('canplay', () => { if (this.buffering && !element.paused) { this.buffering = false; this.event('buffered'); } });
    this.listen('pause', () => { if (!element.ended) this.event('pause'); });
    this.listen('waiting', () => { if (!element.paused && !this.buffering) { this.buffering = true; this.event('buffering'); } });
    this.listen('error', () => this.event('error', { errorCode: ({ 1: 'aborted', 2: 'network', 3: 'decode', 4: 'unsupported' } as Record<number, string>)[element.error?.code ?? 0] ?? 'playback' }));
    this.unwatch = onSessionGenerationChange(() => this.abandon());
    this.timer = setInterval(() => {
      if (this.closed) return;
      if (!this.enabled) void this.start();
      else { this.event('heartbeat'); void this.flush(); }
    }, 15000);
    const hide = () => { this.event('heartbeat'); void this.flush(true); };
    const visibility = () => { if (document.visibilityState === 'hidden') hide(); };
    window.addEventListener('pagehide', hide);
    document.addEventListener('visibilitychange', visibility);
    this.listeners.push(() => window.removeEventListener('pagehide', hide), () => document.removeEventListener('visibilitychange', visibility));
    void this.start();
  }
  private listen(name: string, listener: () => void) {
    this.element.addEventListener(name, listener);
    this.listeners.push(() => this.element.removeEventListener(name, listener));
  }
  private valid() { return !this.abandoned && this.generation === getSessionGeneration() && !!get(session); }
  private async send(path: string, body?: unknown, keepalive = false) {
    const account = get(session);
    if (!this.valid() || !account) throw new Error('account_changed');
    return fetch(path, {
      method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin', cache: 'no-store',
      signal: AbortSignal.timeout(2500), keepalive,
      headers: body === undefined ? undefined : { 'content-type': 'application/json', 'x-csrf-token': account.csrfToken },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
  }
  private async start() {
    if (this.starting || !this.valid() || this.closed) return;
    this.starting = true;
    try {
      const config = await this.send('/api/v1/playback/statistics');
      if (!config.ok || !this.valid() || this.closed) return;
      const settings = await config.json();
      if (!settings.enabled || !this.valid() || this.closed) return;
      this.enabled = true;
      this.epoch = performance.now(); this.lastContact = this.epoch;
      this.event('start');
      const info = this.options.info?.();
      if (info?.audio && (!info.mediaId || info.mediaId === this.options.mediaId)) this.track('audio', info.audio, 'automatic');
      if (this.subtitle) { const subtitle = this.subtitle; this.subtitle = undefined; this.track('subtitle', subtitle, 'automatic'); }
      const id = this.id;
      const response = await this.send('/api/v1/playback/sessions', { id, mediaId: this.options.mediaId, music: this.options.music });
      if (!this.valid() || this.id !== id) return;
      if (!response.ok) { this.reset(); return; }
      this.created = true;
      void this.flush(this.closed);
    } catch { this.reset(); }
    finally { this.starting = false; }
  }
  private reset() {
    this.enabled = false; this.created = false; this.queue = []; this.sequence = 0;
    this.id = crypto.randomUUID(); this.audio = undefined;
  }
  event(kind: EventKind, extra: Partial<Sample> = {}) {
    if (!this.valid() || this.closed || !this.enabled) return;
    if (performance.now() - this.lastContact > 90000 || this.queue.length >= 100) { this.reset(); return; }
    const state = this.options.state?.() ?? {
      position: this.element.currentTime, duration: this.element.duration,
      paused: this.element.paused, speed: this.element.playbackRate
    };
    if (!state || !Number.isFinite(state.position)) return;
    const elapsedMs = performance.now() - this.epoch;
    if (elapsedMs > 604800000) { this.reset(); return; }
    const method = this.options.info?.()?.method;
    this.queue.push({ sequence: ++this.sequence, kind, elapsedMs, position: Math.max(0, state.position),
      duration: Number.isFinite(state.duration) && state.duration > 0 ? state.duration : 0,
      playing: !state.paused && !this.element.ended, buffering: this.buffering,
      rate: Math.min(4, Math.max(.25, state.speed || 1)), origin: this.options.origin?.() ?? 'manual', method, ...extra });
    // Semantic transitions are delivered immediately; periodic samples remain bounded.
    if (kind !== 'heartbeat') void this.flush();
  }
  track(kind: 'audio' | 'subtitle', track: TrackInfo, origin: Origin) {
    const previousTrack = kind === 'audio' ? this.audio : this.subtitle;
    if (previousTrack?.id === track.id && previousTrack.language === track.language) return;
    if (kind === 'audio') this.audio = track; else this.subtitle = track;
    this.event(kind === 'audio' ? 'audio_change' : 'subtitle_change', { previousTrack, track, origin });
  }
  seek(position: number, fromPosition: number, kind: EventKind = 'seek', origin?: Origin) {
    this.event(kind, { position, fromPosition, origin: origin ?? this.options.origin?.() ?? 'manual' });
  }
  suspend() { this.event('buffering', { playing: false, buffering: true }); this.buffering = true; }
  source(info: PlaybackInfo | undefined, mediaId?: string) {
    this.event('source_change', { method: info?.method, origin: 'automatic', sourceMediaId: mediaId });
    if (mediaId) this.options.mediaId = mediaId;
    if (info?.audio) this.track('audio', info.audio, 'automatic');
  }
  private async flush(keepalive = false) {
    if (this.flushing || !this.created || !this.queue.length || !this.valid()) return;
    this.flushing = true;
    const batch = this.queue.slice(0, 100);
    const id = this.id;
    try {
      const response = await this.send(`/api/v1/playback/sessions/${id}/events`, { events: batch }, keepalive);
      if (!this.valid() || this.id !== id) return;
      if (response.ok) {
        this.lastContact = performance.now();
        this.queue.splice(0, batch.length);
      } else if (response.status >= 400 && response.status < 500 && response.status !== 429) this.reset();
    } catch { /* A bounded retry on the next heartbeat never affects playback. */ }
    finally { this.flushing = false; }
    if (this.closed && this.queue.length && this.created && this.valid()) {
      // At most one more batch: transitions appended while the first request was in flight.
      const remaining = this.queue.splice(0, 100);
      try { await this.send(`/api/v1/playback/sessions/${this.id}/events`, { events: remaining }, true); } catch { /* best effort */ }
    }
  }
  stop(ended = false, stopReason: Sample['stopReason'] = 'closed') {
    if (this.closed) return;
    this.event(ended ? 'ended' : 'stop', { playing: false, stopReason });
    this.closed = true; clearInterval(this.timer); this.unwatch();
    this.listeners.splice(0).forEach(remove => remove());
    void this.flush(true);
  }
  private abandon() {
    this.abandoned = true; this.closed = true; this.queue = [];
    clearInterval(this.timer); this.unwatch(); this.listeners.splice(0).forEach(remove => remove());
  }
}

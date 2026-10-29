import { waitForStartupBuffer } from './startup-buffer';

// Preserve encoded audio and channels; never route production audio through a stereo WebAudio graph.
// Independent clocks remain client-dependent; seeks and buffering are coordinated explicitly.
export class ExternalAudio {
  // One maximum-duration Opus packet (120 ms) plus container rounding.
  private static readonly END_TOLERANCE = .15;
  protected audio = new Audio();
  private timer?: ReturnType<typeof setInterval>;
  private stallTimer?: ReturnType<typeof setTimeout>;
  private cancellations = new Set<() => void>();
  private stopped = false;
  private startup?: AbortController;
  private playbackWait?: AbortController;
  private active = false;
  private waiting = false;
  private failed = false;
  private holdingVideo = false;
  private starting = false;
  private previousMuted = false;
  private resumeEpoch = 0;

  constructor(private video: HTMLVideoElement, private offset: number, private onError: (message: string) => void) {
    this.audio.preload = 'auto';
    this.audio.preservesPitch = true;
    this.audio.hidden = true; this.audio.dataset.externalAudio = 'true';
    video.insertAdjacentElement('afterend', this.audio);
    this.audio.addEventListener('error', this.error);
    this.audio.addEventListener('ended', this.ended);
    this.audio.addEventListener('waiting', this.audioWaiting);
    this.audio.addEventListener('stalled', this.audioStalled);
    this.audio.addEventListener('canplay', this.recover);
    this.audio.addEventListener('seeked', this.recover);
  }
  private clearStall() { clearTimeout(this.stallTimer); this.stallTimer = undefined; }
  private error = () => {
    if (this.stopped || this.failed || !this.active) return;
    this.failed = true; this.holdingVideo = false; this.clearStall();
    this.video.pause(); this.audio.pause();
    this.onError('Audio externe interrompu : source indisponible, incompatible ou bloquée pendant 10 secondes.');
  };
  private videoPause = () => { this.audio.pause(); if (!this.holdingVideo) this.clearStall(); };
  pause() { this.startup?.abort(); this.playbackWait?.abort(); this.resumeEpoch++; this.holdingVideo = false; this.clearStall(); this.audio.pause(); }
  isBuffering() { return this.holdingVideo; }
  private naturalTail() {
    const target = this.video.currentTime - this.offset;
    // Compare encoded durations; an explicit offset may intentionally make
    // audio finish earlier than video and must not become a truncation error.
    const gap = this.video.duration - this.audio.duration;
    return Number.isFinite(gap) && this.audio.duration > 0 &&
      this.audio.duration + this.offset > 0 &&
      Math.abs(gap) <= ExternalAudio.END_TOLERANCE &&
      this.video.duration - target <= ExternalAudio.END_TOLERANCE &&
      target >= this.audio.duration - .03;
  }
  private ended = () => {
    if (this.naturalTail()) { this.recover(); return; }
    if (this.video.duration - this.audio.duration > ExternalAudio.END_TOLERANCE) this.error();
  };
  private wait = () => { this.resumeEpoch++; this.waiting = true; this.audio.pause(); };
  private resume = () => { this.waiting = false; this.resync(); };
  private afterSeek = () => {
    this.waiting = false;
    if (!this.active || this.failed || this.stopped) return;
    if (!this.video.paused) this.holdVideo();
    this.align(true); this.recover();
  };
  private settings = () => { this.audio.volume = this.video.volume; this.audio.playbackRate = this.video.playbackRate; };
  private holdVideo() {
    if (this.holdingVideo || this.stopped || this.failed) return;
    this.holdingVideo = true;
    this.video.pause(); this.audio.pause();
    this.stallTimer = setTimeout(this.error, 10000);
  }
  private audioWaiting = () => {
    if (this.naturalTail()) { this.audio.pause(); this.recover(); return; }
    if (this.active && !this.audio.seeking && !this.video.seeking && !this.video.paused && !this.waiting) this.holdVideo();
  };
  private audioStalled = () => { if (this.audio.readyState < 3) this.audioWaiting(); };
  private recover = () => {
    if (!this.active || this.stopped || this.failed || this.audio.seeking || (this.audio.readyState < 2 && !this.naturalTail())) return;
    if (this.holdingVideo) {
      const epoch = this.resumeEpoch;
      this.holdingVideo = false; this.clearStall();
      void this.video.play().then(() => {
        if (!this.stopped && epoch === this.resumeEpoch) return this.play();
      }).catch(() => { if (epoch === this.resumeEpoch) this.error(); });
    }
  };
  private align(force = false, position = this.video.currentTime) {
    const target = position - this.offset;
    if (target < 0 || (Number.isFinite(this.audio.duration) && target >= this.audio.duration)) {
      this.audio.pause(); return false;
    }
    // A user seek supersedes an unfinished audio seek. The previous target's
    // window can already have been cancelled, so waiting for its seeked event
    // would deadlock both clocks. Periodic drift correction still avoids seeks
    // while one is pending.
    if ((force || !this.audio.seeking) && Math.abs(this.audio.currentTime - target) > (force ? .001 : .08)) this.audio.currentTime = target;
    return true;
  }
  private resync = () => {
    if (!this.active || this.stopped || this.failed || this.audio.readyState < 1 || this.waiting || this.holdingVideo) return;
    if (this.naturalTail()) { this.audio.pause(); return; }
    const target = this.video.currentTime - this.offset;
    const drift = this.audio.currentTime - target;
    if (!this.video.paused && !this.audio.seeking && Math.abs(drift) > .25 && target >= 0 && target < this.audio.duration) {
      this.holdVideo(); this.align(true); this.recover(); return;
    }
    if (!this.video.paused && !this.video.seeking && !this.audio.seeking && !this.audio.paused && target >= 0 && target < this.audio.duration) {
      // Small independent-clock drift must not pause the video every 100 ms.
      // A bounded, pitch-preserving correction converges without extra seeks.
      this.audio.playbackRate = this.video.playbackRate * (Math.abs(drift) > .04 ? (drift > 0 ? .98 : 1.02) : 1);
      return;
    }
    if (!this.align() || this.video.paused || this.video.seeking || this.audio.seeking || !this.audio.paused || this.starting) return;
    this.starting = true;
    void this.audio.play().catch(() => { if (!this.video.paused && !this.waiting) this.error(); }).finally(() => { this.starting = false; });
  };
  setOffset(seconds: number) {
    if (Number.isFinite(seconds) && Math.abs(seconds) <= 30) { this.offset = seconds; this.afterSeek(); }
  }
  private async ready(signal?: AbortSignal) {
    if (signal?.aborted) throw new Error('Démarrage annulé.');
    if (!this.audio.seeking && this.audio.readyState >= 2) return;
    await new Promise<void>((resolve, reject) => {
      const finish = (error?: Error) => {
        clearTimeout(timeout);
        this.audio.removeEventListener('seeked', check); this.audio.removeEventListener('canplay', check);
        this.audio.removeEventListener('error', failed); this.cancellations.delete(cancel);
        signal?.removeEventListener('abort', aborted);
        if (error) reject(error); else resolve();
      };
      const check = () => { if (!this.audio.seeking && this.audio.readyState >= 2) finish(); };
      const failed = () => finish(new Error('Audio externe Ogg/Opus illisible.'));
      const aborted = () => finish(new Error('Démarrage annulé.'));
      const timeout = setTimeout(() => finish(new Error('Chargement ou seek audio expiré après 10 secondes.')), 10000);
      const cancel = () => finish(new Error('Lecteur fermé.')); this.cancellations.add(cancel);
      this.audio.addEventListener('seeked', check); this.audio.addEventListener('canplay', check); this.audio.addEventListener('error', failed);
      signal?.addEventListener('abort', aborted, {once:true});
      check();
    });
  }
  async load(url: string, activate = true, mime = 'audio/ogg; codecs="opus"', initialPosition?: number) {
    const format = mime.startsWith('audio/mp4') ? 'MP4/Opus segmenté' : 'Ogg/Opus';
    if (!this.audio.canPlayType(mime)) throw new Error(`Ce navigateur ne déclare pas la lecture audio ${mime}.`);
    await new Promise<void>((resolve, reject) => {
      const finish = (error?: Error) => {
        clearTimeout(timeout); this.audio.removeEventListener('loadedmetadata', ready); this.audio.removeEventListener('error', failed); this.cancellations.delete(cancel);
        if (error) reject(error); else resolve();
      };
      const ready = () => finish();
      const failed = () => finish(new Error(`Audio externe ${format} illisible (MediaError ${this.audio.error?.code ?? '?'}).`));
      const timeout = setTimeout(() => finish(new Error('Chargement audio externe expiré après 10 secondes.')), 10000);
      const cancel = () => finish(new Error('Lecteur fermé.')); this.cancellations.add(cancel);
      this.audio.addEventListener('loadedmetadata', ready); this.audio.addEventListener('error', failed);
      this.audio.src = url; this.audio.load();
    });
    this.settings();
    const position = initialPosition ?? this.video.currentTime;
    if (position >= this.offset && Number.isFinite(this.audio.duration) && position - this.offset >= this.audio.duration && !this.naturalTail()) throw new Error('La piste audio externe se termine avant cette position.');
    this.align(true, position); if (!this.naturalTail()) await this.ready();
    if (this.stopped) throw new Error('Lecteur fermé.');
    if (activate) this.activate();
  }
  activate() {
    if (this.active || this.stopped) return;
    this.active = true; this.audio.dataset.activeAudio = 'true'; this.previousMuted = this.video.muted; this.video.muted = true;
    this.video.addEventListener('pause', this.videoPause); this.video.addEventListener('waiting', this.wait);
    this.video.addEventListener('seeking', this.wait); this.video.addEventListener('playing', this.resume);
    this.video.addEventListener('seeked', this.afterSeek); this.video.addEventListener('volumechange', this.settings);
    this.video.addEventListener('ratechange', this.settings);
    this.timer = setInterval(this.resync, 100);
  }
  assertReady() {
    if (this.stopped || this.failed) throw new Error('Piste audio interrompue.');
  }
  async bufferForPlayback() {
    if (this.naturalTail()) return;
    if (!this.align(true)) return;
    this.startup?.abort();
    const startup = new AbortController(); this.startup = startup;
    try {
      await this.ready(startup.signal);
      if (this.stopped || this.failed) throw new Error('Piste audio interrompue.');
      await waitForStartupBuffer(this.audio, startup.signal);
    }
    finally { if (this.startup === startup) this.startup = undefined; }
  }
  async play() {
    if (this.failed || this.stopped) throw new Error('Piste audio interrompue. Rechargez explicitement la source.');
    this.settings();
    if (this.naturalTail()) { this.audio.pause(); return; }
    if (this.video.currentTime < this.offset) { this.audio.pause(); return; }
    const target = this.video.currentTime - this.offset;
    if (Number.isFinite(this.audio.duration) && target >= this.audio.duration) throw new Error('La piste audio externe se termine avant cette position.');
    this.playbackWait?.abort();
    const waiting = new AbortController(); this.playbackWait = waiting;
    const epoch = this.resumeEpoch;
    try {
      this.align(); await this.ready(waiting.signal);
      if (this.stopped || this.failed) throw new Error('Piste audio interrompue.');
      if (epoch === this.resumeEpoch && !this.video.paused && !this.video.seeking) await this.audio.play();
    } finally { if (this.playbackWait === waiting) this.playbackWait = undefined; }
  }
  destroy() {
    if (this.stopped) return;
    this.stopped = true; this.startup?.abort(); this.playbackWait?.abort(); for (const cancel of this.cancellations) cancel(); this.cancellations.clear();
    clearInterval(this.timer); this.clearStall();
    this.video.removeEventListener('pause', this.videoPause); this.video.removeEventListener('waiting', this.wait); this.video.removeEventListener('seeking', this.wait);
    this.video.removeEventListener('playing', this.resume); this.video.removeEventListener('seeked', this.afterSeek);
    this.video.removeEventListener('volumechange', this.settings); this.video.removeEventListener('ratechange', this.settings);
    this.audio.removeEventListener('error', this.error); this.audio.removeEventListener('ended', this.ended);
    this.audio.removeEventListener('waiting', this.audioWaiting); this.audio.removeEventListener('stalled', this.audioStalled);
    this.audio.removeEventListener('canplay', this.recover); this.audio.removeEventListener('seeked', this.recover);
    this.audio.pause(); this.audio.removeAttribute('src'); this.audio.load(); this.audio.remove();
    if (this.active) this.video.muted = this.previousMuted;
  }
}

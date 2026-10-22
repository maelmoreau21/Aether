// Preserve encoded audio and channels; never route production audio through a stereo WebAudio graph.
// Independent clocks remain client-dependent; seeks and buffering are coordinated explicitly.
export class ExternalAudio {
  protected audio = new Audio();
  private timer?: ReturnType<typeof setInterval>;
  private stallTimer?: ReturnType<typeof setTimeout>;
  private cancellations = new Set<() => void>();
  private stopped = false;
  private active = false;
  private waiting = false;
  private failed = false;
  private holdingVideo = false;
  private starting = false;
  private previousMuted = false;
  private resumeEpoch = 0;

  constructor(private video: HTMLVideoElement, private offset: number, private onError: (message: string) => void) {
    this.audio.preload = 'auto';
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
  pause() { this.resumeEpoch++; this.holdingVideo = false; this.clearStall(); this.audio.pause(); }
  isBuffering() { return this.holdingVideo; }
  private ended = () => { if (this.video.duration - this.audio.duration > .5) this.error(); };
  private wait = () => { this.waiting = true; this.audio.pause(); };
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
    if (this.active && !this.audio.seeking && !this.video.seeking && !this.video.paused && !this.waiting) this.holdVideo();
  };
  private audioStalled = () => { if (this.audio.readyState < 3) this.audioWaiting(); };
  private recover = () => {
    if (!this.active || this.stopped || this.failed || this.audio.seeking || this.audio.readyState < 2) return;
    if (this.holdingVideo) {
      const epoch = this.resumeEpoch;
      this.holdingVideo = false; this.clearStall();
      void this.video.play().then(() => {
        if (!this.stopped && epoch === this.resumeEpoch) return this.play();
      }).catch(() => { if (epoch === this.resumeEpoch) this.error(); });
    }
  };
  private align(force = false) {
    const target = this.video.currentTime - this.offset;
    if (target < 0 || (Number.isFinite(this.audio.duration) && target >= this.audio.duration)) {
      this.audio.pause(); return false;
    }
    if (!this.audio.seeking && Math.abs(this.audio.currentTime - target) > (force ? .001 : .08)) this.audio.currentTime = target;
    return true;
  }
  private resync = () => {
    if (!this.active || this.stopped || this.failed || this.audio.readyState < 1 || this.waiting || this.holdingVideo) return;
    const target = this.video.currentTime - this.offset;
    if (!this.video.paused && !this.audio.seeking && Math.abs(this.audio.currentTime - target) > .08 && target >= 0 && target < this.audio.duration) {
      this.holdVideo(); this.align(true); this.recover(); return;
    }
    if (!this.align() || this.video.paused || this.video.seeking || this.audio.seeking || !this.audio.paused || this.starting) return;
    this.starting = true;
    void this.audio.play().catch(() => { if (!this.video.paused && !this.waiting) this.error(); }).finally(() => { this.starting = false; });
  };
  setOffset(seconds: number) {
    if (Number.isFinite(seconds) && Math.abs(seconds) <= 30) { this.offset = seconds; this.afterSeek(); }
  }
  private async ready() {
    if (!this.audio.seeking && this.audio.readyState >= 2) return;
    await new Promise<void>((resolve, reject) => {
      const finish = (error?: Error) => {
        clearTimeout(timeout);
        this.audio.removeEventListener('seeked', check); this.audio.removeEventListener('canplay', check);
        this.audio.removeEventListener('error', failed); this.cancellations.delete(cancel);
        if (error) reject(error); else resolve();
      };
      const check = () => { if (!this.audio.seeking && this.audio.readyState >= 2) finish(); };
      const failed = () => finish(new Error('Audio externe Ogg/Opus illisible.'));
      const timeout = setTimeout(() => finish(new Error('Chargement ou seek audio expiré après 10 secondes.')), 10000);
      const cancel = () => finish(new Error('Lecteur fermé.')); this.cancellations.add(cancel);
      this.audio.addEventListener('seeked', check); this.audio.addEventListener('canplay', check); this.audio.addEventListener('error', failed);
      check();
    });
  }
  async load(url: string, activate = true, mime = 'audio/ogg; codecs="opus"') {
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
    if (this.video.currentTime >= this.offset && Number.isFinite(this.audio.duration) && this.video.currentTime - this.offset >= this.audio.duration) throw new Error('La piste audio externe se termine avant cette position.');
    this.align(true); await this.ready();
    if (this.stopped) throw new Error('Lecteur fermé.');
    if (activate) this.activate();
  }
  activate() {
    if (this.active || this.stopped) return;
    this.active = true; this.previousMuted = this.video.muted; this.video.muted = true;
    this.video.addEventListener('pause', this.videoPause); this.video.addEventListener('waiting', this.wait);
    this.video.addEventListener('seeking', this.wait); this.video.addEventListener('playing', this.resume);
    this.video.addEventListener('seeked', this.afterSeek); this.video.addEventListener('volumechange', this.settings);
    this.video.addEventListener('ratechange', this.settings);
    this.timer = setInterval(this.resync, 100);
  }
  async play() {
    if (this.failed || this.stopped) throw new Error('Piste audio interrompue. Rechargez explicitement la source.');
    this.settings();
    if (this.video.currentTime < this.offset) { this.audio.pause(); return; }
    const target = this.video.currentTime - this.offset;
    if (Number.isFinite(this.audio.duration) && target >= this.audio.duration) throw new Error('La piste audio externe se termine avant cette position.');
    this.align(); await this.ready();
    if (this.stopped || this.failed) throw new Error('Piste audio interrompue.');
    if (!this.video.paused) await this.audio.play();
  }
  destroy() {
    if (this.stopped) return;
    this.stopped = true; for (const cancel of this.cancellations) cancel(); this.cancellations.clear();
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

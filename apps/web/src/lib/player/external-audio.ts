// Native independent audio: video timeline is the reference, with bounded drift correction.
// This is not a sample-accurate demuxer; unsupported containers/codecs fail explicitly.
export class ExternalAudio {
  private audio = new Audio();
  private timer?: ReturnType<typeof setInterval>;
  private cancelled?: () => void;
  private stopped = false;
  private waiting = false;
  private failed = false;
  constructor(private video: HTMLVideoElement, private offset: number, private onError: (message: string) => void) {
    this.audio.preload = 'auto';
    this.audio.hidden = true; this.audio.dataset.externalAudio = 'true';
    video.insertAdjacentElement('afterend', this.audio);
    this.audio.addEventListener('error', this.error);
    this.audio.addEventListener('ended', this.ended);
    video.addEventListener('pause', this.pause);
    video.addEventListener('waiting', this.wait);
    video.addEventListener('seeking', this.wait);
    video.addEventListener('playing', this.resume);
    video.addEventListener('seeked', this.afterSeek);
    video.addEventListener('volumechange', this.settings);
    video.addEventListener('ratechange', this.settings);
  }
  private error = () => {
    if (this.stopped || this.failed) return;
    this.failed = true;
    this.video.pause(); this.audio.pause();
    this.onError('Audio externe interrompu : source Ogg/Opus indisponible ou incompatible.');
  };
  private pause = () => this.audio.pause();
  private ended = () => { if (this.video.duration - this.video.currentTime > .5) this.error(); };
  private wait = () => { this.waiting = true; this.audio.pause(); };
  private resume = () => { this.waiting = false; this.resync(); };
  private afterSeek = () => { this.waiting = false; this.resync(); };
  private settings = () => { this.audio.volume = this.video.volume; this.audio.playbackRate = this.video.playbackRate; };
  private resync = () => {
    if (this.stopped || this.failed || this.audio.readyState < 1) return;
    const target = this.video.currentTime - this.offset;
    if (target < 0 || (Number.isFinite(this.audio.duration) && target >= this.audio.duration)) {
      this.audio.pause(); return;
    }
    if (Math.abs(this.audio.currentTime - target) > .15) this.audio.currentTime = target;
    if (!this.video.paused && !this.video.seeking && !this.waiting && this.audio.paused) void this.audio.play().catch(this.error);
  };
  setOffset(seconds: number) { if (Number.isFinite(seconds) && Math.abs(seconds) <= 30) { this.offset = seconds; this.resync(); } }
  async load(url: string) {
    if (!this.audio.canPlayType('audio/ogg; codecs="opus"')) throw new Error('Ce navigateur ne déclare pas la lecture Ogg/Opus externe.');
    await new Promise<void>((resolve, reject) => {
      const finish = (error?: Error) => {
        clearTimeout(timeout); this.audio.removeEventListener('loadedmetadata', ready); this.audio.removeEventListener('error', failed); this.cancelled = undefined;
        if (error) reject(error); else resolve();
      };
      const ready = () => finish();
      const failed = () => finish(new Error('Audio externe Ogg/Opus illisible.'));
      const timeout = setTimeout(() => finish(new Error('Chargement audio externe expiré après 10 secondes.')), 10000);
      this.cancelled = () => finish(new Error('Lecteur fermé.'));
      this.audio.addEventListener('loadedmetadata', ready); this.audio.addEventListener('error', failed);
      this.audio.src = url; this.audio.load();
    });
    this.video.muted = true;
    this.settings(); this.resync();
    this.timer = setInterval(this.resync, 100);
  }
  async play() {
    this.settings();
    if (this.video.currentTime < this.offset) { this.audio.pause(); return; }
    const target = Math.max(0, this.video.currentTime - this.offset);
    if (Number.isFinite(this.audio.duration) && target >= this.audio.duration) throw new Error('La piste audio externe se termine avant cette position.');
    if (Math.abs(this.audio.currentTime - target) > .15) this.audio.currentTime = target;
    await this.audio.play();
  }
  destroy() {
    this.stopped = true; this.cancelled?.(); clearInterval(this.timer);
    this.video.removeEventListener('pause', this.pause); this.video.removeEventListener('waiting', this.wait); this.video.removeEventListener('seeking', this.wait);
    this.video.removeEventListener('playing', this.resume); this.video.removeEventListener('seeked', this.afterSeek);
    this.video.removeEventListener('volumechange', this.settings); this.video.removeEventListener('ratechange', this.settings);
    this.audio.removeEventListener('error', this.error); this.audio.pause(); this.audio.removeAttribute('src'); this.audio.load();
    this.audio.remove();
    this.video.muted = false;
  }
}

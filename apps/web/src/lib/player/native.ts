import type { Media, Player, PlaybackState, Source } from './types';

// One finite source replacement. No retries, silent fallback or player-specific API in the UI.
export class NativePlayer implements Player {
  private media?: Media;
  private source?: Source;
  private subtitle: string | null = null;
  private busy = false;
  private destroyed = false;
  private cancel?: () => void;
  private stallTimer?: ReturnType<typeof setTimeout>;
  private clearStall = () => { clearTimeout(this.stallTimer); this.stallTimer = undefined; };
  private stalled = () => {
    if (this.video.paused || this.busy || this.stallTimer) return;
    this.stallTimer = setTimeout(() => {
      this.stallTimer = undefined;
      this.video.pause();
      this.onError('Lecture bloquée pendant 10 secondes. Rechargez explicitement la source.');
    }, 10000);
  };
  constructor(private video: HTMLVideoElement, private onError: (message: string) => void) {
    video.addEventListener('error', this.mediaError);
    video.addEventListener('waiting', this.stalled);
    video.addEventListener('stalled', this.stalled);
    video.addEventListener('playing', this.clearStall);
    video.addEventListener('pause', this.clearStall);
  }
  private mediaError = () => {
    this.video.pause();
    this.onError(`Lecture interrompue : ${this.video.error?.message || 'source indisponible ou codecs/conteneur incompatibles'} (MediaError ${this.video.error?.code ?? '?'})`);
  };
  state(): PlaybackState {
    return { position: this.video.currentTime, duration: this.video.duration, paused: this.video.paused, volume: this.video.volume, speed: this.video.playbackRate };
  }
  async load(media: Media) {
    this.media = media;
    this.subtitle = null;
    const source = media.sources.find(s => s.language === 'fr');
    if (!source) throw new Error('Aucune source française explicitement disponible.');
    await this.replace(source, 0, true);
  }
  private async replace(source: Source, position: number, paused: boolean) {
    if (this.busy || this.destroyed) throw new Error('Lecteur indisponible pendant le changement de source.');
    this.busy = true;
    this.source = undefined;
    const { volume, speed } = this.state();
    this.video.pause();
    this.clearStall();
    // Hide obsolete captions before the new resource is ready.
    this.video.querySelectorAll('track').forEach(track => track.remove());
    try {
      await new Promise<void>((resolve, reject) => {
        const finish = (error?: Error) => {
          clearTimeout(timer);
          this.video.removeEventListener('loadedmetadata', metadata);
          this.video.removeEventListener('seeked', seeked);
          this.video.removeEventListener('error', failed);
          this.cancel = undefined;
          if (error) reject(error); else resolve();
        };
        const seeked = () => finish();
        const metadata = () => {
          this.video.volume = volume;
          this.video.playbackRate = speed;
          const target = Math.min(position, Math.max(0, this.video.duration - 0.05));
          if (target > 0) this.video.currentTime = target;
          else finish();
        };
        const failed = () => finish(new Error(`Source ${source.language.toUpperCase()} illisible : réseau, conteneur ou codec non pris en charge.`));
        const timer = setTimeout(() => finish(new Error('Chargement ou seek expiré après 10 secondes. Réessayez explicitement.')), 10000);
        this.cancel = () => finish(new Error('Lecteur fermé.'));
        this.video.addEventListener('loadedmetadata', metadata);
        this.video.addEventListener('seeked', seeked);
        this.video.addEventListener('error', failed);
        this.video.src = source.url;
        this.video.load();
      });
      this.source = source;
      await this.selectSubtitle(this.subtitle);
      if (!paused) await this.play();
    } catch (error) {
      this.video.pause();
      this.source = undefined;
      throw error;
    } finally { this.busy = false; }
  }
  async selectAudio(id: string) {
    const source = this.media?.sources.find(s => s.audioTrackId === id);
    if (!source) throw new Error('Piste audio absente : aucun remplacement automatique.');
    if (source.id === this.source?.id) return;
    const state = this.state();
    await this.replace(source, state.position, state.paused);
  }
  async selectSubtitle(id: string | null) {
    this.subtitle = id;
    this.video.querySelectorAll('track').forEach(track => track.remove());
    if (id === null) return;
    const subtitle = this.media?.subtitles.find(s => s.id === id);
    if (!subtitle) throw new Error('Sous-titres indisponibles.');
    const track = document.createElement('track');
    track.kind = 'subtitles'; track.label = subtitle.label; track.srclang = subtitle.language;
    track.addEventListener('error', () => this.onError('Échec du chargement des sous-titres WebVTT.'), { once: true });
    track.src = subtitle.url;
    this.video.append(track);
    track.track.mode = 'showing';
  }
  async play() {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([this.video.play(), new Promise<never>((_, reject) => {
        timer = setTimeout(() => { this.video.pause(); reject(new Error('Démarrage de lecture expiré après 10 secondes.')); }, 10000);
      })]);
    } finally { clearTimeout(timer); }
  }
  pause() { this.video.pause(); }
  seek(position: number) { if (Number.isFinite(position)) this.video.currentTime = Math.max(0, Math.min(position, this.video.duration || 0)); }
  setVolume(volume: number) { this.video.volume = Math.max(0, Math.min(1, volume)); }
  setSpeed(speed: number) { if ([0.5, 1, 1.5, 2].includes(speed)) this.video.playbackRate = speed; }
  destroy() {
    this.destroyed = true;
    this.cancel?.();
    this.clearStall();
    this.video.removeEventListener('error', this.mediaError);
    this.video.removeEventListener('waiting', this.stalled);
    this.video.removeEventListener('stalled', this.stalled);
    this.video.removeEventListener('playing', this.clearStall);
    this.video.removeEventListener('pause', this.clearStall);
    this.video.pause(); this.video.removeAttribute('src'); this.video.load();
  }
}

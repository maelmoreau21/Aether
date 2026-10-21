import type { Media, Player, PlaybackState, Source } from './types';
import { ExternalAudio } from './external-audio';
import { chooseSource } from './source-selection';

// One finite source replacement. No retries, silent fallback or player-specific API in the UI.
export class NativePlayer implements Player {
  private media?: Media;
  private source?: Source;
  private externalAudio?: ExternalAudio;
  private pendingAudio?: ExternalAudio;
  private subtitle: string | null = null;
  private audioOffset = 0;
  private subtitleOffset = 0;
  private cueTimes = new WeakMap<TextTrackCue, [number, number]>();
  private preparations = new Set<AbortController>();
  private status:(message:string)=>void = () => {};
  setStatusListener(listener:(message:string)=>void) { this.status=listener; }
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
    if(this.busy)return;
    this.video.pause();
    this.externalAudio?.destroy(); this.externalAudio = undefined;
    this.source = undefined;
    this.onError(`Lecture interrompue : ${this.video.error?.message || 'source indisponible ou codecs/conteneur incompatibles'} (MediaError ${this.video.error?.code ?? '?'})`);
  };
  private audioError = (message: string) => {
    this.pause();
    this.externalAudio?.destroy(); this.externalAudio = undefined;
    this.source = undefined;
    this.onError(message);
  };
  state(): PlaybackState {
    return { position: this.video.currentTime, duration: this.video.duration, paused: this.video.paused && !this.externalAudio?.isBuffering(), volume: this.video.volume, speed: this.video.playbackRate };
  }
  async load(media: Media, initialAudioTrackId?: string) {
    this.media = media;
    this.subtitle = null;
    const source = chooseSource(media, initialAudioTrackId);
    if (!source) throw new Error('Aucune source disponible pour ce média.');
    await this.replace(source, 0, true);
  }
  private async prepareUrl(url: string) {
    const controller = new AbortController(); this.preparations.add(controller);
    this.status('Préparation des pistes sur le Media Node… Première préparation potentiellement longue.');
    const timeout = setTimeout(() => controller.abort(), 3610000);
    try {
      const response = await fetch(url, { method: 'HEAD', signal: controller.signal });
      if (!response.ok) {
        const messages: Record<string, string> = {
          preparation_busy: 'Le Media Node prépare déjà une autre piste. Réessayez une fois cette préparation terminée.',
          preparation_cache_limit_reached: 'La préparation dépasse la capacité du cache du Media Node. Augmentez sa limite ou libérez son cache.',
          preparation_timeout: 'La préparation a dépassé le délai configuré sur le Media Node.',
          preparation_tool_or_source_unavailable: 'FFmpeg ou la source est introuvable sur le Media Node.',
          unsupported_preparation: 'Le Media Node ne peut pas préparer ce format sans réencodage.',
          preparation_configuration_invalid: 'La configuration de préparation du Media Node est invalide.'
        };
        throw new Error(`Préparation impossible : ${messages[response.headers?.get('x-media-preparation-error') ?? ''] ?? 'source indisponible, format incompatible ou cache occupé/plein. Réessayez explicitement.'}`);
      }
    } finally { clearTimeout(timeout); this.preparations.delete(controller); }
    if (this.destroyed) throw new Error('Lecteur fermé.');
  }
  private async replace(source: Source, position: number, paused: boolean) {
    if (this.busy || this.destroyed) throw new Error('Lecteur indisponible pendant le changement de source.');
    this.busy = true;
    const previous = this.source;
    this.pause();
    try {
      if (source.prepare) {
        if (previous?.url !== source.url) await this.prepareUrl(source.url);
        if (source.externalAudioUrl) await this.prepareUrl(source.externalAudioUrl);
      }
      // Independent Opus track switch: keep the same video element, URL and timeline.
      if (previous?.url === source.url && source.externalAudioUrl) {
        const candidate = new ExternalAudio(this.video, (source.audioOffsetSeconds ?? 0) + this.audioOffset, this.audioError);
        this.pendingAudio = candidate;
        try {
          await candidate.load(source.externalAudioUrl, false);
          if (this.destroyed) throw new Error('Lecteur fermé.');
          this.externalAudio?.destroy();
          this.externalAudio = candidate;
          candidate.activate();
        } catch (error) { candidate.destroy(); throw error; }
        finally { if (this.pendingAudio === candidate) this.pendingAudio = undefined; }
        this.source = source;
        if (!paused) await this.play();
        return;
      }
    } catch (error) { for(const controller of this.preparations)controller.abort();this.busy = false; throw error; }
    finally { if (this.source === source) this.busy = false; }
    this.source = undefined;
    const { volume, speed } = this.state();
    this.video.pause();
    this.externalAudio?.destroy(); this.externalAudio = undefined;
    this.clearStall();
    // Hide obsolete captions before the new resource is ready.
    this.video.querySelectorAll('track').forEach(track => track.remove());
    const nowSecs = Math.floor(Date.now() / 1000);
    const hasFreshDirect = Boolean(
      source.directUrl && (!source.tokenExpiresAt || source.tokenExpiresAt > nowSecs + 5)
    );
    const primaryUrl = hasFreshDirect && source.directUrl ? source.directUrl : source.url;
    const fallbackUrl =
      hasFreshDirect && (source.relayUrl ?? source.url) !== primaryUrl
        ? (source.relayUrl ?? source.url)
        : undefined;

    const loadCandidate = (candidateUrl: string, timeoutMs=10000) =>
      new Promise<void>((resolve, reject) => {
        const finish = (error?: Error) => {
          clearTimeout(timer);
          this.video.removeEventListener('loadedmetadata', metadata);
          this.video.removeEventListener('seeked', seeked);
          this.video.removeEventListener('error', failed);
          this.cancel = undefined;
          if (error) reject(error);
          else resolve();
        };
        const seeked = () => finish();
        const metadata = () => {
          this.video.volume = volume;
          this.video.playbackRate = speed;
          const target = Math.min(position, Math.max(0, this.video.duration - 0.05));
          if (target > 0) this.video.currentTime = target;
          else finish();
        };
        const failed = () =>
          finish(
            new Error(
              `Source ${source.language.toUpperCase()} illisible : réseau, conteneur ou codec non pris en charge.`
            )
          );
        const timer = setTimeout(
          () =>
            finish(
              new Error('Délai de chargement ou de seek expiré. Réessayez explicitement.')
            ),
          timeoutMs
        );
        this.cancel = () => finish(new Error('Lecteur fermé.'));
        this.video.addEventListener('loadedmetadata', metadata);
        this.video.addEventListener('seeked', seeked);
        this.video.addEventListener('error', failed);
        this.video.src = candidateUrl;
        this.video.load();
      });

    try {
      try {
        this.status(hasFreshDirect ? 'Connexion directe au Media Node…' : 'Chargement de la vidéo…');
        await loadCandidate(primaryUrl, fallbackUrl ? 2500 : 10000);
      } catch (primaryErr) {
        if (fallbackUrl && !this.destroyed) {
          this.status('Node direct indisponible. Connexion au relais…');
          await loadCandidate(fallbackUrl,7500);
        } else {
          throw primaryErr;
        }
      }
      this.source = source;
      if (source.externalAudioUrl) {
        this.externalAudio = new ExternalAudio(this.video, (source.audioOffsetSeconds ?? 0) + this.audioOffset, this.audioError);
        await this.externalAudio.load(source.externalAudioUrl);
      }
      await this.selectSubtitle(this.subtitle);
      if (!paused) await this.play();
    } catch (error) {
      this.video.pause();
      this.externalAudio?.destroy(); this.externalAudio = undefined;
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
    track.addEventListener('load', () => {
      // Keep text above the custom controls instead of under their gradient.
      for (const cue of Array.from(track.track.cues ?? [])) {
        this.cueTimes.set(cue, [cue.startTime, cue.endTime]);
        this.shiftCue(cue);
        if (typeof VTTCue !== 'undefined' && cue instanceof VTTCue) { cue.snapToLines = false; cue.line = 70; }
      }
    }, { once: true });
    track.addEventListener('error', () => this.onError('Échec du chargement des sous-titres WebVTT.'), { once: true });
    if (subtitle.url.includes('/prepared/')) await this.prepareUrl(subtitle.url);
    if (this.destroyed || this.subtitle !== id) return;
    track.src = subtitle.url;
    this.video.append(track);
    track.track.mode = 'showing';
  }
  async play() {
    if (!this.source || this.destroyed) throw new Error('Source interrompue ou indisponible. Rechargez explicitement la source.');
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([(async () => {
        // Wait until video leaves seeking/buffering before starting independent audio.
        // Otherwise its waiting event can cancel the pending audio play promise.
        await this.video.play();
        if (this.externalAudio) await this.externalAudio.play();
      })(), new Promise<never>((_, reject) => {
        timer = setTimeout(() => { this.video.pause(); reject(new Error('Démarrage de lecture expiré après 10 secondes.')); }, 10000);
      })]);
    } catch (error) { this.pause(); throw error; }
    finally { clearTimeout(timer); }
  }
  pause() { this.externalAudio?.pause(); this.video.pause(); }
  seek(position: number) { if (Number.isFinite(position)) this.video.currentTime = Math.max(0, Math.min(position, this.video.duration || 0)); }
  setVolume(volume: number) { this.video.volume = Math.max(0, Math.min(1, volume)); }
  setAudioOffset(seconds: number) {
    if (!Number.isFinite(seconds) || Math.abs(seconds) > 30) return;
    this.audioOffset = seconds;
    this.externalAudio?.setOffset(seconds + (this.source?.audioOffsetSeconds ?? 0));
  }
  private shiftCue(cue: TextTrackCue) {
    const times = this.cueTimes.get(cue);
    if (!times) return;
    cue.startTime = Math.max(0, times[0] + this.subtitleOffset);
    cue.endTime = Math.max(cue.startTime + .001, times[1] + this.subtitleOffset);
  }
  setSubtitleOffset(seconds: number) {
    if (!Number.isFinite(seconds) || Math.abs(seconds) > 30) return;
    this.subtitleOffset = seconds;
    for (const track of Array.from(this.video.textTracks)) for (const cue of Array.from(track.cues ?? [])) this.shiftCue(cue);
  }
  setSpeed(speed: number) { if ([0.5, 1, 1.5, 2].includes(speed)) this.video.playbackRate = speed; }
  destroy() {
    this.destroyed = true;
    for(const controller of this.preparations)controller.abort();this.preparations.clear();
    this.cancel?.();
    this.pendingAudio?.destroy(); this.pendingAudio = undefined;
    this.clearStall();
    this.externalAudio?.destroy(); this.externalAudio = undefined;
    this.video.removeEventListener('error', this.mediaError);
    this.video.removeEventListener('waiting', this.stalled);
    this.video.removeEventListener('stalled', this.stalled);
    this.video.removeEventListener('playing', this.clearStall);
    this.video.removeEventListener('pause', this.clearStall);
    this.video.pause(); this.video.removeAttribute('src'); this.video.load();
  }
}

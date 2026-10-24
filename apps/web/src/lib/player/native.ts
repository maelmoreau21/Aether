import { waitForStartupBuffer } from './startup-buffer';
import type { Media, Player, PlaybackState, Source } from './types';
import { ExternalAudio } from './external-audio';
import { SegmentedAudio } from './segmented-audio';
import { ProgressiveVideo } from './progressive-video';
import { chooseSource } from './source-selection';
import { AudioSelectionRetainedError } from './errors';
import { unsupportedAudio } from './compatibility';

// One finite source replacement. No retries, silent fallback or player-specific API in the UI.
export class NativePlayer implements Player {
  private media?: Media;
  private source?: Source;
  private externalAudio?: ExternalAudio;
  private pendingAudio?: ExternalAudio;
  private progressiveVideo?: ProgressiveVideo;
  private subtitle: string | null = null;
  private audioOffset = 0;
  private subtitleOffset = 0;
  private cueTimes = new WeakMap<TextTrackCue, [number, number]>();
  private preparations = new Set<AbortController>();
  private status:(message:string)=>void = () => {};
  setStatusListener(listener:(message:string)=>void) { this.status=listener; }
  private busy = false;
  private destroyed = false;
  private startup?: AbortController;
  private pauseRevision = 0;
  private cancel?: () => void;
  private stallTimer?: ReturnType<typeof setTimeout>;
  private clearStall = () => { clearTimeout(this.stallTimer); this.stallTimer = undefined; };
  private stalled = () => {
    if (this.video.paused || this.busy || this.stallTimer) return;
    this.stallTimer = setTimeout(() => {
      this.stallTimer = undefined;
      this.video.pause();
      this.progressiveVideo?.destroy();this.progressiveVideo=undefined;
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
    this.progressiveVideo?.destroy();this.progressiveVideo=undefined;
    this.externalAudio?.destroy(); this.externalAudio = undefined;
    this.source = undefined;
    this.onError(`Lecture interrompue : ${this.video.error?.message || 'source indisponible ou codecs/conteneur incompatibles'} (MediaError ${this.video.error?.code ?? '?'})`);
  };
  private audioError = (message: string) => {
    if(this.source)message=`Piste ${this.source.language.toUpperCase()} (${this.source.audioTrackId}) : ${message}`;
    this.pause();
    this.progressiveVideo?.destroy();this.progressiveVideo=undefined;
    this.externalAudio?.destroy(); this.externalAudio = undefined;
    this.source = undefined;
    this.onError(message);
  };
  state(): PlaybackState {
    return { position: this.video.currentTime, duration: this.video.duration, paused: this.video.paused && !this.externalAudio?.isBuffering(), volume: this.video.volume, speed: this.video.playbackRate };
  }
  async load(media: Media, initialAudioTrackId?: string, initialPosition: number | Promise<number> = 0) {
    this.media = media;
    this.subtitle = null;
    const source = chooseSource(media, initialAudioTrackId);
    if (!source) throw new Error('Aucune source disponible pour ce média.');
    try { await this.replace(source, initialPosition, true); }
    catch(error) { throw new Error(`Piste ${source.language.toUpperCase()} (${source.audioTrackId}) : ${error instanceof Error ? error.message : String(error)}`); }
  }
  private async prepareUrl(url: string) {
    const controller = new AbortController(); this.preparations.add(controller);
    this.status('Préparation des pistes sur le Media Node… Première préparation potentiellement longue.');
    const timeout = setTimeout(() => controller.abort(), 3610000);
    try {
      const response = await fetch(url, { method: 'HEAD', signal: controller.signal });
      if (!response.ok) {
        let detail = response.headers?.get('x-media-preparation-error');
        if (!detail) {
          try {
            const body = await response.clone().json();
            if (body && typeof body.error === 'string') detail = body.error;
          } catch {}
        }
        const messages: Record<string, string> = {
          node_unavailable: 'Media Node indisponible ou déconnecté',
          node_revoked: 'Media Node révoqué',
          asset_changed_rescan_required: 'Fichier source introuvable ou modifié sur disque (réanalyse requise)',
          source_changed_rescan_required: 'Fichier source modifié sur disque (réanalyse requise)',
          preparation_busy: 'Le Media Node prépare déjà une autre piste. Réessayez une fois cette préparation terminée.',
          preparation_cache_limit_reached: 'La préparation dépasse la capacité du cache du Media Node. Augmentez sa limite ou libérez son cache.',
          preparation_timeout: 'La préparation a dépassé le délai configuré sur le Media Node.',
          preparation_tool_or_source_unavailable: 'FFmpeg ou la source est introuvable sur le Media Node.',
          unsupported_preparation: 'Le Media Node ne peut pas préparer ce format sans réencodage.',
          preparation_configuration_invalid: 'La configuration de préparation du Media Node est invalide.',
          video_stream_capacity_reached: 'Capacité de flux vidéo simultanés atteinte'
        };
        const hint = messages[detail ?? ''] ?? detail ?? 'source indisponible, format incompatible ou cache occupé/plein. Réessayez explicitement.';
        throw new Error(`Préparation impossible : ${hint}`);
      }
    } finally { clearTimeout(timeout); this.preparations.delete(controller); }
    if (this.destroyed) throw new Error('Lecteur fermé.');
  }
  private async replace(source: Source, position: number | Promise<number>, paused: boolean) {
    if (this.busy || this.destroyed) throw new Error('Lecteur indisponible pendant le changement de source.');
    const unsupported=unsupportedAudio(source,mime=>this.video.canPlayType(mime));
    if (unsupported) {
      console.info('[media playback]',{stage:'capability-rejected',reason:'unsupported-dts',track: /^internal-(default|\d+)$/.test(source.audioTrackId) ? source.audioTrackId : 'external'});
      throw new Error(unsupported);
    }
    this.busy = true;
    const previous = this.source;
    const independentSwitch = previous?.url === source.url && Boolean(source.externalAudioUrl);
    let pausedForReplacement = false;
    let resumeReplacement = false;
    let replacementPauseRevision: number | undefined;
    const segmented = Boolean(source.audioSegmentUrl && SegmentedAudio.supported());
    const audioUrl = segmented ? source.audioSegmentUrl! : source.externalAudioUrl;
    const createAudio = () => segmented
      ? new SegmentedAudio(this.video, (source.audioOffsetSeconds ?? 0) + this.audioOffset, this.audioError, {mode: source.audioSegmentMode, trackId: source.audioTrackId})
      : new ExternalAudio(this.video, (source.audioOffsetSeconds ?? 0) + this.audioOffset, this.audioError);
    // Keep the current presentation running while the replacement audio is
    // prepared. A cold Node preparation must not freeze an otherwise healthy
    // video; the commit below still aligns both clocks before resuming.
    if (!independentSwitch) { this.pause(); pausedForReplacement = true; }
    try {
      if (source.prepare) {
        if (!source.videoStreamUrl && source.prepareVideo !== false && previous?.url !== source.url) await this.prepareUrl(source.url);
        if (!segmented && source.externalAudioUrl) await this.prepareUrl(source.externalAudioUrl);
      }
      // Independent Opus track switch: keep the same video element, URL and timeline.
      if (previous?.url === source.url && source.externalAudioUrl) {
        const candidate = createAudio();
        this.pendingAudio = candidate;
        try {
          this.status(`Chargement audio ${source.language.toUpperCase()} (${source.audioTrackId})${segmented ? ' par segments' : ''}…`);
          await candidate.load(audioUrl!, false);
          if (this.destroyed) throw new Error('Lecteur fermé.');
          resumeReplacement = !this.state().paused;
          this.pause(); pausedForReplacement = true;
          replacementPauseRevision = this.pauseRevision;
          // The video may have advanced during preparation. Buffer at its
          // current position before discarding the previous usable track.
          await candidate.bufferForPlayback();
          if (this.destroyed) throw new Error('Lecteur fermé.');
          // Validate the candidate and commit synchronously: a preparation
          // failure queued as buffering resolved must retain the old track.
          candidate.assertReady();
          this.externalAudio?.destroy();
          this.externalAudio = candidate;
          candidate.activate();
          this.source = source;
          if (resumeReplacement && replacementPauseRevision === this.pauseRevision) await this.play();
        } catch (error) {
          // Once committed, this is the current source. A rejected play()
          // leaves it paused and retryable rather than pointing at destroyed audio.
          if (this.externalAudio !== candidate) candidate.destroy();
          throw error;
        }
        finally { if (this.pendingAudio === candidate) this.pendingAudio = undefined; }
        return;
      }
    } catch (error) {
      for(const controller of this.preparations)controller.abort();
      try {
        // A rejected candidate must restore the presentation, not just retain its URL.
        const resumePrevious = independentSwitch
          ? resumeReplacement && replacementPauseRevision === this.pauseRevision
          : !paused;
        if (!this.destroyed && previous && this.source === previous && pausedForReplacement && resumePrevious) {
          try { await this.play(); }
          catch (resumeError) {
            throw new Error(`${error instanceof Error ? error.message : String(error)} Ancienne piste conservée mais reprise impossible : ${resumeError instanceof Error ? resumeError.message : String(resumeError)}`);
          }
        }
        throw error;
      } finally { this.busy = false; }
    }
    finally { if (this.source === source) this.busy = false; }
    this.source = undefined;
    const { volume, speed } = this.state();
    this.video.pause();
    this.progressiveVideo?.destroy();this.progressiveVideo=undefined;
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
        let settled = false;
        const finish = (error?: Error) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          this.video.removeEventListener('loadedmetadata', metadata);
          this.video.removeEventListener('seeked', seeked);
          this.video.removeEventListener('error', failed);
          this.cancel = undefined;
          if (error) {
            console.info('[media playback]',{stage:metadataReceived ? 'seek-failed' : 'metadata-failed',networkState:this.video.networkState,readyState:this.video.readyState,mediaError:this.video.error?.code ?? null});
            reject(error);
          }
          else resolve();
        };
        const seeked = () => finish();
        let metadataReceived = false;
        const metadata = () => {
          metadataReceived = true;
          this.video.volume = volume;
          this.video.playbackRate = speed;
          // Resolve saved progress alongside metadata, before audio windows load.
          // Starting segmented audio at zero then seeking copied two windows and
          // raced the startup buffer against the newly requested window.
          void Promise.resolve(position).then(position => {
            if (settled || this.destroyed) return;
            const target = Number.isFinite(position) ? Math.max(0, Math.min(position, Math.max(0, this.video.duration - 0.05))) : 0;
            if (target > 0 && Math.abs(this.video.currentTime - target) > .001) this.video.currentTime = target;
            else finish();
          }, error => finish(error instanceof Error ? error : new Error(String(error))));
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
              new Error(`Délai ${metadataReceived ? 'de seek' : 'de lecture des métadonnées'} expiré après ${timeoutMs / 1000} s (networkState=${this.video.networkState}, readyState=${this.video.readyState}, MediaError=${this.video.error?.code ?? 'aucune'}). ${metadataReceived ? 'Position demandée non atteinte.' : 'Index du conteneur ou données vidéo non reçus/lisibles.'} Cause codec, conteneur ou transport non déterminée ; consulter les diagnostics HTTP du navigateur et du Media Node.`)
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

    const loadVideo = async (url: string, timeoutMs: number) => {
      await loadCandidate(url, timeoutMs);
      if (!source.preparedVideoUrl && !source.videoStreamUrl) return;
      // Matroska acceptance must include decoded data at the requested position,
      // not just a MIME hint or a successful container metadata parse.
      const probe = new AbortController(); this.preparations.add(probe);
      try { await waitForStartupBuffer(this.video, probe.signal); }
      finally { this.preparations.delete(probe); }
    };

    try {
      // Load independent, already-prepared Ogg audio while video metadata arrives.
      // Keep it inactive until both resources are ready; segmented Opus needs the
      // video's duration/index and retains its ordered initialization.
      let audioLoading: Promise<void> | undefined;
      if (source.externalAudioUrl && segmented && Number.isFinite(source.durationSeconds) && source.durationSeconds! > 0) {
        const audio = new SegmentedAudio(this.video, (source.audioOffsetSeconds ?? 0) + this.audioOffset, this.audioError, {mode: source.audioSegmentMode, duration: source.durationSeconds, trackId: source.audioTrackId});
        this.externalAudio = audio;
        audioLoading = audio.preload(audioUrl!, position);
        void audioLoading.catch(() => {});
      } else if (source.externalAudioUrl && !segmented) {
        this.externalAudio = createAudio();
        audioLoading = this.externalAudio.load(audioUrl!, false);
        void audioLoading.catch(() => {});
      }
      try {
        this.status(hasFreshDirect ? 'Connexion directe au Media Node…' : 'Chargement de la vidéo…');
        try {
          await loadVideo(primaryUrl, fallbackUrl ? 2500 : 10000);
        } catch (primaryErr) {
          if (fallbackUrl && !this.destroyed) {
            this.status('Node direct indisponible. Connexion au relais…');
            await loadVideo(fallbackUrl,7500);
          } else {
            throw primaryErr;
          }
        }
      } catch (originalError) {
        if (this.destroyed) throw originalError;
        if(source.videoStreamUrl && source.videoMime && Number.isFinite(source.durationSeconds) && source.durationSeconds!>0 && ProgressiveVideo.supported(source.videoMime)) {
          this.status('Vidéo originale non lisible. Lecture progressive sans réencodage…');
          const progressive=new ProgressiveVideo(this.video,source.videoStreamUrl,source.durationSeconds!,source.videoMime,message=>{
            this.pause();this.externalAudio?.destroy();this.externalAudio=undefined;this.progressiveVideo?.destroy();this.progressiveVideo=undefined;this.source=undefined;this.onError(message);
          });
          this.progressiveVideo=progressive;
          try {await Promise.race([Promise.all([progressive.load(position),loadCandidate(progressive.url)]),progressive.failure]);progressive.activate();}
          catch(error){
            this.cancel?.();progressive.destroy();if(this.progressiveVideo===progressive)this.progressiveVideo=undefined;
            if(source.preparedVideoUrl && !this.destroyed) {
              console.warn('[media playback] Progressive video failed, falling back to prepared remux:', error);
              this.status('Lecture progressive indisponible. Préparation sans réencodage…');
              await this.prepareUrl(source.preparedVideoUrl);
              await loadCandidate(source.preparedVideoUrl);
            } else {
              throw error;
            }
          }
        } else {
          if(source.videoStreamUrl)throw new Error(`La lecture vidéo progressive n’est pas disponible pour ce navigateur ou ce descripteur (${source.videoMime??'codec MSE absent'}). Aucun remux intégral automatique.`);
          if(!source.preparedVideoUrl)throw originalError;
          this.status('Vidéo originale non lisible sur ce navigateur. Préparation sans réencodage…');
          await this.prepareUrl(source.preparedVideoUrl);
          await loadCandidate(source.preparedVideoUrl);
        }
      }
      this.source = source;
      if (source.externalAudioUrl) {
        this.status(`Chargement audio ${source.language.toUpperCase()} (${source.audioTrackId})${segmented ? ' par segments' : ''}…`);
        if (audioLoading) {
          await audioLoading;
          if (this.destroyed) throw new Error('Lecteur fermé.');
          this.externalAudio?.activate();
        } else {
          this.externalAudio = createAudio();
          await this.externalAudio.load(audioUrl!);
        }
      }
      await this.selectSubtitle(this.subtitle);
      if (!paused) await this.play();
    } catch (error) {
      this.video.pause();
      this.progressiveVideo?.destroy();this.progressiveVideo=undefined;
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
    const previous = this.source;
    try { await this.replace(source, state.position, state.paused); }
    catch(error) {
      const message = `Piste ${source.language.toUpperCase()} (${source.audioTrackId}) : ${error instanceof Error ? error.message : String(error)}`;
      if (!this.destroyed && !this.busy && previous && this.source === previous && this.video.paused === state.paused) throw new AudioSelectionRetainedError(message);
      throw new Error(message);
    }
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
    this.startup?.abort();
    const startup = new AbortController(); this.startup = startup;
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([(async () => {
        this.status('Mise en mémoire tampon…');
        await Promise.all([waitForStartupBuffer(this.video, startup.signal), this.externalAudio?.bufferForPlayback()]);
        if (startup.signal.aborted || this.destroyed) throw new Error('Démarrage annulé.');
        // Wait until video leaves seeking/buffering before starting independent audio.
        // Otherwise its waiting event can cancel the pending audio play promise.
        await this.video.play();
        if (startup.signal.aborted || this.destroyed) throw new Error('Démarrage annulé.');
        if (this.externalAudio) {
          await this.externalAudio.play();
          if (startup.signal.aborted || this.destroyed) throw new Error('Démarrage annulé.');
        }
      })(), new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          if (!this.destroyed && this.startup === startup) this.video.pause();
          reject(new Error('Démarrage de lecture expiré après 10 secondes.'));
        }, 10000);
      })]);
    } catch (error) { if (!this.destroyed && this.startup === startup) this.pause(); throw error; }
    finally { clearTimeout(timer); if (this.startup === startup) this.startup = undefined; }
  }
  pause() { this.pauseRevision++; this.startup?.abort(); this.externalAudio?.pause(); this.pendingAudio?.pause(); this.video.pause(); }
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
    this.startup?.abort();
    for(const controller of this.preparations)controller.abort();this.preparations.clear();
    this.cancel?.();
    this.pendingAudio?.destroy(); this.pendingAudio = undefined;
    this.progressiveVideo?.destroy();this.progressiveVideo=undefined;
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

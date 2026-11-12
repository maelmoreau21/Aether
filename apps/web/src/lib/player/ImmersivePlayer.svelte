<script lang="ts">
  import ReportButton from '../applications/ReportButton.svelte';
  import { onMount, tick } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { asset } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import PlayerControls from './PlayerControls.svelte';
  import type { Menu, TranscodeInfo } from './controls-props';
  import type { Media, Player, PlayerFactory, PlaybackState } from './types';
  import { preferredSubtitle, type SubtitlePreference } from './subtitle-preference';
  import { subtitleLine, subtitleStyleToCss, type SubtitleStyle } from './subtitle-style';
  import { maxBitrate, qualityChoice, qualityPreference, type AudioChannels, type QualityChoice, type ToneMapping } from './transcode';
  import { audioEnhancement, type AudioEnhancement } from './audio-enhancement';
  import { SLEEP_OFF, sleepDue, sleepRemaining, startSleep, type SleepChoice, type SleepTimer } from './sleep-timer';
  import { ProgressTracker } from './progress';
  import { PlaybackCollector, type EventKind, type Origin } from '../statistics/collector';
  import { PlaybackPresence } from './presence';
  import { session, getSessionGeneration } from '../auth/session';
  import { preferences, savePreferences, ensurePreferences } from '../preferences';
  import { isForeignAudio, languageDefaults, playbackPreferences, trackPreferences } from './playback-preferences';
  import { chooseSource, playbackMime, audioLanguage } from './source-selection';
  import { AudioSelectionRetainedError } from './errors';
  import { failoverHelps, failureKindOf, type PlaybackFailureKind } from './diagnostics';
  import { revalidateMediaDescriptor } from '../catalog/api';
  import { bufferedAhead } from './startup-buffer';
  import { SyncPartyClient } from '../syncplay/sync-client.svelte';
  import { applyPartyPlayback } from '../syncplay/player-sync';
  import WatchPartyOverlay from '../syncplay/WatchPartyOverlay.svelte';
  import WatchPartyModal from '../syncplay/WatchPartyModal.svelte';
  import { t } from '../i18n';
  import { watchTogetherEnabled } from '../config/presentation';
  import { remote, type RemoteCommand, type RemoteDevice } from '../remote/remote.svelte';
  import { isDirection, tvActive, tvKey, type TvKey } from '../tv/keys';
  import { moveFocus } from '../tv/navigation';
  import StreamStatistics from './StreamStatistics.svelte';
  import NextEpisodeCard from './NextEpisodeCard.svelte';
  let { catalogue, createPlayer, nextTitle, nextLabel = '', nextImage = '', onNext }: {
    catalogue: Media[]; createPlayer: PlayerFactory; nextTitle?: string;
    /** « Saison 1 · Épisode 3 » de l'épisode suivant. */
    nextLabel?: string;
    /** Vignette de l'épisode suivant (ou image de la série en repli). */
    nextImage?: string;
    onNext?: () => void;
  } = $props();
  let autoplayNext = $state(false);
  let skipBackSeconds = $state(10);
  let skipForwardSeconds = $state(10);
  let syncClient = $state<SyncPartyClient | null>(null);
  let dialog: HTMLDialogElement;
  let surface: HTMLDivElement;
  let video: HTMLVideoElement;
  let controls = $state<PlayerControls>();
  let engine: Player | undefined;
  let progressTracker: ProgressTracker | undefined;
  let collector: PlaybackCollector | undefined;
  let collectorWork = '';
  function collect(media: Media) {
    const identity = media.workId ?? media.id;
    if (collector && collectorWork === identity) { collector.suspend(); return; }
    collector?.stop(); collector = undefined; collectorWork = identity;
    if (!media.synthetic) collector = new PlaybackCollector(video, { mediaId: media.id, music: false,
      state: () => engine?.state(), origin: () => syncClient ? 'sync' : 'manual', info: () => engine?.playbackInfo?.() });
  }
  function trackSubtitle(id: string | null, origin: Origin) {
    const track = active?.subtitles.find(item => item.id === id);
    collector?.track('subtitle', { id: track?.id ?? 'off', language: track?.language ?? 'off', codec: track?.sourceFormat ?? '' }, origin);
  }
  function measuredSeek(position: number, kind: EventKind = 'seek', origin: Origin = 'manual') {
    if (!engine) return;
    const before = engine.state().position;
    engine.seek(position);
    collector?.seek(position, before, kind, origin);
  }
  let presence: PlaybackPresence | undefined;
  let opened = $state(false);
  let active = $state<Media | undefined>();
  let busy = $state(false);
  let error = $state('');
  let errorKind = $state<PlaybackFailureKind | ''>('');
  let notice = $state('');
  let statistics = $state(false);
  let markerEditor = $state(false);
  let markerKind = $state<'intro' | 'credits' | 'recap' | 'preview'>('intro');
  let hasAutoSkippedIntro = false;
  let hasAutoSkippedRecap = false;
  let hasAutoSkippedCredits = false;
  let markerEnabled = $state(true);
  let markerStart = $state(0);
  let markerEnd = $state(0);
  let markerSaving = $state(false);
  let markerError = $state('');
  let metrics = $state({ width: 0, height: 0, frames: 0, dropped: 0, buffered: 0, audioBuffered: null as number | null, ready: 0, network: 0 });
  const selectedSource = $derived(active?.sources.find(source => source.audioTrackId === audio));
  let loadStage = $state('Chargement de votre séance…');
  let audio = $state('');
  let preferredLanguage = 'fr';
  let subtitleLanguage = 'fr';
  let preferredAudio: { mediaId: string; trackId: string } | undefined;
  let manualAudio: { mediaId: string; trackId: string; language?: string } | undefined;
  let manualSubtitle: SubtitlePreference | undefined;
  let playDefaultAudio = false;
  let audioOffset = $state(0);
  let subtitleOffset = $state(0);
  let subtitle = $state<string | null>(null);
  let subtitlePreference: SubtitlePreference | undefined;
  /** Subtitles for an original-version audio (another language than the subtitle language). */
  let foreignSubtitlePreference: SubtitlePreference | undefined;
  let manualForeignSubtitle: SubtitlePreference | undefined;
  /** Whether the audio being played is an original version, for remembering subtitle choices. */
  let foreignAudio = false;
  let subtitleStyle = $state<SubtitleStyle | undefined>();
  let quality = $state<QualityChoice>('auto');
  let maxKbps = $state(0);
  let toneMapping: ToneMapping = 'auto';
  let audioChannels: AudioChannels = 'stereo';
  let enhancement = $state<AudioEnhancement>('off');
  let preferAudioDescription = false;
  let preferSdh = false;
  let transcodeInfo = $state<TranscodeInfo>(null);
  let sleepTimer = $state<SleepTimer>(SLEEP_OFF);
  let sleepNow = $state(Date.now());
  const sleepShown = $derived(sleepTimer.kind === 'end' ? 'end' as const : sleepRemaining(sleepTimer, sleepNow));
  const activeSubtitleCss = $derived(subtitleStyleToCss(subtitleStyle));
  let menu = $state<Menu>(null);
  /** "Watch together" started from the player on the item being watched. */
  let partyModal = $state(false);
  let fullscreen = $state(false);
  let visible = $state(true);
  let keyboardPinned = $state(false);
  let paused = $state(true);
  let playback = $state<PlaybackState>({ position: 0, duration: 0, paused: true, volume: 1, speed: 1 });
  // Fin de la plage déjà chargée, exprimée dans le temps du lecteur (pas de la balise vidéo).
  let bufferedUntil = $state(0);
  let nextDismissed = $state(false);
  let nextImageFailed = $state('');
  let lastPointer = { x: -1, y: -1 };
  let aspectRatio = $state<'contain' | 'cover' | 'fill'>('contain');
  let repeat = $state(false);
  let idle: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let opener: HTMLElement | null = null;
  let scrollPosition = 0;
  let previousVolume = 1;
  let triedMediaIds = new SvelteSet<string>();
  let resumeTarget = 0;
  /** Position handed over by another device ("open" command or "Send to"). */
  let handoffPosition: number | undefined;
  let sendMenu = $state(false);
  const sendTargets = $derived(remote.devices);
  function reportRemote(force = false) {
    const item = active;
    if (!opened || !item?.workId || item.synthetic) return;
    remote.report({ workId: item.workId, title: item.title, position: playback.position, duration: playback.duration || null, paused }, force);
  }
  function remoteCommand(command: RemoteCommand) {
    if (!opened || !engine || command.action === 'open') return;
    if (syncClient) { notice = $t('remote_ignored_in_party'); return; }
    notice = $t('remote_command_from', { name: command.fromName });
    if (command.action === 'stop') { close(); return; }
    if (command.action === 'seek' && command.position !== undefined) { seek(command.position); reportRemote(true); return; }
    if ((command.action === 'play') === paused) togglePlayback();
  }
  /** Hands this title over to another device at the current position. */
  function sendTo(device: RemoteDevice) {
    const item = active;
    if (!item?.workId) return;
    sendMenu = false;
    remote.send(device.deviceId, 'open', playback.position, item.workId);
    close();
  }
  let failoverInProgress = false;
  function sync() {
    if (!engine || !opened) return;
    const snapshot = engine.state();
    playback = { ...snapshot, duration: Number.isFinite(snapshot.duration) ? snapshot.duration : 0 };
    paused = snapshot.paused;
    bufferedUntil = bufferedEnd(playback.position);
    const running = engine.transcodeState?.() ?? null;
    transcodeInfo = running ? { height: running.plan.height, kbps: running.plan.kbps, reason: running.plan.reason, encoder: running.encoder, tonemap: running.tonemap } : null;
    if (!busy) { progressTracker?.save(playback); reportRemote(); }
    if (!busy && !paused && syncClient?.connected && syncClient.party?.state === 'playing'
      && syncClient.party.positionUpdatedServerMs <= syncClient.getCurrentServerEpochMs()) {
      const correction = syncClient.evaluateDrift(playback.position);
      if (correction.action === 'rate') {
        const changed = Math.abs(snapshot.speed - correction.playbackRate) > .001;
        engine.setSpeed(correction.playbackRate);
        if (changed) collector?.event('speed_change', { origin: 'sync' });
      } else if (correction.action === 'seek') {
        measuredSeek(correction.targetPosition, 'seek', 'sync');
        engine.setSpeed(1.0);
      } else if (video.playbackRate !== 1.0) {
        engine.setSpeed(1.0);
        collector?.event('speed_change', { origin: 'sync' });
      }
    }
  }
  function bufferedEnd(position: number): number {
    if (!video) return position;
    const now = video.currentTime;
    let ahead = 0;
    for (let index = 0; index < video.buffered.length; index++) {
      if (now >= video.buffered.start(index) - 0.25 && now <= video.buffered.end(index)) ahead = Math.max(ahead, video.buffered.end(index) - now);
    }
    return position + ahead;
  }
  // Les micro-déplacements (souris posée, capteur) ne réveillent plus les commandes,
  // sinon le pointeur ne disparaît jamais.
  function pointerMoved(event: PointerEvent) {
    if (event.pointerType === 'touch') return;
    // L'ancre reste fixe pendant que les commandes sont cachées : un geste lent s'additionne.
    const distance = Math.hypot(event.clientX - lastPointer.x, event.clientY - lastPointer.y);
    if (!visible && distance < 4) return;
    lastPointer = { x: event.clientX, y: event.clientY };
    wake();
  }
  function updateSubtitleStyle(next: SubtitleStyle) {
    subtitleStyle = next;
    engine?.setSubtitlePosition?.(subtitleLine(next));
    savePreferences({ subtitleStyle: next });
  }
  let lastTouchTap: { time: number; side: 'left' | 'right' } | null = null;
  let touchTapTimer: ReturnType<typeof setTimeout> | undefined;
  let touchSeekTimer: ReturnType<typeof setTimeout> | undefined;
  let touchSeekIndicator = $state<{ side: 'left' | 'right'; seconds: number } | null>(null);
  let lastInteractionPointerType = '';
  function toggleControlsVisibility() {
    if (menu) { controls?.closeMenu(); wake(); }
    else if (!paused && !busy && !error) { visible = !visible; if (visible) armIdle(); else surface?.focus(); }
    else wake();
  }
  function handleVideoClick(event: MouseEvent) {
    if (lastInteractionPointerType !== 'touch') {
      toggleControlsVisibility();
      return;
    }
    const target = event.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    const ratio = rect.width > 0 ? (event.clientX - rect.left) / rect.width : 0.5;
    const side: 'left' | 'right' | null = ratio < 0.35 ? 'left' : ratio > 0.65 ? 'right' : null;
    const now = Date.now();
    if (side && lastTouchTap && lastTouchTap.side === side && now - lastTouchTap.time < 320 && !busy && !error && active) {
      clearTimeout(touchTapTimer);
      lastTouchTap = null;
      const delta = side === 'left' ? -skipBackSeconds : skipForwardSeconds;
      const targetTime = Math.max(0, Math.min(playback.duration || Infinity, playback.position + delta));
      seek(targetTime, 'seek');
      touchSeekIndicator = { side, seconds: Math.abs(delta) };
      clearTimeout(touchSeekTimer);
      touchSeekTimer = setTimeout(() => { touchSeekIndicator = null; }, 700);
      return;
    }
    if (side) {
      lastTouchTap = { time: now, side };
      clearTimeout(touchTapTimer);
      touchTapTimer = setTimeout(() => {
        lastTouchTap = null;
        toggleControlsVisibility();
      }, 260);
    } else {
      lastTouchTap = null;
      toggleControlsVisibility();
    }
  }
  // Carte « À suivre » : au début du générique, sinon 30 s avant la fin d'un épisode long.
  const nextCardStart = $derived.by(() => {
    const duration = playback.duration;
    if (!active || duration <= 0) return Infinity;
    if (active.credits && active.credits.start > 0 && active.credits.start < duration) return active.credits.start;
    return duration > 300 ? duration - 30 : Infinity;
  });
  const showNextCard = $derived(Boolean(nextTitle && onNext && !syncClient && opened && !busy && !error && !nextDismissed
    && playback.position >= nextCardStart && playback.position < playback.duration - 0.25));
  const postCreditsScene = $derived(Boolean(active?.credits && active.credits.end < playback.duration - 10 && playback.position < active.credits.end));
  $effect(() => { void active?.id; nextDismissed = false; });
  $effect(() => {
    if (!opened || sleepTimer.kind !== 'at') return;
    const timer = setInterval(() => {
      sleepNow = Date.now();
      if (sleepDue(sleepTimer, sleepNow)) { sleepTimer = SLEEP_OFF; engine?.pause(); sync(); notice = $t('sleep_timer_paused_playback'); wake(); }
    }, 1000);
    return () => clearInterval(timer);
  });
  function setSleep(choice: SleepChoice | null) {
    sleepTimer = startSleep(choice, Date.now()); sleepNow = Date.now();
    notice = choice === null ? $t('sleep_timer_off') : choice === 'end' ? $t('sleep_timer_set_end') : $t('sleep_timer_set', { count: choice });
    wake();
  }
  function streamOptions() { return { quality, maxKbps, toneMapping, audioChannels }; }
  function setQuality(value: QualityChoice) {
    quality = value;
    savePreferences({ streamingQuality: qualityPreference(value) });
    void action(player => player.setStreamOptions?.({ quality: value }));
  }
  function setMaxBitrate(value: number) {
    maxKbps = value;
    savePreferences({ maxBitrateKbps: value });
    void action(player => player.setStreamOptions?.({ maxKbps: value }));
  }
  function setEnhancement(value: AudioEnhancement) {
    enhancement = value;
    savePreferences({ audioEnhancement: value });
    if (engine?.setAudioEnhancement && !engine.setAudioEnhancement(value)) notice = $t('audio_enhancement_unavailable');
    wake();
  }
  function armIdle() {
    clearTimeout(idle);
    if (!opened || statistics || paused || menu || busy || error || keyboardPinned) { visible = true; return; }
    idle = setTimeout(() => visible = false, 3000);
  }
  function wake() { visible = true; armIdle(); }
  $effect(() => { armIdle(); return () => clearTimeout(idle); });
  $effect(() => { if (opened && !visible) surface?.focus(); });
  $effect(() => {
    const pos = playback.position;
    const dur = playback.duration;
    if (!opened || busy || paused || dur <= 0 || syncClient) return;
    const prefs = preferences();
    if (prefs.autoSkipRecap && active?.recap && !hasAutoSkippedRecap) {
      if (pos >= active.recap.start && pos < active.recap.end - 1.0) {
        hasAutoSkippedRecap = true;
        // Récap suivi d'une intro elle aussi sautée : un seul saut, sans double seek.
        const chained = prefs.autoSkipIntro && active.intro && Math.abs(active.intro.start - active.recap.end) <= 1.0;
        if (chained) hasAutoSkippedIntro = true;
        measuredSeek(chained && active.intro ? active.intro.end : active.recap.end, 'skip_recap', 'automatic');
        notice = chained ? $t('recap_and_intro_skipped_automatically') : $t('recap_skipped_automatically');
        return;
      }
    }
    if (prefs.autoSkipIntro && active?.intro && !hasAutoSkippedIntro) {
      if (pos >= active.intro.start && pos < active.intro.end - 1.0) {
        hasAutoSkippedIntro = true;
        measuredSeek(active.intro.end, 'skip_intro', 'automatic');
        notice = $t('intro_skipped_automatically');
      }
    }
    if (prefs.autoSkipCredits && active?.credits && !hasAutoSkippedCredits) {
      if (pos >= active.credits.start && pos < active.credits.end - 1.0) {
        hasAutoSkippedCredits = true;
        if (active.credits.end < dur - 10) {
          measuredSeek(active.credits.end, 'skip_credits', 'automatic');
          notice = $t('credits_skipped_automatically');
        }
      }
    }
  });
  async function tryAutomaticFailover(): Promise<boolean> {
    if (!opened || !engine || !active || failoverInProgress) return false;
    const current = generation;
    triedMediaIds.add(active.id);
    if (active.requestedId) triedMediaIds.add(active.requestedId);
    if (active.failoverFrom) triedMediaIds.add(active.failoverFrom);
    // Only copies the Core vetted for this viewer: same content (identical file or same
    // runtime), readable with this account's libraries and parental policy, on a Node it
    // considers connected. A catalogue entry for the same work is not enough: it may be
    // another cut whose timeline does not match the position being restored.
    const candidates = active.fallbackMediaIds ?? [];
    const nextId = candidates.find(id => !triedMediaIds.has(id));
    if (!nextId) return false;
    triedMediaIds.add(nextId);
    failoverInProgress = true;
    try {
      const snapshot = engine.state();
      const observed = Math.max(playback.position, snapshot.position || 0, video?.currentTime || 0);
      // A failure before the resume seek must not restart at 0 nor overwrite the
      // saved progress: fall back to the loaded resume point.
      const savedPosition = Number.isFinite(observed) && observed > 0 ? observed : resumeTarget;
      const restoreState: PlaybackState = {
        position: savedPosition,
        duration: playback.duration,
        paused: false,
        volume: playback.volume,
        speed: playback.speed
      };
      if (observed > 0) progressTracker?.save(restoreState, true);
      loadStage = $t('automatically_switching_to_an_available_node');
      let fallbackMedia: Media | undefined;
      try {
        fallbackMedia = await revalidateMediaDescriptor(nextId);
      } catch {
        fallbackMedia = catalogue.find(item => item.id === nextId);
      }
      if (!fallbackMedia || current !== generation || !opened) return false;
      triedMediaIds.add(fallbackMedia.id);
      await prepare(fallbackMedia, restoreState, $t('automatically_switched_to_an_available_node'));
      return current === generation && !error;
    } finally {
      failoverInProgress = false;
    }
  }
  function fail(message: string, kind?: PlaybackFailureKind) {
    if (!opened) return;
    collector?.event('error', { errorCode: 'playback' });
    engine?.pause();
    // An equivalent copy is the same file or the same runtime: a codec, container or
    // permission refusal would only repeat on it, so skip the detour and say why.
    const show = () => { errorKind = kind ?? ''; error = message; busy = false; sync(); wake(); };
    if (!failoverHelps(kind)) { show(); return; }
    void tryAutomaticFailover().then(recovered => {
      if (!recovered && opened) show();
    });
  }
  async function action(operation: (player: Player) => void | Promise<void>) {
    if (!engine || busy) return;
    const current = generation; busy = true; error = ''; wake();
    try { await operation(engine); }
    catch (e) {
      if (current === generation) {
        if (e instanceof AudioSelectionRetainedError) { loadStage = e.message; notice = $t('this_track_is_unavailable_the_previous'); }
        else fail(e instanceof Error ? e.message : String(e), failureKindOf(e));
      }
    }
    finally { if (current === generation) { busy = false; sync(); wake(); } }
  }
  async function prepare(media: Media, restore?: PlaybackState, failoverNotice?: string) {
    const current = generation;
    if (!engine) return;
    collect(media);
    triedMediaIds.add(media.id);
    if (media.requestedId) triedMediaIds.add(media.requestedId);
    if (media.failoverFrom) triedMediaIds.add(media.failoverFrom);
    busy = true; error = ''; notice = ''; wake();
    try {
      // Progress and account preferences are independent reads. Start both
      // before choosing a track so a cold preferences request does not delay
      // the saved position request; track selection still waits for preferences.
      const creditsChapter = media.chapters?.find(ch => /cr[eé]dits?|g[eé]n[eé]rique|outro|ending/i.test(ch.title) && ch.start > 0);
      const detectedCreditsStart = media.credits?.start ?? creditsChapter?.start;
      const tracker = !restore && media.workId && !media.synthetic
        ? new ProgressTracker(media.workId, message => { if (current === generation) notice = message; }, detectedCreditsStart, Boolean(syncClient)) : undefined;
      if (tracker) progressTracker = tracker;
      else progressTracker?.setCreditsStart(detectedCreditsStart);
      const resume = tracker?.load();
      if (tracker) { resumeTarget = 0; void resume?.then(position => { if (current === generation && Number.isFinite(position)) resumeTarget = position; }, () => undefined); }
      // Preferences are optional: an outage must not prevent authorized playback.
      // preferences() is cleared by session generation changes.
      let preferenceNotice = '';
      try { await ensurePreferences(); }
      catch { preferenceNotice = $t('preferences_unavailable_settings_are_local'); }
      if (current !== generation) return;
      readPlaybackPreferences();
      const source = chooseSource(media, preferredAudio?.mediaId === media.id ? preferredAudio.trackId : undefined, preferredLanguage, source => Boolean(video.canPlayType(playbackMime(media, source))) && !source.transcodeOnly, playDefaultAudio, preferAudioDescription);
      if (!source) throw new Error($t('no_usable_audio_source_for_this'));
      if (syncClient) await resume; // Load the progress revision before collective playback saves it.
      if (current !== generation) return;
      await engine.setStreamOptions?.({ ...streamOptions(), burnSubtitle: undefined, adaptiveKbps: undefined });
      engine.setSubtitlePosition?.(subtitleLine(subtitleStyle));
      const handoff = handoffPosition; handoffPosition = undefined;
      await engine.load(media, source.audioTrackId, syncClient ? syncClient.getComputedPartyPosition() : restore?.position ?? handoff ?? resume ?? 0);
      if (current !== generation) return;
      const autoFailoverMsg = failoverNotice || (media.failoverFrom ? $t('automatically_switched_to_an_available_node') : '');
      notice = autoFailoverMsg || preferenceNotice;
      await engine.selectAudio(source.audioTrackId); audio = source.audioTrackId;
      collector?.source(engine.playbackInfo?.(), media.id);
      engine.setAudioOffset?.(audioOffset); engine.setSubtitleOffset?.(subtitleOffset);
      if (enhancement !== 'off' && engine.setAudioEnhancement && !engine.setAudioEnhancement(enhancement)) notice = $t('audio_enhancement_unavailable');
      foreignAudio = isForeignAudio(source.language, subtitleLanguage);
      const usableSubtitle = preferredSubtitle(media.subtitles, foreignAudio ? foreignSubtitlePreference : subtitlePreference, subtitleLanguage, preferSdh);
      // Embedded subtitle extraction can take substantially longer than the
      // first video/audio data. Start it now but do not hold playback for it.
      const subtitleLoading = engine.selectSubtitle(usableSubtitle);
      void subtitleLoading.then(() => { if (current === generation) trackSubtitle(usableSubtitle, 'automatic'); }).catch(() => {});
      subtitle = usableSubtitle;
      void subtitleLoading.catch(error => {
        if (current === generation && subtitle === usableSubtitle) {
          subtitle = null;
          notice = error instanceof Error ? error.message : $t('subtitles_unavailable');
        }
      });
      // Legacy audio windows share the Node's single FFmpeg worker with
      // subtitle extraction. Keep their existing ordering so an extraction
      // cannot starve the next audio window during playback.
      if (source.audioSegmentUrl && source.audioSegmentMode !== 'absolute') await subtitleLoading;
      if (restore) { engine.setVolume(restore.volume); engine.setSpeed(restore.speed); }
      if (current !== generation) return;
      hasAutoSkippedIntro = false;
      hasAutoSkippedRecap = false;
      hasAutoSkippedCredits = false;
      const skipPreferences = preferences();
      active = {
        ...media,
        intro: skipPreferences.skipIntroEnabled === false ? undefined : media.intro,
        credits: skipPreferences.skipCreditsEnabled === false ? undefined : media.credits,
        recap: skipPreferences.skipRecapEnabled === false ? undefined : media.recap,
        preview: media.preview
      };
      presence?.stop();presence=undefined;
      if(media.workId&&!media.synthetic)presence=new PlaybackPresence(media.workId,()=>engine?.state(),cmd=>{if(cmd==='stop'){close();}else if(cmd==='pause'){engine?.pause();sync();presence?.ping();}else if(cmd==='play'){void engine?.play().then(()=>{sync();presence?.ping();}).catch(()=>{});}});
      if (syncClient) {
        await applyPartyPlayback(engine, syncClient.party?.state === 'playing'
          && syncClient.party.positionUpdatedServerMs <= syncClient.getCurrentServerEpochMs() ? 'playing' : 'paused',
          syncClient.getComputedPartyPosition());
        syncClient.setReady(true);
        syncClient.setBuffering(false, engine.state().position);
      } else if (!restore || !restore.paused) await engine.play();
    } catch (e) { if (current === generation) fail(e instanceof Error ? e.message : String(e), failureKindOf(e)); }
    finally { if (current === generation) { busy = false; sync(); wake(); } }
  }
  function readPlaybackPreferences() {
    const languages = languageDefaults(preferences());
    preferredLanguage = languages.audio; subtitleLanguage = languages.subtitle; subtitlePreference = undefined; foreignSubtitlePreference = undefined; preferredAudio = undefined; audioOffset = 0; subtitleOffset = 0;
    try {
      const account = preferences();
      const defaults = playbackPreferences(account);
      autoplayNext = defaults.autoplayNextEpisode;
      skipBackSeconds = defaults.skipBackSeconds;
      skipForwardSeconds = defaults.skipForwardSeconds;
      const policy = trackPreferences(account);
      const track = manualAudio ?? policy.audioTrack;
      if (track && typeof track.mediaId === 'string' && typeof track.trackId === 'string') preferredAudio = track;
      const offsets = account.offsets;
      if (offsets && Number.isFinite(offsets.audio) && Math.abs(offsets.audio) <= 30) audioOffset = offsets.audio;
      if (offsets && Number.isFinite(offsets.subtitle) && Math.abs(offsets.subtitle) <= 30) subtitleOffset = offsets.subtitle;
      const language = audioLanguage(manualAudio?.language ?? policy.audioLanguage);
      preferredLanguage = language && language !== 'und' ? language : languages.audio;
      playDefaultAudio = policy.playDefaultAudioTrack && !manualAudio;
      subtitlePreference = manualSubtitle ?? policy.subtitlePreference;
      foreignSubtitlePreference = manualForeignSubtitle ?? policy.foreignSubtitlePreference;
      subtitleStyle = account.subtitleStyle;
      quality = qualityChoice(account.streamingQuality);
      maxKbps = maxBitrate(account.maxBitrateKbps);
      toneMapping = account.hdrToneMapping === 'always' || account.hdrToneMapping === 'never' ? account.hdrToneMapping : 'auto';
      audioChannels = account.audioChannels === 'surround' || account.audioChannels === 'passthrough' ? account.audioChannels : 'stereo';
      enhancement = audioEnhancement(account.audioEnhancement);
      preferAudioDescription = account.preferAudioDescription === true;
      preferSdh = account.preferSdhSubtitles === true;
    } catch { /* Preferences are optional when browser storage is unavailable. */ }
  }
  // What the automatic detection stored for this file, shown beside the manual correction.
  type StoredMarker = { type: string; start: number | null; end: number | null; confidence?: number; extensionId?: string; enabled?: boolean };
  let markerDetections = $state<{ assetId: string; automatic: StoredMarker[]; manual: StoredMarker[] } | null>(null);
  let markerDetectionsLoading = $state(false);
  async function loadMarkerDetections(assetId: string) {
    if (markerDetections?.assetId === assetId || markerDetectionsLoading) return;
    markerDetectionsLoading = true;
    try {
      const response = await fetch(`/api/v1/admin/assets/${encodeURIComponent(assetId)}/markers`, { credentials: 'same-origin', cache: 'no-store' });
      if (response.ok) {
        const body = await response.json() as { automatic?: StoredMarker[]; manual?: StoredMarker[] };
        if (active?.id === assetId) markerDetections = { assetId, automatic: body.automatic ?? [], manual: body.manual ?? [] };
      }
    } catch { /* The correction form stays usable without the detection summary. */ }
    finally { markerDetectionsLoading = false; }
  }
  const markerClock = (seconds: number | null) => {
    if (seconds === null || !Number.isFinite(seconds)) return '—';
    const whole = Math.round(seconds), hours = Math.floor(whole / 3600), minutes = Math.floor(whole % 3600 / 60), rest = String(whole % 60).padStart(2, '0');
    return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`;
  };
  function editMarker(kind: 'intro' | 'credits' | 'recap' | 'preview') {
    if (active) void loadMarkerDetections(active.id);
    markerKind = kind;
    const value = active?.[kind];
    markerEnabled = Boolean(value);
    markerStart = value?.start ?? Math.floor(playback.position);
    markerEnd = value?.end ?? Math.min(playback.duration, markerStart + 60);
    markerError = '';
    markerEditor = true;
    wake();
  }
  async function saveMarker(remove = false) {
    const account = $session;
    if (!account || !active || markerSaving) return;
    const assetId = active.id;
    markerSaving = true; markerError = '';
    try {
      const response = await fetch(`/api/v1/admin/assets/${encodeURIComponent(assetId)}/markers/${markerKind}`, {
        method: remove ? 'DELETE' : 'PUT', credentials: 'same-origin',
        headers: { 'x-csrf-token': account.csrfToken, 'content-type': 'application/json' },
        body: remove ? undefined : JSON.stringify(markerEnabled ? { enabled: true, start: markerStart, end: markerEnd } : { enabled: false })
      });
      if (!response.ok) throw new Error($t('correction_refused', { status: response.status }));
      if (active?.id !== assetId) return;
      if (remove) {
        const fresh = await revalidateMediaDescriptor(assetId);
        if (fresh && active?.id === assetId) active = {
          ...fresh,
          intro: preferences().skipIntroEnabled === false ? undefined : fresh.intro,
          credits: preferences().skipCreditsEnabled === false ? undefined : fresh.credits,
          recap: preferences().skipRecapEnabled === false ? undefined : fresh.recap,
          preview: fresh.preview
        };
      } else {
        active = { ...active, [markerKind]: markerEnabled ? { start: markerStart, end: markerEnd } : undefined };
      }
      markerEditor = false; markerDetections = null;
    } catch (error) { markerError = error instanceof Error ? error.message : $t('correction_unavailable'); }
    finally { markerSaving = false; }
  }
  export function open(media: Media | Promise<Media>, startAt?: number) {
    if (opened) return false;
    handoffPosition = startAt !== undefined && Number.isFinite(startAt) && startAt >= 0 ? startAt : undefined;
    generation++; const current = generation;
    manualAudio = undefined; manualSubtitle = undefined; manualForeignSubtitle = undefined;
    triedMediaIds.clear();
    failoverInProgress = false;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    scrollPosition = window.scrollY;
    statistics = false; loadStage = $t('loading_your_session'); audio = ''; active = media instanceof Promise ? undefined : media; opened = true; busy = true; error = ''; notice = ''; menu = null;
    keyboardPinned = false; visible = true; document.body.classList.add('immersive-open');
    dialog.showModal();
    surface?.focus();
    // Requested synchronously from the opening user gesture, before any network await.
    requestFullscreen();
    engine = createPlayer(video, (message, kind) => { if (current === generation) fail(message, kind); });
    engine.setStatusListener?.(message=>{if(current===generation)loadStage=message;});
    if (media instanceof Promise) {
      void media.then(item => { if (current === generation && opened) { active = item; void prepare(item); } })
        .catch(cause => { if (current === generation) fail(cause instanceof Error ? cause.message : String(cause), failureKindOf(cause)); });
    } else void prepare(media);
    return true;
  }
  export function close() {
    if (!opened) return;
    collector?.stop(); collector = undefined; collectorWork = '';
    if (syncClient) {
      syncClient.leave();
      syncClient = null;
    }
    if (engine) progressTracker?.save(engine.state(), true);
    progressTracker = undefined;
    presence?.stop();presence=undefined;
    remote.report(null, true); sendMenu = false;
    opened = false; generation++; const closedGeneration = generation; clearTimeout(idle); clearTimeout(touchTapTimer); clearTimeout(touchSeekTimer); touchSeekIndicator = null; lastTouchTap = null; menu = null; markerEditor = false; sleepTimer = SLEEP_OFF; transcodeInfo = null; bufferedUntil = 0;
    const returnTarget = opener;
    const restoreNavigation = () => {
      if (opened || generation !== closedGeneration) return;
      window.scrollTo({ top: scrollPosition, behavior: 'instant' });
      returnTarget?.focus({ preventScroll: true });
    };
    engine?.destroy(); engine = undefined;
    video.querySelectorAll('track').forEach(track => track.remove());
    const leavingFullscreen = document.fullscreenElement === surface ? document.exitFullscreen().catch(() => {}) : Promise.resolve();
    if (dialog.open) dialog.close();
    document.body.classList.remove('immersive-open');
    restoreNavigation();
    void leavingFullscreen.then(() => requestAnimationFrame(restoreNavigation));
    busy = false; error = ''; paused = true;
  }
  export function changeMedia(media: Media, requireOpen = false) {
    if (!opened) { if (!requireOpen) open(media); return; }
    if (!engine || busy) return;
    collector?.stop(); collector = undefined; collectorWork = '';
    manualAudio = undefined; manualSubtitle = undefined; manualForeignSubtitle = undefined;
    triedMediaIds.clear();
    failoverInProgress = false;
    progressTracker?.save(engine.state(), true);
    progressTracker = undefined;
    presence?.stop();presence=undefined;
    active = media;
    // Keep the dialog, focus context and fullscreen through episode transitions.
    void prepare(media);
  }
  export function reportError(message: string) { fail(message); }
  /** The host keeps the scene: once the new session is connected, it starts where playback was. */
  function startPartyFromPlayer(partyId: string) {
    partyModal = false;
    const current = active;
    if (!current) return;
    const position = playback.position;
    joinParty(partyId, current);
    const client = syncClient;
    const started = Date.now();
    const timer = setInterval(() => {
      if (syncClient !== client || !client || Date.now() - started > 15000) { clearInterval(timer); return; }
      if (client.connected) { clearInterval(timer); if (position > 1) client.seek(position); }
    }, 250);
  }
  export function joinParty(partyId: string, media: Media | Promise<Media>) {
    if (syncClient) {
      syncClient.leave();
      syncClient = null;
    }
    syncClient = new SyncPartyClient(partyId, $session?.user.id ?? '');
    setupSyncClientHooks();
    if (!opened) open(media);
    else if (media instanceof Promise) {
      const client = syncClient;
      void media.then(item => { if (syncClient === client && opened) changeMedia(item); })
        .catch(cause => { if (syncClient === client) fail(cause instanceof Error ? cause.message : String(cause), failureKindOf(cause)); });
    } else changeMedia(media);
  }
  function setupSyncClientHooks() {
    if (!syncClient) return;
    const client = syncClient;
    syncClient.onPlaybackUpdate = (state, position) => {
      // Received commands apply locally even in host_only mode. Never send a
      // command back through the user-control path (which caused seek loops).
      if (!engine || busy) return;
      if (state === 'playing' && video.ended && active && !collector) collect(active);
      const before = engine.state().position;
      client.setReady(true);
      void applyPartyPlayback(engine, state, position).then(() => {
        if (engine && Math.abs(engine.state().position - before) > 1) collector?.seek(engine.state().position, before, 'seek', 'sync');
      }).catch(cause => {
        notice = cause instanceof Error ? cause.message : $t('group_playback_unavailable');
      });
    };
    syncClient.onMediaChanged = (newMediaId) => {
      const found = catalogue.find(m => m.id === newMediaId || m.workId === newMediaId);
      if (found) changeMedia(found);
      else void revalidateMediaDescriptor(newMediaId).then(media => {
        if (syncClient === client && opened) changeMedia(media);
      }).catch(cause => { notice = cause instanceof Error ? cause.message : $t('media_unavailable'); });
    };
  }
  function reportBuffering(buffering: boolean) {
    if (!busy && engine && syncClient?.connected) {
      syncClient.setBuffering(buffering, engine.state().position);
    }
  }

  function requestFullscreen() {
    if (!surface.requestFullscreen) { notice = $t('full_screen_unavailable_immersive_mode'); return; }
    void surface.requestFullscreen().then(() => {
      if (!opened && document.fullscreenElement === surface) void document.exitFullscreen().catch(() => {});
    }).catch(() => { if (opened) notice = $t('full_screen_refused_immersive_mode_remains'); });
  }
  function toggleFullscreen() {
    if (document.fullscreenElement === surface) void document.exitFullscreen().catch(() => {});
    else requestFullscreen();
    wake();
  }
  function togglePlayback() {
    if (syncClient && !syncClient.canControl) {
      notice = $t('controls_reserved_for_the_organiser');
      wake();
      return;
    }
    if (video.ended && active && !collector) collect(active);
    if (syncClient) {
      if (paused) syncClient.play(playback.position);
      else syncClient.pause(playback.position);
      return;
    }
    void action(async player => { if (player.state().paused) await player.play(); else player.pause(); });
  }
  function volume(value: number) { engine?.setVolume(value); if (value > 0) previousVolume = value; sync(); wake(); }
  function mute() { volume(playback.volume === 0 ? previousVolume || 1 : 0); }
  function seek(value: number, kind: EventKind = 'seek') {
    if (syncClient && !syncClient.canControl) {
      notice = $t('controls_reserved_for_the_organiser');
      wake();
      return;
    }
    if (syncClient) {
      syncClient.seek(value);
      return;
    }
    if (!busy) { measuredSeek(value, kind); sync(); wake(); }
  }
  function selectAudio(id: string) {
    const current = generation;
    const accountGeneration = getSessionGeneration();
    void action(async player => { await player.selectAudio(id);
      if (current !== generation || !opened || accountGeneration !== getSessionGeneration()) return;
      audio = id; notice = '';
      const info = player.playbackInfo?.();
      if (info?.audio) collector?.track('audio', info.audio, 'manual');
      const language = audioLanguage(active?.sources.find(source => source.audioTrackId === id)?.language ?? 'und');
      if (language && language !== 'und') preferredLanguage = language;
      foreignAudio = isForeignAudio(language, subtitleLanguage);
      manualAudio = { mediaId: active?.id ?? '', trackId: id, ...(language && language !== 'und' ? { language } : {}) };
      preferredAudio = manualAudio;
      if (preferences().rememberAudioSelections !== false) savePreferences({ audioTrack: manualAudio });
    });
  }
  function selectSubtitle(id: string | null) {
    const current = generation;
    const accountGeneration = getSessionGeneration();
    void action(async player => {
    // Un sous-titre indisponible (préparation occupée, WebVTT illisible) ne doit
    // jamais interrompre ni basculer la lecture : on prévient et on continue.
    try { await player.selectSubtitle(id); }
    catch {
      if (current === generation) notice = $t('subtitles_temporarily_unavailable_playback');
      return;
    }
    if (current !== generation || !opened || accountGeneration !== getSessionGeneration()) return;
    subtitle = id;
    trackSubtitle(id, 'manual');
    const track = active?.subtitles.find(track => track.id === id);
    const chosen: SubtitlePreference = track ? { enabled: true, language: track.language, forced: track.forced, sdh: track.sdh } : { enabled: false };
    // A choice made in VO applies to every VO; one made in the person's language, to the others.
    if (foreignAudio) {
      foreignSubtitlePreference = chosen; manualForeignSubtitle = chosen;
      if (preferences().rememberSubtitleSelections !== false) savePreferences({ subtitlePreferenceForeign: chosen });
    } else {
      subtitlePreference = chosen; manualSubtitle = chosen;
      if (preferences().rememberSubtitleSelections !== false) savePreferences({ subtitlePreference: chosen });
    }
  }); }
  function offset(kind: 'audio' | 'subtitle', value: number) {
    if (!Number.isFinite(value) || Math.abs(value) > 30) return;
    if (kind === 'audio') { audioOffset = value; engine?.setAudioOffset?.(value); }
    else { subtitleOffset = value; engine?.setSubtitleOffset?.(value); }
    savePreferences({ offsets: { audio: audioOffset, subtitle: subtitleOffset } });
    wake();
  }
  function selectSource(id: string) {
    if (!engine || busy || id === active?.id) return;
    const media = catalogue.find(media => media.id === id);
    if (media) { const restore = engine.state(); engine.pause(); void prepare(media, restore); }
  }
  function toggleSubtitles() {
    if (!active?.subtitles?.length) return;
    if (subtitle) {
      selectSubtitle(null);
    } else {
      const preferred = preferredSubtitle(active.subtitles, foreignAudio ? foreignSubtitlePreference : subtitlePreference, subtitleLanguage, preferSdh);
      selectSubtitle(preferred ?? active.subtitles[0]?.id ?? null);
    }
  }
  /** Media keys of keyboards and TV remotes, whatever has the focus. */
  function mediaKey(key: TvKey | null): boolean {
    if (key === 'playPause' || (key === 'play' && paused) || (key === 'pause' && !paused)) togglePlayback();
    else if (key === 'play' || key === 'pause') wake();
    else if (key === 'stop') close();
    else if (key === 'forward') seek(Math.min(playback.duration, playback.position + skipForwardSeconds));
    else if (key === 'rewind') seek(Math.max(0, playback.position - skipBackSeconds));
    else if (key === 'next' && onNext && !syncClient) onNext();
    else if (key === 'previous') seek(0);
    else return false;
    return true;
  }
  /**
   * TV remote: the arrows move between the controls once one has the focus; on the picture, left and
   * right seek, up and down bring the controls back with the focus on play (the TV owns the volume),
   * and OK plays or pauses.
   */
  function remoteKeyboard(event: KeyboardEvent, key: TvKey | null, target: HTMLElement | null): boolean {
    if (isDirection(key)) {
      const bar = target?.closest<HTMLElement>('.player-controls');
      if (bar && target !== surface) {
        if (target instanceof HTMLInputElement && target.type === 'range' && (key === 'left' || key === 'right')) return true;
        event.preventDefault();
        moveFocus(key, bar);
        return true;
      }
      if (key === 'up' || key === 'down') {
        event.preventDefault();
        wake();
        void tick().then(() => surface?.querySelector<HTMLElement>('.player-controls .play-btn')?.focus());
        return true;
      }
      return false;
    }
    if (key === 'enter' && !target?.closest('button, a, input, select, textarea')) {
      event.preventDefault();
      togglePlayback();
      return true;
    }
    return false;
  }
  function keyboard(event: KeyboardEvent) {
    if (!opened) return;
    const tv = tvActive();
    // On a TV every key comes from the remote: the controls hide again on their own.
    keyboardPinned = !tv; wake();
    const remoteKey = tvKey(event);
    const target = event.target instanceof HTMLElement ? event.target : null;
    const editing = Boolean(target?.closest('input:not([type="range"]), textarea'));
    if (remoteKey === 'back' || (tv && event.key === 'Backspace' && !editing)) {
      event.preventDefault();
      if (statistics) { statistics = false; return; }
      if (menu) controls?.closeMenu();
      // A TV is always full screen: Back leaves the player at once.
      else if (!tv && document.fullscreenElement === surface) void document.exitFullscreen().catch(() => {});
      else close();
      return;
    }
    if (!editing && mediaKey(remoteKey)) { event.preventDefault(); return; }
    if (target?.closest('[role="menu"]')) return;
    if (tv && remoteKeyboard(event, remoteKey, target)) return;
    if (target?.closest('input, select, textarea')) return;
    if ((event.key === ' ' || event.key === 'Enter') && target?.closest('button, a')) return;
    const key = event.key.toLowerCase();
    const isControlKey = [' ', 'arrowleft', 'arrowright', 'arrowup', 'arrowdown', 'm', 'f', 'k', 'j', 'l', 'c', '0'].includes(key)
      || ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key);
    if (isControlKey) {
      event.preventDefault();
      if (key === ' ' || key === 'k') togglePlayback();
      else if (event.key === 'ArrowLeft' || key === 'j') seek(Math.max(0, playback.position - skipBackSeconds));
      else if (event.key === 'ArrowRight' || key === 'l') seek(Math.min(playback.duration, playback.position + skipForwardSeconds));
      else if (event.key === 'ArrowUp') volume(Math.min(1, playback.volume + .05));
      else if (event.key === 'ArrowDown') volume(Math.max(0, playback.volume - .05));
      else if (key === 'm') mute();
      else if (key === 'f') toggleFullscreen();
      else if (key === 'c') toggleSubtitles();
      else if (key === '0') seek(0);
    }
  }
  function windowKeyboard(event: KeyboardEvent) {
    if (!opened || event.defaultPrevented) return;
    keyboard(event);
  }
  function cancel(event: Event) { event.preventDefault(); if (menu) controls?.closeMenu(); else close(); }
  function ended() {
    collector?.stop(true); collector = undefined; collectorWork = '';
    sync(); if (engine) progressTracker?.save(engine.state(), true, true);
    if (sleepTimer.kind === 'end') { sleepTimer = SLEEP_OFF; notice = $t('sleep_timer_paused_playback'); return; }
    if (repeat && active && engine && !syncClient) { collect(active); engine.seek(0); void engine.play().catch(cause => fail(String(cause))); return; }
    if (!syncClient && autoplayNext && onNext) onNext();
  }
  $effect(() => {
    if (!opened || !statistics) return;
    const update = () => {
      const quality = video.getVideoPlaybackQuality?.();
      let buffered = 0;
      for (let i = 0; i < video.buffered.length; i++) {
        if (video.currentTime >= video.buffered.start(i) && video.currentTime <= video.buffered.end(i)) buffered = video.buffered.end(i) - video.currentTime;
      }
      const separateAudio = surface.querySelector<HTMLAudioElement>('audio[data-active-audio="true"]');
      metrics = { width: video.videoWidth, height: video.videoHeight, frames: quality?.totalVideoFrames ?? 0, dropped: quality?.droppedVideoFrames ?? 0, buffered, audioBuffered: separateAudio ? bufferedAhead(separateAudio) : null, ready: video.readyState, network: video.networkState };
    };
    update();
    const timer = setInterval(update, 1000);
    return () => clearInterval(timer);
  });
  onMount(() => {
    const change = () => { fullscreen = document.fullscreenElement === surface; wake(); };
    document.addEventListener('fullscreenchange', change);
    const unsubscribe = remote.onCommand(remoteCommand);
    return () => { unsubscribe(); close(); document.removeEventListener('fullscreenchange', change); clearTimeout(idle); };
  });
</script>

<svelte:window onkeydown={windowKeyboard} />

<dialog id="player" bind:this={dialog} class="immersive-dialog" data-tv-nav="self" aria-label={$t('immersive_player')} oncancel={cancel} onclose={close}>
  <!-- The media shell owns focus and shortcuts; the controls do not know the playback implementation. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="player-shell" class:inactive={!visible && !paused} bind:this={surface} role="document" tabindex="-1" onkeydowncapture={() => { keyboardPinned = !tvActive(); wake(); }} onkeydown={keyboard}
    onpointermove={pointerMoved} onpointerdown={() => { keyboardPinned = false; surface?.focus(); }}>
    <video
      bind:this={video}
      playsinline
      preload="auto"
      loop={false}
      style:object-fit={aspectRatio}
      style:--sub-color={activeSubtitleCss.color}
      style:--sub-size={activeSubtitleCss.fontSize}
      style:--sub-font={activeSubtitleCss.fontFamily}
      style:--sub-bg={activeSubtitleCss.backgroundColor}
      style:--sub-shadow={activeSubtitleCss.textShadow}
      ontimeupdate={sync}
      ondurationchange={sync}
      onwaiting={() => reportBuffering(true)}
      oncanplay={() => reportBuffering(false)}
      onplaying={() => reportBuffering(false)}
      onplay={sync}
      onprogress={sync}
      onpause={() => { sync(); if (!busy && engine) progressTracker?.save(engine.state(), true); }}
      onended={ended}
    ><track kind="captions" /></video>
    <button class="video-interaction" tabindex="-1" aria-label={$t('show_or_hide_controls')} onpointerdown={(e) => { lastInteractionPointerType = e.pointerType; }} onclick={handleVideoClick}></button>
    {#if touchSeekIndicator}
      <div class="touch-seek-badge" class:left={touchSeekIndicator.side === 'left'} class:right={touchSeekIndicator.side === 'right'} aria-hidden="true">
        <span>{touchSeekIndicator.side === 'left' ? '−' : '+'}{touchSeekIndicator.seconds} s</span>
      </div>
    {/if}
    <div class="top-bar" class:concealed={!visible} inert={!visible}>
      <button class="return" aria-label={$t('back_to_home')} onclick={close}><Icon name="back" size={24} /></button>
      <div class="title"><span>{active?.title ?? $t('your_session')}</span></div>
      {#if active && !active.synthetic}<ReportButton variant="player" workId={active.workId} assetId={active.id} title={active.title} positionSeconds={playback.position} context={() => ({ engine: engine?.playbackInfo?.().method, audioTrackId: selectedSource?.reportTrackId, subtitleTrackId: active?.subtitles.find(track => track.id === subtitle)?.reportTrackId })} />{/if}
      {#if nextTitle && onNext && !syncClient}<div class="next-episode"><label title={nextTitle}><input type="checkbox" bind:checked={autoplayNext} onchange={() => savePreferences({ autoplayNextEpisode: autoplayNext })} /> {$t('play_next_automatically')}</label></div>{/if}
      {#if $watchTogetherEnabled && active && !active.synthetic && !syncClient}<button type="button" class="watch-together" title={$t('watch_together_from_player')} aria-label={$t('watch_together')} onclick={() => { menu = null; partyModal = true; }}><Icon name="users" size={20} /></button>{/if}
      {#if active?.workId && !active.synthetic && !syncClient && sendTargets.length}
        <div class="send-to">
          <button type="button" class="watch-together" aria-expanded={sendMenu} aria-haspopup="menu" title={$t('send_to_device')} aria-label={$t('send_to')} onclick={() => { menu = null; sendMenu = !sendMenu; }}><Icon name="tv" size={20} /></button>
          {#if sendMenu}
            <div class="send-menu" role="menu" aria-label={$t('send_to_device')}>
              {#each sendTargets as device (device.deviceId)}<button type="button" role="menuitem" onclick={() => sendTo(device)}>{device.name}</button>{/each}
            </div>
          {/if}
        </div>
      {/if}
      {#if $session?.user.role === 'administrator' && active?.episode}<button type="button" onclick={() => editMarker('intro')}>{$t('fix_markers')}</button>{/if}
    </div>
    {#if markerEditor && active?.episode}
      <section class="marker-editor" aria-label={$t('marker_correction')}>
        <h2>{$t('manual_marker')}</h2>
        <label>{$t('request_type')}<select bind:value={markerKind} onchange={() => editMarker(markerKind)}><option value="intro">{$t('intro')}</option><option value="credits">{$t('credits')}</option><option value="recap">{$t('recap')}</option><option value="preview">{$t('player_preview')}</option></select></label>
        {#if markerDetections?.assetId === active.id}
          {@const detected = markerDetections.automatic.filter(item => item.type === markerKind)}
          <div class="marker-detection" role="status">
            {#each detected as item, index (index)}
              <p>{$t('marker_detected', { start: markerClock(item.start), end: markerClock(item.end), confidence: Math.round((item.confidence ?? 0) * 100), source: item.extensionId ?? '—' })}</p>
            {:else}<p>{$t('marker_not_detected')}</p>{/each}
            {#if markerDetections.manual.some(item => item.type === markerKind)}<p>{$t('marker_manual_active')}</p>{/if}
          </div>
        {:else if markerDetectionsLoading}<p class="marker-detection" role="status">{$t('marker_detection_loading')}</p>{/if}
        <label><input type="checkbox" bind:checked={markerEnabled} /> {$t('enable_this_marker')}</label>
        {#if markerEnabled}
          <label>{$t('report_time_start')}<input type="number" min="0" step="0.1" bind:value={markerStart} /></label>
          <label>{$t('end_seconds')}<input type="number" min="0" max={playback.duration} step="0.1" bind:value={markerEnd} /></label>
        {/if}
        {#if markerError}<p role="alert">{markerError}</p>{/if}
        <button type="button" disabled={markerSaving} onclick={() => void saveMarker()}>{$t('save')}</button>
        <button type="button" disabled={markerSaving} onclick={() => void saveMarker(true)}>{$t('revert_to_detection')}</button>
        <button type="button" onclick={() => markerEditor = false}>{$t('close')}</button>
      </section>
    {/if}
    {#if busy}<div class="loading" role="status"><img src={asset('images/logo-loading.svg')} alt="" class="player-spinner" width="40" height="40" /><span>{$t('loading_the_video')}</span><button onclick={close}>{$t('sched_cancel')}</button></div>{/if}
    {#if error}<div class="player-error" role="alert" data-failure-kind={errorKind || undefined}><Icon name="alert" size={24} /><strong>{$t('playback_interrupted')}</strong><p>{error}</p><button onclick={() => { if (active) void prepare(active, { ...playback, paused: false }); }}>{$t('request_retry')}</button></div>{/if}
    <StreamStatistics bind:statistics {busy} {error} {wake} {active} {subtitle} {audio} {transcodeInfo} {paused} {playback} {metrics} {selectedSource} {loadStage} />
    <p class="fullscreen-notice" role="status">{notice}</p>
    {#if syncClient}
      <WatchPartyOverlay client={syncClient} controlsVisible={visible} onCloseParty={() => { syncClient?.leave(); syncClient = null; }} />
    {/if}
    {#if partyModal && active && $watchTogetherEnabled}
      <WatchPartyModal mediaId={active.workId ?? active.id} mediaTitle={active.title} onClose={() => partyModal = false} onJoinParty={partyId => startPartyFromPlayer(partyId)} />
    {/if}
    {#if active}<PlayerControls bind:this={controls} bind:menu {audioOffset} {subtitleOffset} onAudioOffset={(v) => offset('audio', v)} onSubtitleOffset={(v) => offset('subtitle', v)} {subtitleStyle} onSubtitleStyle={updateSubtitleStyle} state={playback} media={active} {catalogue} {audio} {subtitle} {visible} {busy} {fullscreen}
      {skipBackSeconds} {skipForwardSeconds}
      {aspectRatio} onAspectRatio={r => aspectRatio = r} {repeat} onRepeat={r => repeat = r} onStats={() => { statistics = true; wake(); }}
      onPlay={togglePlayback} onVolume={volume} onMute={mute} onSeek={seek} onAudio={selectAudio} onSubtitle={selectSubtitle} onSource={selectSource}
      onSpeed={speed => action(player => { player.setSpeed(speed); collector?.event('speed_change'); })} onFullscreen={toggleFullscreen} onWake={wake}
      buffered={bufferedUntil} hideCreditsSkip={showNextCard}
      {quality} {maxKbps} onQuality={setQuality} onMaxKbps={setMaxBitrate} transcode={transcodeInfo}
      {enhancement} onEnhancement={setEnhancement} sleep={sleepShown} onSleep={setSleep} />{/if}
    {#if showNextCard}
      <NextEpisodeCard onDismiss={() => { nextDismissed = true; surface?.focus(); }} {visible} {nextImage} bind:nextImageFailed {nextLabel} {nextTitle} {autoplayNext} {onNext} {postCreditsScene} {active} {seek} {wake} />
    {/if}
  </div>
</dialog>

<style>
  .marker-editor { position:absolute;z-index:10;top:90px;right:24px;display:grid;gap:10px;width:min(340px,calc(100% - 48px));padding:16px;background:#171923;color:white;border:1px solid #ffffff50;border-radius:10px; }

  .marker-editor label { display:grid;gap:4px; }

  .marker-detection { margin:0;padding:8px 10px;border-radius:8px;background:#ffffff14;font-size:13px;line-height:1.45; }

  .marker-detection p { margin:0; }

  .marker-detection p + p { margin-top:4px; }

  video::cue {
    color: var(--sub-color, #ffffff);
    background: var(--sub-bg, transparent);
    text-shadow: var(--sub-shadow, 0 2px 4px rgba(0, 0, 0, 0.9));
    font-size: var(--sub-size, clamp(18px, 2.2vw, 30px));
    font-family: var(--sub-font, system-ui, -apple-system, sans-serif);
    line-height: 1.35;
  }

  .next-episode { margin-left:auto; max-width:50%; font-size:12px; }

  .next-episode label { display:flex; align-items:center; gap:8px; }

  .title { min-width:0; flex:1; }

  .title>span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }

  :global(body.immersive-open) { overflow: hidden; }

  .immersive-dialog { position: fixed; inset: 0; width: 100%; height: 100dvh; max-width: none; max-height: none; margin: 0; padding: 0; border: 0; background: #000; color: var(--color-text); overflow: hidden; }

  .immersive-dialog::backdrop { background: #000; }

  .player-shell { width: 100%; height: 100%; position: relative; background: #000; outline: 0; overflow: hidden; }

  .player-shell:fullscreen { width: 100vw; height: 100dvh; }

  .inactive { cursor: none; }

  .inactive :global(*) { cursor: none; }

  video { width: 100%; height: 100%; display: block; object-fit: contain; background: #000; }

  .video-interaction { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; padding: 0; background: transparent; }
  .touch-seek-badge { position: absolute; top: 50%; transform: translateY(-50%); z-index: 6; pointer-events: none; padding: 10px 18px; border-radius: var(--r-pill, 999px); background: rgb(17 21 31 / 0.82); color: #fff; font-size: 16px; font-weight: 800; font-variant-numeric: tabular-nums; backdrop-filter: blur(12px); -webkit-backdrop-filter: blur(12px); box-shadow: 0 8px 24px rgb(0 0 0 / 0.4); }
  .touch-seek-badge.left { left: 14%; }
  .touch-seek-badge.right { right: 14%; }

  .top-bar {
    display: flex;
    position: absolute;
    inset: 0 0 auto;
    align-items: center;
    gap: 20px;
    z-index: 3;
    padding: max(24px, env(safe-area-inset-top)) max(36px, env(safe-area-inset-right)) 80px max(30px, env(safe-area-inset-left));
    background: linear-gradient(180deg, rgba(9, 10, 16, 0.95) 0%, rgba(9, 10, 16, 0.6) 60%, transparent 100%);
    transition: opacity 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .concealed {
    opacity: 0;
    pointer-events: none;
  }

  .return {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.08);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    color: #ffffff;
    transition: all var(--transition-fast);
  }

  .return:hover {
    background: rgba(255, 255, 255, 0.2);
    border-color: var(--color-accent-soft);
    transform: scale(1.06);
    box-shadow: 0 0 16px var(--color-accent-glow);
  }

  .title > span {
    display: block;
    font-size: 17px;
    font-weight: 650;
    letter-spacing: -0.02em;
    color: var(--color-text-bright);
  }

  .loading {
    position: absolute;
    inset: 42% 10% auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    pointer-events: none;
    color: var(--color-text-bright);
    font-size: 13.5px;
    font-weight: 500;
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.9);
  }

  .loading { z-index:5; }

  .loading button { pointer-events:auto;min-height:44px;padding:10px 20px; }

  .player-spinner {
    width: 40px;
    height: 40px;
    display: block;
    filter: drop-shadow(0 0 16px var(--warn));
  }

  .player-error {
    position: absolute;
    z-index: 5;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(440px, calc(100% - 40px));
    padding: 28px 24px;
    background: rgba(20, 16, 26, 0.95);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    border: 1px solid rgba(248, 113, 113, 0.35);
    border-radius: var(--radius-lg);
    text-align: center;
    color: #fca5a5;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.85);
  }

  .player-error strong {
    display: block;
    margin: 12px 0 6px;
    font-size: 16px;
    color: var(--color-text-bright);
  }

  .player-error p {
    font-size: 13px;
    line-height: 1.6;
    overflow-wrap: anywhere;
    color: var(--color-muted);
  }

  .player-error button {
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--color-text-bright);
    color: #090a10;
    padding: 12px 22px;
    margin-top: 16px;
    font-size: 13px;
    font-weight: 700;
    transition: all var(--transition-fast);
  }

  .player-error button:hover {
    background: #ffffff;
    box-shadow: 0 4px 16px rgba(255, 255, 255, 0.25);
    transform: translateY(-1px);
  }

  .fullscreen-notice {
    position: absolute;
    top: max(96px, env(safe-area-inset-top));
    left: 36px;
    color: var(--color-muted);
    font-size: 11px;
    z-index: 3;
    pointer-events: none;
    text-shadow: 0 1px 4px black;
  }

  @media (max-width: 700px) {
  .top-bar {
          padding: max(14px, env(safe-area-inset-top)) 16px 50px;
          gap: 12px;
        }
  .return {
          width: 42px;
          height: 42px;
        }
  .title > span {
          font-size: 14px;
        }
  .fullscreen-notice {
          left: 18px;
          top: 80px;
          font-size: 10px;
          max-width: calc(100% - 36px);
        }
}

  /* ---------- Halo : barre supérieure, erreurs et épisode suivant ---------- */
  .top-bar {
    background: linear-gradient(180deg, rgb(0 0 0 / 0.6) 0%, rgb(0 0 0 / 0) 100%);
    font-family: var(--font);
    transition: opacity var(--t-slow) ease;
  }

  .return {
    border: 0;
    border-radius: 16px;
    background: transparent;
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
  }

  .return:hover { background: rgb(255 255 255 / 0.14); border-color: transparent; transform: none; box-shadow: none; }

  .title > span { font-weight: 800; }

  .next-episode label { font-size: 13px; font-weight: 700; }

  /* Icon-only actions (watch together, send to): the label is the tooltip and aria-label. */
  .watch-together { display: inline-flex; align-items: center; justify-content: center; width: 40px; height: 40px; padding: 0; border: 0; border-radius: 50%; background: rgb(255 255 255 / 0.14); color: #f4f6fb; cursor: pointer; }

  .watch-together:hover { background: rgb(255 255 255 / 0.22); }

  .watch-together:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }

  .send-to { position: relative; }

  .send-menu { position: absolute; top: calc(100% + 6px); right: 0; z-index: 5; display: grid; min-width: 200px; padding: 6px; border-radius: 14px; background: rgb(18 20 26 / 0.94); box-shadow: 0 12px 32px rgb(0 0 0 / 0.4); }

  .send-menu button { min-height: 40px; padding: 0 12px; border: 0; border-radius: 10px; background: none; color: #f4f6fb; font: inherit; font-size: 14px; text-align: left; cursor: pointer; }

  .send-menu button:hover,
  .send-menu button:focus-visible { background: rgb(255 255 255 / 0.14); outline: none; }

  .next-episode input { accent-color: var(--ac); width: 18px; height: 18px; }

  .loading { font-family: var(--font); font-size: 14px; font-weight: 700; }

  .loading button { border: 0; border-radius: 14px; background: rgb(255 255 255 / 0.16); color: #f4f6fb; font-weight: 700; }

  .loading button:hover { background: rgb(255 255 255 / 0.26); }

  .player-spinner { filter: none; }

  .player-error {
    padding: 28px;
    border: 0;
    border-radius: 24px;
    background: rgb(17 21 31 / 0.94);
    color: var(--err);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    font-family: var(--font);
  }

  .player-error strong { font-size: 18px; font-weight: 800; color: #fff; }

  .player-error p { font-size: 14px; color: #a2a9ba; }

  .player-error button { height: 48px; padding: 0 22px; border-radius: 16px; background: #eef1f8; color: #0b0e18; font-size: 14px; font-weight: 800; }

  .player-error button:hover { background: #fff; box-shadow: none; transform: none; }

  .marker-editor {
    padding: 20px;
    border: 0;
    border-radius: 24px;
    background: rgb(17 21 31 / 0.92);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    color: #eef1f8;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    font-family: var(--font);
  }

  .marker-editor label { font-size: 14px; font-weight: 700; }

  .fullscreen-notice { color: #a2a9ba; font-size: 12px; font-weight: 600; }

  .marker-editor h2 { margin: 0 0 4px; font-size: 17px; font-weight: 800; }

  .marker-editor select,
  .marker-editor input[type='number'] { height: 44px; padding: 0 12px; border: 1px solid transparent; border-radius: 12px; background-color: rgb(255 255 255 / 0.07); color: #fff; box-shadow: none; }

  .marker-editor select:focus,
  .marker-editor input[type='number']:focus { outline: none; border-color: var(--ac); }

  .marker-editor input[type='checkbox'] { accent-color: var(--ac); width: 18px; height: 18px; }

  .marker-editor button { min-height: 44px; border: 0; border-radius: 14px; background: rgb(255 255 255 / 0.14); color: #f4f6fb; font-weight: 700; }

  .marker-editor button:first-of-type { background: #eef1f8; color: #0b0e18; }

  .marker-editor [role='alert'] { margin: 0; color: var(--err); font-weight: 600; }

  @keyframes next-in { from { opacity: 0; transform: translateY(16px) scale(0.98); } to { opacity: 1; transform: none; } }

  .inactive :global(*) { cursor: none !important; }
</style>

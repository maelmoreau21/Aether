<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { asset } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import PlayerControls, { type Menu } from './PlayerControls.svelte';
  import type { Media, Player, PlayerFactory, PlaybackState } from './types';
  import { preferredSubtitle, type SubtitlePreference } from './subtitle-preference';
  import { subtitleStyleToCss, type SubtitleStyle } from './subtitle-style';
  import { ProgressTracker } from './progress';
  import { PlaybackPresence } from './presence';
  import { session, getSessionGeneration } from '../auth/session';
  import { preferences, savePreferences, ensurePreferences } from '../preferences';
  import { chooseSource, playbackMime, audioLanguage } from './source-selection';
  import { AudioSelectionRetainedError } from './errors';
  import { revalidateMediaDescriptor } from '../catalog/api';
  import { bufferedAhead } from './startup-buffer';
  import { SyncPartyClient } from '../syncplay/sync-client.svelte';
  import WatchPartyOverlay from '../syncplay/WatchPartyOverlay.svelte';
  let { catalogue, createPlayer, nextTitle, onNext }: { catalogue: Media[]; createPlayer: PlayerFactory; nextTitle?: string; onNext?: () => void } = $props();
  let autoplayNext = $state(false);
  let syncClient = $state<SyncPartyClient | null>(null);
  let dialog: HTMLDialogElement;
  let surface: HTMLDivElement;
  let video: HTMLVideoElement;
  let controls = $state<PlayerControls>();
  let engine: Player | undefined;
  let progressTracker: ProgressTracker | undefined;
  let presence: PlaybackPresence | undefined;
  let opened = $state(false);
  let active = $state<Media | undefined>();
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let statistics = $state(false);
  let markerEditor = $state(false);
  let markerKind = $state<'intro' | 'credits' | 'recap' | 'preview'>('intro');
  let hasAutoSkippedIntro = false;
  let hasAutoSkippedCredits = false;
  let hasAutoSkippedRecap = false;
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
  let preferredAudio: { mediaId: string; trackId: string } | undefined;
  let audioOffset = $state(0);
  let subtitleOffset = $state(0);
  let subtitle = $state<string | null>(null);
  let subtitlePreference: SubtitlePreference | undefined;
  let subtitleStyle = $state<SubtitleStyle | undefined>();
  const activeSubtitleCss = $derived(subtitleStyleToCss(subtitleStyle));
  let menu = $state<Menu>(null);
  let fullscreen = $state(false);
  let visible = $state(true);
  let keyboardPinned = $state(false);
  let paused = $state(true);
  let playback = $state<PlaybackState>({ position: 0, duration: 0, paused: true, volume: 1, speed: 1 });
  let aspectRatio = $state<'contain' | 'cover' | 'fill'>('contain');
  let repeat = $state(false);
  let idle: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let opener: HTMLElement | null = null;
  let scrollPosition = 0;
  let previousVolume = 1;
  let triedMediaIds = new SvelteSet<string>();
  let failoverInProgress = false;
  function sync() {
    if (!engine || !opened) return;
    const snapshot = engine.state();
    playback = { ...snapshot, duration: Number.isFinite(snapshot.duration) ? snapshot.duration : 0 };
    paused = snapshot.paused;
    if (!busy) progressTracker?.save(playback);
    if (syncClient && syncClient.party?.state === 'playing') {
      const correction = syncClient.evaluateDrift(playback.position);
      if (correction.action === 'rate') {
        video.playbackRate = correction.playbackRate;
      } else if (correction.action === 'seek') {
        engine.seek(correction.targetPosition);
        video.playbackRate = 1.0;
      } else if (video.playbackRate !== 1.0) {
        video.playbackRate = 1.0;
      }
    }
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
    if (!opened || busy || paused || dur <= 0) return;
    const prefs = preferences();
    if (prefs.autoSkipIntro && active?.intro && !hasAutoSkippedIntro) {
      if (pos >= active.intro.start && pos < active.intro.end - 1.0) {
        hasAutoSkippedIntro = true;
        engine?.seek(active.intro.end);
        notice = "Introduction sautée automatiquement";
      }
    }
    if (prefs.autoSkipCredits && active?.credits && !hasAutoSkippedCredits) {
      if (pos >= active.credits.start && pos < active.credits.end - 1.0) {
        hasAutoSkippedCredits = true;
        if (active.credits.end < dur - 10) {
          engine?.seek(active.credits.end);
          notice = "Générique sauté automatiquement";
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
    const candidates = [
      ...(active.fallbackMediaIds ?? []),
      ...catalogue
        .filter(item => item.available !== false && item.nodeStatus !== 'disconnected' && item.nodeStatus !== 'revoked' && (!active?.workId || !item.workId || item.workId === active.workId))
        .map(item => item.id)
    ];
    const nextId = candidates.find(id => !triedMediaIds.has(id));
    if (!nextId) return false;
    triedMediaIds.add(nextId);
    failoverInProgress = true;
    try {
      const snapshot = engine.state();
      const savedPosition = Math.max(playback.position, snapshot.position || 0, video?.currentTime || 0);
      const restoreState: PlaybackState = {
        position: Number.isFinite(savedPosition) ? savedPosition : 0,
        duration: playback.duration,
        paused: false,
        volume: playback.volume,
        speed: playback.speed
      };
      progressTracker?.save(restoreState, true);
      loadStage = 'Basculement automatique vers un nœud disponible…';
      let fallbackMedia: Media | undefined;
      try {
        fallbackMedia = await revalidateMediaDescriptor(nextId);
      } catch {
        fallbackMedia = catalogue.find(item => item.id === nextId);
      }
      if (!fallbackMedia || current !== generation || !opened) return false;
      triedMediaIds.add(fallbackMedia.id);
      await prepare(fallbackMedia, restoreState, 'Basculement automatique sur un nœud disponible.');
      return current === generation && !error;
    } finally {
      failoverInProgress = false;
    }
  }
  function fail(message: string) {
    if (!opened) return;
    engine?.pause();
    void tryAutomaticFailover().then(recovered => {
      if (!recovered && opened) {
        error = message; busy = false; sync(); wake();
      }
    });
  }
  async function action(operation: (player: Player) => void | Promise<void>) {
    if (!engine || busy) return;
    const current = generation; busy = true; error = ''; wake();
    try { await operation(engine); }
    catch (e) {
      if (current === generation) {
        if (e instanceof AudioSelectionRetainedError) { loadStage = e.message; notice = 'Cette piste est indisponible. La piste précédente a été conservée.'; }
        else fail(e instanceof Error ? e.message : String(e));
      }
    }
    finally { if (current === generation) { busy = false; sync(); wake(); } }
  }
  async function prepare(media: Media, restore?: PlaybackState, failoverNotice?: string) {
    const current = generation;
    if (!engine) return;
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
        ? new ProgressTracker(media.workId, message => { if (current === generation) notice = message; }, detectedCreditsStart) : undefined;
      if (tracker) progressTracker = tracker;
      else progressTracker?.setCreditsStart(detectedCreditsStart);
      const resume = tracker?.load();
      // Preferences are optional: an outage must not prevent authorized playback.
      // preferences() is cleared by session generation changes.
      let preferenceNotice = '';
      try { await ensurePreferences(); }
      catch { preferenceNotice = 'Préférences indisponibles : réglages locaux à cette séance.'; }
      if (current !== generation) return;
      readPlaybackPreferences();
      const source = chooseSource(media, preferredAudio?.mediaId === media.id ? preferredAudio.trackId : undefined, preferredLanguage, source => Boolean(video.canPlayType(playbackMime(media, source))));
      if (!source) throw new Error('Aucune source audio utilisable pour ce média.');
      await engine.load(media, source.audioTrackId, restore?.position ?? resume ?? 0);
      if (current !== generation) return;
      const autoFailoverMsg = failoverNotice || (media.failoverFrom ? 'Basculement automatique sur un nœud disponible.' : '');
      notice = autoFailoverMsg || preferenceNotice;
      await engine.selectAudio(source.audioTrackId); audio = source.audioTrackId;
      engine.setAudioOffset?.(audioOffset); engine.setSubtitleOffset?.(subtitleOffset);
      const usableSubtitle = preferredSubtitle(media.subtitles, subtitlePreference);
      // Embedded subtitle extraction can take substantially longer than the
      // first video/audio data. Start it now but do not hold playback for it.
      const subtitleLoading = engine.selectSubtitle(usableSubtitle);
      subtitle = usableSubtitle;
      void subtitleLoading.catch(error => {
        if (current === generation && subtitle === usableSubtitle) {
          subtitle = null;
          notice = error instanceof Error ? error.message : 'Sous-titres indisponibles.';
        }
      });
      // Legacy audio windows share the Node's single FFmpeg worker with
      // subtitle extraction. Keep their existing ordering so an extraction
      // cannot starve the next audio window during playback.
      if (source.audioSegmentUrl && source.audioSegmentMode !== 'absolute') await subtitleLoading;
      if (restore) { engine.setVolume(restore.volume); engine.setSpeed(restore.speed); }
      if (current !== generation) return;
      hasAutoSkippedIntro = false;
      hasAutoSkippedCredits = false;
      hasAutoSkippedRecap = false;
      const skipPreferences = preferences();
      active = {
        ...media,
        intro: skipPreferences.skipIntroEnabled === false ? undefined : media.intro,
        credits: skipPreferences.skipCreditsEnabled === false ? undefined : media.credits,
        recap: media.recap,
        preview: media.preview
      };
      presence?.stop();presence=undefined;
      if(media.workId&&!media.synthetic)presence=new PlaybackPresence(media.workId,()=>engine?.state());
      if (!restore || !restore.paused) await engine.play();
    } catch (e) { if (current === generation) fail(e instanceof Error ? e.message : String(e)); }
    finally { if (current === generation) { busy = false; sync(); wake(); } }
  }
  function readPlaybackPreferences() {
    preferredLanguage = 'fr'; subtitlePreference = undefined; preferredAudio = undefined; audioOffset = 0; subtitleOffset = 0;
    try {
      const account = preferences();
      const track = account.audioTrack;
      if (track && typeof track.mediaId === 'string' && typeof track.trackId === 'string') preferredAudio = track;
      const offsets = account.offsets;
      if (offsets && Number.isFinite(offsets.audio) && Math.abs(offsets.audio) <= 30) audioOffset = offsets.audio;
      if (offsets && Number.isFinite(offsets.subtitle) && Math.abs(offsets.subtitle) <= 30) subtitleOffset = offsets.subtitle;
      const language = audioLanguage(account.audioLanguage ?? 'fr');
      preferredLanguage = language && language !== 'und' ? language : 'fr';
      const initialSubtitles = account.subtitles;
      if (initialSubtitles === 'off') subtitlePreference = { enabled: false };
      else if (initialSubtitles === 'always') subtitlePreference = { enabled: true, language: 'fr', forced: false };
      else subtitlePreference = undefined;
      subtitleStyle = account.subtitleStyle;
    } catch { /* Preferences are optional when browser storage is unavailable. */ }
  }
  function editMarker(kind: 'intro' | 'credits' | 'recap' | 'preview') {
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
      if (!response.ok) throw new Error(`Correction refusée (${response.status}).`);
      if (active?.id !== assetId) return;
      if (remove) {
        const fresh = await revalidateMediaDescriptor(assetId);
        if (fresh && active?.id === assetId) active = {
          ...fresh,
          intro: preferences().skipIntroEnabled === false ? undefined : fresh.intro,
          credits: preferences().skipCreditsEnabled === false ? undefined : fresh.credits,
          recap: fresh.recap,
          preview: fresh.preview
        };
      } else {
        active = { ...active, [markerKind]: markerEnabled ? { start: markerStart, end: markerEnd } : undefined };
      }
      markerEditor = false;
    } catch (error) { markerError = error instanceof Error ? error.message : 'Correction indisponible.'; }
    finally { markerSaving = false; }
  }
  export function open(media: Media | Promise<Media>) {
    if (opened) return false;
    generation++; const current = generation;
    triedMediaIds.clear();
    failoverInProgress = false;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    scrollPosition = window.scrollY;
    statistics = false; loadStage = 'Chargement de votre séance…'; audio = ''; active = media instanceof Promise ? undefined : media; opened = true; busy = true; error = ''; notice = ''; menu = null;
    keyboardPinned = false; visible = true; document.body.classList.add('immersive-open');
    dialog.showModal();
    surface?.focus();
    // Requested synchronously from the opening user gesture, before any network await.
    requestFullscreen();
    engine = createPlayer(video, message => { if (current === generation) fail(message); });
    engine.setStatusListener?.(message=>{if(current===generation)loadStage=message;});
    if (media instanceof Promise) {
      void media.then(item => { if (current === generation && opened) { active = item; void prepare(item); } })
        .catch(cause => { if (current === generation) fail(cause instanceof Error ? cause.message : String(cause)); });
    } else void prepare(media);
    return true;
  }
  export function close() {
    if (!opened) return;
    if (syncClient) {
      syncClient.leave();
      syncClient = null;
    }
    if (engine) progressTracker?.save(engine.state(), true);
    progressTracker = undefined;
    presence?.stop();presence=undefined;
    opened = false; generation++; const closedGeneration = generation; clearTimeout(idle); menu = null; markerEditor = false;
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
  export function joinParty(partyId: string, media: Media | Promise<Media>) {
    if (syncClient) {
      syncClient.leave();
      syncClient = null;
    }
    syncClient = new SyncPartyClient(partyId, $session?.user.id ?? '');
    setupSyncClientHooks();
    open(media);
  }
  function setupSyncClientHooks() {
    if (!syncClient) return;
    syncClient.onPlaybackUpdate = (state, position) => {
      if (state === 'playing') {
        if (engine && engine.state().paused) {
          void action(p => p.play());
        }
      } else if (state === 'paused') {
        if (engine && !engine.state().paused) {
          engine.pause();
        }
      }
      if (Math.abs(playback.position - position) > 1.5) {
        seek(position);
      }
    };
    syncClient.onCountdown = (_sec, pos) => {
      seek(pos);
    };
    syncClient.onMediaChanged = (newMediaId) => {
      const found = catalogue.find(m => m.id === newMediaId);
      if (found) changeMedia(found);
    };
  }
  function requestFullscreen() {
    if (!surface.requestFullscreen) { notice = 'Plein écran indisponible. Le mode immersif reste actif.'; return; }
    void surface.requestFullscreen().then(() => {
      if (!opened && document.fullscreenElement === surface) void document.exitFullscreen().catch(() => {});
    }).catch(() => { if (opened) notice = 'Plein écran refusé. Le mode immersif reste actif.'; });
  }
  function toggleFullscreen() {
    if (document.fullscreenElement === surface) void document.exitFullscreen().catch(() => {});
    else requestFullscreen();
    wake();
  }
  function togglePlayback() {
    if (syncClient && !syncClient.canControl) {
      notice = 'Contrôles réservés à l’organisateur';
      wake();
      return;
    }
    if (syncClient) {
      if (paused) syncClient.play(playback.position);
      else syncClient.pause(playback.position);
      return;
    }
    void action(async player => { if (player.state().paused) await player.play(); else player.pause(); });
  }
  function volume(value: number) { engine?.setVolume(value); if (value > 0) previousVolume = value; sync(); wake(); }
  function mute() { volume(playback.volume === 0 ? previousVolume || 1 : 0); }
  function seek(value: number) {
    if (syncClient && !syncClient.canControl) {
      notice = 'Contrôles réservés à l’organisateur';
      wake();
      return;
    }
    if (syncClient) {
      syncClient.seek(value);
    }
    if (!busy) { engine?.seek(value); sync(); wake(); }
  }
  function selectAudio(id: string) {
    const current = generation;
    const accountGeneration = getSessionGeneration();
    void action(async player => { await player.selectAudio(id);
      if (current !== generation || !opened) return;
      audio = id; notice = '';
      const language = audioLanguage(active?.sources.find(source => source.audioTrackId === id)?.language ?? 'und');
      if (language && language !== 'und') preferredLanguage = language;
      if (accountGeneration !== getSessionGeneration()) return;
      preferredAudio = { mediaId: active?.id ?? '', trackId: id };
      savePreferences({ audioTrack: preferredAudio, audioLanguage: preferredLanguage });
    });
  }
  function selectSubtitle(id: string | null) { void action(async player => {
    await player.selectSubtitle(id); subtitle = id;
    const track = active?.subtitles.find(track => track.id === id);
    subtitlePreference = track ? { enabled: true, language: track.language, forced: track.forced, sdh: track.sdh } : { enabled: false };
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
  function keyboard(event: KeyboardEvent) {
    if (!opened) return;
    keyboardPinned = true; wake();
    if (event.key === 'Escape') {
      event.preventDefault();
      if (statistics) { statistics = false; return; }
      if (menu) controls?.closeMenu();
      else if (document.fullscreenElement === surface) void document.exitFullscreen().catch(() => {});
      else close();
      return;
    }
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target?.closest('input, select, textarea, [role="menu"]')) return;
    if ((event.key === ' ' || event.key === 'Enter') && target?.closest('button, a')) return;
    if ([' ', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'm', 'M', 'f', 'F'].includes(event.key)) {
      event.preventDefault();
      if (event.key === ' ') togglePlayback();
      else if (event.key === 'ArrowLeft') seek(playback.position - 5);
      else if (event.key === 'ArrowRight') seek(playback.position + 5);
      else if (event.key === 'ArrowUp') volume(Math.min(1, playback.volume + .05));
      else if (event.key === 'ArrowDown') volume(Math.max(0, playback.volume - .05));
      else if (event.key.toLowerCase() === 'm') mute();
      else toggleFullscreen();
    }
  }
  function windowKeyboard(event: KeyboardEvent) {
    if (!opened || event.defaultPrevented) return;
    keyboard(event);
  }
  function cancel(event: Event) { event.preventDefault(); if (menu) controls?.closeMenu(); else close(); }
  function ended() { sync(); if (engine) progressTracker?.save(engine.state(), true, true); if (autoplayNext && onNext) onNext(); }
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
    return () => { close(); document.removeEventListener('fullscreenchange', change); clearTimeout(idle); };
  });
</script>

<svelte:window onkeydown={windowKeyboard} />

<dialog id="player" bind:this={dialog} class="immersive-dialog" aria-label="Lecteur immersif" oncancel={cancel} onclose={close}>
  <!-- The media shell owns focus and shortcuts; the controls do not know the playback implementation. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="player-shell" class:inactive={!visible && !paused} bind:this={surface} role="document" tabindex="-1" onkeydowncapture={() => { keyboardPinned = true; wake(); }} onkeydown={keyboard}
    onpointermove={event => { if (event.pointerType !== 'touch') wake(); }} onpointerdown={() => { keyboardPinned = false; surface?.focus(); }}>
    <video
      bind:this={video}
      playsinline
      preload="auto"
      loop={repeat}
      style:object-fit={aspectRatio}
      style:--sub-color={activeSubtitleCss.color}
      style:--sub-size={activeSubtitleCss.fontSize}
      style:--sub-font={activeSubtitleCss.fontFamily}
      style:--sub-bg={activeSubtitleCss.backgroundColor}
      style:--sub-shadow={activeSubtitleCss.textShadow}
      ontimeupdate={sync}
      ondurationchange={sync}
      onplay={sync}
      onpause={() => { sync(); if (!busy && engine) progressTracker?.save(engine.state(), true); }}
      onended={ended}
    ><track kind="captions" /></video>
    {#if active?.chapters?.length && visible}<label class="chapters">Chapitres<select aria-label="Chapitres vidéo" disabled={busy} onchange={event => seek(Number(event.currentTarget.value))}><option value="" disabled selected>Choisir un chapitre</option>{#each active.chapters as chapter,i (`${chapter.start}-${i}`)}<option value={chapter.start}>{i+1}. {chapter.title}</option>{/each}</select></label>{/if}
    <button class="video-interaction" tabindex="-1" aria-label="Afficher ou masquer les commandes" onclick={() => { if (menu) { controls?.closeMenu(); wake(); } else if (!paused && !busy && !error) { visible = !visible; if (visible) armIdle(); else surface?.focus(); } else wake(); }}></button>
    <div class="top-bar" class:concealed={!visible} inert={!visible}>
      <button class="return" aria-label="Retour à l’accueil" onclick={close}><Icon name="back" size={24} /></button>
      <div class="title"><span>{active?.title ?? 'Votre séance'}</span></div>
      {#if nextTitle && onNext}<div class="next-episode"><label title={nextTitle}><input type="checkbox" bind:checked={autoplayNext} /> Enchaîner automatiquement</label></div>{/if}
      {#if $session?.user.role === 'administrator' && active?.episode}<button type="button" onclick={() => editMarker('intro')}>Corriger les marqueurs</button>{/if}
    </div>
    {#if markerEditor && active?.episode}
      <section class="marker-editor" aria-label="Correction des marqueurs">
        <h2>Marqueur manuel</h2>
        <label>Type<select bind:value={markerKind} onchange={() => editMarker(markerKind)}><option value="intro">Introduction</option><option value="credits">Générique de fin</option><option value="recap">Résumé</option><option value="preview">Aperçu</option></select></label>
        <label><input type="checkbox" bind:checked={markerEnabled} /> Activer ce marqueur</label>
        {#if markerEnabled}
          <label>Début (secondes)<input type="number" min="0" step="0.1" bind:value={markerStart} /></label>
          <label>Fin (secondes)<input type="number" min="0" max={playback.duration} step="0.1" bind:value={markerEnd} /></label>
        {/if}
        {#if markerError}<p role="alert">{markerError}</p>{/if}
        <button type="button" disabled={markerSaving} onclick={() => void saveMarker()}>Enregistrer</button>
        <button type="button" disabled={markerSaving} onclick={() => void saveMarker(true)}>Revenir à la détection</button>
        <button type="button" onclick={() => markerEditor = false}>Fermer</button>
      </section>
    {/if}
    {#if busy}<div class="loading" role="status"><img src={asset('images/logo-loading.svg')} alt="" class="player-spinner" width="40" height="40" /><span>Chargement de la vidéo…</span><button onclick={close}>Annuler</button></div>{/if}
    {#if error}<div class="player-error" role="alert"><Icon name="alert" size={24} /><strong>Lecture interrompue</strong><p>{error}</p><button onclick={() => { if (active) void prepare(active, { ...playback, paused: false }); }}>Réessayer</button></div>{/if}
    {#if statistics}<section id="stream-statistics" class="stream-statistics" aria-label="Données de lecture">
      <header><strong>Données de lecture</strong><button onclick={() => { statistics = false; wake(); }}>Fermer</button></header>
      <p>Mesures vidéo du navigateur · actualisation chaque seconde</p>
      <dl>
        <dt>État</dt><dd>{busy ? 'Chargement' : error ? 'Erreur' : paused ? 'En pause' : 'Lecture'}</dd>
        <dt>Vidéo décodée</dt><dd>{metrics.width} × {metrics.height}</dd>
        <dt>Images / perdues</dt><dd>{metrics.frames} / {metrics.dropped}</dd>
        <dt>Vidéo en mémoire tampon</dt><dd>{metrics.buffered.toFixed(1)} s</dd>
        <dt>Audio séparé en mémoire tampon</dt><dd>{metrics.audioBuffered === null ? 'Non utilisé' : `${metrics.audioBuffered.toFixed(1)} s`}</dd>
        <dt>Position / durée</dt><dd>{playback.position.toFixed(1)} / {playback.duration.toFixed(1)} s</dd>
        <dt>Vitesse</dt><dd>{playback.speed}×</dd>
        <dt>readyState / networkState</dt><dd>{metrics.ready} / {metrics.network}</dd>
        <dt>Nœud serveur</dt><dd>{selectedSource?.nodeId ?? active?.nodeId ?? 'Local'}</dd>
        <dt>Mode de transport</dt><dd>{selectedSource?.directUrl ? 'Direct Node' : selectedSource?.relayUrl ? 'Relais Core' : 'Local'}</dd>
        <dt>Source / piste audio</dt><dd>{selectedSource?.id ?? 'En attente'} / {audio || 'En attente'}</dd>
        <dt>Conteneur / langue</dt><dd>{selectedSource?.mime ?? 'Non renseigné'} / {selectedSource?.language ?? 'Non renseignée'}</dd>
        <dt>Mode audio déclaré</dt><dd>{selectedSource?.audioSegmentUrl ? 'Segments' : selectedSource?.externalAudioUrl ? 'Séparé' : 'Interne'}</dd>
        <dt>Sous-titres</dt><dd>{subtitle ?? 'Désactivés'}</dd>
      </dl>
      {#each selectedSource?.tracks ?? [] as track, i (i)}<p>Piste {track.index ?? i} · {track.codec_type} · {track.codec_name} {track.profile ?? ''} {track.channels ? `· ${track.channels} canaux` : ''}</p>{/each}
      <p>Diagnostic technique : {error || loadStage}</p>
      <p>Les pistes déclarées ne prouvent pas la sortie audio physique. Débit réseau non mesuré.</p>
    </section>{/if}
    <p class="fullscreen-notice" role="status">{notice}</p>
    {#if syncClient}
      <WatchPartyOverlay client={syncClient} onCloseParty={() => { syncClient?.leave(); syncClient = null; }} />
    {/if}
    {#if active}<PlayerControls bind:this={controls} bind:menu {audioOffset} {subtitleOffset} onAudioOffset={(v) => offset('audio', v)} onSubtitleOffset={(v) => offset('subtitle', v)} state={playback} media={active} {catalogue} {audio} {subtitle} {visible} {busy} {fullscreen}
      {aspectRatio} onAspectRatio={r => aspectRatio = r} {repeat} onRepeat={r => repeat = r} onStats={() => { statistics = true; wake(); }}
      onPlay={togglePlayback} onVolume={volume} onMute={mute} onSeek={seek} onAudio={selectAudio} onSubtitle={selectSubtitle} onSource={selectSource}
      onSpeed={speed => action(player => player.setSpeed(speed))} onFullscreen={toggleFullscreen} onWake={wake} />{/if}
  </div>
</dialog>

<style>
  .marker-editor { position:absolute;z-index:10;top:90px;right:24px;display:grid;gap:10px;width:min(340px,calc(100% - 48px));padding:16px;background:#171923;color:white;border:1px solid #ffffff50;border-radius:10px; }
  .marker-editor label { display:grid;gap:4px; }
  .stream-statistics button { color:#eee; background:#ffffff15; border:1px solid #ffffff30; border-radius:8px; padding:10px 14px; cursor:pointer; }
  .stream-statistics { position:absolute; z-index:7; top:90px; right:24px; width:min(460px,calc(100% - 48px)); max-height:calc(100dvh - 230px); overflow:auto; padding:20px; box-sizing:border-box; background:rgba(16,17,25,.96); color:#eee; border:1px solid #ffffff25; border-radius:12px; font-size:12px; overflow-wrap:anywhere; }
  .stream-statistics header { display:flex; align-items:center; justify-content:space-between; gap:12px; }
  .stream-statistics dl { display:grid; grid-template-columns:1fr 1fr; gap:8px 16px; }
  .stream-statistics dd { margin:0; } .stream-statistics dt, .stream-statistics p { color:#bbb8ca; }

  .chapters { position:absolute;top:90px;right:24px;z-index:4;display:grid;gap:6px;max-width:70%; } .chapters select { padding:10px;max-width:100%;color:var(--color-text);background:var(--color-surface);border:1px solid var(--color-line);border-radius:6px; }
  video::cue {
    color: var(--sub-color, #ffffff);
    background: var(--sub-bg, transparent);
    text-shadow: var(--sub-shadow, 0 2px 4px rgba(0, 0, 0, 0.9));
    font-size: var(--sub-size, clamp(18px, 2.2vw, 30px));
    font-family: var(--sub-font, system-ui, -apple-system, sans-serif);
    line-height: 1.35;
  }
  .next-episode { margin-left:auto; max-width:50%; font-size:12px; } .next-episode label { display:flex; align-items:center; gap:8px; }
  .title { min-width:0; flex:1; } .title>span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  :global(body.immersive-open) { overflow: hidden; }
  .immersive-dialog { position: fixed; inset: 0; width: 100%; height: 100dvh; max-width: none; max-height: none; margin: 0; padding: 0; border: 0; background: #000; color: var(--color-text); overflow: hidden; }.immersive-dialog::backdrop { background: #000; }
  .player-shell { width: 100%; height: 100%; position: relative; background: #000; outline: 0; overflow: hidden; }.player-shell:fullscreen { width: 100vw; height: 100dvh; }.inactive { cursor: none; }.inactive :global(*) { cursor: none; }
  video { width: 100%; height: 100%; display: block; object-fit: contain; background: #000; }.video-interaction { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; padding: 0; background: transparent; }
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
    filter: drop-shadow(0 0 16px rgba(245, 158, 11, 0.35));
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
</style>

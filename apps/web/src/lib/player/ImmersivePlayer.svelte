<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import PlayerControls, { type Menu } from './PlayerControls.svelte';
  import type { Media, Player, PlayerFactory, PlaybackState } from './types';
  import { preferredSubtitle, type SubtitlePreference } from './subtitle-preference';
  import { ProgressTracker } from './progress';
  import { getSessionGeneration } from '../auth/session';
  import { preferences, savePreferences } from '../preferences';
  const base = resolve('/').replace(/\/$/, '');
  let { catalogue, createPlayer, nextTitle, onNext }: { catalogue: Media[]; createPlayer: PlayerFactory; nextTitle?: string; onNext?: () => void } = $props();
  let autoplayNext = $state(false);
  let dialog: HTMLDialogElement;
  let surface: HTMLDivElement;
  let video: HTMLVideoElement;
  let controls = $state<PlayerControls>();
  let engine: Player | undefined;
  let progressTracker: ProgressTracker | undefined;
  let opened = $state(false);
  let active = $state<Media | undefined>();
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let audio = $state('');
  let preferredLanguage = 'fr';
  let preferredAudio: { mediaId: string; trackId: string } | undefined;
  let audioOffset = $state(0);
  let subtitleOffset = $state(0);
  let subtitle = $state<string | null>(null);
  let subtitlePreference: SubtitlePreference | undefined;
  let menu = $state<Menu>(null);
  let fullscreen = $state(false);
  let visible = $state(true);
  let keyboardPinned = $state(false);
  let paused = $state(true);
  let playback = $state<PlaybackState>({ position: 0, duration: 0, paused: true, volume: 1, speed: 1 });
  let idle: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let opener: HTMLElement | null = null;
  let scrollPosition = 0;
  let previousVolume = 1;
  function sync() {
    if (!engine || !opened) return;
    const snapshot = engine.state();
    playback = { ...snapshot, duration: Number.isFinite(snapshot.duration) ? snapshot.duration : 0 };
    paused = snapshot.paused;
    if (!busy) progressTracker?.save(playback);
  }
  function armIdle() {
    clearTimeout(idle);
    if (!opened || paused || menu || busy || error || keyboardPinned) { visible = true; return; }
    idle = setTimeout(() => visible = false, 3000);
  }
  function wake() { visible = true; armIdle(); }
  $effect(() => { armIdle(); return () => clearTimeout(idle); });
  $effect(() => { if (opened && !visible) surface?.focus(); });
  function fail(message: string) {
    if (!opened) return;
    engine?.pause(); error = message; busy = false; sync(); wake();
  }
  async function action(operation: (player: Player) => void | Promise<void>) {
    if (!engine || busy) return;
    const current = generation; busy = true; error = ''; wake();
    try { await operation(engine); }
    catch (e) { if (current === generation) fail(e instanceof Error ? e.message : String(e)); }
    finally { if (current === generation) { busy = false; sync(); wake(); } }
  }
  async function prepare(media: Media, restore?: PlaybackState) {
    const current = generation;
    if (!engine) return;
    busy = true; error = ''; wake();
    try {
      const compatible=media.sources.filter(source=>video.canPlayType(source.mime));
      const source = media.sources.find(source => preferredAudio?.mediaId === media.id && source.audioTrackId === preferredAudio.trackId) ?? compatible.find(source => source.language === preferredLanguage) ?? compatible.find(source => source.language === 'fr') ?? compatible[0] ?? media.sources[0];
      if (!source) throw new Error('Aucune source audio utilisable pour ce média.');
      const tracker = !restore && media.workId && !media.synthetic
        ? new ProgressTracker(media.workId, message => { if (current === generation) notice = message; }) : undefined;
      if (tracker) progressTracker = tracker;
      const resume = tracker?.load();
      await engine.load(media, source.audioTrackId);
      if (current !== generation) return;
      if (source.language !== preferredLanguage) notice = 'Votre langue précédente est indisponible. Première source disponible sélectionnée.';
      await engine.selectAudio(source.audioTrackId); audio = source.audioTrackId;
      engine.setAudioOffset?.(audioOffset); engine.setSubtitleOffset?.(subtitleOffset);
      const usableSubtitle = preferredSubtitle(media.subtitles, subtitlePreference);
      await engine.selectSubtitle(usableSubtitle); subtitle = usableSubtitle;
      if (resume) {
        const position = await resume;
        if (current !== generation) return;
        if (position > 0) engine.seek(position);
      }
      if (restore) { engine.setVolume(restore.volume); engine.setSpeed(restore.speed); engine.seek(Math.min(restore.position, Math.max(0, engine.state().duration - .05))); }
      if (current !== generation) return;
      active = media;
      if (!restore || !restore.paused) await engine.play();
    } catch (e) { if (current === generation) fail(e instanceof Error ? e.message : String(e)); }
    finally { if (current === generation) { busy = false; sync(); wake(); } }
  }
  export function open(media: Media) {
    if (opened) return;
    preferredLanguage = 'fr'; subtitlePreference = undefined; preferredAudio = undefined; audioOffset = 0; subtitleOffset = 0;
    try {
      const account = preferences();
      const track = account.audioTrack;
      if (track && typeof track.mediaId === 'string' && typeof track.trackId === 'string') preferredAudio = track;
      const offsets = account.offsets;
      if (offsets && Number.isFinite(offsets.audio) && Math.abs(offsets.audio) <= 30) audioOffset = offsets.audio;
      if (offsets && Number.isFinite(offsets.subtitle) && Math.abs(offsets.subtitle) <= 30) subtitleOffset = offsets.subtitle;
      preferredLanguage = account.audioLanguage ?? 'fr';
      const initialSubtitles = account.subtitles;
      if (initialSubtitles === 'off') subtitlePreference = { enabled: false };
      else if (initialSubtitles === 'always') subtitlePreference = { enabled: true, language: 'fr', forced: false };
      else subtitlePreference = undefined;
    } catch { /* Preferences are optional when browser storage is unavailable. */ }
    generation++; const current = generation;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    scrollPosition = window.scrollY;
    active = media; opened = true; busy = true; error = ''; notice = ''; menu = null;
    keyboardPinned = false; visible = true; document.body.classList.add('immersive-open');
    dialog.showModal();
    surface?.focus();
    // Requested synchronously from the opening user gesture, before any network await.
    requestFullscreen();
    engine = createPlayer(video, message => { if (current === generation) fail(message); });
    void prepare(media);
  }
  export function close() {
    if (!opened) return;
    if (engine) progressTracker?.save(engine.state(), true);
    progressTracker = undefined;
    opened = false; generation++; const closedGeneration = generation; clearTimeout(idle); menu = null;
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
  export function changeMedia(media: Media) {
    if (!opened) { open(media); return; }
    if (!engine || busy) return;
    progressTracker?.save(engine.state(), true);
    progressTracker = undefined;
    active = media;
    // Keep the dialog, focus context and fullscreen through episode transitions.
    void prepare(media);
  }
  export function reportError(message: string) { fail(message); }
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
  function togglePlayback() { void action(async player => { if (player.state().paused) await player.play(); else player.pause(); }); }
  function volume(value: number) { engine?.setVolume(value); if (value > 0) previousVolume = value; sync(); wake(); }
  function mute() { volume(playback.volume === 0 ? previousVolume || 1 : 0); }
  function seek(value: number) { if (!busy) { engine?.seek(value); sync(); wake(); } }
  function selectAudio(id: string) {
    const accountGeneration = getSessionGeneration();
    void action(async player => { await player.selectAudio(id); audio = id; preferredLanguage = active?.sources.find(source => source.audioTrackId === id)?.language ?? preferredLanguage;
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
    <video bind:this={video} playsinline preload="metadata" ontimeupdate={sync} ondurationchange={sync} onplay={sync} onpause={() => { sync(); if (!busy && engine) progressTracker?.save(engine.state(), true); }} onended={ended}><track kind="captions" /></video>
    {#if active?.chapters?.length && visible}<label class="chapters">Chapitres<select aria-label="Chapitres vidéo" disabled={busy} onchange={event => seek(Number(event.currentTarget.value))}><option value="" disabled selected>Choisir un chapitre</option>{#each active.chapters as chapter,i (`${chapter.start}-${i}`)}<option value={chapter.start}>{i+1}. {chapter.title}</option>{/each}</select></label>{/if}
    <button class="video-interaction" tabindex="-1" aria-label="Afficher ou masquer les commandes" onclick={() => { if (menu) { controls?.closeMenu(); wake(); } else if (!paused && !busy && !error) { visible = !visible; if (visible) armIdle(); else surface?.focus(); } else wake(); }}></button>
    <div class="top-bar" class:concealed={!visible} inert={!visible}>
      <button class="return" aria-label="Retour à l’accueil" onclick={close}><Icon name="back" size={24} /></button>
      <div class="title"><span>{active?.title ?? 'Votre séance'}</span><small>LECTURE LOCALE{active?.synthetic ? ' · MIRE SYNTHÉTIQUE' : ''}</small></div>
      {#if nextTitle && onNext}<div class="next-episode"><button disabled={busy} onclick={onNext}>Épisode suivant : {nextTitle}</button><label><input type="checkbox" bind:checked={autoplayNext} /> Enchaîner automatiquement</label></div>{/if}
    </div>
    {#if busy}<div class="loading" role="status"><img src="{base}/images/logo-loading.svg" alt="" class="player-spinner" width="40" height="40" />Préparation de votre séance…</div>{/if}
    {#if error}<div class="player-error" role="alert"><Icon name="alert" size={24} /><strong>Lecture interrompue</strong><p>{error}</p><button onclick={() => { if (active) void prepare(active, playback); }}>Réessayer explicitement</button></div>{/if}
    <p class="fullscreen-notice" role="status">{notice}</p>
    {#if active}<PlayerControls bind:this={controls} bind:menu {audioOffset} {subtitleOffset} onAudioOffset={(v) => offset('audio', v)} onSubtitleOffset={(v) => offset('subtitle', v)} state={playback} media={active} {catalogue} {audio} {subtitle} {visible} {busy} {fullscreen}
      onPlay={togglePlayback} onVolume={volume} onMute={mute} onSeek={seek} onAudio={selectAudio} onSubtitle={selectSubtitle} onSource={selectSource}
      onSpeed={speed => action(player => player.setSpeed(speed))} onFullscreen={toggleFullscreen} onWake={wake} />{/if}
  </div>
</dialog>

<style>
  .chapters { position:absolute;top:90px;right:24px;z-index:4;display:grid;gap:6px;max-width:70%; } .chapters select { padding:10px;max-width:100%;color:var(--color-text);background:var(--color-surface);border:1px solid var(--color-line);border-radius:6px; }
  video::cue { color:#fff; background:rgba(0,0,0,.8); text-shadow:0 1px 2px #000; font-size:clamp(18px,2vw,30px); }
  .next-episode { margin-left:auto; max-width:50%; display:grid; gap:8px; font-size:12px; } .next-episode button { padding:10px; border:1px solid #ffffff40; border-radius:6px; color:inherit; background:#17121ee0; overflow-wrap:anywhere; } .next-episode label { display:flex; align-items:center; gap:8px; }
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
    border-color: rgba(167, 139, 250, 0.4);
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
  .title small {
    display: block;
    margin-top: 4px;
    font-size: 10.5px;
    letter-spacing: 0.14em;
    color: var(--color-accent);
    font-weight: 700;
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
    .title small {
      font-size: 9.5px;
      margin-top: 2px;
    }
    .fullscreen-notice {
      left: 18px;
      top: 80px;
      font-size: 10px;
      max-width: calc(100% - 36px);
    }
  }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import PlayerControls, { type Menu } from './PlayerControls.svelte';
  import type { Media, Player, PlayerFactory, PlaybackState } from './types';
  let { catalogue, createPlayer }: { catalogue: Media[]; createPlayer: PlayerFactory } = $props();
  let dialog: HTMLDialogElement;
  let surface: HTMLDivElement;
  let video: HTMLVideoElement;
  let controls = $state<PlayerControls>();
  let engine: Player | undefined;
  let opened = $state(false);
  let active = $state<Media | undefined>();
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let audio = $state('');
  let preferredLanguage = 'fr';
  let subtitle = $state<string | null>(null);
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
      await engine.load(media);
      if (current !== generation) return;
      const source = media.sources.find(source => source.language === preferredLanguage) ?? media.sources.find(source => source.language === 'fr') ?? media.sources[0];
      if (!source) throw new Error('Aucune source audio utilisable pour ce média.');
      if (source.language !== preferredLanguage) notice = 'Votre langue précédente est indisponible. Première source disponible sélectionnée.';
      await engine.selectAudio(source.audioTrackId); audio = source.audioTrackId;
      const usableSubtitle = media.subtitles.some(track => track.id === subtitle) ? subtitle : null;
      await engine.selectSubtitle(usableSubtitle); subtitle = usableSubtitle;
      if (restore) { engine.setVolume(restore.volume); engine.setSpeed(restore.speed); engine.seek(Math.min(restore.position, Math.max(0, engine.state().duration - .05))); }
      if (current !== generation) return;
      active = media;
      if (!restore || !restore.paused) await engine.play();
    } catch (e) { if (current === generation) fail(e instanceof Error ? e.message : String(e)); }
    finally { if (current === generation) { busy = false; sync(); wake(); } }
  }
  export function open(media: Media) {
    if (opened) return;
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
    void action(async player => { await player.selectAudio(id); audio = id; preferredLanguage = active?.sources.find(source => source.audioTrackId === id)?.language ?? preferredLanguage; });
  }
  function selectSubtitle(id: string | null) { void action(async player => { await player.selectSubtitle(id); subtitle = id; }); }
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
    <video bind:this={video} playsinline preload="metadata" ontimeupdate={sync} ondurationchange={sync} onplay={sync} onpause={sync} onended={sync}><track kind="captions" /></video>
    <button class="video-interaction" tabindex="-1" aria-label="Afficher ou masquer les commandes" onclick={() => { if (menu) { controls?.closeMenu(); wake(); } else if (!paused && !busy && !error) { visible = !visible; if (visible) armIdle(); else surface?.focus(); } else wake(); }}></button>
    <div class="top-bar" class:concealed={!visible} inert={!visible}>
      <button class="return" aria-label="Retour à l’accueil" onclick={close}><Icon name="back" size={24} /></button>
      <div class="title"><span>{active?.title ?? 'Votre séance'}</span><small>LECTURE LOCALE{active?.synthetic ? ' · MIRE SYNTHÉTIQUE' : ''}</small></div>
    </div>
    {#if busy}<div class="loading" role="status"><span class="loader"></span>Préparation de votre séance…</div>{/if}
    {#if error}<div class="player-error" role="alert"><Icon name="alert" size={24} /><strong>Lecture interrompue</strong><p>{error}</p><button onclick={() => { if (active) void prepare(active, playback); }}>Réessayer explicitement</button></div>{/if}
    <p class="fullscreen-notice" role="status">{notice}</p>
    {#if active}<PlayerControls bind:this={controls} bind:menu state={playback} media={active} {catalogue} {audio} {subtitle} {visible} {busy} {fullscreen}
      onPlay={togglePlayback} onVolume={volume} onMute={mute} onSeek={seek} onAudio={selectAudio} onSubtitle={selectSubtitle} onSource={selectSource}
      onSpeed={speed => action(player => player.setSpeed(speed))} onFullscreen={toggleFullscreen} onWake={wake} />{/if}
  </div>
</dialog>

<style>
  :global(body.immersive-open) { overflow: hidden; }
  .immersive-dialog { position: fixed; inset: 0; width: 100%; height: 100dvh; max-width: none; max-height: none; margin: 0; padding: 0; border: 0; background: #000; color: var(--color-text); overflow: hidden; }.immersive-dialog::backdrop { background: #000; }
  .player-shell { width: 100%; height: 100%; position: relative; background: #000; outline: 0; overflow: hidden; }.player-shell:fullscreen { width: 100vw; height: 100dvh; }.inactive { cursor: none; }.inactive :global(*) { cursor: none; }
  video { width: 100%; height: 100%; display: block; object-fit: contain; background: #000; }.video-interaction { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; padding: 0; background: transparent; }
  .top-bar { display: flex; position: absolute; inset: 0 0 auto; align-items: center; gap: 18px; z-index: 3; padding: max(24px,env(safe-area-inset-top)) max(32px,env(safe-area-inset-right)) 70px max(26px,env(safe-area-inset-left)); background: linear-gradient(#000b,transparent); transition: opacity .2s ease; }.concealed { opacity: 0; pointer-events: none; }
  .return { display: grid; place-items: center; width: 48px; height: 48px; border: 0; border-radius: 50%; background: #ffffff09; color: white; }.return:hover { background: #ffffff20; }.title>span { display: block; font-size: 16px; font-weight: 500; letter-spacing: -.02em; }.title small { display: block; margin-top: 6px; font-size: 10px; letter-spacing: .12em; color: #c4bacf; }
  .loading { position: absolute; inset: 40% 10% auto; display: flex; flex-direction: column; align-items: center; gap: 18px; pointer-events: none; color: #e6daff; font-size: 12px; text-shadow: 0 2px 5px #000; }.loader { border: 2px solid #ffffff30; border-top-color: var(--color-accent); width: 32px; height: 32px; border-radius: 50%; animation: spin 1s linear infinite; }@keyframes spin { to { transform: rotate(360deg); } }
  .player-error { position: absolute; z-index: 5; top: 50%; left: 50%; transform: translate(-50%,-50%); width: min(420px,calc(100% - 40px)); padding: 24px; background: #1b1420f5; border-radius: 12px; text-align: center; color: #edc4d2; }.player-error strong { display: block; margin: 12px 0; }.player-error p { font-size: 12px; line-height: 1.7; overflow-wrap: anywhere; }.player-error button { border: 0; border-radius: 5px; background: #eae1fa; color: #24172c; padding: 12px 16px; margin-top: 8px; font-size: 12px; }
  .fullscreen-notice { position: absolute; top: max(90px,env(safe-area-inset-top)); left: 32px; color: #c0b6cb; font-size: 10px; z-index: 3; pointer-events: none; text-shadow: 0 1px 4px black; }
  @media(max-width:700px) { .top-bar { padding: max(12px,env(safe-area-inset-top)) 14px 45px; gap: 12px; }.return { width: 44px; height: 44px; }.title>span { font-size: 13px; }.title small { font-size: 10px; margin-top: 4px; }.fullscreen-notice { left: 20px; top: 78px; font-size: 9px; max-width: calc(100% - 40px); } }
</style>

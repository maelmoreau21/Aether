<script lang="ts">
  import { tick } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { audioLabel, videoLabel } from './track-labels';
  import type { Media, PlaybackState } from './types';
  export type Menu = 'audio' | 'subtitles' | 'video' | 'speed' | null;
  let { state: playback, media, catalogue, audio, subtitle, visible, busy, menu = $bindable(null), fullscreen,
    onPlay, onVolume, onMute, onSeek, onAudio, onSubtitle, onSource, onSpeed, onFullscreen, onWake
  }: {
    state: PlaybackState; media: Media; catalogue: Media[]; audio: string; subtitle: string | null;
    visible: boolean; busy: boolean; menu?: Menu; fullscreen: boolean;
    onPlay: () => void; onVolume: (value: number) => void; onMute: () => void; onSeek: (value: number) => void;
    onAudio: (id: string) => void; onSubtitle: (id: string | null) => void; onSource: (id: string) => void;
    onSpeed: (speed: number) => void; onFullscreen: () => void; onWake: () => void;
  } = $props();
  let panel = $state<HTMLDivElement>();
  let triggers: HTMLDivElement;
  let hoverTime = $state<number | null>(null);
  const source = $derived(media.sources.find(source => source.audioTrackId === audio) ?? media.sources[0]);
  const videoTracks = $derived(source?.tracks?.filter(track => track.codec_type === 'video') ?? []);
  const title = $derived(menu === 'audio' ? 'Langue audio' : menu === 'subtitles' ? 'Sous-titres' : menu === 'video' ? 'Vidéo et sources' : 'Vitesse de lecture');
  function clock(seconds: number) {
    if (!Number.isFinite(seconds)) return '00:00';
    const hours = Math.floor(seconds / 3600);
    return `${hours ? hours + ':' : ''}${Math.floor(seconds / 60 % 60).toString().padStart(2, '0')}:${Math.floor(seconds % 60).toString().padStart(2, '0')}`;
  }
  export function closeMenu() {
    const active = menu; menu = null;
    triggers?.querySelector<HTMLButtonElement>(`[data-menu="${active}"]`)?.focus();
  }
  async function toggleMenu(value: Menu) {
    if (menu === value) { closeMenu(); return; }
    menu = value; onWake(); await tick();
    panel?.querySelector<HTMLButtonElement>('[role="menuitemradio"][aria-checked="true"]:not(:disabled), [role="menuitemradio"]:not(:disabled)')?.focus();
  }
  function navigateMenu(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeMenu(); return; }
    const items = Array.from(panel!.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled)'));
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
      event.preventDefault(); event.stopPropagation();
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length;
      items[next]?.focus();
    }
  }
  function choose(action: () => void) { closeMenu(); action(); }
</script>

<div class="player-controls" class:concealed={!visible} inert={!visible}>
  {#if menu}
    <!-- Menu navigation follows the ARIA menu keyboard pattern. -->
    <div class="floating-menu" bind:this={panel} role="menu" tabindex="-1" aria-label={title} onkeydown={navigateMenu}>
      <div class="menu-heading"><span>{title}</span><button aria-label="Fermer le menu" onclick={closeMenu}>×</button></div>
      {#if menu === 'audio'}
        {#each media.sources as item (item.id)}<button data-audio-id={item.audioTrackId} role="menuitemradio" aria-checked={item.audioTrackId === audio} disabled={busy} onclick={() => choose(() => onAudio(item.audioTrackId))}><span>{audioLabel(item)}</span>{#if item.audioTrackId === audio}<Icon name="check" size={18} />{/if}</button>{/each}
      {:else if menu === 'subtitles'}
        <button data-subtitle-id="off" role="menuitemradio" aria-checked={subtitle === null} disabled={busy} onclick={() => choose(() => onSubtitle(null))}><span>Désactivés</span>{#if subtitle === null}<Icon name="check" size={18} />{/if}</button>
        {#each media.subtitles as item (item.id)}<button data-subtitle-id={item.id} role="menuitemradio" aria-checked={item.id === subtitle} disabled={busy} onclick={() => choose(() => onSubtitle(item.id))}><span>{item.label}<small>WebVTT · {item.language.toUpperCase()}</small></span>{#if item.id === subtitle}<Icon name="check" size={18} />{/if}</button>{/each}
      {:else if menu === 'video'}
        <p class="menu-section">Piste vidéo de la source active</p>
        {#each videoTracks as track, index (track.index ?? index)}<div class="track-info"><Icon name="film" size={17} /><span>{videoLabel(track)}</span></div>{/each}
        <p class="menu-note">Sélection interne non disponible dans ce prototype.</p>
        <p class="menu-section">Sources de démonstration</p>
        {#each catalogue as item (item.id)}<button data-source-id={item.id} role="menuitemradio" aria-checked={item.id === media.id} disabled={busy} onclick={() => choose(() => onSource(item.id))}><span>{item.title}<small>Source préparée · même mire synthétique</small></span>{#if item.id === media.id}<Icon name="check" size={18} />{/if}</button>{/each}
      {:else if menu === 'speed'}
        {#each [0.5, 1, 1.5, 2] as speed (speed)}<button data-speed={speed} role="menuitemradio" aria-checked={playback.speed === speed} disabled={busy} onclick={() => choose(() => onSpeed(speed))}><span>{speed === 1 ? 'Normale · 1×' : `${speed.toLocaleString('fr')}×`}</span>{#if playback.speed === speed}<Icon name="check" size={18} />{/if}</button>{/each}
      {/if}
    </div>
  {/if}
  <div class="progress-line">
    {#if hoverTime !== null}<output class="seek-preview" aria-hidden="true">{clock(hoverTime)}</output>{/if}
    <input class="seek" aria-label="Position" type="range" min="0" max={playback.duration || 1} step="0.01" value={playback.position} disabled={busy || !playback.duration}
      oninput={event => onSeek(Number(event.currentTarget.value))}
      onpointermove={event => { const rect = event.currentTarget.getBoundingClientRect(); hoverTime = Math.max(0, Math.min(playback.duration, (event.clientX - rect.left) / rect.width * playback.duration)); }}
      onpointerleave={() => hoverTime = null} />
  </div>
  <div class="control-row">
    <div class="left-controls">
      <button class="primary" aria-label={playback.paused ? 'Lire' : 'Pause'} disabled={busy} onclick={onPlay}><Icon name={playback.paused ? 'play' : 'pause'} size={27} /></button>
      <button class="mute" aria-label={playback.volume === 0 ? 'Rétablir le son' : 'Couper le son'} aria-pressed={playback.volume === 0} onclick={onMute}><Icon name={playback.volume === 0 ? 'muted' : 'volume'} size={23} /></button>
      <input class="volume" aria-label="Volume" type="range" min="0" max="1" step="0.01" value={playback.volume} oninput={event => onVolume(Number(event.currentTarget.value))} />
      <span class="time" aria-label="{clock(playback.position)} sur {clock(playback.duration)}">{clock(playback.position)}<span> / {clock(playback.duration)}</span></span>
    </div>
    <div class="right-controls" bind:this={triggers}>
      <button data-menu="audio" aria-label="Audio" aria-haspopup="menu" aria-expanded={menu === 'audio'} onclick={() => toggleMenu('audio')} disabled={busy}><Icon name="globe" size={22} /><span class="control-label">Audio</span></button>
      <button data-menu="subtitles" aria-label="Sous-titres" aria-haspopup="menu" aria-expanded={menu === 'subtitles'} onclick={() => toggleMenu('subtitles')} disabled={busy}><Icon name="captions" size={24} /></button>
      <button data-menu="video" aria-label="Vidéo et sources" aria-haspopup="menu" aria-expanded={menu === 'video'} onclick={() => toggleMenu('video')} disabled={busy}><Icon name="film" size={22} /></button>
      <button data-menu="speed" aria-label="Vitesse" aria-haspopup="menu" aria-expanded={menu === 'speed'} onclick={() => toggleMenu('speed')} disabled={busy}><span class="speed-value">{playback.speed.toLocaleString('fr')}×</span></button>
      <button aria-label={fullscreen ? 'Quitter le plein écran' : 'Plein écran'} onclick={onFullscreen}><Icon name={fullscreen ? 'minimize' : 'fullscreen'} size={24} /></button>
    </div>
  </div>
</div>

<style>
  .player-controls { position: absolute; z-index: 4; bottom: 0; left: 0; right: 0; padding: 100px max(32px, env(safe-area-inset-right)) max(22px, env(safe-area-inset-bottom)) max(32px, env(safe-area-inset-left)); background: linear-gradient(transparent,#000b 55%,#000e); transition: opacity .2s ease; }
  .concealed { opacity: 0; pointer-events: none; }
  .control-row,.left-controls,.right-controls { display: flex; align-items: center; }.control-row { justify-content: space-between; gap: 16px; }.left-controls { gap: 14px; }.right-controls { gap: 12px; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-width: 44px; min-height: 44px; padding: 8px; background: transparent; color: #f7f5fa; border: 0; border-radius: 6px; }button:hover:not(:disabled) { background: #ffffff19; }button[aria-expanded='true'] { color: var(--color-accent); background: #b4a1f820; }
  .primary :global(svg) { fill: currentColor; stroke-width: 1.8; }.volume { width: 80px; }.time { margin-left: 8px; font-size: 12px; font-variant-numeric: tabular-nums; color: #f0edf5; white-space: nowrap; }.time>span { color: #a5a0ae; }.control-label { font-size: 12px; }.speed-value { font-size: 13px; }
  .progress-line { position: relative; margin-bottom: 12px; }.seek { width: 100%; height: 24px; margin: 0; accent-color: var(--color-accent); }.seek-preview { position: absolute; right: 0; top: -25px; font-size: 11px; font-variant-numeric: tabular-nums; background: #17131fe6; padding: 5px 8px; border-radius: 4px; }
  .floating-menu { position: absolute; bottom: calc(100% - 80px); right: 32px; width: 340px; max-height: min(460px,calc(100dvh - 200px)); overflow-y: auto; padding: 8px; border-radius: 12px; background: #18151ff5; box-shadow: 0 12px 50px #0008; border: 1px solid #ffffff15; backdrop-filter: blur(20px); }
  .menu-heading { display: flex; align-items: center; justify-content: space-between; padding: 6px 8px 8px 14px; font-size: 13px; font-weight: 600; border-bottom: 1px solid #ffffff10; margin-bottom: 6px; }.menu-heading button { font-size: 24px; font-weight: 300; }.floating-menu button[role] { width: 100%; min-height: 48px; padding: 12px 14px; justify-content: space-between; text-align: left; font-size: 13px; gap: 16px; }.floating-menu button[aria-checked='true'] { color: #d1c1ff; background: #b4a1f815; }.floating-menu small { display: block; font-size: 10px; color: #a39aac; margin-top: 6px; }.menu-section { font-size: 10px; letter-spacing: .06em; color: #aaa0b7; margin: 18px 14px 10px; }.menu-note { font-size: 10px; color: #b4a8c2; line-height: 1.6; margin: 12px 14px; }.track-info { display: flex; gap: 10px; padding: 8px 14px; font-size: 12px; color: #ddd5ea; }
  @media(max-width:700px) { .player-controls { padding: 80px max(16px,env(safe-area-inset-right)) max(14px,env(safe-area-inset-bottom)) max(16px,env(safe-area-inset-left)); }.control-row { flex-wrap: wrap; gap: 2px; }.left-controls { gap: 4px; }.right-controls { margin-left: auto; gap: 2px; }.control-label { display: none; }.volume { width: 58px; }.time { font-size: 10px; margin-left: 7px; }.floating-menu { right: 16px; bottom: calc(100% - 60px); width: min(340px,calc(100vw - 32px)); }.progress-line { margin-bottom: 8px; } }
  @media(max-height:500px) { .player-controls { padding-top: 50px; padding-bottom: 8px; }.floating-menu { bottom: calc(100% - 35px); max-height: calc(100dvh - 145px); }.control-row { flex-wrap: nowrap; }.volume { width: 45px; } }
</style>

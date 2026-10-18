<script lang="ts">
  import { tick } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { audioLabel, videoLabel, videoResolutionBadge, containerLabel } from './track-labels';
  import type { Media, PlaybackState } from './types';

  export type Menu = 'audio' | 'subtitles' | 'video' | 'speed' | null;

  let {
    state: playback,
    media,
    catalogue,
    audio,
    subtitle,
    visible,
    busy,
    menu = $bindable(null),
    fullscreen,
    onPlay,
    onVolume,
    onMute,
    onSeek,
    onAudio,
    onSubtitle,
    onSource,
    onSpeed,
    onFullscreen,
    audioOffset = 0, subtitleOffset = 0, onAudioOffset, onSubtitleOffset,
    onWake
  }: {
    audioOffset?: number; subtitleOffset?: number; onAudioOffset?: (value: number) => void; onSubtitleOffset?: (value: number) => void;
    state: PlaybackState;
    media: Media;
    catalogue: Media[];
    audio: string;
    subtitle: string | null;
    visible: boolean;
    busy: boolean;
    menu?: Menu;
    fullscreen: boolean;
    onPlay: () => void;
    onVolume: (value: number) => void;
    onMute: () => void;
    onSeek: (value: number) => void;
    onAudio: (id: string) => void;
    onSubtitle: (id: string | null) => void;
    onSource: (id: string) => void;
    onSpeed: (speed: number) => void;
    onFullscreen: () => void;
    onWake: () => void;
  } = $props();

  let panel = $state<HTMLDivElement>();
  let triggers: HTMLDivElement;
  let hoverTime = $state<number | null>(null);

  const source = $derived(media.sources.find(source => source.audioTrackId === audio) ?? media.sources[0]);
  const videoTracks = $derived((source?.tracks?.some(track => track.codec_type === 'video') ? source.tracks : media.sources[0]?.tracks)?.filter(track => track.codec_type === 'video') ?? []);
  const remainingTime = $derived(Math.max(0, (playback.duration || 0) - (playback.position || 0)));

  const title = $derived(
    menu === 'audio'
      ? 'Langue audio'
      : menu === 'subtitles'
        ? 'Sous-titres'
        : menu === 'video'
          ? 'Vidéo et sources'
          : 'Vitesse de lecture'
  );

  function clock(seconds: number) {
    if (!Number.isFinite(seconds)) return '00:00';
    const hours = Math.floor(seconds / 3600);
    return `${hours ? hours + ':' : ''}${Math.floor((seconds / 60) % 60).toString().padStart(2, '0')}:${Math.floor(seconds % 60).toString().padStart(2, '0')}`;
  }

  export function closeMenu() {
    const active = menu;
    menu = null;
    triggers?.querySelector<HTMLButtonElement>(`[data-menu="${active}"]`)?.focus();
  }

  async function toggleMenu(value: Menu) {
    if (menu === value) {
      closeMenu();
      return;
    }
    menu = value;
    onWake();
    await tick();
    const items = panel ? Array.from(panel.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled), [role="menuitem"]:not(:disabled)')) : [];
    const target = panel?.querySelector<HTMLButtonElement>('[role="menuitemradio"][aria-checked="true"]:not(:disabled)') ?? items[0];
    if (target) {
      for (const item of items) item.tabIndex = -1;
      target.tabIndex = 0;
      target.focus();
    }
  }

  function navigateMenu(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      closeMenu();
      return;
    }
    if (event.key === 'Tab') {
      closeMenu();
      return;
    }
    if (event.target instanceof HTMLInputElement) return;
    const items = Array.from(panel!.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled), [role="menuitem"]:not(:disabled)'));
    if (items.length === 0) return;
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      event.stopPropagation();
      let next: number;
      if (event.key === 'Home') next = 0;
      else if (event.key === 'End') next = items.length - 1;
      else if (index === -1) next = event.key === 'ArrowUp' ? items.length - 1 : 0;
      else next = (index + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length;
      for (const item of items) item.tabIndex = -1;
      items[next].tabIndex = 0;
      items[next]?.focus();
    }
  }

  function choose(action: () => void) {
    closeMenu();
    action();
  }
</script>

<div class="player-controls" class:concealed={!visible} inert={!visible}>
  {#if menu}
    <!-- Menu navigation follows the ARIA menu keyboard pattern. -->
    <div
      class="floating-menu"
      bind:this={panel}
      id="player-menu"
      role="menu"
      tabindex="-1"
      aria-label={title}
      onkeydown={navigateMenu}
    >
      <div class="menu-heading" role="none">
        <span>{title}</span>
        <button type="button" class="menu-close" aria-label="Fermer le menu" tabindex="-1" onclick={closeMenu}>×</button>
      </div>

      {#if menu === 'audio'}
        <div role="group" aria-label="Langues audio">
          {#each media.sources as item (item.id)}
            <button
              data-audio-id={item.audioTrackId}
              role="menuitemradio"
              tabindex={item.audioTrackId === audio ? 0 : -1}
              aria-checked={item.audioTrackId === audio}
              disabled={busy}
              onclick={() => choose(() => onAudio(item.audioTrackId))}
            >
              <span>{audioLabel(item)}</span>
              {#if item.audioTrackId === audio}
                <Icon name="check" size={18} />
              {/if}
            </button>
          {/each}
        </div>
        <button role="menuitem" tabindex="-1" disabled={busy || !source?.externalAudioUrl} onclick={() => panel?.querySelector<HTMLInputElement>('input')?.focus()}>Régler le décalage audio</button>
        <label class="offset-control">Décalage audio (s)
          <input aria-label="Décalage audio en secondes" type="number" min="-30" max="30" step="0.05" value={audioOffset} disabled={busy || !source?.externalAudioUrl} onchange={(e) => onAudioOffset?.(e.currentTarget.valueAsNumber)} />
        </label>
        <p class="menu-note">Positif : retarder. Appliqué aux pistes préparées et externes.</p>
      {:else if menu === 'subtitles'}
        <div role="group" aria-label="Pistes de sous-titres">
          <button
            data-subtitle-id="off"
            role="menuitemradio"
            tabindex={subtitle === null ? 0 : -1}
            aria-checked={subtitle === null}
            disabled={busy}
            onclick={() => choose(() => onSubtitle(null))}
          >
            <span>Désactivés</span>
            {#if subtitle === null}
              <Icon name="check" size={18} />
            {/if}
          </button>
          {#each media.subtitles as item (item.id)}
            <button
              data-subtitle-id={item.id}
              role="menuitemradio"
              tabindex={item.id === subtitle ? 0 : -1}
              aria-checked={item.id === subtitle}
              disabled={busy}
              onclick={() => choose(() => onSubtitle(item.id))}
            >
              <span>{item.label}<small>WebVTT · {item.language.toUpperCase()}</small></span>
              {#if item.id === subtitle}
                <Icon name="check" size={18} />
              {/if}
            </button>
          {/each}
        </div>
        <button role="menuitem" tabindex="-1" disabled={busy} onclick={() => panel?.querySelector<HTMLInputElement>('input')?.focus()}>Régler le décalage des sous-titres</button>
        <label class="offset-control">Décalage sous-titres (s)
          <input aria-label="Décalage sous-titres en secondes" type="number" min="-30" max="30" step="0.05" value={subtitleOffset} disabled={busy} onchange={(e) => onSubtitleOffset?.(e.currentTarget.valueAsNumber)} />
        </label>
        <p class="menu-note">Positif : retarder. ASS/SSA : texte, styles avancés non reproduits.</p>
      {:else if menu === 'video'}
        <div role="group" aria-label="Piste vidéo de la source active">
          <p class="menu-section" role="presentation">Piste vidéo active</p>
          {#each videoTracks as track, index (track.index ?? index)}
            <div class="track-info" role="note">
              <Icon name="film" size={17} />
              <div>
                <span>{videoLabel(track)}</span>
                {#if videoResolutionBadge(track.width, track.height)}
                  <small class="track-badge">{videoResolutionBadge(track.width, track.height)}</small>
                {/if}
              </div>
            </div>
          {/each}
          <p class="menu-note" role="note">
            Conteneur : {containerLabel(source?.mime)} · Décodage assuré par le navigateur.
          </p>
        </div>
        <div role="separator" class="menu-separator"></div>
        <div role="group" aria-label="Sources de démonstration">
          <p class="menu-section" role="presentation">Versions / Sources disponibles</p>
          {#each catalogue as item (item.id)}
            <button
              data-source-id={item.id}
              role="menuitemradio"
              tabindex={item.id === media.id ? 0 : -1}
              aria-checked={item.id === media.id}
              disabled={busy}
              onclick={() => choose(() => onSource(item.id))}
            >
              <span>{item.title}<small>{item.synthetic ? 'Source préparée · même mire synthétique' : 'Fichier local indexé'}</small></span>
              {#if item.id === media.id}
                <Icon name="check" size={18} />
              {/if}
            </button>
          {/each}
        </div>
      {:else if menu === 'speed'}
        <div role="group" aria-label="Vitesses de lecture">
          {#each [0.5, 0.75, 1, 1.25, 1.5, 2] as speed (speed)}
            <button
              data-speed={speed}
              role="menuitemradio"
              tabindex={playback.speed === speed ? 0 : -1}
              aria-checked={playback.speed === speed}
              disabled={busy}
              onclick={() => choose(() => onSpeed(speed))}
            >
              <span>{speed === 1 ? 'Normale · 1×' : `${speed.toLocaleString('fr')}×`}</span>
              {#if playback.speed === speed}
                <Icon name="check" size={18} />
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <div class="progress-line">
    {#if hoverTime !== null}
      <output class="seek-preview" aria-hidden="true">{clock(hoverTime)}</output>
    {/if}
    <input
      class="seek"
      aria-label="Position"
      type="range"
      min="0"
      max={playback.duration || 1}
      step="0.01"
      value={playback.position}
      disabled={busy || !playback.duration}
      oninput={event => onSeek(Number(event.currentTarget.value))}
      onpointermove={event => {
        const rect = event.currentTarget.getBoundingClientRect();
        hoverTime = Math.max(0, Math.min(playback.duration, ((event.clientX - rect.left) / rect.width) * playback.duration));
      }}
      onpointerleave={() => (hoverTime = null)}
    />
  </div>

  <div class="control-row">
    <div class="left-controls">
      <button
        class="skip-btn"
        aria-label="Reculer de 10 secondes"
        disabled={busy || !playback.duration}
        onclick={() => onSeek(Math.max(0, playback.position - 10))}
      >
        <span class="skip-icon">↺</span>
        <span class="skip-num">10</span>
      </button>

      <button
        class="primary play-btn"
        aria-label={playback.paused ? 'Lire' : 'Pause'}
        disabled={busy}
        onclick={onPlay}
      >
        <Icon name={playback.paused ? 'play' : 'pause'} size={26} />
      </button>

      <button
        class="skip-btn"
        aria-label="Avancer de 10 secondes"
        disabled={busy || !playback.duration}
        onclick={() => onSeek(Math.min(playback.duration, playback.position + 10))}
      >
        <span class="skip-icon">↻</span>
        <span class="skip-num">10</span>
      </button>

      <div class="volume-group">
        <button
          class="mute"
          aria-label={playback.volume === 0 ? 'Rétablir le son' : 'Couper le son'}
          aria-pressed={playback.volume === 0}
          onclick={onMute}
        >
          <Icon name={playback.volume === 0 ? 'muted' : 'volume'} size={22} />
        </button>
        <input
          class="volume"
          aria-label="Volume"
          type="range"
          min="0"
          max="1"
          step="0.01"
          value={playback.volume}
          oninput={event => onVolume(Number(event.currentTarget.value))}
        />
      </div>

      <span class="time" aria-label="{clock(playback.position)} sur {clock(playback.duration)}">
        <span class="current-time">{clock(playback.position)}</span>
        <span class="duration-time"> / {clock(playback.duration)}</span>
        {#if playback.duration > 0}
          <span class="remaining-time"> (-{clock(remainingTime)})</span>
        {/if}
      </span>
    </div>

    <div class="right-controls" bind:this={triggers}>
      <button
        data-menu="audio"
        aria-label="Audio"
        aria-haspopup="menu"
        aria-expanded={menu === 'audio'}
        aria-controls={menu === 'audio' ? 'player-menu' : undefined}
        onclick={() => toggleMenu('audio')}
        disabled={busy}
      >
        <Icon name="globe" size={20} />
        <span class="control-label">Audio</span>
      </button>

      <button
        data-menu="subtitles"
        aria-label="Sous-titres"
        aria-haspopup="menu"
        aria-expanded={menu === 'subtitles'}
        aria-controls={menu === 'subtitles' ? 'player-menu' : undefined}
        onclick={() => toggleMenu('subtitles')}
        disabled={busy}
      >
        <Icon name="captions" size={22} />
        <span class="control-label">Sous-titres</span>
      </button>

      <button
        data-menu="video"
        aria-label="Vidéo et sources"
        aria-haspopup="menu"
        aria-expanded={menu === 'video'}
        aria-controls={menu === 'video' ? 'player-menu' : undefined}
        onclick={() => toggleMenu('video')}
        disabled={busy}
      >
        <Icon name="film" size={20} />
        <span class="control-label">Vidéo</span>
      </button>

      <button
        data-menu="speed"
        aria-label="Vitesse"
        aria-haspopup="menu"
        aria-expanded={menu === 'speed'}
        aria-controls={menu === 'speed' ? 'player-menu' : undefined}
        onclick={() => toggleMenu('speed')}
        disabled={busy}
      >
        <span class="speed-value">{playback.speed.toLocaleString('fr')}×</span>
      </button>

      <button
        aria-label={fullscreen ? 'Quitter le plein écran' : 'Plein écran'}
        onclick={onFullscreen}
      >
        <Icon name={fullscreen ? 'minimize' : 'fullscreen'} size={22} />
      </button>
    </div>
  </div>
</div>

<style>
  .offset-control { display: flex; align-items: center; gap: 1rem; padding: .75rem; }
  .offset-control input { width: 6rem; color: inherit; background: #202020; border: 1px solid #666; border-radius: .25rem; padding: .4rem; }
  .player-controls {
    position: absolute;
    z-index: 4;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 100px max(32px, env(safe-area-inset-right)) max(22px, env(safe-area-inset-bottom)) max(32px, env(safe-area-inset-left));
    background: linear-gradient(transparent, #000b 55%, #000e);
    transition: opacity 0.2s ease;
  }
  .concealed {
    opacity: 0;
    pointer-events: none;
  }

  .control-row, .left-controls, .right-controls {
    display: flex;
    align-items: center;
  }
  .control-row {
    justify-content: space-between;
    gap: 16px;
  }
  .left-controls {
    gap: 10px;
  }
  .right-controls {
    gap: 10px;
  }

  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 42px;
    min-height: 42px;
    padding: 8px 10px;
    background: transparent;
    color: #f7f5fa;
    border: 0;
    border-radius: 6px;
    transition: background 0.15s, color 0.15s;
  }
  button:hover:not(:disabled) {
    background: #ffffff20;
    color: white;
  }
  button[aria-expanded='true'] {
    color: var(--color-accent);
    background: #b4a1f825;
  }

  .primary.play-btn {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    background: #ffffff18;
    margin: 0 4px;
  }
  .primary.play-btn:hover:not(:disabled) {
    background: var(--color-accent);
    color: #121018;
  }
  .primary.play-btn :global(svg) {
    fill: currentColor;
    stroke-width: 1.8;
  }

  .skip-btn {
    font-size: 11px;
    position: relative;
    width: 38px;
    height: 38px;
  }
  .skip-icon {
    font-size: 16px;
    line-height: 1;
  }
  .skip-num {
    font-size: 9px;
    position: absolute;
    bottom: 6px;
    font-weight: 600;
  }

  .volume-group {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .volume {
    width: 80px;
    accent-color: var(--color-accent);
  }

  .time {
    margin-left: 8px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: #f0edf5;
    white-space: nowrap;
  }
  .duration-time {
    color: #a5a0ae;
  }
  .remaining-time {
    color: #8c8699;
    font-size: 11px;
  }

  .control-label {
    font-size: 12px;
  }
  .speed-value {
    font-size: 12px;
    font-weight: 600;
  }

  .progress-line {
    position: relative;
    margin-bottom: 12px;
  }
  .seek {
    width: 100%;
    height: 24px;
    margin: 0;
    accent-color: var(--color-accent);
  }
  .seek-preview {
    position: absolute;
    right: 0;
    top: -26px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    background: #17131fe6;
    padding: 4px 8px;
    border-radius: 4px;
    border: 1px solid var(--color-line);
  }

  .floating-menu {
    position: absolute;
    bottom: calc(100% - 80px);
    right: 32px;
    width: 340px;
    max-height: min(460px, calc(100dvh - 200px));
    overflow-y: auto;
    padding: 8px;
    border-radius: 12px;
    background: #18151ff5;
    box-shadow: 0 12px 50px #0008;
    border: 1px solid #ffffff15;
    backdrop-filter: blur(20px);
  }
  .menu-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px 8px 14px;
    font-size: 13px;
    font-weight: 600;
    border-bottom: 1px solid #ffffff10;
    margin-bottom: 6px;
  }
  .menu-heading button {
    width: auto;
    min-width: 40px;
    min-height: 40px;
    padding: 6px;
    font-size: 22px;
    font-weight: 300;
    line-height: 1;
  }

  .floating-menu button[role="menuitemradio"] {
    width: 100%;
    min-height: 48px;
    padding: 12px 14px;
    justify-content: space-between;
    text-align: left;
    font-size: 13px;
    gap: 16px;
  }
  .floating-menu button[aria-checked='true'] {
    color: #d1c1ff;
    background: #b4a1f818;
  }
  .floating-menu small {
    display: block;
    font-size: 10px;
    color: #a39aac;
    margin-top: 4px;
  }

  .menu-section {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #aaa0b7;
    margin: 16px 14px 8px;
    font-weight: 600;
  }
  .menu-note {
    font-size: 11px;
    color: #b4a8c2;
    line-height: 1.5;
    margin: 10px 14px;
  }
  .track-info {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 8px 14px;
    font-size: 12px;
    color: #ddd5ea;
  }
  .track-badge {
    display: inline-block;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 4px;
    margin-left: 6px;
  }
  .menu-separator {
    border-top: 1px solid #ffffff10;
    margin: 8px 14px;
  }

  @media(max-width: 768px) {
    .player-controls {
      padding: 48px max(14px, env(safe-area-inset-right)) max(12px, env(safe-area-inset-bottom)) max(14px, env(safe-area-inset-left));
    }
    .control-row {
      flex-wrap: wrap;
      row-gap: 8px;
    }
    .volume-group {
      display: none;
    }
    .control-label {
      display: none;
    }
    .floating-menu {
      right: 14px;
      bottom: calc(100% - 60px);
      width: min(340px, calc(100vw - 28px));
    }
  }
</style>

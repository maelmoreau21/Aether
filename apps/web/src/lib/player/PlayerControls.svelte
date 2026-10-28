<script lang="ts">
  import { tick } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { audioLabel, videoLabel, videoResolutionBadge, containerLabel } from './track-labels';
  import { audioChoices } from './available-audio';
  import type { Media, PlaybackState } from './types';

  export type Menu = 'audio' | 'subtitles' | 'video' | 'speed' | 'settings' | null;

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
    aspectRatio = 'contain', onAspectRatio, repeat = false, onRepeat, onStats,
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
    aspectRatio?: 'contain' | 'cover' | 'fill';
    onAspectRatio?: (ratio: 'contain' | 'cover' | 'fill') => void;
    repeat?: boolean;
    onRepeat?: (repeat: boolean) => void;
    onStats?: () => void;
    onWake: () => void;
  } = $props();

  let panel = $state<HTMLDivElement>();
  let triggers: HTMLDivElement;
  let hoverTime = $state<number | null>(null);
  let canSkipIntro = $derived(Boolean(media?.intro && playback.position >= media.intro.start && playback.position < media.intro.end));
  let canSkipCredits = $derived(Boolean(media?.credits && playback.position >= media.credits.start && playback.position < media.credits.end));

  const source = $derived(media.sources.find(source => source.audioTrackId === audio) ?? media.sources[0]);
  const selectableAudio = $derived(audioChoices(media, audio));
  const selectableVersions = $derived(catalogue.filter(item => item.available !== false && item.nodeStatus !== 'revoked' && item.nodeStatus !== 'disconnected'));
  $effect(() => { if (menu === 'audio' && selectableAudio.length <= 1) menu = null; });
  const videoTracks = $derived((source?.tracks?.some(track => track.codec_type === 'video') ? source.tracks : media.sources[0]?.tracks)?.filter(track => track.codec_type === 'video') ?? []);
  const remainingTime = $derived(Math.max(0, (playback.duration || 0) - (playback.position || 0)));

  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => { now = Date.now(); }, 5000);
    return () => clearInterval(timer);
  });

  const projectedEndTime = $derived.by(() => {
    if (!Number.isFinite(remainingTime) || remainingTime <= 0 || !playback.duration) return '';
    const speed = playback.speed > 0 ? playback.speed : 1;
    const end = new Date(now + (remainingTime / speed) * 1000);
    const hours = end.getHours().toString().padStart(2, '0');
    const minutes = end.getMinutes().toString().padStart(2, '0');
    return `${hours}h${minutes}`;
  });

  function skipChapter(direction: -1 | 1) {
    const chapters = media?.chapters;
    if (!chapters || chapters.length === 0) return;
    const sorted = [...chapters].sort((a, b) => a.start - b.start);
    const pos = playback.position;

    if (direction === 1) {
      const next = sorted.find(c => c.start > pos + 1);
      if (next) {
        onSeek(next.start);
      } else if (playback.duration) {
        onSeek(playback.duration);
      }
    } else {
      let currentIndex = -1;
      for (let i = 0; i < sorted.length; i++) {
        if (sorted[i].start <= pos + 0.5) {
          currentIndex = i;
        } else {
          break;
        }
      }

      if (currentIndex === -1) {
        onSeek(0);
      } else {
        const currentChapter = sorted[currentIndex];
        if (pos - currentChapter.start > 3) {
          onSeek(currentChapter.start);
        } else if (currentIndex > 0) {
          onSeek(sorted[currentIndex - 1].start);
        } else {
          onSeek(0);
        }
      }
    }
  }

  const title = $derived(
    menu === 'audio'
      ? 'Langue audio'
      : menu === 'subtitles'
        ? 'Sous-titres'
        : menu === 'video'
          ? 'Vidéo et sources'
          : menu === 'settings'
            ? 'Paramètres'
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
        {#if menu === 'speed'}
          <button type="button" class="menu-back" aria-label="Retour aux paramètres" tabindex="-1" onclick={() => menu = 'settings'}>
            <Icon name="back" size={18} />
          </button>
        {/if}
        <span>{title}</span>
        <button type="button" class="menu-close" aria-label="Fermer le menu" tabindex="-1" onclick={closeMenu}>×</button>
      </div>

      {#if menu === 'audio'}
        <div role="group" aria-label="Langues audio">
          {#each selectableAudio as item (item.id)}
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
          <input aria-label="Décalage sous-titres en secondes" type="number" min="-30" max="30" step="0.05" value={subtitleOffset} disabled={busy} oninput={(e) => onSubtitleOffset?.(e.currentTarget.valueAsNumber)} onchange={(e) => onSubtitleOffset?.(e.currentTarget.valueAsNumber)} />
        </label>
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
        {#if selectableVersions.length > 1}
        <div role="separator" class="menu-separator"></div>
        <div role="group" aria-label="Sources de démonstration">
          <p class="menu-section" role="presentation">Versions / Sources disponibles</p>
          {#each selectableVersions as item (item.id)}
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
        {/if}
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
      {:else if menu === 'settings'}
        <div role="group" aria-label="Paramètres du lecteur">
          <button
            type="button"
            role="menuitem"
            onclick={() => {
              const next = aspectRatio === 'contain' ? 'cover' : aspectRatio === 'cover' ? 'fill' : 'contain';
              onAspectRatio?.(next);
            }}
          >
            <span>Format d'image</span>
            <span class="setting-value">{aspectRatio === 'cover' ? 'Remplir' : aspectRatio === 'fill' ? 'Étirer' : 'Normal'}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            onclick={() => menu = 'speed'}
          >
            <span>Vitesse de lecture</span>
            <span class="setting-value">{playback.speed === 1 ? '1x' : `${playback.speed.toLocaleString('fr')}x`}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            onclick={() => onRepeat?.(!repeat)}
          >
            <span>Mode de répétition</span>
            <span class="setting-value">{repeat ? 'Répéter' : 'Aucun'}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            onclick={() => {
              closeMenu();
              onStats?.();
            }}
          >
            <span>Données de lecture</span>
            <span class="setting-arrow">›</span>
          </button>
        </div>
      {/if}
    </div>
  {/if}

  {#if canSkipIntro}

    <div class="skip-marker-overlay">

      <button type="button" class="skip-marker-btn" onclick={() => { onSeek(media.intro!.end); onWake(); }}>

        <span>Passer l'introduction</span>

        <span class="skip-marker-arrow">›</span>

      </button>

    </div>

  {:else if canSkipCredits}

    <div class="skip-marker-overlay">

      <button type="button" class="skip-marker-btn" onclick={() => { onSeek(media.credits!.end); onWake(); }}>

        <span>Passer les crédits</span>

        <span class="skip-marker-arrow">›</span>

      </button>

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
      onchange={event => onSeek(Number(event.currentTarget.value))}
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
        class="control-btn"
        aria-label="Reculer de 10 secondes"
        title="Reculer de 10 secondes"
        disabled={busy || !playback.duration}
        onclick={() => onSeek(Math.max(0, playback.position - 10))}
      >
        <Icon name="jump-backward" size={22} />
      </button>

      <button
        class="control-btn chapter-btn"
        aria-label="Chapitre précédent"
        title={media.chapters?.length ? 'Chapitre précédent' : 'Aucun chapitre'}
        disabled={busy || !media.chapters?.length}
        onclick={() => skipChapter(-1)}
      >
        <Icon name="chapter-prev" size={20} />
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
        class="control-btn chapter-btn"
        aria-label="Chapitre suivant"
        title={media.chapters?.length ? 'Chapitre suivant' : 'Aucun chapitre'}
        disabled={busy || !media.chapters?.length}
        onclick={() => skipChapter(1)}
      >
        <Icon name="chapter-next" size={20} />
      </button>

      <button
        class="control-btn"
        aria-label="Avancer de 10 secondes"
        title="Avancer de 10 secondes"
        disabled={busy || !playback.duration}
        onclick={() => onSeek(Math.min(playback.duration, playback.position + 10))}
      >
        <Icon name="jump-forward" size={22} />
      </button>

      <span class="time" aria-label="{clock(playback.position)} sur {clock(playback.duration)}{projectedEndTime ? ', fin à ' + projectedEndTime : ''}">
        <span class="current-time">{clock(playback.position)}</span>
        <span class="duration-time"> / {clock(playback.duration)}</span>
        {#if playback.duration > 0 && projectedEndTime}
          <span class="remaining-time"> (Fin à {projectedEndTime})</span>
        {/if}
      </span>
    </div>

    <div class="right-controls" bind:this={triggers}>
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
      </button>

      {#if selectableAudio.length > 1}
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
      </button>
      {/if}

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

      <button
        data-menu="settings"
        aria-label="Paramètres"
        aria-haspopup="menu"
        aria-expanded={menu === 'settings'}
        aria-controls={menu === 'settings' ? 'player-menu' : undefined}
        onclick={() => toggleMenu('settings')}
        disabled={busy}
      >
        <Icon name="settings" size={20} />
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
  .offset-control {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    background: rgba(255, 255, 255, 0.03);
    border-radius: var(--radius-xs);
    font-size: 12px;
    color: var(--color-muted);
  }
  .offset-control input {
    width: 6.5rem;
    color: var(--color-text-bright);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-xs);
    padding: 6px 10px;
    font-size: 13px;
  }
  .offset-control input:focus {
    outline: none;
    border-color: var(--color-accent);
  }
  .player-controls {
    position: absolute;
    z-index: 4;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 120px max(36px, env(safe-area-inset-right)) max(26px, env(safe-area-inset-bottom)) max(36px, env(safe-area-inset-left));
    background: linear-gradient(180deg, transparent 0%, rgba(9, 10, 16, 0.75) 50%, rgba(9, 10, 16, 0.98) 100%);
    transition: opacity 0.25s cubic-bezier(0.4, 0, 0.2, 1);
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
    gap: 8px;
    min-width: 44px;
    min-height: 44px;
    padding: 8px 12px;
    background: transparent;
    color: #e2e8f0;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    transition: all var(--transition-fast);
  }
  button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
    color: #ffffff;
    border-color: rgba(255, 255, 255, 0.15);
  }
  button[aria-expanded='true'] {
    color: var(--color-accent);
    background: var(--color-accent-soft);
    border-color: var(--color-accent-soft);
  }

  .control-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    min-width: 40px;
    min-height: 40px;
    padding: 0;
    border-radius: 50%;
    color: #e2e8f0;
    background: transparent;
    border: 1px solid transparent;
    transition: all var(--transition-fast);
  }
  .control-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
    color: #ffffff;
    border-color: rgba(255, 255, 255, 0.15);
    transform: scale(1.08);
  }
  .control-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .control-btn :global(svg) {
    stroke-width: 2;
  }
  .chapter-btn :global(svg) {
    fill: currentColor;
    stroke: none;
  }

  .primary.play-btn {
    width: 52px;
    height: 52px;
    min-width: 52px;
    min-height: 52px;
    border-radius: 50%;
    background: #ffffff;
    color: #090a10;
    margin: 0 4px;
    border: 1px solid transparent;
    box-shadow: 0 4px 18px rgba(0, 0, 0, 0.5), 0 0 16px var(--color-accent-glow);
    transition: all var(--transition-fast);
  }
  .primary.play-btn:hover:not(:disabled) {
    background: #ffffff;
    color: #090a10;
    border-color: transparent;
    transform: scale(1.08);
    box-shadow: 0 6px 24px rgba(255, 255, 255, 0.35), 0 0 24px var(--color-accent-glow);
  }
  .primary.play-btn:active:not(:disabled) {
    transform: scale(0.96);
  }
  .primary.play-btn :global(svg) {
    fill: currentColor;
    stroke-width: 1.8;
  }

  .volume-group {
    display: flex;
    align-items: center;
    gap: 6px;
    background: rgba(255, 255, 255, 0.04);
    padding: 2px 10px 2px 2px;
    border-radius: var(--radius-full);
    border: 1px solid var(--color-line);
  }
  .volume {
    width: 85px;
    accent-color: var(--color-accent);
  }

  .time {
    margin-left: 8px;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    color: var(--color-text-bright);
    white-space: nowrap;
    font-weight: 500;
  }
  .duration-time {
    color: var(--color-muted);
  }
  .remaining-time {
    color: var(--color-subtle);
    font-size: 11.5px;
  }

  .progress-line {
    position: relative;
    margin-bottom: 16px;
  }
  .seek {
    width: 100%;
    height: 28px;
    margin: 0;
    accent-color: var(--color-accent);
    cursor: pointer;
  }
  .seek-preview {
    position: absolute;
    right: 0;
    top: -28px;
    font-size: 11.5px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    background: rgba(17, 19, 28, 0.95);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    padding: 4px 10px;
    border-radius: var(--radius-xs);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
  }

  .floating-menu {
    position: absolute;
    bottom: calc(100% - 90px);
    right: 36px;
    width: 360px;
    max-height: min(480px, calc(100dvh - 200px));
    overflow-y: auto;
    padding: 10px;
    border-radius: var(--radius-lg);
    background: rgba(18, 20, 30, 0.96);
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.12);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
  }
  .menu-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px 10px 14px;
    font-size: 14px;
    font-weight: 700;
    color: var(--color-text-bright);
    border-bottom: 1px solid var(--color-line);
    margin-bottom: 8px;
  }
  .menu-heading button {
    width: auto;
    min-width: 36px;
    min-height: 36px;
    padding: 4px;
    font-size: 22px;
    font-weight: 300;
    line-height: 1;
    border-radius: 50%;
  }

  .floating-menu button[role="menuitem"],
  .floating-menu button[role="menuitemradio"] {
    width: 100%;
    min-height: 48px;
    padding: 12px 14px;
    justify-content: space-between;
    text-align: left;
    font-size: 13.5px;
    font-weight: 500;
    gap: 16px;
    border-radius: var(--radius-sm);
  }
  .setting-value {
    color: var(--color-muted, #94a3b8);
    font-size: 13px;
    font-weight: 500;
  }
  .setting-arrow {
    color: var(--color-muted, #94a3b8);
    font-size: 18px;
    line-height: 1;
  }
  .menu-back {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    min-width: 32px;
    min-height: 32px;
    padding: 0;
    margin-right: 8px;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--color-text-bright);
  }
  .floating-menu button[aria-checked='true'] {
    color: var(--color-accent-hover);
    background: var(--color-accent-soft);
    border-color: var(--color-accent-soft);
    font-weight: 600;
  }
  .floating-menu small {
    display: block;
    font-size: 11px;
    color: var(--color-muted);
    margin-top: 3px;
  }

  .menu-section {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--color-accent);
    margin: 14px 14px 6px;
    font-weight: 700;
  }
  .menu-note {
    font-size: 11.5px;
    color: var(--color-muted);
    line-height: 1.5;
    margin: 10px 14px;
  }
  .track-info {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 8px 14px;
    font-size: 12.5px;
    color: var(--color-text);
  }
  .track-badge {
    display: inline-block;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-size: 10.5px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: var(--radius-xs);
    margin-left: 6px;
  }
  .menu-separator {
    border-top: 1px solid var(--color-line);
    margin: 8px 14px;
  }

  @media (max-width: 768px) {
    .player-controls {
      padding: 56px max(16px, env(safe-area-inset-right)) max(14px, env(safe-area-inset-bottom)) max(16px, env(safe-area-inset-left));
    }
    .control-row {
      flex-wrap: wrap;
      row-gap: 8px;
    }
    .left-controls, .right-controls {
      width: 100%;
      justify-content: center;
      gap: 4px;
      min-width: 0;
    }
    .left-controls { flex-wrap: wrap; }
    .control-btn { min-width: 44px; min-height: 44px; }
    .time { margin-left: 4px; }
    .remaining-time { display: none; }
    .progress-line { margin-bottom: 4px; }
    .seek { height: 44px; }
    .volume-group {
      display: none;
    }

    .floating-menu {
      right: 16px;
      bottom: calc(100% - 70px);
      width: min(340px, calc(100vw - 32px));
      box-sizing: border-box;
      max-height: max(100px, calc(100dvh - 220px));
    }
  }
  @media (max-width: 380px) {
    .chapter-btn { display: none; }
  }
  .skip-marker-overlay {
    position: absolute;
    right: 2rem;
    bottom: 5.5rem;
    z-index: 40;
    pointer-events: auto;
  }
  .skip-marker-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1.25rem;
    background: rgba(15, 23, 42, 0.85);
    backdrop-filter: blur(8px);
    border: 1px solid rgba(255, 255, 255, 0.25);
    border-radius: 9999px;
    color: #ffffff;
    font-size: 0.95rem;
    font-weight: 600;
    cursor: pointer;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.4);
    transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  }
  .skip-marker-btn:hover {
    background: rgba(255, 255, 255, 0.2);
    border-color: rgba(255, 255, 255, 0.5);
    transform: scale(1.04);
  }
  .skip-marker-arrow {
    font-size: 1.25rem;
    line-height: 1;
  }
</style>

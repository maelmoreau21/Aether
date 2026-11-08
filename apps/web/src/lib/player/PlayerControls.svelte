<script lang="ts">
  import { fade } from 'svelte/transition';
  const reducedMotion = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  import { tick } from 'svelte';

  import Icon from '../ui/Icon.svelte';
  import { audioLabel, videoLabel, videoResolutionBadge, containerLabel } from './track-labels';
  import { audioChoices } from './available-audio';
  import type { Media, PlaybackState } from './types';
  import {
    SPEEDS, aspectRatioLabel, clock, menuTitle, nextAspectRatio, percentOf, projectedEndTime,
    speedOptionLabel, speedSettingLabel, type ControlsMenu
  } from './controls-format';
  import {
    chapterSkipTarget, inRange, rangeInSegment, segmentFill, segmentIndexAt,
    timelineChapters as listTimelineChapters, timelineSegments
  } from './chapters';
  import { clampHoverPosition } from './trickplay';
  import { TrickplayPreview } from './trickplay.svelte';
  import { focusMenuItem, menuItems, nextMenuIndex } from './menu-navigation';
  import { t } from '../i18n';

  export type Menu = ControlsMenu;

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
    skipBackSeconds = 10, skipForwardSeconds = 10,
    onWake,
    buffered = 0,
    hideCreditsSkip = false
  }: {
    audioOffset?: number; subtitleOffset?: number; onAudioOffset?: (value: number) => void; onSubtitleOffset?: (value: number) => void;
    skipBackSeconds?: number; skipForwardSeconds?: number;
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
    onSeek: (value: number, kind?: 'seek' | 'skip_intro' | 'skip_credits' | 'skip_recap' | 'skip_preview') => void;
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
    /** Position (s) jusqu’où le média est déjà chargé. */
    buffered?: number;
    /** La carte « épisode suivant » remplace le bouton « Passer le générique ». */
    hideCreditsSkip?: boolean;
  } = $props();

  let panel = $state<HTMLDivElement>();
  let triggers: HTMLDivElement;
  let isSeeking = $state(false);

  // Vignettes de survol : état, chargement et préchargement dans trickplay.svelte.ts.
  const preview = new TrickplayPreview({ media: () => media, duration: () => playback.duration });
  const previewFrame = $derived(preview.frame);

  function updateHoverPosition(clientX: number, target: HTMLElement) {
    if (!playback.duration) return;
    preview.hoverTime = clampHoverPosition(clientX, target.getBoundingClientRect(), playback.duration);
  }

  // Barre de lecture découpée en tronçons de chapitres (un seul tronçon sans chapitre).
  const segments = $derived(timelineSegments(listTimelineChapters(media.chapters, playback.duration), playback.duration));
  // Pendant un glissement, la barre suit la valeur saisie plutôt que la lecture.
  let scrubTime = $state<number | null>(null);
  const shownPosition = $derived(scrubTime ?? playback.position);
  const shownBuffered = $derived(Math.max(buffered, shownPosition));
  const activeSegment = $derived(segments[segmentIndexAt(segments, shownPosition)]);
  const hoverIndex = $derived(preview.hoverTime === null ? -1 : segmentIndexAt(segments, preview.hoverTime));
  const hoverSegment = $derived(hoverIndex >= 0 ? segments[hoverIndex] : undefined);
  const markerRanges = $derived([
    { kind: 'recap', label: $t('skip_recap'), range: media.recap },
    { kind: 'intro', label: $t('skip_intro'), range: media.intro },
    { kind: 'credits', label: $t('skip_credits'), range: media.credits },
    { kind: 'preview', label: $t('skip_preview'), range: media.preview }
  ].filter(marker => marker.range));
  const hoverMarker = $derived(preview.hoverTime === null ? undefined : markerRanges.find(marker => inRange(marker.range, preview.hoverTime!)));

  // Infobulle de survol : centrée sur le pointeur, bornée aux extrémités de la barre.
  let timelineWidth = $state(0);
  let tooltipWidth = $state(0);
  const tooltipLeft = $derived.by(() => {
    if (preview.hoverTime === null || !playback.duration) return 0;
    const x = (preview.hoverTime / playback.duration) * timelineWidth;
    const half = tooltipWidth / 2;
    return half * 2 >= timelineWidth ? timelineWidth / 2 : Math.min(timelineWidth - half, Math.max(half, x));
  });

  // Clavier sur la barre : pas de ±N s (préférences), chapitres, début et fin.
  let keyPreviewTimer: ReturnType<typeof setTimeout> | undefined;
  function seekKeyboard(event: KeyboardEvent) {
    if (!playback.duration) return;
    const position = playback.position;
    let target: number | null = null;
    if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') target = position - skipBackSeconds;
    else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') target = position + skipForwardSeconds;
    else if (event.key === 'Home') target = 0;
    else if (event.key === 'End') target = playback.duration;
    else if (event.key === 'PageUp' || event.key === 'PageDown') {
      const direction = event.key === 'PageUp' ? 1 : -1;
      target = chapterSkipTarget(media.chapters, position, playback.duration, direction) ?? position + direction * 60;
    }
    if (target === null) return;
    event.preventDefault();
    target = Math.min(playback.duration, Math.max(0, target));
    onSeek(target);
    preview.hoverTime = target;
    clearTimeout(keyPreviewTimer);
    keyPreviewTimer = setTimeout(() => { if (!isSeeking) preview.hoverTime = null; }, 1500);
  }
  $effect(() => () => clearTimeout(keyPreviewTimer));

  // Un seul bouton « passer » à la fois, par ordre de priorité : résumé, introduction, générique, aperçu.
  const skipMarker = $derived.by(() => {
    if (inRange(media?.recap, playback.position)) return { label: $t('skip_recap'), end: media.recap!.end, kind: 'skip_recap' as const };
    if (inRange(media?.intro, playback.position)) return { label: $t('skip_intro'), end: media.intro!.end, kind: 'skip_intro' as const };
    if (!hideCreditsSkip && inRange(media?.credits, playback.position)) return { label: $t('skip_credits'), end: media.credits!.end, kind: 'skip_credits' as const };
    if (inRange(media?.preview, playback.position)) return { label: $t('skip_preview'), end: media.preview!.end, kind: 'skip_preview' as const };
    return null;
  });

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

  const endTime = $derived(projectedEndTime(now, remainingTime, playback.duration, playback.speed));

  function skipChapter(direction: -1 | 1) {
    const target = chapterSkipTarget(media?.chapters, playback.position, playback.duration, direction);
    if (target !== null) onSeek(target);
  }

  const title = $derived(menu === 'audio' ? $t('audio') : menu === 'subtitles' ? $t('subtitles') : menu === 'video' ? $t('version') : menu === 'speed' ? $t('speed') : menu === 'settings' ? $t('player_settings') : menuTitle(menu));

  const menuTabs = $derived(([
    ...(selectableAudio.length > 1 ? [{ id: 'audio', label: $t('audio') }] : []),
    { id: 'subtitles', label: $t('subtitles') },
    { id: 'video', label: $t('version') },
    { id: 'speed', label: $t('speed') },
    { id: 'settings', label: $t('player_settings') }
  ]) as { id: Exclude<Menu, null>; label: string }[]);

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
    const items = menuItems(panel);
    const target = panel?.querySelector<HTMLButtonElement>('[role="menuitemradio"][aria-checked="true"]:not(:disabled)') ?? items[0];
    if (target) focusMenuItem(items, target);
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
    const items = menuItems(panel);
    if (items.length === 0) return;
    const next = nextMenuIndex(event.key, items.indexOf(document.activeElement as HTMLButtonElement), items.length);
    if (next !== null) {
      event.preventDefault();
      event.stopPropagation();
      focusMenuItem(items, items[next]);
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
          <button type="button" class="menu-back" aria-label={$t('return_to_settings')} tabindex="-1" onclick={() => menu = 'settings'}>
            <Icon name="back" size={18} />
          </button>
        {/if}
        <span>{title}</span>
        <button type="button" class="menu-close" aria-label={$t('close_menu')} tabindex="-1" onclick={closeMenu}>×</button>
      </div>
      <!-- Halo : le panneau garde toutes les sections à portée, une par onglet. -->
      <div class="menu-tabs" role="group" aria-label={$t('player_panel_sections')}>
        {#each menuTabs as tab (tab.id)}
          <button type="button" role="menuitem" tabindex="-1" aria-current={menu === tab.id ? 'true' : undefined} onclick={() => { if (menu !== tab.id) void toggleMenu(tab.id); }}>{tab.label}</button>
        {/each}
      </div>
      {#if menu === 'audio'}
        <div role="group" aria-label={$t('audio_languages')}>
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
        <button role="menuitem" tabindex="-1" disabled={busy || !source?.externalAudioUrl} onclick={() => panel?.querySelector<HTMLInputElement>('input')?.focus()}>{$t('audio_offset_action')}</button>
        <label class="offset-control">{$t('audio_offset')}
          <input aria-label={$t('audio_offset_seconds')} type="number" min="-30" max="30" step="0.05" value={audioOffset} disabled={busy || !source?.externalAudioUrl} onchange={(e) => onAudioOffset?.(e.currentTarget.valueAsNumber)} />
        </label>
        <p class="menu-note">{$t('positive_delays')}</p>
      {:else if menu === 'subtitles'}
        <div role="group" aria-label={$t('subtitle_tracks')}>
          <button
            data-subtitle-id="off"
            role="menuitemradio"
            tabindex={subtitle === null ? 0 : -1}
            aria-checked={subtitle === null}
            disabled={busy}
            onclick={() => choose(() => onSubtitle(null))}
          >
            <span>{$t('subtitles_off')}</span>
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
              <span>{item.label}<small>{$t('webvtt', { value: item.language.toUpperCase() })}</small></span>
              {#if item.id === subtitle}
                <Icon name="check" size={18} />
              {/if}
            </button>
          {/each}
        </div>
        <button role="menuitem" tabindex="-1" disabled={busy} onclick={() => panel?.querySelector<HTMLInputElement>('input')?.focus()}>{$t('subtitle_offset_action')}</button>
        <label class="offset-control">{$t('subtitle_offset')}
          <input aria-label={$t('subtitle_offset_seconds')} type="number" min="-30" max="30" step="0.05" value={subtitleOffset} disabled={busy} oninput={(e) => onSubtitleOffset?.(e.currentTarget.valueAsNumber)} onchange={(e) => onSubtitleOffset?.(e.currentTarget.valueAsNumber)} />
        </label>
      {:else if menu === 'video'}
        <div role="group" aria-label={$t('active_video_source')}>
          <p class="menu-section" role="presentation">{$t('active_video_track')}</p>
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
            {$t('container_decoding_handled_by_the_browser', { value: containerLabel(source?.mime) })}
          </p>
        </div>
        {#if selectableVersions.length > 1}
        <div role="separator" class="menu-separator"></div>
        <div role="group" aria-label={$t('demo_sources')}>
          <p class="menu-section" role="presentation">{$t('available_versions')}</p>
          {#each selectableVersions as item (item.id)}
            <button
              data-source-id={item.id}
              role="menuitemradio"
              tabindex={item.id === media.id ? 0 : -1}
              aria-checked={item.id === media.id}
              disabled={busy}
              onclick={() => choose(() => onSource(item.id))}
            >
              <span>{item.title}<small>{item.synthetic ? $t('prepared_source') : $t('indexed_local_file')}</small></span>
              {#if item.id === media.id}
                <Icon name="check" size={18} />
              {/if}
            </button>
          {/each}
        </div>
        {/if}
      {:else if menu === 'speed'}
        <div role="group" aria-label={$t('playback_speeds')}>
          {#each SPEEDS as speed (speed)}
            <button
              data-speed={speed}
              role="menuitemradio"
              tabindex={playback.speed === speed ? 0 : -1}
              aria-checked={playback.speed === speed}
              disabled={busy}
              onclick={() => choose(() => onSpeed(speed))}
            >
              <span>{speedOptionLabel(speed)}</span>
              {#if playback.speed === speed}
                <Icon name="check" size={18} />
              {/if}
            </button>
          {/each}
        </div>
      {:else if menu === 'settings'}
        <div role="group" aria-label={$t('player_settings_group')}>
          <button
            type="button"
            role="menuitem"
            onclick={() => {
              onAspectRatio?.(nextAspectRatio(aspectRatio));
            }}
          >
            <span>{$t('aspect_ratio')}</span>
            <span class="setting-value">{aspectRatioLabel(aspectRatio)}</span>
          </button>
          <button
            type="button"
            role="menuitem"
            onclick={() => menu = 'speed'}
          >
            <span>{$t('playback_speed')}</span>
            <span class="setting-value">{speedSettingLabel(playback.speed)}</span>
          </button>
          <button
            type="button"
            role="menuitem"
            onclick={() => onRepeat?.(!repeat)}
          >
            <span>{$t('repeat_mode')}</span>
            <span class="setting-value">{repeat ? $t('repeat') : $t('none')}</span>
          </button>
          <button
            type="button"
            role="menuitem"
            onclick={() => {
              closeMenu();
              onStats?.();
            }}
          >
            <span>{$t('playback_data')}</span>
            <span class="setting-arrow">›</span>
          </button>
        </div>
      {/if}
    </div>
  {/if}
  <!-- Barre de lecture : tronçons de chapitres séparés par un fin espace, aperçu au survol. -->
  <div
    class="timeline"
    class:segmented={segments.length > 1}
    class:hovering={preview.hoverTime !== null}
    class:scrubbing={isSeeking || scrubTime !== null}
    bind:clientWidth={timelineWidth}
  >
    <div class="track" aria-hidden="true">
      {#each segments as segment, index (segment.start)}
        <span
          class="segment"
          class:hovered={index === hoverIndex}
          style:--seg-left={`${percentOf(segment.start, playback.duration)}%`}
          style:--seg-width={`${percentOf(segment.end - segment.start, playback.duration)}%`}
        >
          <span class="segment-fill buffered" style:width={`${segmentFill(segment, shownBuffered)}%`}></span>
          {#each markerRanges as marker (marker.kind)}
            {@const band = rangeInSegment(marker.range, segment)}
            {#if band}
              <span class="segment-marker {marker.kind}" style:left={`${band.left}%`} style:width={`${band.width}%`}></span>
            {/if}
          {/each}
          <span class="segment-fill played" style:width={`${segmentFill(segment, shownPosition)}%`}></span>
        </span>
      {/each}
    </div>
    {#if playback.duration > 0}
      <span class="knob" aria-hidden="true" style:left={`${percentOf(shownPosition, playback.duration)}%`}></span>
    {/if}
    {#if preview.hoverTime !== null && playback.duration > 0}
      <output class="seek-preview" class:has-frame={!!previewFrame} aria-hidden="true" bind:offsetWidth={tooltipWidth} style:left={`${tooltipLeft}px`}>
        {#if previewFrame}
          <!-- Vignette au ratio exact de la tuile, mise à l'échelle (jamais rognée) :
               la planche est positionnée en pourcentages de la tuile affichée. -->
          <span class="trickplay-frame"
            style:aspect-ratio={`${previewFrame.width} / ${previewFrame.height}`}
            style:width={`min(${previewFrame.width}px, var(--trickplay-max-w), calc(var(--trickplay-max-h) * ${previewFrame.width / previewFrame.height}))`}>
            <img src={previewFrame.url} alt="" draggable="false"
              style:width={`${(previewFrame.fullWidth / previewFrame.width) * 100}%`}
              style:height={`${(previewFrame.fullHeight / previewFrame.height) * 100}%`}
              style:left={`${(-previewFrame.x / previewFrame.width) * 100}%`}
              style:top={`${(-previewFrame.y / previewFrame.height) * 100}%`}
              onerror={() => { if (previewFrame) preview.markHoveredSheetFailed(); }}
            />
          </span>
        {/if}
        <span class="preview-label">
          {#if hoverSegment?.title}<span class="preview-chapter">{hoverSegment.title}</span>{/if}
          <span class="preview-time">{clock(preview.hoverTime)}{#if hoverMarker}<span class="preview-marker">{` · ${hoverMarker.label}`}</span>{/if}</span>
        </span>
      </output>
    {/if}
    <input
      class="seek"
      aria-label={$t('position')}
      aria-valuetext={$t('clock_of_duration', { position: clock(shownPosition), duration: clock(playback.duration) }) + (activeSegment?.title ? ' · ' + activeSegment.title : '')}
      type="range"
      min="0"
      max={playback.duration || 1}
      step="0.01"
      value={shownPosition}
      disabled={busy || !playback.duration}
      onkeydown={seekKeyboard}
      onpointerdown={event => {
        isSeeking = true;
        updateHoverPosition(event.clientX, event.currentTarget);
      }}
      onpointermove={event => {
        updateHoverPosition(event.clientX, event.currentTarget);
      }}
      onpointerup={() => {
        isSeeking = false;
        preview.hoverTime = null;
      }}
      onpointercancel={() => {
        isSeeking = false;
        scrubTime = null;
        preview.hoverTime = null;
      }}
      onpointerleave={() => {
        if (!isSeeking) preview.hoverTime = null;
      }}
      onblur={() => {
        if (!isSeeking) preview.hoverTime = null;
      }}
      oninput={event => {
        scrubTime = Number(event.currentTarget.value);
        preview.hoverTime = scrubTime;
      }}
      onchange={event => {
        isSeeking = false;
        scrubTime = null;
        onSeek(Number(event.currentTarget.value));
      }}
    />
  </div>
  <div class="control-row">
    <div class="left-controls">
      <button
        class="control-btn"
        aria-label={$t('back_seconds', { count: skipBackSeconds })}
        title={$t('back_seconds', { count: skipBackSeconds })}
        disabled={busy || !playback.duration}
        onclick={() => onSeek(Math.max(0, playback.position - skipBackSeconds))}
      >
        <Icon name="jump-backward" size={22} />
      </button>
      <button
        class="control-btn chapter-btn"
        aria-label={$t('previous_chapter')}
        title={media.chapters?.length ? $t('previous_chapter') : $t('no_chapter')}
        disabled={busy || !media.chapters?.length}
        onclick={() => skipChapter(-1)}
      >
        <Icon name="chapter-prev" size={20} />
      </button>
      <button
        class="primary play-btn"
        aria-label={playback.paused ? $t('play_action') : $t('pause')}
        disabled={busy}
        onclick={onPlay}
      >
        <Icon name={playback.paused ? 'play' : 'pause'} size={26} />
      </button>
      <button
        class="control-btn chapter-btn"
        aria-label={$t('next_chapter')}
        title={media.chapters?.length ? $t('next_chapter') : $t('no_chapter')}
        disabled={busy || !media.chapters?.length}
        onclick={() => skipChapter(1)}
      >
        <Icon name="chapter-next" size={20} />
      </button>
      <button
        class="control-btn"
        aria-label={$t('forward_seconds', { count: skipForwardSeconds })}
        title={$t('forward_seconds', { count: skipForwardSeconds })}
        disabled={busy || !playback.duration}
        onclick={() => onSeek(Math.min(playback.duration, playback.position + skipForwardSeconds))}
      >
        <Icon name="jump-forward" size={22} />
      </button>
    </div>
    <span class="time" aria-label={$t('clock_of_duration', { position: clock(playback.position), duration: clock(playback.duration) }) + (endTime ? $t('ends_at') + endTime : '')}>
      <span class="current-time">{clock(shownPosition)}</span>
      <span class="duration-time"> / {clock(playback.duration)}</span>
      {#if playback.duration > 0 && endTime}
        <span class="remaining-time"> · {$t('end_at', { time: endTime })}</span>
      {/if}
    </span>
    <div class="right-controls" bind:this={triggers}>
      <button
        data-menu="subtitles"
        aria-label={$t('subtitles')}
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
        aria-label={$t('audio')}
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
          aria-label={playback.volume === 0 ? $t('restore_sound') : $t('mute')}
          aria-pressed={playback.volume === 0}
          onclick={onMute}
        >
          <Icon name={playback.volume === 0 ? 'muted' : 'volume'} size={22} />
        </button>
        <input
          class="volume"
          aria-label={$t('volume')}
          type="range"
          min="0"
          max="1"
          step="0.01"
          value={playback.volume}
          style:--volume-level={`${Math.round(playback.volume * 100)}%`}
          oninput={event => onVolume(Number(event.currentTarget.value))}
        />
      </div>
      <button
        data-menu="settings"
        aria-label={$t('player_settings_label')}
        aria-haspopup="menu"
        aria-expanded={menu === 'settings'}
        aria-controls={menu === 'settings' ? 'player-menu' : undefined}
        onclick={() => toggleMenu('settings')}
        disabled={busy}
      >
        <Icon name="settings" size={20} />
      </button>
      <button
        aria-label={fullscreen ? $t('exit_fullscreen') : $t('fullscreen')}
        onclick={onFullscreen}
      >
        <Icon name={fullscreen ? 'minimize' : 'fullscreen'} size={22} />
      </button>
    </div>
  </div>
</div>
<!-- Hors du dock : le bouton reste visible et cliquable quand les commandes se masquent. -->
{#if skipMarker}
  <div class="skip-marker-overlay" class:raised={visible} transition:fade={{ duration: reducedMotion ? 0 : 200 }}>
    <button type="button" class="skip-marker-btn" onclick={() => { if (skipMarker) onSeek(skipMarker.end, skipMarker.kind); onWake(); }}>
      <span>{skipMarker.label}</span>
      <span class="skip-marker-arrow">&rsaquo;</span>
    </button>
  </div>
{/if}
<style>
  /* ==========================================================================
     Commandes du lecteur (Halo, toujours sombre). Pas de dock vitré : un voile
     dégradé en bas de l'image, les commandes posées directement dessus.
     ========================================================================== */
  .player-controls {
    --pc-gutter: var(--gutter, 40px);
    --pc-btn: 48px;
    --pc-play: 60px;
    --pc-icon-gap: 4px;
    position: absolute;
    z-index: 4;
    bottom: 0;
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 120px max(var(--pc-gutter), env(safe-area-inset-right)) max(20px, env(safe-area-inset-bottom)) max(var(--pc-gutter), env(safe-area-inset-left));
    background: linear-gradient(0deg, rgb(0 0 0 / 0.78) 0%, rgb(0 0 0 / 0.42) 55%, rgb(0 0 0 / 0) 100%);
    color: var(--on-media);
    font-family: var(--font);
    transition: opacity var(--t-slow) ease;
  }
  .concealed {
    opacity: 0;
    pointer-events: none;
  }

  /* ---------- Boutons ---------- */
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-width: 44px;
    min-height: 44px;
    padding: 8px 12px;
    border: 0;
    border-radius: var(--r-md);
    background: transparent;
    color: var(--on-media);
    font: inherit;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  button:hover:not(:disabled) { background: var(--pn-3); color: #fff; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button[aria-expanded='true'] { background: var(--pn-3); color: #fff; }

  .control-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .left-controls, .right-controls {
    display: flex;
    align-items: center;
    gap: var(--pc-icon-gap);
  }
  .right-controls { margin-left: auto; }
  .control-btn,
  .right-controls > button,
  .volume-group .mute {
    width: var(--pc-btn);
    height: var(--pc-btn);
    min-width: var(--pc-btn);
    min-height: var(--pc-btn);
    padding: 0;
  }
  .control-btn :global(svg) { stroke-width: 2; }
  .chapter-btn :global(svg) { fill: currentColor; stroke: none; }
  .primary.play-btn {
    width: var(--pc-play);
    height: var(--pc-play);
    min-width: var(--pc-play);
    min-height: var(--pc-play);
    margin: 0 4px;
    padding: 0;
    border-radius: 20px;
    background: rgb(255 255 255 / 0.16);
  }
  .primary.play-btn:hover:not(:disabled) { background: rgb(255 255 255 / 0.26); }
  .primary.play-btn:active:not(:disabled) { transform: scale(0.96); }
  .primary.play-btn :global(svg) { fill: currentColor; stroke-width: 1.8; }

  .time {
    margin-left: 8px;
    font-size: 14px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: var(--on-media);
  }
  .duration-time, .remaining-time { color: rgb(244 246 251 / 0.65); font-weight: 600; }

  .volume-group { display: flex; align-items: center; gap: 2px; padding-right: 8px; }
  .volume {
    -webkit-appearance: none;
    appearance: none;
    width: 84px;
    height: 20px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  .volume::-webkit-slider-runnable-track { height: 4px; border-radius: var(--r-pill); background: linear-gradient(90deg, var(--on-media) var(--volume-level), rgb(255 255 255 / 0.28) var(--volume-level)); }
  .volume::-webkit-slider-thumb { -webkit-appearance: none; width: 12px; height: 12px; margin-top: -4px; border: 0; border-radius: 50%; background: #fff; }
  .volume::-moz-range-track { height: 4px; border-radius: var(--r-pill); background: rgb(255 255 255 / 0.28); }
  .volume::-moz-range-progress { height: 4px; border-radius: var(--r-pill); background: var(--on-media); }
  .volume::-moz-range-thumb { width: 12px; height: 12px; border: 0; border-radius: 50%; background: #fff; }
  .volume:focus-visible { outline: 2px solid var(--ac); outline-offset: 4px; border-radius: var(--r-pill); }

  /* ---------- Barre de lecture segmentée par chapitres ---------- */
  .timeline {
    --track-h: 4px;
    --track-h-hover: 6px;
    --track-h-active: 10px;
    --seg-gap: 3px;
    --knob: 14px;
    position: relative;
    height: 28px;
    margin: 0 2px;
  }
  .track {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .segment {
    position: absolute;
    top: 50%;
    left: var(--seg-left);
    width: max(1px, calc(var(--seg-width) - var(--seg-gap)));
    height: var(--track-h);
    overflow: hidden;
    border-radius: 2px;
    background: rgb(255 255 255 / 0.22);
    transform: translateY(-50%);
    transition: height var(--t-fast) var(--ease);
  }
  .segment:last-child { width: var(--seg-width); }
  .timeline:not(.segmented) .segment { border-radius: var(--r-pill); }
  .timeline.hovering .segment, .timeline:focus-within .segment { height: var(--track-h-hover); }
  .timeline.segmented.hovering .segment.hovered { height: var(--track-h-active); }
  .timeline:not(.segmented).hovering .segment { height: var(--track-h-active); }
  .segment-fill, .segment-marker {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
  }
  .segment-fill.buffered { background: rgb(255 255 255 / 0.4); }
  /* Intro, générique, résumé, aperçu : bande plus claire, recouverte par la partie lue. */
  .segment-marker { background: rgb(255 255 255 / 0.3); }
  .segment-fill.played { background: var(--ac); }
  .knob {
    position: absolute;
    top: 50%;
    width: var(--knob);
    height: var(--knob);
    margin: calc(var(--knob) / -2) 0 0 calc(var(--knob) / -2);
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 6px rgb(0 0 0 / 0.45);
    pointer-events: none;
    transform: scale(0.85);
    transition: transform var(--t-fast) var(--ease);
  }
  .timeline.hovering .knob, .timeline.scrubbing .knob, .timeline:focus-within .knob { transform: scale(1.1); }
  /* Le vrai curseur (clavier, souris, tactile, lecteurs d'écran) couvre toute la barre, invisible. */
  .seek {
    -webkit-appearance: none;
    appearance: none;
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
    touch-action: none;
  }
  .seek:disabled { cursor: default; }
  .seek::-webkit-slider-runnable-track { height: 100%; background: transparent; }
  .seek::-webkit-slider-thumb { -webkit-appearance: none; width: 1px; height: 100%; border: 0; background: transparent; }
  .seek::-moz-range-track { height: 100%; background: transparent; }
  .seek::-moz-range-thumb { width: 1px; height: 100%; border: 0; background: transparent; }
  .timeline:has(.seek:focus-visible) .track {
    outline: 2px solid var(--ac);
    outline-offset: 2px;
    border-radius: var(--r-xs);
  }

  /* Aperçu au survol : vignette Trickplay, chapitre et horodatage. */
  /* Carte en verre discret : la vignette remplit la carte, le temps est incrusté en bas. */
  .seek-preview {
    position: absolute;
    bottom: calc(100% + 10px);
    z-index: 2;
    display: flex;
    flex-direction: column;
    align-items: center;
    max-width: min(320px, 80vw);
    pointer-events: none;
    transform: translateX(-50%);
    color: #fff;
    animation: seek-preview-in 120ms ease-out;
  }
  .seek-preview.has-frame {
    max-width: none;
    padding: 3px;
    border-radius: 14px;
    background: rgb(18 20 28 / 0.55);
    border: 1px solid rgb(255 255 255 / 0.14);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.45), inset 0 1px 0 rgb(255 255 255 / 0.08);
    backdrop-filter: blur(16px) saturate(1.4);
    -webkit-backdrop-filter: blur(16px) saturate(1.4);
  }
  @keyframes seek-preview-in { from { opacity: 0; translate: 0 4px; } }
  .trickplay-frame {
    --trickplay-max-w: min(70vw, 320px);
    --trickplay-max-h: 180px;
    display: block;
    overflow: hidden;
    position: relative;
    border-radius: 11px;
    background: #000;
  }
  .trickplay-frame img { position: absolute; display: block; max-width: none; max-height: none; }
  .preview-label {
    display: flex;
    flex-direction: column;
    align-items: center;
    max-width: 100%;
    padding: 4px 10px;
    border-radius: 999px;
    background: rgb(18 20 28 / 0.62);
    border: 1px solid rgb(255 255 255 / 0.12);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    text-align: center;
  }
  .has-frame .preview-label {
    position: absolute;
    inset: auto 3px 3px;
    max-width: none;
    padding: 18px 10px 7px;
    border: 0;
    border-radius: 0 0 11px 11px;
    background: linear-gradient(to top, rgb(0 0 0 / 0.78), rgb(0 0 0 / 0));
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
    text-shadow: 0 1px 2px rgb(0 0 0 / 0.6);
  }
  .has-frame .preview-time { color: #fff; }
  .preview-chapter {
    display: -webkit-box;
    max-width: 260px;
    overflow: hidden;
    font-size: 13px;
    font-weight: 700;
    line-height: 1.3;
    overflow-wrap: anywhere;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
  .preview-time { font-size: 12px; font-weight: 700; font-variant-numeric: tabular-nums; color: rgb(244 246 251 / 0.8); white-space: nowrap; }
  .preview-marker { font-weight: 600; }

  /* ---------- Bouton « Passer… » ---------- */
  .skip-marker-overlay {
    position: absolute;
    right: max(var(--gutter, 40px), env(safe-area-inset-right));
    bottom: max(48px, calc(env(safe-area-inset-bottom) + 32px));
    z-index: 40;
    pointer-events: auto;
    transition: bottom var(--t-slow) ease;
  }
  .skip-marker-overlay.raised { bottom: 168px; }
  .skip-marker-btn {
    gap: 10px;
    min-height: 52px;
    padding: 0 22px;
    border-radius: var(--r-md);
    background: rgb(255 255 255 / 0.92);
    color: #0b0e18;
    font-size: 15px;
    font-weight: 800;
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.4);
  }
  .skip-marker-btn:hover:not(:disabled) { background: #fff; color: #0b0e18; }
  .skip-marker-btn:active { transform: scale(0.98); }
  .skip-marker-arrow { font-size: 1.25rem; line-height: 1; color: #0b0e18; }

  /* ---------- Panneau de lecture (audio, sous-titres, version, vitesse, réglages) ---------- */
  .floating-menu {
    position: absolute;
    bottom: calc(100% - 90px);
    right: 36px;
    width: 360px;
    max-height: min(480px, calc(100dvh - 200px));
    overflow-y: auto;
    padding: 10px;
    border-radius: 24px;
    background: rgb(17 21 31 / 0.92);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    color: var(--tx);
    font-family: var(--font);
  }
  .menu-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px 10px 14px;
    margin-bottom: 8px;
    border-bottom: 1px solid var(--line);
    font-size: 17px;
    font-weight: 800;
    letter-spacing: -0.01em;
    color: #fff;
  }
  .menu-heading button {
    width: auto;
    min-width: 36px;
    min-height: 36px;
    padding: 4px;
    border-radius: 14px;
    font-size: 22px;
    font-weight: 300;
    line-height: 1;
    color: var(--tx);
  }
  .menu-heading button:hover:not(:disabled) { background: var(--pn-2); }
  .menu-back {
    width: 32px;
    height: 32px;
    min-width: 32px;
    min-height: 32px;
    padding: 0;
    margin-right: 8px;
  }
  .floating-menu button[role='menuitem'],
  .floating-menu button[role='menuitemradio'] {
    width: 100%;
    min-height: 48px;
    padding: 12px 14px;
    justify-content: space-between;
    gap: 16px;
    border-radius: 14px;
    text-align: left;
    font-size: 13.5px;
    font-weight: 700;
    color: var(--tx);
  }
  .floating-menu button[role='menuitem']:hover:not(:disabled),
  .floating-menu button[role='menuitemradio']:hover:not(:disabled) { background: rgb(255 255 255 / 0.07); }
  .floating-menu button[aria-checked='true'] {
    background: var(--pn-2);
    color: #fff;
    box-shadow: inset 3px 0 0 var(--ac);
  }
  .floating-menu small {
    display: block;
    margin-top: 3px;
    font-size: 11px;
    color: var(--mu);
  }
  .setting-value { color: var(--mu); font-size: 13px; font-weight: 500; }
  .setting-arrow { color: var(--mu-2); font-size: 18px; line-height: 1; }
  .menu-section {
    margin: 14px 14px 6px;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--mu-2);
  }
  .menu-note {
    margin: 10px 14px;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--mu);
  }
  .track-info {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 8px 14px;
    font-size: 12.5px;
    color: var(--tx);
  }
  .track-badge {
    display: inline-block;
    margin-left: 6px;
    padding: 2px 7px;
    border-radius: var(--r-xs);
    background: rgb(255 255 255 / 0.12);
    color: var(--tx);
    font-size: 10.5px;
    font-weight: 700;
  }
  .menu-separator { margin: 8px 14px; border-top: 1px solid var(--line); }
  .menu-tabs { display: flex; gap: 4px; margin: 2px 0 10px; padding: 4px; border-radius: var(--r-pill); background: var(--pn); overflow-x: auto; scrollbar-width: none; }
  .menu-tabs::-webkit-scrollbar { display: none; }
  .floating-menu .menu-tabs button { flex: none; width: auto; min-width: 0; justify-content: center; min-height: 34px; height: 34px; padding: 0 12px; border-radius: var(--r-pill); color: var(--mu); font-size: 13px; font-weight: 700; white-space: nowrap; }
  .floating-menu .menu-tabs button:hover:not(:disabled) { background: var(--pn-2); color: #fff; }
  .floating-menu .menu-tabs button[aria-current='true'] { background: var(--btn); color: var(--btn-tx); }
  .offset-control {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-radius: var(--r-xs);
    background: rgb(255 255 255 / 0.03);
    font-size: 12px;
    color: var(--mu);
  }
  .offset-control input {
    width: 6.5rem;
    padding: 6px 10px;
    border: 1px solid transparent;
    border-radius: 12px;
    background: var(--pn-2);
    color: #fff;
    font-size: 13px;
  }
  .offset-control input:focus { outline: none; border-color: var(--ac); }
  @media (min-width: 769px) {
    .floating-menu {
      position: fixed;
      top: 16px;
      right: 16px;
      bottom: 16px;
      left: auto;
      width: 380px;
      max-height: none;
    }
  }

  /* ---------- Tailles d'écran ---------- */
  /* Tablette : la réglette de volume (doublée par les touches de l'appareil) et l'heure de fin s'effacent. */
  @media (max-width: 1023px) {
    .player-controls { --pc-btn: 44px; --pc-play: 52px; --pc-icon-gap: 2px; }
    .volume { display: none; }
    .volume-group { padding-right: 0; }
    .remaining-time { display: none; }
  }
  /* Téléphone : barre, puis temps + options, puis transport centré au pouce. */
  @media (max-width: 720px) {
    .player-controls {
      gap: 2px;
      padding: 72px max(12px, env(safe-area-inset-right)) max(10px, env(safe-area-inset-bottom)) max(12px, env(safe-area-inset-left));
    }
    .timeline { --seg-gap: 2px; --track-h-hover: 5px; --track-h-active: 8px; height: 40px; }
    .control-row { flex-wrap: wrap; row-gap: 2px; column-gap: 8px; }
    .time { order: 1; margin-left: 4px; font-size: 13px; }
    .right-controls { order: 2; }
    .left-controls { order: 3; width: 100%; justify-content: center; }
    .skip-marker-overlay { right: max(12px, env(safe-area-inset-right)); bottom: max(24px, calc(env(safe-area-inset-bottom) + 16px)); }
    .skip-marker-overlay.raised { bottom: 132px; }
    .skip-marker-btn { min-height: 44px; padding: 0 18px; font-size: 14px; }
    .seek-preview { max-width: 70vw; }
    .trickplay-frame { --trickplay-max-w: min(48vw, 200px); --trickplay-max-h: 120px; }
  }
  @media (max-width: 768px) {
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
  /* TV et grands écrans : cibles et barre plus généreuses, lisibles à distance. */
  @media (min-width: 1920px) {
    .player-controls { --pc-btn: 56px; --pc-play: 68px; --pc-icon-gap: 8px; padding-bottom: max(32px, env(safe-area-inset-bottom)); }
    .timeline { --track-h: 6px; --track-h-hover: 8px; --track-h-active: 12px; --seg-gap: 4px; --knob: 18px; height: 36px; }
    .time { font-size: 17px; }
    .preview-chapter { font-size: 15px; max-width: 320px; }
    .preview-time { font-size: 14px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .segment, .knob { transition: none; }
  }
</style>

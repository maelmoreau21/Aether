<script lang="ts">
  import { fade } from 'svelte/transition';
  const reducedMotion = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  import { tick } from 'svelte';

  import Icon from '../ui/Icon.svelte';
  import { tvActive } from '../tv/keys';
  import { audioChoices } from './available-audio';
  import { clock, menuTitle, percentOf, projectedEndTime } from './controls-format';
  import { chapterSkipTarget, inRange, rangeInSegment, segmentFill, segmentIndexAt, timelineChapters as listTimelineChapters, timelineSegments } from './chapters';
  import { clampHoverPosition } from './trickplay';
  import { TrickplayPreview } from './trickplay.svelte';
  import { preferences } from '../preferences';
  import { t } from '../i18n';
  import type { ControlsProps, Menu } from './controls-props';
  import ControlsMenu from './ControlsMenu.svelte';

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
    hideCreditsSkip = false,
    quality = 'auto', maxKbps = 0, onQuality, onMaxKbps, transcode = null,
    enhancement = 'off', onEnhancement, sleep = null, onSleep
  }: ControlsProps = $props();

  let triggers: HTMLDivElement;
  let menuView = $state<ReturnType<typeof ControlsMenu>>();
  let isSeeking = $state(false);

  // Vignettes de survol : état, chargement et préchargement dans trickplay.svelte.ts.
  const preview = new TrickplayPreview({ media: () => media, duration: () => playback.duration, enabled: () => preferences().trickplayPreviews !== false });
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
    // With a TV remote, up and down leave the bar for the other controls.
    if (tvActive() && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) return;
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

  const selectableAudio = $derived(audioChoices(media, audio));

  $effect(() => { if (menu === 'audio' && selectableAudio.length <= 1) menu = null; });

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

  const title = $derived(menu === 'audio' ? $t('audio') : menu === 'subtitles' ? $t('subtitles') : menu === 'video' ? $t('version') : menu === 'quality' ? $t('quality') : menu === 'speed' ? $t('speed') : menu === 'sleep' ? $t('sleep_timer') : menu === 'settings' ? $t('player_settings') : menuTitle(menu));

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
    menuView?.focusSelected();
  }

</script>
<div class="player-controls" class:concealed={!visible} inert={!visible}>
  <ControlsMenu bind:this={menuView} {playback} {media} {catalogue} {audio} {subtitle} {busy} bind:menu {onAudio} {onSubtitle} {onSource} {onSpeed} {audioOffset} {subtitleOffset} {onAudioOffset} {onSubtitleOffset} {aspectRatio} {onAspectRatio} {repeat} {onRepeat} {onStats} {quality} {maxKbps} {onQuality} {onMaxKbps} {transcode} {enhancement} {onEnhancement} {sleep} {onSleep} {selectableAudio} {title} {closeMenu} {toggleMenu} />
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

  .left-controls,
  .right-controls {
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

  .duration-time,
  .remaining-time { color: rgb(244 246 251 / 0.65); font-weight: 600; }

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

  .timeline.hovering .segment,
  .timeline:focus-within .segment { height: var(--track-h-hover); }

  .timeline.segmented.hovering .segment.hovered { height: var(--track-h-active); }

  .timeline:not(.segmented).hovering .segment { height: var(--track-h-active); }

  .segment-fill,
  .segment-marker {
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

  .timeline.hovering .knob,
  .timeline.scrubbing .knob,
  .timeline:focus-within .knob { transform: scale(1.1); }

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

<script lang="ts">
  import { fitArtwork } from '../ui/fit-artwork';
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { history, loadHistory } from './watch-state';
  import { isResumable } from '../player/progress';
  import { serverPresentation } from '../config/presentation';
  import Icon from '../ui/Icon.svelte';
  import type { Work } from './api';
  import { selectArtwork } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import { t } from '../i18n';
  let { works = [] }: { works?: Work[] } = $props();
  // landscape.jpg is the 16:9 artwork made for resume cards; backdrop and thumb are fallbacks.
  function thumbnail(id: string, seriesId?: string) { const work=works.find(work=>work.id===(seriesId || id)); const artwork=selectArtwork(work, $session); return imagePreviewUrl(artwork.landscape || artwork.backdrop || artwork.thumb,640); }
  let failedImages = $state<string[]>([]);

  let error = $state('');
  let track = $state<HTMLDivElement>();
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);

  $effect(() => {
    if (!track) return;
    const resize = new ResizeObserver(updateScrollState);
    resize.observe(track);
    const content = new MutationObserver(updateScrollState);
    content.observe(track, { childList: true });
    updateScrollState();
    return () => { resize.disconnect(); content.disconnect(); };
  });

  onMount(() => {
    void loadHistory().catch(e => (error = e.message));
  });

  $effect(() => {
    if ($session) void loadHistory().catch(e => (error = e.message));
  });

  const continuing = $derived($history.filter(item => isResumable(item, $serverPresentation)));

  function updateScrollState() {
    if (!track) return;
    canScrollLeft = track.scrollLeft > 10;
    canScrollRight = track.scrollLeft + track.clientWidth < track.scrollWidth - 10;
  }

  function scroll(direction: 'left' | 'right') {
    if (!track) return;
    const distance = track.clientWidth * 0.75;
    track.scrollBy({
      left: direction === 'left' ? -distance : distance,
      behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth'
    });
  }
</script>

{#if error || continuing.length}
<section class="continue-shelf" aria-label={$t('continue_watching')}>
  <div class="shelf-header">
    <div class="title-group">
      <h2>{$t('resume')}</h2>
      {#if continuing.length > 0}
        <span class="count-badge">{continuing.length}</span>
      {/if}
    </div>

    {#if continuing.length > 2}
      <div class="carousel-nav">
        <button
          type="button"
          class="nav-arrow"
          disabled={!canScrollLeft}
          onclick={() => scroll('left')}
          aria-label={$t('scroll_left')}
        >
          <Icon name="chevron-left" size={18} />
        </button>
        <button
          type="button"
          class="nav-arrow"
          disabled={!canScrollRight}
          onclick={() => scroll('right')}
          aria-label={$t('scroll_right')}
        >
          <Icon name="chevron-right" size={18} />
        </button>
      </div>
    {/if}
  </div>

  {#if error}
    <div class="error-box" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button
        class="retry-btn"
        onclick={() => {
          error = '';
          void loadHistory().catch(e => (error = e.message));
        }}
      >
        {$t('request_retry')}
      </button>
    </div>
  {:else if !continuing.length}
    <div class="empty-state">
      <div class="empty-icon" aria-hidden="true">
        <Icon name="clock" size={28} />
      </div>
      <div class="empty-copy">
        <strong>{$t('no_sessions_in_progress')}</strong>
        <p>{$t('your_sessions_in_progress_will_appear')}</p>
      </div>
    </div>
  {:else}
    <div
      class="continue-list"
      bind:this={track}
      onscroll={updateScrollState}
    >
      {#each continuing as item (item.workId)}
        {@const image = thumbnail(item.workId,item.seriesId)}
        <a
          class="continue-card"
          href={resolve(`${item.kind === 'episode' ? 'series/' : 'films/'}?work=${encodeURIComponent(item.seriesId ?? item.workId)}`)}
          aria-label={$t('catalog_resume', { title: item.title })}
        >
          <div class="card-thumb">
            {#if image && !failedImages.includes(image)}<img class="resume-image" src={image} alt="" width="640" height="360" loading="lazy" decoding="async" use:fitArtwork onerror={() => failedImages = [...failedImages, image]} />{/if}
            <div class="thumb-bg" aria-hidden="true">
              <span class="thumb-letter">{(item.title ?? 'M').charAt(0)}</span>
            </div>
            <div class="thumb-overlay" aria-hidden="true">
              <div class="play-btn-circle">
                <Icon name="play" size={18} />
              </div>
            </div>
            <div class="card-progress-track">
              <progress
                value={item.position}
                max={item.duration}
                aria-label={$t('catalog_progress_of', { title: item.title })}
              ></progress>
            </div>
          </div>

          <div class="card-info">
            <div class="card-header-row">
              <span class="card-kind-badge">
                {item.kind === 'episode' ? 'ÉPISODE' : 'FILM'}
              </span>
              <span class="card-time-remaining">
                {$t('min_left', { value: Math.max(1, Math.round((item.duration - item.position) / 60)) })}
              </span>
            </div>
            <strong>{item.title}</strong>
            <span class="card-position-text">
              {$t('position_of_duration_min', { position: Math.floor(item.position / 60), duration: Math.ceil(item.duration / 60) })}
            </span>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</section>
{/if}

<style>
  .resume-image { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; z-index: 1; }
  .thumb-overlay, .card-progress-track { z-index: 2; }
  .continue-shelf { margin: 0; }

  .shelf-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 14px; }
  .title-group { display: flex; align-items: baseline; gap: 10px; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  .count-badge { font-size: 14px; font-weight: 600; color: var(--mu); }

  .carousel-nav { display: flex; gap: 6px; }
  .nav-arrow { display: grid; place-items: center; width: 36px; height: 36px; border: 0; border-radius: 12px; background: var(--pn); color: var(--tx); }
  .nav-arrow:hover:not(:disabled) { background: var(--pn-2); }
  .nav-arrow:disabled { opacity: 0.35; cursor: default; }

  .error-box { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding: 14px 16px; border-radius: var(--r-md); background: var(--err-soft); color: var(--err); font-size: 14px; font-weight: 600; }
  .retry-btn { margin-left: auto; height: 36px; padding: 0 14px; border: 0; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); font-weight: 700; }

  .empty-state { display: flex; align-items: center; gap: 16px; padding: 20px; border-radius: var(--r-lg); background: var(--bg-2); }
  .empty-icon { display: grid; place-items: center; width: 52px; height: 52px; border-radius: 16px; background: var(--pn-2); color: var(--mu); flex: none; }
  .empty-copy strong { display: block; font-size: 15px; font-weight: 800; color: var(--tx); }
  .empty-copy p { margin: 4px 0 0; font-size: 14px; color: var(--mu); }

  .continue-list {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: calc((100% - 2 * 16px) / 3);
    gap: 16px;
    overflow-x: auto;
    scroll-snap-type: x mandatory;
    scroll-behavior: smooth;
    scrollbar-width: none;
    padding: 4px 2px 8px;
  }

  .continue-list::-webkit-scrollbar { display: none; }

  /* landscape.jpg : carte 16:9 avec progression intégrée */
  .continue-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 10px;
    border-radius: var(--r-lg);
    background: var(--pn);
    scroll-snap-align: start;
    min-width: 0;
    transition: background-color var(--t-fast) ease, transform var(--t-med) var(--ease);
  }

  @media (hover: hover) and (pointer: fine) {
    .continue-card:hover { background: var(--pn-2); transform: translateY(-2px); }
  }

  .continue-card:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  .card-thumb {
    position: relative;
    aspect-ratio: var(--artwork-ratio, 16 / 9);
    overflow: hidden;
    border-radius: 14px;
    background: var(--bg-2);
  }

  .thumb-bg { position: absolute; inset: 0; display: grid; place-items: center; }
  .thumb-letter { font-size: 56px; font-weight: 800; color: var(--mu-2); opacity: 0.5; }

  .thumb-overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(6 8 14 / 0.3);
    opacity: 0;
    transition: opacity var(--t-med) ease;
  }

  .continue-card:hover .thumb-overlay, .continue-card:focus-visible .thumb-overlay { opacity: 1; }

  .play-btn-circle {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    border-radius: 18px;
    background: var(--on-media);
    color: #0b0e18;
  }

  .card-progress-track {
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: 10px;
    height: 4px;
    border-radius: 4px;
    overflow: hidden;
    background: rgb(255 255 255 / 0.25);
  }

  progress { display: block; width: 100%; height: 100%; appearance: none; border: none; background: transparent; }
  progress::-webkit-progress-bar { background: transparent; }
  progress::-webkit-progress-value { background: var(--ac); border-radius: 4px; }
  progress::-moz-progress-bar { background: var(--ac); border-radius: 4px; }

  .card-info { padding: 0 6px 6px; display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .card-header-row { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .card-kind-badge { font-weight: 700; letter-spacing: 0.06em; color: var(--mu); }
  .card-time-remaining { margin-left: auto; font-weight: 600; color: var(--mu); }
  .card-info strong { font-size: 16px; font-weight: 800; color: var(--tx); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card-position-text { font-size: 13px; color: var(--mu); font-variant-numeric: tabular-nums; }

  @media (max-width: 1023px) {
    .continue-list { grid-auto-columns: 46%; }
  }

  @media (max-width: 640px) {
    .continue-list { grid-auto-columns: 82%; gap: 12px; }
    .carousel-nav { display: none; }
    h2 { font-size: 21px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .continue-list { scroll-behavior: auto; }
  }
</style>

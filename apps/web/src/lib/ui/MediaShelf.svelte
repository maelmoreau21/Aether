<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let {
    title,
    count,
    viewAllHref,
    viewAllLabel = 'Tout voir →',
    onAction,
    actionLabel,
    layout = 'row',
    children
  }: {
    title: string;
    count?: number;
    viewAllHref?: string;
    viewAllLabel?: string;
    onAction?: () => void;
    actionLabel?: string;
    layout?: 'row' | 'grid';
    children: Snippet;
  } = $props();

  let track = $state<HTMLDivElement>();
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);

  function updateScrollState() {
    if (!track) return;
    canScrollLeft = track.scrollLeft > 10;
    canScrollRight = track.scrollLeft + track.clientWidth < track.scrollWidth - 10;
  }

  onMount(() => {
    if (!track) return;
    const resize = new ResizeObserver(updateScrollState);
    resize.observe(track);
    const content = new MutationObserver(updateScrollState);
    content.observe(track, { childList: true });
    updateScrollState();
    return () => { resize.disconnect(); content.disconnect(); };
  });

  function scroll(direction: 'left' | 'right') {
    if (!track) return;
    const distance = track.clientWidth * 0.78;
    track.scrollBy({
      left: direction === 'left' ? -distance : distance,
      behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth'
    });
  }
</script>

<section class="shelf-container" class:grid-mode={layout === 'grid'}>
  <div class="shelf-header">
    <div class="title-group">
      <h2>{title}</h2>
      {#if count !== undefined}
        <span class="count-badge">{count}</span>
      {/if}
    </div>

    <div class="actions-group">
      {#if onAction && actionLabel}
        <button type="button" class="shelf-action-btn" onclick={onAction}>
          <Icon name="refresh" size={13} />
          <span>{actionLabel}</span>
        </button>
      {/if}

      {#if viewAllHref}
        <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- viewAllHref already resolved by caller -->
        <a class="view-all-link" href={viewAllHref}>{viewAllLabel}</a>
      {/if}

      {#if layout === 'row'}
        <div class="carousel-nav">
          <button
            type="button"
            class="nav-arrow"
            disabled={!canScrollLeft}
            onclick={() => scroll('left')}
            aria-label="Faire défiler vers la gauche"
          >
            <Icon name="chevron-left" size={18} />
          </button>
          <button
            type="button"
            class="nav-arrow"
            disabled={!canScrollRight}
            onclick={() => scroll('right')}
            aria-label="Faire défiler vers la droite"
          >
            <Icon name="chevron-right" size={18} />
          </button>
        </div>
      {/if}
    </div>
  </div>

  <div
    class="shelf-track"
    class:carousel={layout === 'row'}
    class:grid={layout === 'grid'}
    bind:this={track}
    onscroll={updateScrollState}
  >
    {@render children()}
  </div>
</section>

<style>
  .shelf-container { display: flex; flex-direction: column; min-width: 0; }

  .shelf-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 14px;
  }

  .title-group { display: flex; align-items: baseline; gap: 10px; min-width: 0; }

  h2 {
    margin: 0;
    font-size: calc(24px * var(--ui-scale, 1));
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.15;
    color: var(--tx);
  }

  .count-badge { font-size: calc(14px * var(--ui-scale, 1)); font-weight: 600; color: var(--mu); font-variant-numeric: tabular-nums; }

  .actions-group { display: flex; align-items: center; gap: 8px; flex: none; }

  .shelf-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 36px;
    padding: 0 14px;
    border: 0;
    border-radius: var(--r-sm);
    background: var(--pn);
    color: var(--tx);
    font-size: 13px;
    font-weight: 700;
  }

  .shelf-action-btn:hover { background: var(--pn-2); }

  .view-all-link { font-size: 14px; font-weight: 600; color: var(--mu); transition: color var(--t-fast) ease; }
  .view-all-link:hover { color: var(--tx); }

  .carousel-nav { display: flex; gap: 6px; }

  .nav-arrow {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border: 0;
    border-radius: 12px;
    background: var(--pn);
    color: var(--tx);
  }

  .nav-arrow:hover:not(:disabled) { background: var(--pn-2); }
  .nav-arrow:disabled { opacity: 0.35; cursor: default; }

  .shelf-track.carousel {
    display: flex;
    gap: var(--card-gap, 16px);
    overflow-x: auto;
    scroll-behavior: smooth;
    padding: 8px 4px 16px;
    margin: -8px -4px 0;
    scrollbar-width: none;
    scroll-snap-type: x mandatory;
  }

  .shelf-track.carousel::-webkit-scrollbar { display: none; }

  .shelf-track.carousel :global(> *) {
    flex: 0 0 var(--poster-width, clamp(150px, 12.5vw, 190px));
    scroll-snap-align: start;
  }

  .shelf-track.grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--poster-width, clamp(150px, 12.5vw, 190px)), 1fr));
    gap: var(--card-row-gap, 28px) var(--card-gap, 16px);
    padding: 8px 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .shelf-track.carousel { scroll-behavior: auto; }
  }

  @media (max-width: 640px) {
    .shelf-header { gap: 8px; margin-bottom: 10px; }
    h2 { font-size: 21px; }
    .carousel-nav { display: none; }
    .shelf-track.carousel :global(> *) { flex: 0 0 calc((100vw - 48px) / var(--mobile-shelf-columns, 2.5)); }
    .shelf-track.carousel { gap: var(--card-gap, 12px); }
  }
</style>

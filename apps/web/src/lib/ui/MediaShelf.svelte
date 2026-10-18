<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let {
    title,
    count,
    viewAllHref,
    viewAllLabel = 'Tout voir →',
    layout = 'row',
    children
  }: {
    title: string;
    count?: number;
    viewAllHref?: string;
    viewAllLabel?: string;
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

  function scroll(direction: 'left' | 'right') {
    if (!track) return;
    const distance = track.clientWidth * 0.75;
    track.scrollBy({
      left: direction === 'left' ? -distance : distance,
      behavior: 'smooth'
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
      {#if viewAllHref}
        <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- viewAllHref already resolved by caller -->
        <a class="view-all-link" href={viewAllHref}>{viewAllLabel}</a>
      {/if}

      {#if layout === 'row'}
        <div class="carousel-nav" aria-hidden="true">
          <button
            type="button"
            class="nav-arrow"
            disabled={!canScrollLeft}
            onclick={() => scroll('left')}
            aria-label="Faire défiler vers la gauche"
            tabindex="-1"
          >
            <Icon name="chevron-left" size={18} />
          </button>
          <button
            type="button"
            class="nav-arrow"
            disabled={!canScrollRight}
            onclick={() => scroll('right')}
            aria-label="Faire défiler vers la droite"
            tabindex="-1"
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
  .shelf-container {
    margin: 40px 0;
    position: relative;
  }

  .shelf-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
    gap: 16px;
  }

  .title-group {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  h2 {
    font-size: clamp(20px, 2.2vw, 26px);
    font-weight: 650;
    letter-spacing: -0.03em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .count-badge {
    font-size: 11px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: var(--radius-full);
    background: rgba(255, 255, 255, 0.08);
    color: var(--color-muted);
  }

  .actions-group {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .view-all-link {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-accent);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    transition: color var(--transition-fast), transform var(--transition-fast);
  }

  .view-all-link:hover {
    color: var(--color-accent-hover);
    transform: translateX(2px);
  }

  .carousel-nav {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .nav-arrow {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid var(--color-line);
    color: var(--color-text);
    transition: all var(--transition-fast);
  }

  .nav-arrow:hover:not(:disabled) {
    background: var(--color-accent-soft);
    border-color: rgba(167, 139, 250, 0.35);
    color: var(--color-accent-hover);
    transform: scale(1.08);
  }

  .nav-arrow:disabled {
    opacity: 0.25;
    cursor: default;
  }

  /* Carousel Track */
  .shelf-track.carousel {
    display: flex;
    gap: 18px;
    overflow-x: auto;
    scroll-behavior: smooth;
    padding: 8px 4px 16px;
    scrollbar-width: none;
    -ms-overflow-style: none;
    scroll-snap-type: x mandatory;
  }

  .shelf-track.carousel::-webkit-scrollbar {
    display: none;
  }

  .shelf-track.carousel :global(> *) {
    flex: 0 0 clamp(160px, 16vw, 220px);
    scroll-snap-align: start;
  }

  /* Grid Track */
  .shelf-track.grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(clamp(160px, 16vw, 220px), 1fr));
    gap: 22px 18px;
    padding: 8px 0;
  }

  @media (max-width: 640px) {
    .shelf-container {
      margin: 28px 0;
    }

    .carousel-nav {
      display: none;
    }

    .shelf-track.carousel :global(> *) {
      flex: 0 0 145px;
    }
  }
</style>

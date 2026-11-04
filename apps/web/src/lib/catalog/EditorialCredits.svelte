<script lang="ts">
  import { onMount } from 'svelte';
  import type { Work } from './api';
  import { creditPeople, initials } from './credits';
  import Icon from '../ui/Icon.svelte';

  let { work }: { work: Work } = $props();

  let failedPhotos = $state<Record<string, boolean>>({});
  let loadedPhotos = $state<Record<string, boolean>>({});
  let track = $state<HTMLDivElement>();
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);

  const people = $derived(creditPeople(work));

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
    return () => {
      resize.disconnect();
      content.disconnect();
    };
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

{#if people.length > 0}
  <section aria-label="Distribution et équipe" class="credits-section">
    <div class="credits-header">
      <h2>Distribution &amp; équipe</h2>
      <div class="carousel-nav">
        <button
          type="button"
          class="nav-arrow"
          disabled={!canScrollLeft}
          onclick={() => scroll('left')}
          aria-label="Faire défiler la distribution vers la gauche"
        >
          <Icon name="chevron-left" size={18} />
        </button>
        <button
          type="button"
          class="nav-arrow"
          disabled={!canScrollRight}
          onclick={() => scroll('right')}
          aria-label="Faire défiler la distribution vers la droite"
        >
          <Icon name="chevron-right" size={18} />
        </button>
      </div>
    </div>

    <div class="cast-carousel" bind:this={track} onscroll={updateScrollState}>
      {#each people as person (person.key)}
        <article class="cast-card">
          <div class="portrait-card">
            <!-- Initials stay underneath: visible while the portrait loads, if it is missing or fails. -->
            <div class="portrait-fallback" aria-hidden="true">
              <span class="initials-badge">{initials(person.name)}</span>
            </div>
            {#if person.photoUrl && !failedPhotos[person.key]}
              <img
                class="portrait-img"
                class:loaded={loadedPhotos[person.key]}
                src={person.photoUrl}
                alt=""
                width="400"
                height="600"
                loading="lazy"
                decoding="async"
                onload={() => {
                  loadedPhotos = { ...loadedPhotos, [person.key]: true };
                }}
                onerror={() => {
                  failedPhotos = { ...failedPhotos, [person.key]: true };
                }}
              />
            {/if}
          </div>
          <div class="cast-meta">
            <strong class="cast-name">{person.name}</strong>
            {#if person.role}
              <span class="cast-role">{person.role}</span>
            {/if}
          </div>
        </article>
      {/each}
    </div>
  </section>
{/if}

<style>
  .credits-section {
    margin-top: 40px;
  }

  .credits-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
  }

  h2 {
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    margin: 0;
    color: var(--tx);
  }

  .carousel-nav {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .nav-arrow {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    padding: 0;
    border-radius: 14px;
    background: var(--pn);
    border: 1px solid transparent;
    color: var(--tx);
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .nav-arrow:hover:not(:disabled) {
    background: var(--pn-2);
  }

  .nav-arrow:active:not(:disabled) {
    transform: scale(0.98);
  }

  .nav-arrow:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  .nav-arrow:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .cast-carousel {
    display: flex;
    gap: 20px;
    overflow-x: auto;
    scroll-behavior: smooth;
    padding: 6px 6px 12px;
    margin: -6px -6px 0;
    scrollbar-width: none;
    -ms-overflow-style: none;
    scroll-snap-type: x mandatory;
  }

  .cast-carousel::-webkit-scrollbar {
    display: none;
  }

  .cast-card {
    flex: 0 0 132px;
    scroll-snap-align: start;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    min-width: 0;
    text-align: center;
  }

  .portrait-card {
    width: 112px;
    aspect-ratio: 1;
    position: relative;
    overflow: hidden;
    border-radius: 50%;
    background: var(--pn-2);
    transition: transform var(--t-med) var(--ease);
  }

  .cast-card:hover .portrait-card {
    transform: scale(1.03);
  }

  .portrait-img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 20%;
    display: block;
    opacity: 0;
    transition: opacity var(--t-med) var(--ease);
  }

  .portrait-img.loaded {
    opacity: 1;
  }

  .portrait-fallback {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--pn-2);
  }

  .initials-badge {
    color: var(--tx);
    font-size: 30px;
    font-weight: 800;
    letter-spacing: -0.02em;
  }

  .cast-meta {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 0 2px;
  }

  .cast-name {
    color: var(--tx);
    font-size: 14px;
    font-weight: 700;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .cast-role {
    color: var(--mu);
    font-size: 12px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  @media (prefers-reduced-motion: reduce) {
    .portrait-img {
      transition: none;
    }
  }

  @media (min-width: 1920px) {
    .cast-carousel {
      gap: 28px;
    }
    .cast-card {
      flex-basis: 168px;
    }
    .portrait-card {
      width: 144px;
    }
    .initials-badge {
      font-size: 38px;
    }
    .cast-name {
      font-size: 17px;
    }
    .cast-role {
      font-size: 14px;
    }
  }

  @media (max-width: 639px) {
    .carousel-nav {
      display: none;
    }
    .cast-carousel {
      gap: 14px;
    }
    .cast-card {
      flex-basis: 104px;
    }
    .portrait-card {
      width: 88px;
    }
    .initials-badge {
      font-size: 24px;
    }
  }
</style>

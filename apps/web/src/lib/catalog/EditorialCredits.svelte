<script lang="ts">
  import { onMount } from 'svelte';
  import type { Work } from './api';
  import Icon from '../ui/Icon.svelte';

  let { work }: { work: Work } = $props();

  interface CreditPerson {
    key: string;
    name: string;
    role: string;
    photoUrl: string;
  }

  let failedPhotos = $state<Record<string, boolean>>({});
  let track = $state<HTMLDivElement>();
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);

  const people = $derived.by(() => {
    const list: CreditPerson[] = [];
    const seen: Record<string, boolean> = {};

    for (const actor of work.actors ?? []) {
      const name = actor.name?.trim();
      if (!name) continue;
      const key = `actor:${name.toLowerCase()}`;
      if (seen[key]) continue;
      seen[key] = true;
      list.push({
        key,
        name,
        role: actor.role?.trim() || 'Distribution',
        photoUrl: (actor.thumbUrl || actor.thumb_url || '').trim()
      });
    }

    for (const director of work.directors ?? []) {
      const name = director?.trim();
      if (!name) continue;
      const key = `director:${name.toLowerCase()}`;
      if (seen[key]) continue;
      seen[key] = true;
      list.push({
        key,
        name,
        role: 'Réalisateur',
        photoUrl: ''
      });
    }

    for (const writer of work.writers ?? []) {
      const name = writer?.trim();
      if (!name) continue;
      const key = `writer:${name.toLowerCase()}`;
      if (seen[key]) continue;
      seen[key] = true;
      list.push({
        key,
        name,
        role: 'Scénario',
        photoUrl: ''
      });
    }

    return list;
  });

  function initials(name: string): string {
    return name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map(part => part[0]?.toUpperCase() ?? '')
      .join('');
  }

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
            {#if person.photoUrl && !failedPhotos[person.key]}
              <img
                class="portrait-img"
                src={person.photoUrl}
                alt={person.name}
                width="300"
                height="450"
                loading="lazy"
                decoding="async"
                onerror={() => {
                  failedPhotos = { ...failedPhotos, [person.key]: true };
                }}
              />
            {:else}
              <div class="portrait-fallback" aria-hidden="true">
                <span class="initials-badge">{initials(person.name)}</span>
              </div>
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
    margin-top: 36px;
  }

  .credits-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
  }

  h2 {
    font-family: var(--font-display);
    font-size: clamp(20px, 2.1vw, 25px);
    font-weight: 700;
    letter-spacing: -0.03em;
    margin: 0;
    color: var(--color-text-bright);
  }

  .carousel-nav {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .nav-arrow {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--color-glass-raised);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .nav-arrow:hover:not(:disabled) {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: var(--color-on-accent);
    transform: scale(1.06);
  }

  .nav-arrow:disabled {
    opacity: 0.25;
    cursor: default;
  }

  .cast-carousel {
    display: flex;
    gap: 18px;
    overflow-x: auto;
    scroll-behavior: smooth;
    padding: 8px 6px 16px;
    margin: -4px -6px;
    scrollbar-width: none;
    -ms-overflow-style: none;
    scroll-snap-type: x mandatory;
  }

  .cast-carousel::-webkit-scrollbar {
    display: none;
  }

  .cast-card {
    flex: 0 0 clamp(142px, 12.5vw, 178px);
    scroll-snap-align: start;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
    text-align: center;
  }

  .portrait-card {
    aspect-ratio: 2 / 3;
    position: relative;
    overflow: hidden;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    box-shadow: var(--shadow-card);
    transition:
      transform 240ms cubic-bezier(0.25, 0.8, 0.25, 1),
      border-color 240ms ease,
      box-shadow 240ms ease;
  }

  .cast-card:hover .portrait-card {
    transform: scale(1.04);
    border-color: var(--color-line-strong);
    box-shadow: var(--shadow-card-hover);
  }

  .portrait-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .portrait-fallback {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    background:
      radial-gradient(circle at 50% 30%, rgba(249, 115, 22, 0.16), transparent 68%),
      var(--color-raised);
  }

  .initials-badge {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--color-surface);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
    font-family: var(--font-display);
    font-size: 22px;
    font-weight: 750;
    letter-spacing: -0.02em;
  }

  .cast-meta {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 0 4px;
  }

  .cast-name {
    color: var(--color-text-bright);
    font-size: 14px;
    font-weight: 650;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .cast-role {
    color: var(--color-muted);
    font-size: 12.5px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  @media (max-width: 640px) {
    .carousel-nav {
      display: none;
    }
    .cast-card {
      flex: 0 0 calc((100vw - 64px) / 2.35);
    }
    .cast-carousel {
      gap: 12px;
    }
  }
</style>

<script lang="ts">
  import { resolve } from '$app/paths';
  import type { Work } from './api';
  import HeroBanner from '../ui/HeroBanner.svelte';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import ContinueWatching from './ContinueWatching.svelte';
  import Icon from '../ui/Icon.svelte';

  let {
    films = [],
    series = [],
    totalFilms = 0,
    totalSeries = 0,
    featured = null,
    onPlay,
    detailUrl
  }: {
    films: Work[];
    series: Work[];
    totalFilms: number;
    totalSeries: number;
    featured?: Work | null;
    onPlay: (work: Work) => void;
    detailUrl: (work: Work) => string;
  } = $props();

  // Dynamically extract real genres present in the catalog
  const discoveredGenres = $derived.by(() => {
    const allWorks = [...films, ...series];
    const genreMap: Record<string, Work[]> = {};
    for (const work of allWorks) {
      if (work.genres?.length) {
        for (const genre of work.genres) {
          if (!genreMap[genre]) {
            genreMap[genre] = [];
          }
          if (!genreMap[genre].some(w => w.id === work.id)) {
            genreMap[genre].push(work);
          }
        }
      }
    }
    return Object.entries(genreMap)
      .filter(([, items]) => items.length >= 2)
      .slice(0, 3);
  });
</script>

<div class="catalog-home">
  <!-- Immersive Cinematic Hero -->
  <HeroBanner {featured} {onPlay} />

  <!-- Continue Watching Section -->
  <ContinueWatching />

  <!-- Recently Added Movies -->
  {#if films.length > 0}
    <MediaShelf
      title="Films récemment ajoutés"
      count={totalFilms}
      viewAllHref={resolve('films/')}
      viewAllLabel="Tout voir →"
    >
      {#each films as work (work.id)}
        <Poster {work} href={detailUrl(work)} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Recently Added Series -->
  {#if series.length > 0}
    <MediaShelf
      title="Séries récemment ajoutées"
      count={totalSeries}
      viewAllHref={resolve('series/')}
      viewAllLabel="Tout voir →"
    >
      {#each series as work (work.id)}
        <Poster {work} href={detailUrl(work)} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Real Genre Rows (if metadata contains genres) -->
  {#each discoveredGenres as [genreName, genreWorks] (genreName)}
    <MediaShelf title={genreName}>
      {#each genreWorks as work (work.id)}
        <Poster {work} href={detailUrl(work)} />
      {/each}
    </MediaShelf>
  {/each}

  <!-- Preserved Slots with upgraded aesthetics -->
  <section class="shelf" aria-labelledby="shelf-next-episodes-heading">
    <div class="section-heading">
      <h2 id="shelf-next-episodes-heading">Continuer les séries en cours</h2>
    </div>
    <div class="compact-slot">
      <div class="slot-icon" aria-hidden="true">
        <Icon name="tv" size={24} />
      </div>
      <div class="slot-text">
        <strong>Progression des épisodes</strong>
        <p>Vos prochains épisodes à regarder apparaîtront automatiquement ici au fur et à mesure de votre progression.</p>
      </div>
    </div>
  </section>

  <section class="shelf" aria-labelledby="shelf-favorites-heading">
    <div class="section-heading">
      <h2 id="shelf-favorites-heading">Favoris & Listes d’envies</h2>
    </div>
    <div class="compact-slot">
      <div class="slot-icon" aria-hidden="true">
        <Icon name="star" size={24} />
      </div>
      <div class="slot-text">
        <strong>Votre sélection personnalisée</strong>
        <p>Enregistrez vos coups de cœur et composez votre liste de visionnage pour vos futures séances.</p>
      </div>
    </div>
  </section>
</div>

<style>
  .catalog-home {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .shelf {
    margin: 36px 0;
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  h2 {
    font-size: clamp(20px, 2.2vw, 24px);
    font-weight: 650;
    letter-spacing: -0.03em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .compact-slot {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 22px 24px;
    border-radius: var(--radius-md);
    background: rgba(255, 255, 255, 0.02);
    border: 1px dashed var(--color-line);
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }

  .compact-slot:hover {
    background: rgba(255, 255, 255, 0.04);
    border-color: rgba(167, 139, 250, 0.25);
  }

  .slot-icon {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-accent);
    flex-shrink: 0;
  }

  .slot-text strong {
    display: block;
    font-size: 14px;
    font-weight: 600;
    color: var(--color-text);
    margin-bottom: 4px;
  }

  .slot-text p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--color-muted);
  }
</style>

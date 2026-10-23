<script lang="ts">
  import { resolve } from '$app/paths';
  import type { Work } from './api';
  import HeroBanner from '../ui/HeroBanner.svelte';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import ContinueWatching from './ContinueWatching.svelte';

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
    totalFilms: number | null;
    totalSeries: number | null;
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
  <ContinueWatching works={[...films,...series]} />

  <!-- Recently Added Movies -->
  {#if films.length > 0}
    <MediaShelf
      title="Films récemment ajoutés"
      count={totalFilms || undefined}
      viewAllHref={resolve('films/')}
      viewAllLabel="Tout voir →"
    >
      {#each films as work, index (work.id)}
        <Poster {work} href={detailUrl(work)} eager={index < 6} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Recently Added Series -->
  {#if series.length > 0}
    <MediaShelf
      title="Séries récemment ajoutées"
      count={totalSeries || undefined}
      viewAllHref={resolve('series/')}
      viewAllLabel="Tout voir →"
    >
      {#each series as work, index (work.id)}
        <Poster {work} href={detailUrl(work)} eager={films.length === 0 && index < 6} />
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

</div>

<style>
.catalog-home { display:flex; flex-direction:column; gap:var(--space-8); }
</style>

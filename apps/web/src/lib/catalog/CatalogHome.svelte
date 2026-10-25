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
    featured = null,
    onPlay,
    detailUrl
  }: {
    films: Work[];
    series: Work[];
    featured?: Work | null;
    onPlay: (work: Work) => void;
    detailUrl: (work: Work) => string;
  } = $props();

  const allWorks = $derived([...films, ...series]);

  // Featured selection: prioritize works with backdrops, rotate among them dynamically
  const backdropWorks = $derived(allWorks.filter(w => Boolean(w.artwork?.backdrop)));
  const candidateWorks = $derived(backdropWorks.length > 0 ? backdropWorks : allWorks);

  let featuredIndex = $state(0);
  let randomSeed = $state(0);

  // Initialize with a dynamic index so the hero changes across visits
  $effect(() => {
    if (candidateWorks.length > 1 && featuredIndex === 0) {
      featuredIndex = Math.floor(Math.random() * candidateWorks.length);
    }
  });

  const activeFeatured = $derived.by(() => {
    if (candidateWorks.length === 0) return featured ?? null;
    return candidateWorks[featuredIndex % candidateWorks.length] ?? featured ?? null;
  });

  function randomizeFeatured() {
    if (candidateWorks.length <= 1) return;
    const nextOffset = 1 + Math.floor(Math.random() * (candidateWorks.length - 1));
    featuredIndex = (featuredIndex + nextOffset) % candidateWorks.length;
    randomSeed += 1;
  }

  function reRollRandom() {
    randomSeed += 1;
  }

  // Shelf 1: Random gems / surprise discoveries (shuffled and changes on reRoll or visit)
  const randomPicks = $derived.by(() => {
    void randomSeed;
    if (allWorks.length <= 2) return [];
    const shuffled = [...allWorks];
    for (let i = shuffled.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
    }
    return shuffled.slice(0, 16);
  });

  // Shelf 2: Top rated works (if ratings are present)
  const topRated = $derived.by(() => {
    return allWorks
      .filter(w => (w.rating ?? 0) >= 6)
      .sort((a, b) => (b.rating ?? 0) - (a.rating ?? 0))
      .slice(0, 16);
  });

  // Shelf 3: Vintage & Classics (released before 2010)
  const classics = $derived.by(() => {
    return allWorks
      .filter(w => w.year && w.year < 2010)
      .sort((a, b) => (a.year ?? 0) - (b.year ?? 0))
      .slice(0, 16);
  });

  // Shelf 4: Mini-series & Short series (<= 2 seasons)
  const shortSeries = $derived.by(() => {
    return series
      .filter(w => (w.seasonCount ?? 0) > 0 && (w.seasonCount ?? 0) <= 2)
      .slice(0, 16);
  });

  // Shelf 5: Rotating diverse genres (shuffled so themes change every time)
  const rotatingGenres = $derived.by(() => {
    void randomSeed;
    const genreMap: Record<string, Work[]> = {};
    for (const work of allWorks) {
      if (work.genres?.length) {
        for (const genre of work.genres) {
          if (!genreMap[genre]) genreMap[genre] = [];
          if (!genreMap[genre].some(w => w.id === work.id)) {
            genreMap[genre].push(work);
          }
        }
      }
    }
    let entries = Object.entries(genreMap).filter(([, items]) => items.length >= 2);
    if (entries.length === 0) {
      entries = Object.entries(genreMap).filter(([, items]) => items.length >= 1);
    }
    for (let i = entries.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [entries[i], entries[j]] = [entries[j], entries[i]];
    }
    return entries.slice(0, 4);
  });
</script>

<div class="catalog-home">
  <!-- Immersive Cinematic Hero with dynamic rotation -->
  <HeroBanner
    featured={activeFeatured}
    {onPlay}
    onRandomize={candidateWorks.length > 1 ? randomizeFeatured : undefined}
  />

  <!-- Continue Watching Section -->
  <ContinueWatching works={allWorks} />

  <!-- Random Discoveries Shelf (Pépites au hasard) -->
  {#if randomPicks.length >= 2}
    <MediaShelf
      title="Pépites au hasard · À découvrir"
      onAction={reRollRandom}
      actionLabel="Mélanger"
    >
      {#each randomPicks as work, index (work.id)}
        <Poster {work} href={detailUrl(work)} eager={index < 6} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Top Rated Section (if available) -->
  {#if topRated.length >= 2}
    <MediaShelf
      title="Les mieux notés · Coups de cœur"
    >
      {#each topRated as work, index (work.id)}
        <Poster {work} href={detailUrl(work)} eager={index < 4} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Recently Added Movies -->
  {#if films.length > 0}
    <MediaShelf
      title="Films récemment ajoutés"
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
      viewAllHref={resolve('series/')}
      viewAllLabel="Tout voir →"
    >
      {#each series as work, index (work.id)}
        <Poster {work} href={detailUrl(work)} eager={films.length === 0 && index < 6} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Vintage & Classics (if available) -->
  {#if classics.length >= 2}
    <MediaShelf
      title="Grands classiques & Cinéma d'époque"
    >
      {#each classics as work (work.id)}
        <Poster {work} href={detailUrl(work)} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Short series / Mini-series to binge (if available) -->
  {#if shortSeries.length >= 2}
    <MediaShelf
      title="Séries à dévorer · Formats courts"
    >
      {#each shortSeries as work (work.id)}
        <Poster {work} href={detailUrl(work)} />
      {/each}
    </MediaShelf>
  {/if}

  <!-- Rotating Diverse Genre Rows -->
  {#each rotatingGenres as [genreName, genreWorks] (genreName)}
    <MediaShelf title={`Genre · ${genreName}`}>
      {#each genreWorks as work (work.id)}
        <Poster {work} href={detailUrl(work)} />
      {/each}
    </MediaShelf>
  {/each}
</div>

<style>
.catalog-home { display:flex; flex-direction:column; gap:var(--space-8); }
</style>

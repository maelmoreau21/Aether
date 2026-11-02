<script lang="ts">
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import type { Work } from './api';
  import HeroBanner from '../ui/HeroBanner.svelte';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import ContinueWatching from './ContinueWatching.svelte';

  let { films = [], series = [], featured = null, onPlay, detailUrl }: {
    films: Work[];
    series: Work[];
    featured?: Work | null;
    onPlay: (work: Work) => void;
    detailUrl: (work: Work) => string;
  } = $props();
  const allWorks = $derived([...films, ...series]);
  // Use the same language/manual artwork selection as cards and detail pages.
  const activeFeatured = $derived(allWorks.find(work => selectArtwork(work, $session).backdrop) ?? featured ?? allWorks[0] ?? null);
  const topRated = $derived(allWorks.filter(work => (work.rating ?? 0) >= 6).sort((a, b) => (b.rating ?? 0) - (a.rating ?? 0)).slice(0, 12));
</script>

<div class="catalog-home">
  <HeroBanner featured={activeFeatured} {onPlay} />
  <ContinueWatching works={allWorks} />
  {#if films.length}
    <MediaShelf title="Films récemment ajoutés" viewAllHref={resolve('films/?sort=added-desc')}>
      {#each films.slice(0, 16) as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
    </MediaShelf>
  {/if}
  {#if series.length}
    <MediaShelf title="Séries récemment ajoutées" viewAllHref={resolve('series/?sort=added-desc')}>
      {#each series.slice(0, 16) as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
    </MediaShelf>
  {/if}
  {#if topRated.length >= 4}
    <MediaShelf title="Les mieux notés de votre médiathèque">
      {#each topRated as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
    </MediaShelf>
  {/if}
</div>

<style>
  .catalog-home { display:flex; flex-direction:column; gap:clamp(28px,4vw,48px); }
</style>

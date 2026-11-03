<script lang="ts">
  import { resolve } from '$app/paths';
  import { preferenceState, DEFAULT_HOME_SECTIONS } from '../preferences';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import type { Work } from './api';
  import HeroBanner from '../ui/HeroBanner.svelte';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import ContinueWatching from './ContinueWatching.svelte';
  import RecommendationShelf from './RecommendationShelf.svelte';

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
  <RecommendationShelf />
  {#each ($preferenceState.homeSections ?? DEFAULT_HOME_SECTIONS) as section (section)}
    {#if section === 'hero'}
      <HeroBanner featured={activeFeatured} {onPlay} />
    {:else if section === 'continue'}
      <ContinueWatching works={allWorks} />
    {:else if section === 'films' && films.length}
      <MediaShelf title="Films récemment ajoutés" viewAllHref={resolve('films/?sort=added-desc')}>
        {#each films.slice(0, 16) as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
      </MediaShelf>
    {:else if section === 'series' && series.length}
      <MediaShelf title="Séries récemment ajoutées" viewAllHref={resolve('series/?sort=added-desc')}>
        {#each series.slice(0, 16) as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
      </MediaShelf>
    {:else if section === 'rated' && topRated.length >= 4}
      <MediaShelf title="Les mieux notés de votre médiathèque">
        {#each topRated as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
      </MediaShelf>
    {/if}
  {/each}
  {#if $preferenceState.homeSections?.length === 0}
    <p>Les sections de l’accueil sont masquées. Vous pouvez les réactiver dans les <a href={resolve('settings/#display')}>paramètres d’affichage</a>.</p>
  {/if}
</div>

<style>
  .catalog-home { display:flex; flex-direction:column; gap:clamp(28px,4vw,48px); }
</style>

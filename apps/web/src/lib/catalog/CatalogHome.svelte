<script lang="ts">
  import { resolve } from '$app/paths';
  import { preferenceState, DEFAULT_HOME_SECTIONS } from '../preferences';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import type { Work } from './api';
  import HeroBanner from '../ui/HeroBanner.svelte';
  import { newHeroSeed, pickHero } from './hero-picks';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import ContinueWatching from './ContinueWatching.svelte';
  import RecommendationShelf from './RecommendationShelf.svelte';
  import TonightMosaic from './TonightMosaic.svelte';
  import { pickTonight } from './tonight';
  import DiscoverFeed from './DiscoverFeed.svelte';
  import { page } from '$app/state';

  let { films = [], series = [], featured = null, onPlay, detailUrl }: {
    films: Work[];
    series: Work[];
    featured?: Work | null;
    onPlay: (work: Work) => void;
    detailUrl: (work: Work) => string;
  } = $props();
  const sections = $derived($preferenceState.homeSections ?? DEFAULT_HOME_SECTIONS);
  const allWorks = $derived([...films, ...series]);
  // Use the same language/manual artwork selection as cards and detail pages.
  const hasWideArt = (work: Work) => { const artwork = selectArtwork(work, $session); return Boolean(artwork.backdrop || artwork.landscape); };
  const hasArt = (work: Work) => { const artwork = selectArtwork(work, $session); return Boolean(artwork.poster || artwork.landscape || artwork.backdrop); };
  // À l'affiche : quatre œuvres avec une grande image, tirées au hasard à chaque visite de l'accueil
  // (graine fixe pendant la visite pour ne pas remélanger quand les listes se mettent à jour).
  const heroSeed = newHeroSeed();
  const heroCandidates = $derived(pickHero(films, series, hasWideArt, heroSeed));
  const activeFeatured = $derived(heroCandidates[0] ?? featured ?? allWorks[0] ?? null);
  const showHero = $derived(sections.includes('hero'));
  const tonight = $derived(pickTonight(films, series, showHero ? heroCandidates.map(work => work.id) : [], hasArt));
  const topRated = $derived(allWorks.filter(work => (work.rating ?? 0) >= 6).sort((a, b) => (b.rating ?? 0) - (a.rating ?? 0)).slice(0, 12));
  let heroTint = $state<string | null>(null);
  // Œuvres déjà mises en avant plus haut : le fil sans fin ne les repropose pas.
  const shownAbove = $derived([...heroCandidates, ...tonight, ...films.slice(0, 16), ...series.slice(0, 16), ...topRated].map(work => work.id));
  const library = $derived(page.url.searchParams.get('library') ?? '');
</script>

<div class="catalog-home">
  {#if showHero}
    <!-- Couche d'ambiance unique : teinte de l'œuvre à l'affiche (aucun formulaire sur l'accueil). -->
    <div class="ambient" aria-hidden="true" style:--ambient-tint={heroTint ?? 'transparent'}></div>
  {/if}
  <RecommendationShelf />
  {#each sections as section (section)}
    {#if section === 'hero'}
      <HeroBanner featured={activeFeatured} candidates={heroCandidates} {onPlay} onTint={color => (heroTint = color)} />
    {:else if section === 'continue'}
      <ContinueWatching works={allWorks} />
    {:else if section === 'tonight' && tonight.length}
      <TonightMosaic works={tonight} {detailUrl} />
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
  {#if allWorks.length}
    <DiscoverFeed {films} {series} exclude={shownAbove} {library} {detailUrl} />
  {/if}
  {#if $preferenceState.homeSections?.length === 0}
    <p>Les sections de l’accueil sont masquées. Vous pouvez les réactiver dans les <a href={resolve('settings/#display')}>paramètres d’affichage</a>.</p>
  {/if}
</div>

<style>
  /* Interpolation de la teinte entre deux œuvres (sans @property : changement direct). */
  @property --ambient-tint {
    syntax: '<color>';
    inherits: false;
    initial-value: transparent;
  }

  .catalog-home { position: relative; isolation: isolate; display: flex; flex-direction: column; gap: clamp(28px, 4vw, 48px); }

  /* Fond d'ambiance : une seule couche par page, sous le contenu, jamais derrière un formulaire */
  .ambient {
    position: absolute;
    z-index: -1;
    top: -160px;
    left: 50%;
    width: 100vw;
    height: 960px;
    margin-left: -50vw;
    background: radial-gradient(70% 60% at 40% 0%, var(--ambient-tint) 0%, transparent 70%);
    opacity: var(--ambient-opacity);
    pointer-events: none;
    transition: background var(--t-slow) var(--ease), --ambient-tint var(--t-slow) var(--ease);
  }

  @media (max-width: 639px) {
    .ambient { height: 720px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .ambient { transition: none; }
  }

  .catalog-home > p { max-width: 65ch; margin: 0 auto; padding: 40px 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); color: var(--mu); font-size: 15px; line-height: 1.6; text-align: center; }
  .catalog-home > p a { color: var(--tx); font-weight: 700; text-decoration: underline; text-decoration-color: var(--line-2); text-underline-offset: 3px; }
  .catalog-home > p a:hover { text-decoration-color: var(--tx); }
  .catalog-home > p a:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; border-radius: 4px; }
</style>

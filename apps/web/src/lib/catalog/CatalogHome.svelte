<script module lang="ts">
  import type { Work } from './api';

  /** Prioritise les œuvres notées >= 7, et complète avec >= 6 si moins de 8 atteignent 7. */
  export function selectTopRatedSample(items: Work[]): Work[] {
    const sevenPlus = items.filter(work => (work.rating ?? 0) >= 7);
    if (sevenPlus.length >= 8) return sevenPlus;
    const seen = new Set(sevenPlus.map(work => work.id));
    const sixToSeven = items.filter(work => {
      const rating = work.rating ?? 0;
      return rating >= 6 && rating < 7 && !seen.has(work.id);
    });
    return [...sevenPlus, ...sixToSeven];
  }
</script>

<script lang="ts">
  import { untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import { preferenceState, DEFAULT_HOME_SECTIONS } from '../preferences';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { cachedCatalog, homeSampleUrls, isCatalogFresh, loadCatalog, takeHomeVisitSeed } from './page-cache';
  import HeroBanner from '../ui/HeroBanner.svelte';
  import { pickHero } from './hero-picks';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import ContinueWatching from './ContinueWatching.svelte';
  import RecommendationShelf from './RecommendationShelf.svelte';
  import TonightMosaic from './TonightMosaic.svelte';
  import { pickTonight, pickTonightFrom } from './tonight';
  import DiscoverFeed from './DiscoverFeed.svelte';
  import Icon from '../ui/Icon.svelte';
  import { accessibleLibraries } from './libraries';
  import { homeLibraryCardsEnabled } from '../config/presentation';
  import { page } from '$app/state';
  import { t } from '../i18n';

  let { films = [], series = [], featured = null, catalogReady = true, onPlay, detailUrl }: {
    films: Work[];
    series: Work[];
    featured?: Work | null;
    catalogReady?: boolean;
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
  const heroSeed = takeHomeVisitSeed(untrack(() => page.url.searchParams.get('library') ?? ''));
  const heroCandidates = $derived(pickHero(films, series, hasWideArt, heroSeed));
  const activeFeatured = $derived(heroCandidates[0] ?? featured ?? allWorks[0] ?? null);
  const showHero = $derived(sections.includes('hero'));
  const library = $derived(page.url.searchParams.get('library') ?? '');
  // « Pour ce soir » et « Les mieux notés » : échantillons aléatoires tirés par le Core dans toute
  // la médiathèque visible (œuvres non vues ; œuvres bien notées), nouvelle graine à chaque visite.
  // `null` pendant le chargement ; en cas d'échec, repli sur les ajouts récents déjà chargés.
  type Samples = { tonight: Work[] | null; rated: Work[] | null };
  let samples = $state<Samples | null>(null);
  $effect.pre(() => {
    const scope = library || undefined;
    const urls = homeSampleUrls(heroSeed, scope);
    const cachedTonight = cachedCatalog(urls.tonight);
    const cachedRated = cachedCatalog(urls.rated);
    if (cachedTonight || cachedRated) {
      samples = {
        tonight: cachedTonight?.items ?? null,
        rated: cachedRated ? selectTopRatedSample(cachedRated.items ?? []) : null
      };
    } else {
      samples = null;
    }
    if (isCatalogFresh(urls.tonight) && isCatalogFresh(urls.rated)) return;
    const controller = new AbortController();
    void loadSamples(scope, controller.signal);
    return () => controller.abort();
  });
  async function loadSamples(scope: string | undefined, signal: AbortSignal) {
    const urls = homeSampleUrls(heroSeed, scope);
    const [tonightSample, ratedSample] = await Promise.allSettled([
      loadCatalog(urls.tonight, urls.tonight).then(result => result.items ?? []),
      loadCatalog(urls.rated, urls.rated).then(result => selectTopRatedSample(result.items ?? []))
    ]);
    if (signal.aborted) return;
    samples = {
      tonight: tonightSample.status === 'fulfilled' ? tonightSample.value : null,
      rated: ratedSample.status === 'fulfilled' ? ratedSample.value : null
    };
  }
  const heroIds = $derived(showHero ? heroCandidates.map(work => work.id) : []);
  const tonight = $derived(!samples ? [] : samples.tonight ? pickTonightFrom(samples.tonight, heroIds, hasArt) : pickTonight(films, series, heroIds, hasArt));
  const topRated = $derived.by(() => {
    if (!samples) return [];
    if (!samples.rated) return allWorks.filter(work => (work.rating ?? 0) >= 6).sort((a, b) => (b.rating ?? 0) - (a.rating ?? 0)).slice(0, 12);
    // Pas deux fois la même œuvre entre la mosaïque et la rangée.
    const taken = new Set(tonight.map(work => work.id));
    return samples.rated.filter(work => !taken.has(work.id)).slice(0, 12);
  });
  let heroTint = $state<string | null>(null);
  // Œuvres déjà mises en avant plus haut : le fil sans fin ne les repropose pas.
  const shownAbove = $derived([...heroCandidates, ...tonight, ...films.slice(0, 16), ...series.slice(0, 16), ...topRated].map(work => work.id));
</script>

<div class="catalog-home">
  {#if showHero}
    <!-- Couche d'ambiance unique : teinte de l'œuvre à l'affiche (aucun formulaire sur l'accueil). -->
    <div class="ambient" aria-hidden="true" style:--ambient-tint={heroTint ?? 'transparent'}></div>
  {:else if $homeLibraryCardsEnabled && !library && $accessibleLibraries.length > 0}
    <section class="home-libraries-row" aria-labelledby="home-libraries-heading">
      <h2 id="home-libraries-heading">{$t('media_library')}</h2>
      <div class="home-libraries-grid">
        {#each $accessibleLibraries as lib (lib.id)}
          {@const visualKind = lib.kind === 'series' && lib.anime ? 'anime' : lib.kind}
          {@const kindLabel = lib.kind === 'music' ? $t('music') : lib.kind === 'films' ? $t('films') : lib.anime ? $t('anime') : $t('series')}
          {@const libHref = lib.kind === 'music' ? resolve('music/') : resolve(`${lib.kind}/?library=${encodeURIComponent(lib.id)}` as `films/?${string}`)}
          <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- libHref is already resolved above. -->
          <a class="home-library-card" href={libHref}>
            <span class="home-lib-art" data-kind={visualKind}>
              {#if lib.imageUrl}
                <img class="home-lib-img" src={lib.imageUrl} alt="" loading="lazy" decoding="async" onerror={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }} />
              {:else if lib.coverUrls?.length}
                <span class="home-lib-collage" data-count={Math.min(lib.coverUrls.length, 4)} aria-hidden="true">
                  {#each lib.coverUrls.slice(0, 4) as url (url)}
                    <img src={url} alt="" loading="lazy" decoding="async" onerror={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }} />
                  {/each}
                </span>
              {/if}
              <span class="home-lib-overlay" aria-hidden="true"></span>
              <span class="home-lib-info">
                <span class="home-lib-badge"><Icon name={lib.kind === 'films' ? 'film' : lib.kind === 'music' ? 'playlist' : 'tv'} size={15} /> {kindLabel}</span>
                <span class="home-lib-title">{lib.name}</span>
              </span>
            </span>
          </a>
        {/each}
      </div>
    </section>
  {/if}
  {#each sections as section (section)}
    {#if section === 'hero'}
      <HeroBanner featured={activeFeatured} candidates={heroCandidates} {onPlay} onTint={color => (heroTint = color)} />
      {#if $homeLibraryCardsEnabled && !library && $accessibleLibraries.length > 0}
        <section class="home-libraries-row" aria-labelledby="home-libraries-heading">
          <h2 id="home-libraries-heading">{$t('media_library')}</h2>
          <div class="home-libraries-grid">
            {#each $accessibleLibraries as lib (lib.id)}
              {@const visualKind = lib.kind === 'series' && lib.anime ? 'anime' : lib.kind}
              {@const kindLabel = lib.kind === 'music' ? $t('music') : lib.kind === 'films' ? $t('films') : lib.anime ? $t('anime') : $t('series')}
              {@const libHref = lib.kind === 'music' ? resolve('music/') : resolve(`${lib.kind}/?library=${encodeURIComponent(lib.id)}` as `films/?${string}`)}
              <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- libHref is already resolved above. -->
              <a class="home-library-card" href={libHref}>
                <span class="home-lib-art" data-kind={visualKind}>
                  {#if lib.imageUrl}
                    <img class="home-lib-img" src={lib.imageUrl} alt="" loading="lazy" decoding="async" onerror={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }} />
                  {:else if lib.coverUrls?.length}
                    <span class="home-lib-collage" data-count={Math.min(lib.coverUrls.length, 4)} aria-hidden="true">
                      {#each lib.coverUrls.slice(0, 4) as url (url)}
                        <img src={url} alt="" loading="lazy" decoding="async" onerror={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }} />
                      {/each}
                    </span>
                  {/if}
                  <span class="home-lib-overlay" aria-hidden="true"></span>
                  <span class="home-lib-info">
                    <span class="home-lib-badge"><Icon name={lib.kind === 'films' ? 'film' : lib.kind === 'music' ? 'playlist' : 'tv'} size={15} /> {kindLabel}</span>
                    <span class="home-lib-title">{lib.name}</span>
                  </span>
                </span>
              </a>
            {/each}
          </div>
        </section>
      {/if}
    {:else if section === 'continue'}
      <ContinueWatching works={allWorks} />
    {:else if section === 'tonight' && tonight.length}
      <TonightMosaic works={tonight} {detailUrl} />
    {:else if section === 'films' && films.length}
      <MediaShelf title={$t('recently_added_movies')} viewAllHref={resolve('films/?sort=added-desc')}>
        {#each films.slice(0, 16) as work, index (work.id)}<Poster {work} href={detailUrl(work)} eager={sections[0] === 'films' && index < 4} />{/each}
      </MediaShelf>
    {:else if section === 'series' && series.length}
      <MediaShelf title={$t('recently_added_series')} viewAllHref={resolve('series/?sort=added-desc')}>
        {#each series.slice(0, 16) as work, index (work.id)}<Poster {work} href={detailUrl(work)} eager={sections[0] === 'series' && index < 4} />{/each}
      </MediaShelf>
    {:else if section === 'rated' && topRated.length >= 4}
      <MediaShelf title={$t('top_rated_library')}>
        {#each topRated as work, index (work.id)}<Poster {work} href={detailUrl(work)} eager={sections[0] === 'rated' && index < 4} />{/each}
      </MediaShelf>
    {/if}
    {#if section === 'tonight'}<RecommendationShelf />{/if}
  {/each}
  {#if sections.length && !sections.includes('tonight')}<RecommendationShelf />{/if}
  {#if allWorks.length && catalogReady}
    <DiscoverFeed {films} {series} exclude={shownAbove} {library} {detailUrl} />
  {/if}
  {#if $preferenceState.homeSections?.length === 0}
    <p>{$t('home_sections_hidden')} <a href={resolve('settings/#display')}>{$t('settings_nav_display').toLowerCase()}</a>.</p>
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

  .home-libraries-row { display: flex; flex-direction: column; gap: 14px; }
  .home-libraries-row h2 { margin: 0; font-size: calc(22px * var(--ui-scale, 1)); font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  .home-libraries-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 16px; }
  .home-library-card { display: block; border-radius: var(--r-md, 14px); text-decoration: none; color: #fff; transition: transform var(--t-fast) ease, box-shadow var(--t-fast) ease; }
  .home-library-card:hover { transform: translateY(-2px); box-shadow: 0 12px 28px rgb(0 0 0 / 0.35); }
  .home-library-card:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .home-lib-art { position: relative; display: block; width: 100%; aspect-ratio: 16 / 9; border-radius: inherit; overflow: hidden; border: 1px solid rgb(255 255 255 / 0.12); background: linear-gradient(135deg, #1c1218 0%, #9a3412 100%); }
  .home-lib-art[data-kind='series'] { background: linear-gradient(135deg, #0c1426 0%, #1d4ed8 100%); }
  .home-lib-art[data-kind='anime'] { background: linear-gradient(135deg, #1a0f2e 0%, #7e22ce 100%); }
  .home-lib-art[data-kind='music'] { background: linear-gradient(135deg, #09201c 0%, #059669 100%); }
  .home-lib-img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .home-lib-collage { display: grid; grid-template-columns: repeat(4, 1fr); width: 100%; height: 100%; gap: 2px; }
  .home-lib-collage[data-count='1'] { grid-template-columns: 1fr; }
  .home-lib-collage[data-count='2'] { grid-template-columns: repeat(2, 1fr); }
  .home-lib-collage[data-count='3'] { grid-template-columns: repeat(3, 1fr); }
  .home-lib-collage img { width: 100%; height: 100%; object-fit: cover; }
  .home-lib-overlay { position: absolute; inset: 0; background: linear-gradient(180deg, rgb(0 0 0 / 0.1) 0%, rgb(0 0 0 / 0.78) 100%); pointer-events: none; }
  .home-lib-info { position: absolute; inset: auto 14px 12px 14px; display: flex; flex-direction: column; align-items: flex-start; gap: 4px; z-index: 2; }
  .home-lib-badge { display: inline-flex; align-items: center; gap: 6px; padding: 3px 8px; border-radius: 6px; background: rgb(0 0 0 / 0.5); backdrop-filter: blur(4px); font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em; color: rgb(255 255 255 / 0.9); }
  .home-lib-title { font-size: 17px; font-weight: 800; letter-spacing: -0.01em; color: #fff; text-shadow: 0 2px 8px rgb(0 0 0 / 0.6); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
</style>

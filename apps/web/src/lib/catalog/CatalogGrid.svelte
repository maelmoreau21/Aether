<script lang="ts">
  import { page } from '$app/state';
  import { resolve } from '$app/paths';
  import type { Work } from './api';
  import Poster from './Poster.svelte';
  import Icon from '../ui/Icon.svelte';

  let {
    section,
    films = [],
    series = [],
    totalFilms = 0,
    totalSeries = 0,
    search = $bindable(''),
    sort = $bindable('title-asc'),
    selectedKindFilter = $bindable('all'),
    query = '',
    offset = 0,
    title = '',
    detailUrl
  }: {
    section: 'films' | 'series' | 'search';
    films: Work[];
    series: Work[];
    totalFilms: number;
    totalSeries: number;
    search: string;
    sort: string;
    selectedKindFilter: 'all' | 'film' | 'series';
    query: string;
    offset: number;
    title: string;
    detailUrl: (work: Work) => string;
  } = $props();

  const paginationTotal = $derived(section === 'films' || selectedKindFilter === 'film' ? totalFilms : section === 'series' || selectedKindFilter === 'series' ? totalSeries : Math.max(totalFilms,totalSeries));
  const base = $derived(resolve('/').replace(/\/$/, ''));

  // Local genre quick filter
  let activeGenre = $state<string | null>(null);

  const allAvailableGenres = $derived.by(() => {
    const works = [...films, ...series];
    const genres = works.flatMap(w => w.genres ?? []);
    return genres.filter((g, index, arr) => arr.indexOf(g) === index).sort();
  });

  const filteredFilms = $derived.by(() => {
    let list = films;
    if (activeGenre) {
      list = list.filter(w => w.genres?.includes(activeGenre!));
    }
    return list;
  });

  const filteredSeries = $derived.by(() => {
    let list = series;
    if (activeGenre) {
      list = list.filter(w => w.genres?.includes(activeGenre!));
    }
    return list;
  });
</script>

<div class="catalog-grid-view">
  <!-- Page Header -->
  <div class="page-heading">
    <div class="heading-badge-row">
      <span class="eyebrow">CATALOGUE AETHER</span>
      {#if section === 'films'}
        <span class="count-pill">{totalFilms} film{totalFilms > 1 ? 's' : ''}</span>
      {:else if section === 'series'}
        <span class="count-pill">{totalSeries} série{totalSeries > 1 ? 's' : ''}</span>
      {:else}
        <span class="count-pill">{totalFilms + totalSeries} résultat{(totalFilms + totalSeries) > 1 ? 's' : ''}</span>
      {/if}
    </div>

    <h1>{title}</h1>
    <p class="heading-sub">
      {#if section === 'films'}
        Tous vos films indexés, avec leurs différentes versions physiques et pistes audio.
      {:else if section === 'series'}
        Toutes vos séries, organisées par saisons et épisodes.
      {:else}
        Recherchez une œuvre dans l’ensemble de vos bibliothèques.
      {/if}
    </p>
  </div>

  <!-- Filters & Search Form -->
  <form class="filters-bar" action={`${base}/${section}/`}>
    {#if page.url.searchParams.get('library')}<input type="hidden" name="library" value={page.url.searchParams.get('library')??''} />{/if}
    <div class="search-field">
      <label for="catalog-search" class="field-label">Rechercher dans le catalogue</label>
      <div class="search-input-wrapper">
        <Icon name="search" size={18} />
        <input
          id="catalog-search"
          type="search"
          name="q"
          bind:value={search}
          placeholder="Titre de film ou série…"
          maxlength="200"
        />
        {#if search}
          <button
            type="button"
            class="clear-search-btn"
            aria-label="Effacer la recherche"
            onclick={() => (search = '')}
          >
            ×
          </button>
        {/if}
      </div>
    </div>

    {#if section === 'search'}
      <div class="filter-field">
        <label for="kind-select" class="field-label">Type</label>
        <select id="kind-select" bind:value={selectedKindFilter}>
          <option value="all">Tous les médias</option>
          <option value="film">Films uniquement</option>
          <option value="series">Séries uniquement</option>
        </select>
      </div>
    {/if}

    <div class="filter-field">
      <label for="catalog-sort" class="field-label">Trier par</label>
      <select id="catalog-sort" name="sort" bind:value={sort}>
        <option value="title-asc">Titre · A à Z</option>
        <option value="title-desc">Titre · Z à A</option>
        <option value="year-desc">Année · plus récente</option>
        <option value="year-asc">Année · plus ancienne</option>
        <option value="added-desc">Ajout · plus récent</option>
        <option value="added-asc">Ajout · plus ancien</option>
      </select>
    </div>

    <button class="submit-btn" type="submit">
      Appliquer
    </button>
  </form>

  <!-- Quick Genre Filters Bar if genres exist -->
  {#if allAvailableGenres.length > 0}
    <div class="genre-pills-bar" aria-label="Filtrer par genre">
      <button
        type="button"
        class="genre-chip"
        class:active={activeGenre === null}
        onclick={() => (activeGenre = null)}
      >
        Tous les genres
      </button>
      {#each allAvailableGenres as g (g)}
        <button
          type="button"
          class="genre-chip"
          class:active={activeGenre === g}
          onclick={() => (activeGenre = activeGenre === g ? null : g)}
        >
          {g}
        </button>
      {/each}
    </div>
  {/if}

  <!-- Films Shelf / Grid -->
  {#if section !== 'series' && (selectedKindFilter === 'all' || selectedKindFilter === 'film')}
    <section class="shelf" aria-labelledby="shelf-films-title">
      <div class="section-heading">
        <h2 id="shelf-films-title">
          {#if section === 'search'}
            Films correspondants
          {:else}
            Tous les films
          {/if}
        </h2>
        <span class="count-badge">{totalFilms} film{totalFilms > 1 ? 's' : ''}</span>
      </div>

      <div class="media-grid">
        {#each filteredFilms as work, index (work.id)}
          <Poster {work} href={detailUrl(work)} eager={index < 6} />
        {:else}
          <div class="empty-media-box">
            <Icon name="film" size={32} />
            <p>
              {query ? 'Aucun film ne correspond à votre recherche.' : 'Aucun film dans cette bibliothèque.'}
            </p>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <!-- Series Shelf / Grid -->
  {#if section !== 'films' && (selectedKindFilter === 'all' || selectedKindFilter === 'series')}
    <section class="shelf" aria-labelledby="shelf-series-title">
      <div class="section-heading">
        <h2 id="shelf-series-title">
          {#if section === 'search'}
            Séries correspondantes
          {:else}
            Toutes les séries
          {/if}
        </h2>
        <span class="count-badge">{totalSeries} série{totalSeries > 1 ? 's' : ''}</span>
      </div>

      <div class="media-grid">
        {#each filteredSeries as work, index (work.id)}
          <Poster {work} href={detailUrl(work)} eager={filteredFilms.length === 0 && index < 6} />
        {:else}
          <div class="empty-media-box">
            <Icon name="tv" size={32} />
            <p>
              {query ? 'Aucune série ne correspond à votre recherche.' : 'Aucune série dans cette bibliothèque.'}
            </p>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <!-- Pagination -->
  {#if offset > 0 || paginationTotal > 0}
    <nav class="pagination" aria-label="Pages du catalogue">
      {#if offset > 0}
        <a
          class="page-nav-btn"
          href={resolve(
            `${section === 'films' ? 'films/' : section === 'series' ? 'series/' : 'search/'}?${new URLSearchParams({
              ...Object.fromEntries(page.url.searchParams),
              q: query,
              sort,
              offset: String(Math.max(0, offset - 24))
            })}`
          )}
        >
          ← Page précédente
        </a>
      {/if}
      <span class="page-indicator">Page {Math.floor(offset / 24) + 1} sur {Math.max(1, Math.ceil(paginationTotal / 24))}</span>
      {#if paginationTotal > offset + 24}
        <a
          class="page-nav-btn"
          href={resolve(
            `${section === 'films' ? 'films/' : section === 'series' ? 'series/' : 'search/'}?${new URLSearchParams({
              ...Object.fromEntries(page.url.searchParams),
              q: query,
              sort,
              offset: String(offset + 24)
            })}`
          )}
        >
          Page suivante →
        </a>
      {/if}
    </nav>
  {/if}
</div>

<style>
  .catalog-grid-view {
    display: flex;
    flex-direction: column;
    gap: 32px;
  }

  .page-heading {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 12px;
  }

  .heading-badge-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .count-pill {
    font-size: 11px;
    font-weight: 700;
    padding: 2px 10px;
    border-radius: var(--radius-full);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid rgba(167, 139, 250, 0.25);
  }

  h1 {
    font-family: var(--font-display);
    font-size: clamp(30px, 3.8vw, 48px);
    font-weight: 750;
    line-height: 1.15;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .heading-sub {
    color: var(--color-muted);
    font-size: 14.5px;
    line-height: 1.5;
    margin: 0;
    max-width: 700px;
  }

  /* Filters Bar */
  .filters-bar {
    display: flex;
    align-items: flex-end;
    gap: 16px;
    flex-wrap: wrap;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    padding: 18px 22px;
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-sm);
  }

  .field-label {
    display: block;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--color-muted);
    margin-bottom: 8px;
    letter-spacing: 0.02em;
  }

  .search-field {
    flex: 1 1 280px;
  }

  .search-input-wrapper {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-input-wrapper :global(svg) {
    position: absolute;
    left: 14px;
    color: var(--color-subtle);
    pointer-events: none;
  }

  input[type='search'] {
    width: 100%;
    min-height: 44px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    color: var(--color-text-bright);
    padding: 10px 38px 10px 42px;
    font-size: 13.5px;
    transition: all var(--transition-fast);
  }

  input[type='search']:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 12px var(--color-accent-glow);
  }

  .clear-search-btn {
    position: absolute;
    right: 10px;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.1);
    border: 0;
    color: var(--color-muted);
    font-size: 16px;
    line-height: 1;
  }

  .clear-search-btn:hover {
    color: var(--color-text-bright);
    background: rgba(255, 255, 255, 0.2);
  }

  .filter-field {
    flex: 0 1 200px;
    min-width: 160px;
  }

  .submit-btn {
    min-height: 44px;
    padding: 0 22px;
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: var(--radius-sm);
    background: rgba(255, 255, 255, 0.06);
    color: var(--color-text-bright);
    font-size: 13px;
    font-weight: 600;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .submit-btn:hover {
    background: rgba(255, 255, 255, 0.12);
    border-color: var(--color-accent);
    transform: translateY(-1px);
  }

  /* Genre Pills Bar */
  .genre-pills-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 6px;
    scrollbar-width: none;
  }

  .genre-pills-bar::-webkit-scrollbar {
    display: none;
  }

  .genre-chip {
    padding: 6px 14px;
    border-radius: var(--radius-pill);
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid var(--color-line);
    color: var(--color-muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .genre-chip:hover {
    color: var(--color-text-bright);
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.2);
  }

  .genre-chip.active {
    color: var(--color-text-bright);
    background: var(--color-accent-soft);
    border-color: rgba(167, 139, 250, 0.4);
    box-shadow: 0 0 12px rgba(139, 92, 246, 0.2);
    font-weight: 600;
  }

  /* Shelf / Grid */
  .shelf {
    margin: 8px 0 24px;
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
  }

  h2 {
    font-size: clamp(20px, 2.2vw, 24px);
    font-weight: 650;
    letter-spacing: -0.03em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .count-badge {
    font-size: 12px;
    color: var(--color-muted);
  }

  .media-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(clamp(160px, 16vw, 220px), 1fr));
    gap: 24px 18px;
  }

  .empty-media-box {
    grid-column: 1 / -1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 56px 24px;
    border-radius: var(--radius-lg);
    background: rgba(255, 255, 255, 0.02);
    border: 1px dashed var(--color-line);
    color: var(--color-subtle);
    text-align: center;
  }

  .empty-media-box p {
    margin: 0;
    color: var(--color-muted);
    font-size: 14px;
  }

  /* Pagination */
  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin: 32px 0 16px;
    padding: 20px 0;
    border-top: 1px solid var(--color-line);
  }

  .page-nav-btn {
    display: inline-flex;
    align-items: center;
    padding: 10px 20px;
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    color: var(--color-text);
    font-size: 13px;
    font-weight: 600;
    transition: all var(--transition-fast);
  }

  .page-nav-btn:hover {
    background: var(--color-raised);
    border-color: rgba(167, 139, 250, 0.3);
    color: var(--color-accent-hover);
    transform: translateY(-1px);
  }

  .page-indicator {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-muted);
    padding: 0 8px;
  }

  @media (max-width: 640px) {
    .filters-bar {
      padding: 14px;
      gap: 12px;
    }

    .filter-field {
      flex: 1 1 100%;
    }

    .submit-btn {
      width: 100%;
    }

    .media-grid {
      grid-template-columns: repeat(2, 1fr);
      gap: 16px 12px;
    }
  }
</style>

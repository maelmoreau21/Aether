<script lang="ts">
  import { page } from '$app/state';
  import { onMount, tick } from 'svelte';
  import { slide } from 'svelte/transition';
  import { resolve } from '$app/paths';
  import { goto } from '$app/navigation';
  import type { Work } from './api';
  import Poster from './Poster.svelte';
  import Icon from '../ui/Icon.svelte';

  let {
    section,
    films = [],
    series = [],
    totalFilms = 0,
    totalSeries = 0,
    moreFilms = false,
    moreSeries = false,
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
    totalFilms: number | null;
    totalSeries: number | null;
    moreFilms?: boolean;
    moreSeries?: boolean;
    search: string;
    sort: string;
    selectedKindFilter: 'all' | 'film' | 'series';
    query: string;
    offset: number;
    title: string;
    detailUrl: (work: Work) => string;
  } = $props();

  const paginationTotal = $derived(
    section === 'films' || selectedKindFilter === 'film'
      ? totalFilms
      : section === 'series' || selectedKindFilter === 'series'
        ? totalSeries
        : totalFilms === null || totalSeries === null
          ? null
          : Math.max(totalFilms, totalSeries)
  );
  const hasNext = $derived(
    section === 'films' || selectedKindFilter === 'film'
      ? moreFilms
      : section === 'series' || selectedKindFilter === 'series'
        ? moreSeries
        : moreFilms || moreSeries
  );
  const filmCount = $derived(totalFilms ?? films.length);
  const seriesCount = $derived(totalSeries ?? series.length);

  const countText = $derived.by(() => {
    if (section === 'films') {
      return `${filmCount} film${filmCount > 1 ? 's' : ''}${totalFilms === null ? ' affichés' : ''}`;
    }
    if (section === 'series') {
      return `${seriesCount} série${seriesCount > 1 ? 's' : ''}${totalSeries === null ? ' affichées' : ''}`;
    }
    const total = filmCount + seriesCount;
    return `${total} résultat${total > 1 ? 's' : ''}${paginationTotal === null ? ' affichés' : ''}`;
  });

  // Advanced query filters
  let filterGenre = $state(page.url.searchParams.get('genre') ?? '');
  let filterYear = $state(page.url.searchParams.get('year') ?? '');
  let filterQuality = $state(page.url.searchParams.get('quality') ?? '');
  let filterCodec = $state(page.url.searchParams.get('codec') ?? '');
  let filterLanguage = $state(page.url.searchParams.get('language') ?? '');

  const activeFilterCount = $derived.by(() => {
    let count = 0;
    if (filterGenre.trim()) count++;
    if (filterYear.trim()) count++;
    if (filterQuality.trim()) count++;
    if (filterCodec.trim()) count++;
    if (filterLanguage.trim()) count++;
    return count;
  });

  let showFilters = $state(false);

  // Search input ref &
  let searchInputRef = $state<HTMLInputElement | null>(null);

  function focusSearch() {
    void tick().then(() => {
      if (searchInputRef && document.activeElement !== searchInputRef) {
        searchInputRef.focus();
        if (searchInputRef.value) {
          const len = searchInputRef.value.length;
          searchInputRef.setSelectionRange(len, len);
        }
      }
    });
  }

  onMount(() => {
    if (activeFilterCount > 0) showFilters = true;
    focusSearch();
  });

  $effect(() => {
    const libId = page.url.searchParams.get('library') ?? '';
    void libId;
    void section;
    focusSearch();
  });

  function handleFilterSubmit(event?: SubmitEvent) {
    if (event) event.preventDefault();
    const params = new URLSearchParams();
    const library = page.url.searchParams.get('library');
    if (library) params.set('library', library);
    if (search.trim()) params.set('q', search.trim());
    if (sort && sort !== 'title-asc') params.set('sort', sort);
    if (section === 'search' && selectedKindFilter !== 'all') params.set('kind', selectedKindFilter);

    if (filterGenre.trim()) params.set('genre', filterGenre.trim());
    if (filterYear.trim()) params.set('year', filterYear.trim());
    if (filterQuality.trim()) params.set('quality', filterQuality.trim());
    if (filterCodec.trim()) params.set('codec', filterCodec.trim());
    if (filterLanguage.trim()) params.set('language', filterLanguage.trim());

    const targetUrl = resolve(`${section}/?${params}` as `films/?${string}`);
    void goto(targetUrl, { replaceState: true });
  }

  function clearSearch() {
    search = '';
    handleFilterSubmit();
    focusSearch();
  }

  function resetFilters() {
    filterGenre = '';
    filterYear = '';
    filterQuality = '';
    filterCodec = '';
    filterLanguage = '';
    handleFilterSubmit();
  }

  // Quick genre pill filters
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
  <!-- Sleek Minimalist Header & Toolbar -->
  <header class="catalog-header">
    <div class="title-cluster">
      <h1>{title}</h1>
      <span class="count-pill">{countText}</span>
    </div>

    <form class="toolbar" onsubmit={handleFilterSubmit}>
      <div class="search-box">
        <span class="search-icon">
          <Icon name="search" size={17} />
        </span>
        <input
          bind:this={searchInputRef}
          id="catalog-search"
          type="search"
          name="q"
          bind:value={search}
          placeholder="Rechercher…"
          maxlength="200"
          autocomplete="off"
        />
        {#if search}
          <button
            type="button"
            class="clear-btn"
            aria-label="Effacer la recherche"
            onclick={clearSearch}
          >
            ×
          </button>
        {/if}
      </div>

      {#if section === 'search'}
        <div class="control-select">
          <select
            aria-label="Filtrer par type de média"
            bind:value={selectedKindFilter}
            onchange={() => handleFilterSubmit()}
          >
            <option value="all">Tout</option>
            <option value="film">Films</option>
            <option value="series">Séries</option>
          </select>
        </div>
      {/if}

      <div class="control-select">
        <select
          aria-label="Trier par"
          bind:value={sort}
          onchange={() => handleFilterSubmit()}
        >
          <option value="title-asc">Titre · A à Z</option>
          <option value="title-desc">Titre · Z à A</option>
          <option value="year-desc">Année · Récent</option>
          <option value="year-asc">Année · Ancien</option>
          <option value="added-desc">Ajout récent</option>
          <option value="added-asc">Ajout ancien</option>
        </select>
      </div>

      <button
        type="button"
        class="filter-toggle-btn"
        class:active={showFilters || activeFilterCount > 0}
        onclick={() => (showFilters = !showFilters)}
        aria-expanded={showFilters}
        aria-label="Filtres avancés"
      >
        <Icon name="filter" size={15} />
        <span class="filter-btn-text">Filtres</span>
        {#if activeFilterCount > 0}
          <span class="filter-count-badge">{activeFilterCount}</span>
        {/if}
      </button>
    </form>
  </header>

  <!-- Expandable Advanced Filter Drawer -->
  {#if showFilters}
    <div class="filters-drawer" transition:slide={{ duration: 180 }}>
      <div class="drawer-grid">
        <label class="drawer-field">
          <span class="drawer-label">Genre</span>
          <input
            bind:value={filterGenre}
            placeholder="Animation, Drame…"
            maxlength="100"
          />
        </label>
        <label class="drawer-field">
          <span class="drawer-label">Année</span>
          <input
            type="number"
            bind:value={filterYear}
            placeholder="Ex: 2024"
            min="1800"
            max="2200"
          />
        </label>
        <label class="drawer-field">
          <span class="drawer-label">Qualité min.</span>
          <select bind:value={filterQuality}>
            <option value="">Toutes</option>
            <option value="720">720p</option>
            <option value="1080">1080p</option>
            <option value="2160">2160p (4K)</option>
          </select>
        </label>
        <label class="drawer-field">
          <span class="drawer-label">Codec</span>
          <select bind:value={filterCodec}>
            <option value="">Tous</option>
            <option value="av1">AV1</option>
            <option value="hevc">HEVC</option>
            <option value="h264">H.264</option>
            <option value="opus">Opus</option>
          </select>
        </label>
        <label class="drawer-field">
          <span class="drawer-label">Langue</span>
          <input
            bind:value={filterLanguage}
            placeholder="fra, eng…"
            maxlength="35"
          />
        </label>
      </div>
      <div class="drawer-actions">
        <button type="button" class="apply-filter-btn" onclick={() => handleFilterSubmit()}>
          Appliquer
        </button>
        {#if activeFilterCount > 0}
          <button type="button" class="reset-filter-btn" onclick={resetFilters}>
            Réinitialiser
          </button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Quick Genre Filters Bar if available -->
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
    <section class="shelf" aria-labelledby={section === 'search' ? 'shelf-films-title' : undefined}>
      {#if section === 'search'}
        <div class="section-heading">
          <h2 id="shelf-films-title">Films</h2>
          <span class="count-badge">{filmCount} film{filmCount > 1 ? 's' : ''}</span>
        </div>
      {/if}

      <div class="media-grid">
        {#each filteredFilms as work, index (work.id)}
          <Poster {work} href={detailUrl(work)} eager={index < 8} />
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
    <section class="shelf" aria-labelledby={section === 'search' ? 'shelf-series-title' : undefined}>
      {#if section === 'search'}
        <div class="section-heading">
          <h2 id="shelf-series-title">Séries</h2>
          <span class="count-badge">{seriesCount} série{seriesCount > 1 ? 's' : ''}</span>
        </div>
      {/if}

      <div class="media-grid">
        {#each filteredSeries as work, index (work.id)}
          <Poster {work} href={detailUrl(work)} eager={filteredFilms.length === 0 && index < 8} />
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
  {#if offset > 0 || films.length || series.length}
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
      <span class="page-indicator">
        Page {Math.floor(offset / 24) + 1}{#if paginationTotal !== null} sur {Math.max(1, Math.ceil(paginationTotal / 24))}{/if}
      </span>
      {#if hasNext || (paginationTotal !== null && paginationTotal > offset + 24)}
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
    gap: 20px;
    padding-top: 6px;
  }

  /* Minimalist Header & Action Bar */
  .catalog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--color-line-subtle);
  }

  .title-cluster {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  h1 {
    font-family: var(--font-display);
    font-size: clamp(22px, 2.4vw, 28px);
    font-weight: 700;
    line-height: 1.2;
    letter-spacing: -0.03em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .count-pill {
    font-size: 11.5px;
    font-weight: 600;
    padding: 3px 10px;
    border-radius: var(--radius-full);
    background: var(--color-raised);
    color: var(--color-muted);
    border: 1px solid var(--color-line);
    white-space: nowrap;
  }

  /* Toolbar */
  .toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    flex: 1 1 auto;
    justify-content: flex-end;
  }

  /* Search Box with Guaranteed Icon Padding */
  .search-box {
    position: relative;
    display: flex;
    align-items: center;
    flex: 1 1 200px;
    min-width: 170px;
    max-width: 380px;
  }

  .search-icon {
    position: absolute;
    left: 13px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--color-subtle);
    pointer-events: none;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2;
  }

  /* Explicit selector to override tokens.css input:not(...) */
  .search-box input[type='search'],
  .search-box input {
    width: 100%;
    min-height: 38px;
    height: 38px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-full);
    color: var(--color-text-bright);
    padding: 6px 34px 6px 40px !important; /* Critical: 40px left padding prevents icon overlap */
    font-size: 13.5px;
    transition: all var(--transition-fast);
  }

  .search-box input::placeholder {
    color: var(--color-muted);
    opacity: 0.8;
  }

  .search-box input:focus {
    outline: none;
    border-color: var(--color-accent);
    background: var(--color-surface);
    box-shadow: 0 0 0 2px var(--color-accent-soft);
  }

  .clear-btn {
    position: absolute;
    right: 8px;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.08);
    border: 0;
    color: var(--color-muted);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .clear-btn:hover {
    color: var(--color-text-bright);
    background: rgba(255, 255, 255, 0.18);
  }

  /* Control Selects */
  .control-select {
    position: relative;
    min-width: 130px;
    max-width: 170px;
  }

  .control-select select {
    min-height: 38px;
    height: 38px;
    padding: 6px 30px 6px 12px;
    font-size: 12.5px;
    border-radius: var(--radius-full);
    background-color: var(--color-raised);
    border: 1px solid var(--color-line);
  }

  .control-select select:hover {
    border-color: var(--color-line-strong);
  }

  /* Filter Toggle Button */
  .filter-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 38px;
    height: 38px;
    padding: 0 14px;
    border-radius: var(--radius-full);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    color: var(--color-text);
    font-size: 12.5px;
    font-weight: 500;
    transition: all var(--transition-fast);
    cursor: pointer;
    white-space: nowrap;
  }

  .filter-toggle-btn:hover {
    background: var(--color-surface-hover);
    border-color: var(--color-line-strong);
    color: var(--color-text-bright);
  }

  .filter-toggle-btn.active {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent);
    font-weight: 600;
  }

  .filter-count-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: var(--radius-full);
    background: var(--color-accent);
    color: #fff;
    font-size: 10.5px;
    font-weight: 700;
    line-height: 1;
  }

  /* Filter Drawer */
  .filters-drawer {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 20px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-sm);
  }

  .drawer-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 12px;
  }

  .drawer-field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .drawer-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-muted);
  }

  .drawer-field input,
  .drawer-field select {
    min-height: 36px;
    height: 36px;
    font-size: 12.5px;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    color: var(--color-text-bright);
  }

  .drawer-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    padding-top: 4px;
  }

  .apply-filter-btn {
    min-height: 34px;
    padding: 0 16px;
    background: var(--color-accent);
    color: var(--color-on-accent);
    border: none;
    border-radius: var(--radius-full);
    font-size: 12px;
    font-weight: 600;
    transition: all var(--transition-fast);
  }

  .apply-filter-btn:hover {
    filter: brightness(1.1);
    transform: translateY(-1px);
  }

  .reset-filter-btn {
    background: transparent;
    border: none;
    color: var(--color-muted);
    font-size: 12px;
    text-decoration: underline;
    cursor: pointer;
    padding: 4px 8px;
  }

  .reset-filter-btn:hover {
    color: var(--color-text-bright);
  }

  /* Quick Genre Pills Bar */
  .genre-pills-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow-x: auto;
    padding-bottom: 2px;
    scrollbar-width: none;
  }

  .genre-pills-bar::-webkit-scrollbar {
    display: none;
  }

  .genre-chip {
    padding: 5px 12px;
    border-radius: var(--radius-full);
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid var(--color-line);
    color: var(--color-muted);
    font-size: 11.5px;
    font-weight: 500;
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .genre-chip:hover {
    color: var(--color-text-bright);
    background: rgba(255, 255, 255, 0.08);
  }

  .genre-chip.active {
    color: var(--color-text-bright);
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    font-weight: 600;
  }

  /* Shelf / Grid */
  .shelf {
    margin: 4px 0 16px;
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
  }

  h2 {
    font-size: 18px;
    font-weight: 650;
    letter-spacing: -0.02em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .count-badge {
    font-size: 11.5px;
    color: var(--color-muted);
  }

  .media-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(clamp(155px, 12.5vw, 190px), 1fr));
    gap: 24px 18px;
  }

  @media (min-width: 1920px) {
    .media-grid {
      grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
      gap: 28px 22px;
    }
  }

  .empty-media-box {
    grid-column: 1 / -1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 64px 24px;
    border-radius: var(--radius-xl);
    background: radial-gradient(circle at 50% 0%, rgba(249, 115, 22, 0.08), transparent 65%), var(--color-surface);
    border: 1px solid var(--color-line);
    color: var(--color-subtle);
    text-align: center;
    box-shadow: var(--shadow-sm);
  }

  .empty-media-box p {
    margin: 0;
    color: var(--color-muted);
    font-size: 14px;
    max-width: 46ch;
    line-height: 1.5;
  }

  /* Pagination */
  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin: 24px 0 12px;
    padding: 16px 0;
    border-top: 1px solid var(--color-line);
  }

  .page-nav-btn {
    display: inline-flex;
    align-items: center;
    padding: 8px 16px;
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    color: var(--color-text);
    font-size: 12.5px;
    font-weight: 600;
    transition: all var(--transition-fast);
  }

  .page-nav-btn:hover {
    background: var(--color-raised);
    border-color: var(--color-accent-soft);
    color: var(--color-accent-hover);
    transform: translateY(-1px);
  }

  .page-indicator {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-muted);
    padding: 0 8px;
  }

  @media (max-width: 640px) {
    .catalog-header {
      flex-direction: column;
      align-items: stretch;
      gap: 12px;
    }

    .toolbar {
      justify-content: stretch;
    }

    .search-box {
      max-width: 100%;
      flex: 1 1 100%;
    }

    .control-select {
      flex: 1 1 calc(50% - 6px);
      max-width: none;
    }

    .filter-toggle-btn {
      flex: 1 1 100%;
      justify-content: center;
    }

    .media-grid {
      grid-template-columns: repeat(2, 1fr);
      gap: 14px 10px;
    }
  }
</style>

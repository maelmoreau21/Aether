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
  .catalog-grid-view { display: flex; flex-direction: column; gap: 20px; padding: 12px 0 48px; }

  .catalog-header { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 16px 24px; }
  .title-cluster { display: flex; align-items: baseline; gap: 14px; min-width: 0; }
  h1 { margin: 0; font-size: clamp(32px, 4vw, 44px); font-weight: 800; letter-spacing: -0.03em; line-height: 1.02; color: var(--tx); }
  .count-pill { font-size: 15px; font-weight: 600; color: var(--mu); white-space: nowrap; font-variant-numeric: tabular-nums; }

  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-left: auto; }
  .search-box { position: relative; display: flex; align-items: center; width: min(320px, 100%); }
  .search-icon { position: absolute; left: 14px; display: grid; color: var(--mu); pointer-events: none; }
  .search-box input { width: 100%; height: 44px; padding: 0 40px 0 42px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font-size: 14px; transition: background-color var(--t-fast) ease, border-color var(--t-fast) ease; }
  .search-box input:hover { background: var(--pn-2); }
  .search-box input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  .clear-btn { position: absolute; right: 6px; width: 32px; height: 32px; border: 0; border-radius: 10px; background: none; color: var(--mu); font-size: 18px; }
  .clear-btn:hover { background: var(--pn-2); color: var(--tx); }

  .control-select select { height: 44px; min-height: 44px; width: auto; min-width: 170px; border: 0; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font-size: 14px; font-weight: 600; box-shadow: none; }
  .control-select select:hover { background-color: var(--pn-2); }

  .filter-toggle-btn { display: inline-flex; align-items: center; gap: 8px; height: 44px; padding: 0 16px; border: 0; border-radius: var(--r-pill); background: var(--pn); color: var(--tx); font-size: 14px; font-weight: 700; }
  .filter-toggle-btn:hover { background: var(--pn-2); }
  .filter-toggle-btn.active { background: var(--ac-soft); color: var(--ac); }
  .filter-count-badge { display: grid; place-items: center; min-width: 20px; height: 20px; padding: 0 6px; border-radius: 10px; background: var(--ac); color: var(--on-ac); font-size: 11px; font-weight: 800; }

  .filters-drawer { padding: 20px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .drawer-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 14px; }
  .drawer-field { display: flex; flex-direction: column; gap: 8px; }
  .drawer-label { font-size: 14px; font-weight: 700; color: var(--tx); }
  .drawer-field input, .drawer-field select { height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font-size: 14px; box-shadow: none; }
  .drawer-field input:focus, .drawer-field select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  .drawer-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 16px; }
  .apply-filter-btn, .reset-filter-btn { height: 40px; padding: 0 18px; border: 0; border-radius: var(--r-sm); font-size: 14px; font-weight: 700; }
  .apply-filter-btn { background: var(--btn); color: var(--btn-tx); }
  .apply-filter-btn:hover { opacity: 0.9; }
  .reset-filter-btn { background: none; color: var(--mu); }
  .reset-filter-btn:hover { background: var(--pn); color: var(--tx); }

  .genre-pills-bar { display: flex; gap: 8px; overflow-x: auto; scrollbar-width: none; padding-bottom: 2px; }
  .genre-pills-bar::-webkit-scrollbar { display: none; }
  .genre-chip { flex: none; height: 38px; padding: 0 14px; border: 0; border-radius: var(--r-pill); background: var(--pn); color: var(--tx); font-size: 14px; font-weight: 600; white-space: nowrap; }
  .genre-chip:hover { background: var(--pn-2); }
  .genre-chip.active { background: var(--ac-soft); color: var(--ac); }

  .shelf { display: flex; flex-direction: column; gap: 14px; }
  .section-heading { display: flex; align-items: baseline; gap: 12px; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  .count-badge { font-size: 14px; font-weight: 600; color: var(--mu); }

  .media-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--poster-width, clamp(150px, 12vw, 190px)), 1fr));
    gap: var(--card-row-gap, 28px) var(--card-gap, 16px);
  }

  .empty-media-box { grid-column: 1 / -1; display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 48px 24px; border-radius: var(--r-lg); background: var(--bg-2); color: var(--mu); text-align: center; }
  .empty-media-box p { margin: 0; font-size: 15px; }

  .pagination { display: flex; flex-wrap: wrap; align-items: center; justify-content: center; gap: 12px; margin-top: 12px; }
  .page-nav-btn { display: inline-flex; align-items: center; height: 44px; padding: 0 18px; border-radius: 14px; background: var(--pn); color: var(--tx); font-size: 14px; font-weight: 700; }
  .page-nav-btn:hover { background: var(--pn-2); }
  .page-indicator { font-size: 14px; font-weight: 600; color: var(--mu); font-variant-numeric: tabular-nums; }

  @media (max-width: 1023px) {
    .toolbar { margin-left: 0; width: 100%; }
    .search-box { flex: 1 1 220px; width: auto; }
  }

  @media (max-width: 640px) {
    .catalog-grid-view { gap: 16px; padding-top: 4px; }
    .control-select select { min-width: 0; }
    .filter-btn-text { display: none; }
    .search-box { flex: 1 1 100%; }
    .media-grid {
      grid-template-columns: repeat(var(--mobile-card-columns, 3), minmax(0, 1fr));
      gap: var(--card-row-gap, 18px) var(--card-gap, 10px);
    }
  }
</style>

<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import { page } from '$app/state';
  import { onMount, tick } from 'svelte';
  import { slide } from 'svelte/transition';
  import { resolve } from '$app/paths';
  import { goto } from '$app/navigation';
  import type { Work } from './api';
  import Poster from './Poster.svelte';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  import {
    features, loadFeatures, discoverRequests, posterUrl, requestPayload,
    statusLabels, statusTone, applicationRequest, type SearchResult, type MediaRequest
  } from '../applications/api';

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
    void loadFeatures();
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

  // On-demand discovery state
  let discoverResults = $state<SearchResult[]>([]);
  // Posters that failed to load fall back to the placeholder (reactive set).
  const failedPosters = new SvelteSet<string>();
  const discoverMissing = $derived.by(() => {
    return discoverResults.filter(r => !r.inLibrary && (selectedKindFilter === 'all' || selectedKindFilter === r.kind));
  });

  // Request modal dialog state
  let requestDialog = $state<HTMLDialogElement>();
  let requestingItem = $state<SearchResult | null>(null);
  let requestingScope = $state<'all' | 'pick'>('all');
  let chosenSeasons = $state<number[]>([]);
  let requestingBusy = $state(false);
  let requestSuccessMessage = $state('');

  $effect(() => {
    const trimmed = query.trim();
    // Titles missing from Aether are offered only on the global search page, never while
    // filtering the films or series catalogue.
    if (section !== 'search' || !$features.requests || trimmed.length < 2) {
      discoverResults = [];
      return;
    }
    const controller = new AbortController();
    void discoverRequests(trimmed, controller.signal)
      .then(data => {
        discoverResults = data.results ?? [];
      })
      .catch(() => {
        discoverResults = [];
      });
    return () => controller.abort();
  });

  function openRequestDialog(item: SearchResult) {
    requestingItem = item;
    requestingScope = 'all';
    chosenSeasons = [];
    requestDialog?.showModal();
  }

  async function submitRequest(event: SubmitEvent) {
    event.preventDefault();
    if (!requestingItem || requestingBusy) return;
    const target = requestingItem;
    requestingBusy = true;
    try {
      const payload = requestPayload(target, {
        seasons: requestingScope === 'all' ? [] : chosenSeasons,
        monitorFuture: false
      });
      const created = await applicationRequest<MediaRequest>('/api/v1/requests', {
        method: 'POST',
        body: JSON.stringify(payload)
      });
      requestDialog?.close();
      requestSuccessMessage = $t('rq_request_created', {
        title: created.title ?? target.title,
        status: statusLabels[created.status] ?? created.status
      });
      discoverResults = discoverResults.map(r => r === target ? {
        ...r,
        request: { status: created.status, mine: true, id: created.id }
      } : r);
    } catch {
      // Ignored or handled
    } finally {
      requestingBusy = false;
    }
  }
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
          placeholder={$t('admin_search')}
          maxlength="200"
          autocomplete="off"
        />
        {#if search}
          <button
            type="button"
            class="clear-btn"
            aria-label={$t('clear_search')}
            onclick={clearSearch}
          >
            ×
          </button>
        {/if}
      </div>

      {#if section === 'search'}
        <div class="control-select">
          <select
            aria-label={$t('filter_by_media_type')}
            bind:value={selectedKindFilter}
            onchange={() => handleFilterSubmit()}
          >
            <option value="all">{$t('all')}</option>
            <option value="film">{$t('films')}</option>
            <option value="series">{$t('series')}</option>
          </select>
        </div>
      {/if}

      <div class="control-select">
        <select
          aria-label={$t('sort_by')}
          bind:value={sort}
          onchange={() => handleFilterSubmit()}
        >
          <option value="title-asc">{$t('title_a_to_z')}</option>
          <option value="title-desc">{$t('title_z_to_a')}</option>
          <option value="year-desc">{$t('year_newest')}</option>
          <option value="year-asc">{$t('year_oldest')}</option>
          <option value="added-desc">{$t('recently_added')}</option>
          <option value="added-asc">{$t('added_long_ago')}</option>
        </select>
      </div>

      <button
        type="button"
        class="filter-toggle-btn"
        class:active={showFilters || activeFilterCount > 0}
        onclick={() => (showFilters = !showFilters)}
        aria-expanded={showFilters}
        aria-label={$t('stats_advanced_filters')}
      >
        <Icon name="filter" size={15} />
        <span class="filter-btn-text">{$t('filters')}</span>
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
          <span class="drawer-label">{$t('genre')}</span>
          <input
            bind:value={filterGenre}
            placeholder={$t('animation_drama')}
            maxlength="100"
          />
        </label>
        <label class="drawer-field">
          <span class="drawer-label">{$t('stats_w_year')}</span>
          <input
            type="number"
            bind:value={filterYear}
            placeholder={$t('e_g_2024')}
            min="1800"
            max="2200"
          />
        </label>
        <label class="drawer-field">
          <span class="drawer-label">{$t('min_quality')}</span>
          <select bind:value={filterQuality}>
            <option value="">{$t('all_extensions_filter')}</option>
            <option value="720">720p</option>
            <option value="1080">1080p</option>
            <option value="2160">{$t('n_2160p_4k')}</option>
          </select>
        </label>
        <label class="drawer-field">
          <span class="drawer-label">{$t('codec')}</span>
          <select bind:value={filterCodec}>
            <option value="">{$t('stats_all_m')}</option>
            <option value="av1">AV1</option>
            <option value="hevc">HEVC</option>
            <option value="h264">H.264</option>
            <option value="opus">{$t('opus')}</option>
          </select>
        </label>
        <label class="drawer-field">
          <span class="drawer-label">{$t('catalog_language')}</span>
          <input
            bind:value={filterLanguage}
            placeholder={$t('fra_eng')}
            maxlength="35"
          />
        </label>
      </div>
      <div class="drawer-actions">
        <button type="button" class="apply-filter-btn" onclick={() => handleFilterSubmit()}>
          {$t('catalog_apply')}
        </button>
        {#if activeFilterCount > 0}
          <button type="button" class="reset-filter-btn" onclick={resetFilters}>
            {$t('reset')}
          </button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Quick Genre Filters Bar if available -->
  {#if allAvailableGenres.length > 0}
    <div class="genre-pills-bar" aria-label={$t('filter_by_genre')}>
      <button
        type="button"
        class="genre-chip"
        class:active={activeGenre === null}
        onclick={() => (activeGenre = null)}
      >
        {$t('all_genres')}
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
  {#if section !== 'series' && (selectedKindFilter === 'all' || selectedKindFilter === 'film') && (section !== 'search' || filteredFilms.length > 0)}
    <section class="shelf" aria-labelledby={section === 'search' ? 'shelf-films-title' : undefined}>
      {#if section === 'search'}
        <div class="section-heading">
          <h2 id="shelf-films-title">{$t('films')}</h2>
          <span class="count-badge">{$t('film_count', { count: filmCount })}</span>
        </div>
      {/if}

      <div class="media-grid">
        {#each filteredFilms as work, index (work.id)}
          <Poster {work} href={detailUrl(work)} eager={index < 8} />
        {:else}
          <div class="empty-media-box">
            <Icon name="film" size={32} />
            <p>
              {query ? $t('no_film_matches_your_search') : $t('no_films_in_this_library')}
            </p>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <!-- Series Shelf / Grid -->
  {#if section !== 'films' && (selectedKindFilter === 'all' || selectedKindFilter === 'series') && (section !== 'search' || filteredSeries.length > 0)}
    <section class="shelf" aria-labelledby={section === 'search' ? 'shelf-series-title' : undefined}>
      {#if section === 'search'}
        <div class="section-heading">
          <h2 id="shelf-series-title">{$t('series')}</h2>
          <span class="count-badge">{$t('series_count', { count: seriesCount })}</span>
        </div>
      {/if}

      <div class="media-grid">
        {#each filteredSeries as work, index (work.id)}
          <Poster {work} href={detailUrl(work)} eager={filteredFilms.length === 0 && index < 8} />
        {:else}
          <div class="empty-media-box">
            <Icon name="tv" size={32} />
            <p>
              {query ? $t('no_series_matches_your_search') : $t('no_series_in_this_library')}
            </p>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if section === 'search' && filteredFilms.length === 0 && filteredSeries.length === 0 && discoverMissing.length === 0}
    <div class="empty-media-box">
      <Icon name="search" size={32} />
      <p>{$t('no_results_search')}</p>
    </div>
  {/if}

  <!-- Pagination -->
  {#if offset > 0 || films.length || series.length}
    <nav class="pagination" aria-label={$t('catalogue_pages')}>
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
          {$t('catalog_previous_page')}
        </a>
      {/if}
      <span class="page-indicator">
        {$t('admin_page', { value: Math.floor(offset / 24) + 1 })}{#if paginationTotal !== null} {$t('page_of_total', { total: Math.max(1, Math.ceil(paginationTotal / 24)) })}{/if}
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
          {$t('catalog_next_page')}
        </a>
      {/if}
    </nav>
  {/if}

  {#if requestSuccessMessage}
    <p role="status" class="feedback success">{requestSuccessMessage}</p>
  {/if}

  <!-- On-Demand Discovery Shelf -->
  {#if section === 'search' && $features.requests && query.trim().length >= 2 && discoverMissing.length > 0}
    <section class="shelf discover-shelf" aria-labelledby="discover-shelf-title">
      <div class="section-heading">
        <div class="discover-title-wrap">
          <Icon name="download" size={20} />
          <h2 id="discover-shelf-title">{$t('rq_discover_section_title')}</h2>
        </div>
        <span class="count-badge">{$t('rq_discover_count', { count: discoverMissing.length })}</span>
      </div>
      <p class="discover-hint">{$t('rq_discover_help')}</p>

      <div class="discover-grid">
        {#each discoverMissing as item (`${item.externalProvider}:${item.externalId}`)}
          <div class="discover-card">
            <div class="discover-poster-wrap">
              {#if item.poster && !failedPosters.has(`${item.externalProvider}:${item.externalId}`)}
                <img
                  src={posterUrl(item.poster)}
                  alt=""
                  loading="lazy"
                  class="discover-poster"
                  onerror={() => {
                    failedPosters.add(`${item.externalProvider}:${item.externalId}`);
                  }}
                />
              {:else}
                <div class="discover-poster placeholder">
                  <span>{item.title.trim().charAt(0).toUpperCase() || '?'}</span>
                </div>
              {/if}
              {#if item.request}
                <span class="discover-badge tone-{statusTone(item.request.status)}">
                  {item.request.mine ? $t('rq_requested_by_you') : $t('rq_already_requested')}
                </span>
              {/if}
            </div>

            <div class="discover-info">
              <h3>
                {item.title}
                {#if item.year}<span class="year">({item.year})</span>{/if}
              </h3>
              <p class="discover-meta">
                {item.kind === 'film' ? $t('kind_movie') : $t('kind_series')}
                {#if item.runtime} · {$t('rq_runtime', { minutes: item.runtime })}{/if}
                {#if item.genres?.length} · {item.genres.slice(0, 2).join(', ')}{/if}
              </p>
              {#if item.summary}
                <p class="discover-summary">{item.summary}</p>
              {/if}

              <div class="discover-action">
                {#if item.request}
                  <button type="button" class="requested-btn" disabled>
                    <Icon name="check" size={14} />
                    <span>{statusLabels[item.request.status] ?? item.request.status}</span>
                  </button>
                {:else if item.canRequest}
                  <button type="button" class="request-btn" onclick={() => openRequestDialog(item)}>
                    <Icon name="download" size={14} />
                    <span>{$t('rq_request_now')}</span>
                  </button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    </section>
  {/if}
</div>

<dialog bind:this={requestDialog} class="discover-dialog" onclose={() => requestingItem = null}>
  {#if requestingItem}
    <form class="dialog-stack" onsubmit={submitRequest}>
      <header class="dialog-header">
        <h2>{$t('confirm_request_for', { title: requestingItem.title })}</h2>
        <p class="muted small">
          {requestingItem.kind === 'film' ? $t('kind_movie') : $t('kind_series')}
          {#if requestingItem.year} · {requestingItem.year}{/if}
        </p>
      </header>

      <p class="dialog-mode-hint" class:auto={requestingItem.autoApprove}>
        {requestingItem.autoApprove ? $t('rq_confirm_dialog_auto') : $t('rq_confirm_dialog_manual')}
      </p>

      {#if requestingItem.kind === 'series' && requestingItem.seasons?.length}
        <fieldset class="dialog-fieldset">
          <legend>{$t('rq_seasons_legend')}</legend>
          <label class="dialog-radio">
            <input type="radio" name="scope" value="all" bind:group={requestingScope} />
            <span>{$t('rq_all_seasons')}</span>
          </label>
          <label class="dialog-radio">
            <input type="radio" name="scope" value="pick" bind:group={requestingScope} />
            <span>{$t('rq_pick_seasons')}</span>
          </label>
          {#if requestingScope === 'pick'}
            <div class="season-chips">
              {#each requestingItem.seasons as season (season.number)}
                <label class="season-chip">
                  <input type="checkbox" value={season.number} bind:group={chosenSeasons} />
                  <span>{season.number === 0 ? $t('rq_specials') : $t('rq_season_n', { number: season.number })}</span>
                  {#if season.episodes}<small>({$t('rq_episode_count', { count: season.episodes })})</small>{/if}
                </label>
              {/each}
            </div>
          {/if}
        </fieldset>
      {/if}

      <footer class="dialog-footer">
        <button type="button" class="cancel-btn" onclick={() => requestDialog?.close()}>{$t('cancel')}</button>
        <button type="submit" class="submit-btn" disabled={requestingBusy}>
          <Icon name="download" size={15} />
          <span>{$t('confirm_request')}</span>
        </button>
      </footer>
    </form>
  {/if}
</dialog>

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
  .search-box input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); box-shadow: 0 0 0 3px var(--ac-soft); }
  .search-box input::-webkit-search-cancel-button { display: none; }
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
  h2 { margin: 0; font-size: calc(24px * var(--ui-scale, 1)); font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
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

  .discover-shelf { margin-top: 24px; padding-top: 24px; border-top: 1px solid var(--line); }
  .discover-title-wrap { display: flex; align-items: center; gap: 10px; color: var(--ac); }
  .discover-hint { margin: 0; font-size: 14px; color: var(--mu); }
  .discover-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 360px), 1fr)); gap: 16px; margin-top: 8px; }
  .discover-card { display: flex; gap: 14px; padding: 14px; border: 1px solid var(--line); border-radius: var(--r-md, 12px); background: var(--bg-2); align-items: flex-start; }
  .discover-poster-wrap { flex: none; width: 84px; aspect-ratio: 2/3; border-radius: var(--r-xs, 6px); overflow: hidden; position: relative; background: var(--pn-2); }
  .discover-poster { width: 100%; height: 100%; object-fit: cover; display: block; }
  .discover-poster.placeholder { display: grid; place-items: center; color: var(--mu); font-size: 24px; font-weight: 700; }
  .discover-badge { position: absolute; top: 4px; left: 4px; right: 4px; font-size: 10px; font-weight: 700; padding: 3px 4px; border-radius: 4px; text-align: center; text-transform: uppercase; letter-spacing: 0.02em; }
  .discover-badge.tone-ok { color: var(--ok); background: rgba(0, 0, 0, 0.75); border: 1px solid var(--ok); }
  .discover-badge.tone-info { color: var(--info); background: rgba(0, 0, 0, 0.75); border: 1px solid var(--info); }
  .discover-badge.tone-warn { color: var(--warn); background: rgba(0, 0, 0, 0.75); border: 1px solid var(--warn); }
  .discover-info { display: flex; flex-direction: column; gap: 6px; flex: 1; min-width: 0; }
  .discover-info h3 { margin: 0; font-size: 16px; font-weight: 700; color: var(--tx); display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; }
  .discover-info .year { font-size: 14px; color: var(--mu); font-weight: 500; }
  .discover-meta { margin: 0; font-size: 13px; color: var(--mu); }
  .discover-summary { margin: 0; font-size: 12px; color: var(--mu); display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; line-height: 1.4; }
  .discover-action { margin-top: auto; padding-top: 6px; }
  .request-btn { height: 36px; padding: 0 14px; border: 0; border-radius: var(--r-pill); background: var(--ac); color: var(--on-ac); font-size: 13px; font-weight: 700; display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
  .request-btn:hover { opacity: 0.9; }
  .requested-btn { height: 36px; padding: 0 14px; border: 1px solid var(--line); border-radius: var(--r-pill); background: var(--pn); color: var(--mu); font-size: 13px; font-weight: 600; display: inline-flex; align-items: center; gap: 6px; cursor: default; }
  .feedback.success { padding: 12px 16px; border-radius: 10px; background: var(--bg-3); color: var(--ok); margin: 0; }

  .discover-dialog { border: 1px solid var(--line); border-radius: var(--r-lg, 16px); background: var(--bg-2); color: var(--tx); padding: 24px; max-width: min(90vw, 480px); width: 100%; box-shadow: 0 16px 40px rgba(0, 0, 0, 0.4); }
  .discover-dialog::backdrop { background: rgba(0, 0, 0, 0.7); backdrop-filter: blur(4px); }
  .dialog-stack { display: flex; flex-direction: column; gap: 16px; }
  .dialog-header h2 { margin: 0; font-size: 20px; font-weight: 800; color: var(--tx); }
  .dialog-mode-hint { margin: 0; font-size: 14px; color: var(--mu); padding: 10px 14px; border-radius: var(--r-sm); background: var(--pn); }
  .dialog-mode-hint.auto { color: var(--ok); background: var(--ok-soft, rgba(34, 197, 94, 0.1)); }
  .dialog-fieldset { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 14px; margin: 0; display: flex; flex-direction: column; gap: 10px; }
  .dialog-fieldset legend { font-size: 13px; font-weight: 700; color: var(--tx); padding: 0 6px; }
  .dialog-radio { display: flex; align-items: center; gap: 8px; font-size: 14px; color: var(--tx); cursor: pointer; }
  .season-chips { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px; }
  .season-chip { display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; border-radius: var(--r-pill); background: var(--pn); font-size: 13px; cursor: pointer; }
  .dialog-footer { display: flex; justify-content: flex-end; gap: 10px; margin-top: 8px; }
  .cancel-btn { height: 40px; padding: 0 16px; border: 1px solid var(--line); border-radius: var(--r-sm); background: none; color: var(--tx); font-size: 14px; font-weight: 600; cursor: pointer; }
  .cancel-btn:hover { background: var(--pn); }
  .submit-btn { height: 40px; padding: 0 18px; border: 0; border-radius: var(--r-sm); background: var(--ac); color: var(--on-ac); font-size: 14px; font-weight: 700; display: inline-flex; align-items: center; gap: 8px; cursor: pointer; }
  .submit-btn:hover { opacity: 0.9; }
</style>

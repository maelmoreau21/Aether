<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { request, type Work } from './api';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import { buildDiscoverPlan, shuffleWithSeed, type DiscoverSpec } from './discover';

  // Fil sans fin de l'accueil : des rangées thématiques chargées au fil du défilement,
  // puis tout le catalogue en grille paginée. Aucune requête tant que l'utilisateur
  // n'approche pas du bas de la page.
  let { films = [], series = [], exclude = [], library = '', kind, gridTitle = 'Tout votre catalogue', detailUrl }: {
    films: Work[];
    series: Work[];
    /** Œuvres déjà visibles plus haut : elles ne sont pas reproposées dans les rangées. */
    exclude?: string[];
    library?: string;
    /** Limite le fil à un seul type (fiche d'un film ou d'une série). */
    kind?: 'film' | 'series';
    gridTitle?: string;
    detailUrl: (work: Work) => string;
  } = $props();

  const SHELF_SIZE = 24;
  const GRID_PAGE = 30;
  const MIN_SHELF = 4;

  type Shelf = { key: string; title: string; items: Work[] };
  let shelves = $state<Shelf[]>([]);
  let gridItems = $state<Work[]>([]);
  let gridOffset = 0;
  let gridDone = $state(false);
  let loading = $state(false);
  let failed = $state(false);
  let plan: DiscoverSpec[] = [];
  let planIndex = 0;
  // Registre interne non affiché : pas besoin de réactivité.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  let seen = new Set<string>();
  let sentinel = $state<HTMLDivElement>();
  let controller: AbortController | undefined;
  let generation = 0;

  const seed = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : String(Date.now());

  // Recommence le fil seulement si la bibliothèque ou le contenu de départ change vraiment.
  const resetKey = $derived(`${kind ?? ''}|${exclude.length}|${library}|${films.length}:${films[0]?.id ?? ''}|${series.length}:${series[0]?.id ?? ''}`);
  $effect(() => {
    void resetKey;
    untrack(() => {
      controller?.abort();
      generation++;
      shelves = []; gridItems = []; gridOffset = 0; gridDone = false; failed = false; loading = false;
       
      seen = new Set(exclude);
      plan = buildDiscoverPlan([...films, ...series], seed).filter(spec => !kind || spec.kind === kind);
      planIndex = 0;
      if (sentinel && isNearViewport(sentinel)) queueMicrotask(() => void loadMore());
    });
  });

  function catalogUrl(params: Record<string, string>) {
    return `/api/v1/catalog?${new URLSearchParams({ ...(library ? { library } : {}), q: '', count: 'false', ...params })}`;
  }

  async function loadNextShelf(signal: AbortSignal): Promise<boolean> {
    // Quelques tentatives : une rangée trop courte après déduplication est sautée.
    for (let attempts = 0; attempts < 4 && planIndex < plan.length; attempts++) {
      const spec = plan[planIndex++];
      const result = await request<{ items: Work[] }>(catalogUrl({
        kind: spec.kind,
        sort: spec.sort,
        offset: '0',
        limit: String(SHELF_SIZE),
        ...(spec.genre ? { genre: spec.genre } : {})
      }), signal);
      if (signal.aborted) return true;
      const fresh = shuffleWithSeed((result.items ?? []).filter(item => item.id && !seen.has(item.id)), `${seed}:${spec.key}`);
      if (fresh.length < MIN_SHELF) continue;
      for (const item of fresh) seen.add(item.id);
      shelves = [...shelves, { key: spec.key, title: spec.title, items: fresh }];
      return true;
    }
    return planIndex < plan.length;
  }

  async function loadGridPage(signal: AbortSignal) {
    const result = await request<{ items: Work[] }>(catalogUrl({ ...(kind ? { kind } : {}), sort: 'added-desc', offset: String(gridOffset), limit: String(GRID_PAGE) }), signal);
    if (signal.aborted) return;
    const items = result.items ?? [];
    gridOffset += items.length;
    if (items.length < GRID_PAGE) gridDone = true;
     
    const known = new Set(gridItems.map(item => item.id));
    gridItems = [...gridItems, ...items.filter(item => item.id && !known.has(item.id) && !(kind && seen.has(item.id)))];
  }

  async function loadMore() {
    if (loading || gridDone || failed) return;
    const current = generation;
    controller = new AbortController();
    const signal = controller.signal;
    loading = true;
    try {
      const progressed = planIndex < plan.length && await loadNextShelf(signal);
      if (!progressed && !signal.aborted) await loadGridPage(signal);
    } catch {
      if (!signal.aborted && current === generation) failed = true;
    } finally {
      if (current === generation) loading = false;
    }
    // Si le bas de page est encore visible (grand écran), on continue.
    if (current === generation && !failed && sentinel && isNearViewport(sentinel)) queueMicrotask(() => void loadMore());
  }

  function isNearViewport(node: HTMLElement) {
    return node.getBoundingClientRect().top < window.innerHeight + 900;
  }

  onMount(() => {
    if (!sentinel) return;
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) void loadMore();
    }, { rootMargin: '900px 0px' });
    observer.observe(sentinel);
    return () => { observer.disconnect(); controller?.abort(); };
  });
</script>

<div class="discover-feed">
  {#each shelves as shelf (shelf.key)}
    <div class="feed-item">
      <MediaShelf title={shelf.title}>
        {#each shelf.items as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
      </MediaShelf>
    </div>
  {/each}

  {#if gridItems.length}
    <div class="feed-item">
      <MediaShelf title={gridTitle} layout="grid">
        {#each gridItems as work (work.id)}<Poster {work} href={detailUrl(work)} />{/each}
      </MediaShelf>
    </div>
  {/if}

  <div class="sentinel" bind:this={sentinel} aria-hidden="true"></div>
  {#if loading}
    <div class="loading-row" role="status" aria-label="Chargement d’autres suggestions">
      {#each Array.from({ length: 6 }, (_, index) => index) as index (index)}<span class="ghost"></span>{/each}
    </div>
  {:else if failed}
    <p class="feed-note" role="status">Impossible de charger d’autres suggestions. <button type="button" onclick={() => { failed = false; void loadMore(); }}>Réessayer</button></p>
  {:else if gridDone && (shelves.length || gridItems.length)}
    <p class="feed-note">Vous avez fait le tour de votre médiathèque.</p>
  {/if}
</div>

<style>
  .discover-feed { display: flex; flex-direction: column; gap: clamp(28px, 4vw, 48px); }
  .feed-item { animation: feed-in var(--t-slow) var(--ease); }
  @keyframes feed-in { from { opacity: 0; transform: translateY(12px); } to { opacity: 1; transform: none; } }
  .sentinel { height: 1px; margin-top: -1px; }
  .loading-row { display: flex; gap: var(--card-gap, 16px); overflow: hidden; }
  .ghost { flex: none; width: var(--poster-width, clamp(150px, 12.5vw, 190px)); aspect-ratio: 2 / 3; border-radius: var(--r-md); background: var(--pn-2); animation: pulse 1.4s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.5; } }
  .feed-note { margin: 0; padding: 24px 0 8px; text-align: center; color: var(--mu); font-size: 14px; }
  .feed-note button { margin-left: 8px; min-height: 36px; padding: 0 14px; border: 0; border-radius: var(--r-pill); background: var(--pn-2); color: var(--tx); font-weight: 700; }
  @media (prefers-reduced-motion: reduce) { .feed-item, .ghost { animation: none; } }
</style>

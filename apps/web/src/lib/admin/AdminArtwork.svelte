<script lang="ts">
  import ArtworkCollection from './ArtworkCollection.svelte';
  import { invalidateCatalog } from '../catalog/page-cache';
  import ImageManager from '../catalog/ImageManager.svelte';
  import { onDestroy } from 'svelte';
  import type { Library } from './types';

  let { libraries }: { libraries: Library[] } = $props();
  type Item = { workId: string; title?: string };
  type WorkPage = { items: { workId: string; title: string; posterCount: number }[]; nextCursor: string | null };
  let libraryId = $state('');
  let selected = $state<Item | null>(null);
  let page = $state<WorkPage | null>(null), pageLoading = $state(false), pageError = $state('');
  let pageCursor = $state<string | null>(null), previousCursors = $state<(string | null)[]>([]), pageGeneration = 0;
  let search = $state(''), posterFilter = $state('all'), appliedSearch = $state(''), appliedFilter = $state('all'), filterError = $state('');
  onDestroy(() => { pageGeneration++; });
  const videoLibraries = $derived(libraries.filter(library => library.kind !== 'music' && library.enabled));
  async function loadPage(after: string | null = null) {
    const generation = ++pageGeneration;
    pageLoading = true; pageError = '';
    try {
      const params = new URLSearchParams({ libraryId, limit: '25' });
      if (after) params.set('after', after);
      if (appliedSearch) params.set('q', appliedSearch);
      if (appliedFilter !== 'all') params.set('posters', appliedFilter);
      const response = await fetch(`/api/v1/admin/artwork/works?${params}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(15000) });
      if (!response.ok) throw new Error(`Liste des médias indisponible (HTTP ${response.status}).`);
      const result = await response.json() as WorkPage;
      if (!Array.isArray(result.items)) throw new Error('Liste des médias invalide.');
      if (generation === pageGeneration) { page = result; pageCursor = after; return true; }
    } catch (cause) {
      if (generation === pageGeneration) pageError = cause instanceof Error ? cause.message : 'Liste des médias indisponible.';
    } finally { if (generation === pageGeneration) pageLoading = false; }
    return false;
  }
  async function previousPage() { const previous = previousCursors.at(-1) ?? null; if (await loadPage(previous)) previousCursors = previousCursors.slice(0, -1); }
  async function nextPage() { if (!page?.nextCursor) return; const current = pageCursor; if (await loadPage(page.nextCursor)) previousCursors = [...previousCursors, current]; }
  function applyFilters() {
    const query = search.trim();
    if (new TextEncoder().encode(query).length > 100) { filterError = 'Le titre recherché dépasse 100 octets. Réduisez sa longueur.'; return; }
    filterError = ''; appliedSearch = query; appliedFilter = posterFilter; page = null; previousCursors = []; pageCursor = null; void loadPage();
  }
  function changeLibrary() {
 page = null; pageCursor = null; previousCursors = []; pageError = ''; pageLoading = false; pageGeneration++;
    search = appliedSearch; posterFilter = appliedFilter; filterError = '';
    if (libraryId) void loadPage();
  }
</script>

<section class="artwork-admin" aria-labelledby="artwork-admin-title">
  <header><h2 id="artwork-admin-title">Images des médiathèques</h2><p>Enrichissez les films et séries avec les images des fournisseurs activés. Les fichiers de vos médias restent inchangés.</p></header>
  <div class="panel">
    <label>Bibliothèque<select bind:value={libraryId} onchange={changeLibrary}><option value="">Choisir une bibliothèque</option>{#each videoLibraries as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
    {#if libraryId}<form class="filters" onsubmit={(event) => { event.preventDefault(); applyFilters(); }}><label>Rechercher un titre<input type="search" bind:value={search} maxlength="100" /></label><label>Variantes enregistrées<select bind:value={posterFilter}><option value="all">Toutes les œuvres</option><option value="with">Avec variante</option><option value="without">Sans variante</option></select></label><button type="submit" disabled={pageLoading}>Rechercher</button></form>{#if filterError}<p class="error" role="alert">{filterError}</p>{/if}<p>Pour certains titres, respectez la casse et les accents.</p>{/if}
    {#if pageLoading}<p role="status">Chargement des médias…</p>{/if}
    {#if pageError}<p class="error" role="alert">{pageError}</p><button type="button" onclick={() => void loadPage(pageCursor)}>Réessayer</button>{/if}
    {#if page && !pageError}<h3>Affiches disponibles</h3><p>Ouvrez un média pour consulter, ajouter ou retirer ses images. Le compteur indique les variantes enregistrées dans Aether ; les affiches locales ne sont pas comptées.</p><ul class="works">{#each page.items as item (item.workId)}<li><span><strong>{item.title}</strong><small>{item.posterCount} variante{item.posterCount > 1 ? 's' : ''} enregistrée{item.posterCount > 1 ? 's' : ''}</small></span><button type="button" onclick={() => selected = item}>Gérer les images</button></li>{:else}<li>Aucun média dans cette bibliothèque.</li>{/each}</ul><div class="actions"><button type="button" disabled={pageLoading || !previousCursors.length} onclick={() => void previousPage()}>Page précédente</button><button type="button" disabled={pageLoading || !page.nextCursor} onclick={() => void nextPage()}>Page suivante</button></div>{/if}
  </div>
  <div class="panel"><ArtworkCollection {libraryId} /></div>
  <div class="panel"><h3>Affichage et gestion des affiches</h3><p>Chaque utilisateur choisit la rotation dans ses préférences d’affichage. Les affiches sont sélectionnées parmi les variantes disponibles ; les images explicitement épinglées restent prioritaires.</p><p>Pour gérer les images d’un média à tout moment, ouvrez sa fiche dans le catalogue puis choisissez « Modifier les images » dans son menu.</p></div>
</section>
{#if selected}<ImageManager title={selected.title || selected.workId} workId={selected.workId} onclose={() => selected = null} onchanged={async () => { invalidateCatalog(libraryId); await loadPage(pageCursor); }} />{/if}
<style>
  .artwork-admin { display: grid; gap: 20px; min-width: 0; max-width: 1040px; font-family: var(--font); color: var(--tx); }
  h2, h3 { margin: 0; }
  h2 { font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.15; }
  h3 { margin-top: 4px; font-size: 17px; font-weight: 700; }
  header p { margin: 6px 0 0; }
  p { font-size: 14px; line-height: 1.6; color: var(--mu); }
  .panel { min-width: 0; padding: 24px; border-radius: var(--r-md); background: var(--color-surface); }
  .panel > :first-child { margin-top: 0; }
  .panel > :last-child { margin-bottom: 0; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input:not([type='checkbox']), select { box-sizing: border-box; width: 100%; min-width: 0; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-sm); background-color: var(--color-surface); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  input:not([type='checkbox']):hover, select:hover:not(:disabled) { background-color: var(--color-surface-hover); border-color: var(--color-line); }
  input:not([type='checkbox']):focus, select:focus, select:focus-visible { outline: none; border-color: var(--ac); background-color: var(--color-surface); }
  .actions { display: flex; flex-wrap: wrap; gap: 10px; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 18px; border: 1px solid var(--color-line); border-radius: var(--r-md); background: transparent; color: var(--tx); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--color-surface-hover); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .error { color: var(--err); }
  li { margin: 0; font-size: 14px; color: var(--mu); overflow-wrap: anywhere; }
  li strong { color: var(--tx); font-weight: 700; }
  .works { margin: 16px 0 20px; padding: 0; list-style: none; border-bottom: 1px solid var(--color-line-subtle); }
  .works li { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 64px; padding: 10px 0; border-top: 1px solid var(--color-line-subtle); transition: background-color var(--t-fast) ease; }
  .works li strong { font-size: 15px; }
  .works small { display: block; margin-top: 2px; font-size: 13px; color: var(--mu); font-variant-numeric: tabular-nums; }
  .works li button { flex: none; }
  .filters { display: grid; grid-template-columns: 2fr 1fr auto; align-items: end; gap: 12px; margin: 20px 0 0; }
  .filters + p, .filters + .error { margin-top: 8px; font-size: 13px; }
  @media (max-width: 1023px) { .filters { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); } .filters button { grid-column: 1 / -1; justify-self: start; } }
  @media (max-width: 640px) {
    .panel { padding: 16px; border-radius: var(--r-md); }
    .filters { grid-template-columns: minmax(0, 1fr); }
    .filters button { justify-self: stretch; }
    .actions button { width: 100%; }
    .works li { flex-wrap: wrap; }
    .works li button { width: 100%; margin-left: 0; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { button, input, select, .works li { transition: none; } }
</style>

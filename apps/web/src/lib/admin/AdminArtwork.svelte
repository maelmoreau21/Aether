<script lang="ts">
  import { session } from '../auth/session';
  import { invalidateCatalog } from '../catalog/page-cache';
  import ImageManager from '../catalog/ImageManager.svelte';
  import { onDestroy } from 'svelte';
  import type { Library } from './types';

  let { libraries }: { libraries: Library[] } = $props();
  type CollectionResult = { downloaded?: number; duplicates?: number; skipped?: number; errors?: unknown[]; partial?: boolean };
  type Item = { workId: string; title?: string; result?: CollectionResult; error?: { status: number; code: string } };
  type Batch = { items: Item[]; nextCursor: string | null };
  type WorkPage = { items: { workId: string; title: string; posterCount: number }[]; nextCursor: string | null };
  let libraryId = $state(''), languages = $state('fr, en'), includeNeutral = $state(true), maxImages = $state(12);
  let busy = $state(false), stopRequested = $state(false), cursor = $state<string | null>(null), started = $state(false), finished = $state(false);
  let processed = $state(0), downloaded = $state(0), errors = $state(0), partial = $state(0), recent = $state<Item[]>([]), error = $state('');
  let problems = $state<Item[]>([]), omittedProblems = $state(0);
  let selected = $state<Item | null>(null);
  let page = $state<WorkPage | null>(null), pageLoading = $state(false), pageError = $state('');
  let pageCursor = $state<string | null>(null), previousCursors = $state<(string | null)[]>([]), pageGeneration = 0;
  let search = $state(''), posterFilter = $state('all'), appliedSearch = $state(''), appliedFilter = $state('all'), filterError = $state('');
  let destroyed = false;
  onDestroy(() => { destroyed = true; stopRequested = true; pageGeneration++; });
  const videoLibraries = $derived(libraries.filter(library => library.kind !== 'music' && library.enabled));
  function collectionError(error: { status: number; code: string }): string {
    const messages: Record<string, string> = {
      external_ids_required: 'Identifiants externes manquants',
      unknown_work: 'Média introuvable',
      work_denied: 'Accès au média refusé',
      too_many_providers: 'Trop de fournisseurs configurés',
      invalid_package: 'Paquet de fournisseur invalide',
      invalid_output: 'Réponse de fournisseur invalide'
    };
    return messages[error.code] ?? `Collecte impossible (HTTP ${error.status})`;
  }
  function reset() { cursor = null; started = false; finished = false; processed = 0; downloaded = 0; errors = 0; partial = 0; recent = []; problems = []; omittedProblems = 0; error = ''; }
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
    reset(); page = null; pageCursor = null; previousCursors = []; pageError = ''; pageLoading = false; pageGeneration++;
    search = appliedSearch; posterFilter = appliedFilter; filterError = '';
    if (libraryId) void loadPage();
  }
  async function run() {
    if (busy || !libraryId || finished) return;
    const selectedLanguages = [...new Set(languages.split(',').map(value => value.trim().toLowerCase()).filter(Boolean))];
    if (!selectedLanguages.length || selectedLanguages.length > 5 || selectedLanguages.some(value => !/^[a-z]{2,3}(?:-[a-z0-9]{2,8})*$/.test(value))) {
      error = 'Indiquez une à cinq langues valides, dans l’ordre de préférence (par exemple fr, en).'; return;
    }
    busy = true; stopRequested = false; error = '';
    const actor = $session;
    try {
      do {
        if (!actor || $session?.user.id !== actor.user.id || $session.csrfToken !== actor.csrfToken) throw new Error('La session a changé. Relancez la collecte après connexion.');
        const response = await fetch('/api/v1/admin/artwork/collect-batch', {
          method: 'POST', credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(70000),
          headers: { 'content-type': 'application/json', 'x-csrf-token': actor.csrfToken },
          body: JSON.stringify({ libraryId, ...(cursor ? { after: cursor } : {}), limit: 1, languages: selectedLanguages, includeNeutral, maxImages })
        });
        if (!response.ok) {
          const detail = await response.json().catch(() => null) as { error?: string } | null;
          throw new Error(detail?.error ? `Collecte interrompue : ${detail.error} (HTTP ${response.status}).` : `Collecte interrompue (HTTP ${response.status}).`);
        }
        const batch = await response.json() as Batch;
        if (!Array.isArray(batch.items) || (batch.nextCursor !== null && typeof batch.nextCursor !== 'string')) throw new Error('Réponse de collecte invalide.');
        if (batch.nextCursor && batch.nextCursor === cursor) throw new Error('La collecte ne progresse plus. Réessayez plus tard.');
        started = true; processed += batch.items.length;
        for (const item of batch.items) {
          downloaded += item.result?.downloaded ?? 0;
          errors += (item.error ? 1 : 0) + (item.result?.errors?.length ?? 0);
          if (item.result?.partial) partial += 1;
          if (item.error || item.result?.partial || item.result?.errors?.length) {
            if (problems.length < 100) problems = [...problems, item];
            else omittedProblems += 1;
          }
        }
        recent = [...batch.items, ...recent].slice(0, 10);
        cursor = batch.nextCursor; finished = cursor === null;
        invalidateCatalog(libraryId);
      } while (!finished && !stopRequested);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'La collecte a échoué.';
    } finally { busy = false; if (!destroyed && started) void loadPage(pageCursor); }
  }
</script>

<section class="artwork-admin" aria-labelledby="artwork-admin-title">
  <header><h2 id="artwork-admin-title">Images des médiathèques</h2><p>Enrichissez les films et séries avec les images des fournisseurs activés. Les fichiers de vos médias restent inchangés.</p></header>
  <div class="panel">
    <label>Bibliothèque<select bind:value={libraryId} disabled={busy || (started && !finished)} onchange={changeLibrary}><option value="">Choisir une bibliothèque</option>{#each videoLibraries as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
    {#if libraryId}<form class="filters" onsubmit={(event) => { event.preventDefault(); applyFilters(); }}><label>Rechercher un titre<input type="search" bind:value={search} maxlength="100" /></label><label>Variantes enregistrées<select bind:value={posterFilter}><option value="all">Toutes les œuvres</option><option value="with">Avec variante</option><option value="without">Sans variante</option></select></label><button type="submit" disabled={pageLoading}>Rechercher</button></form>{#if filterError}<p class="error" role="alert">{filterError}</p>{/if}<p>Pour certains titres, respectez la casse et les accents.</p>{/if}
    {#if pageLoading}<p role="status">Chargement des médias…</p>{/if}
    {#if pageError}<p class="error" role="alert">{pageError}</p><button type="button" onclick={() => void loadPage(pageCursor)}>Réessayer</button>{/if}
    {#if page && !pageError}<h3>Affiches disponibles</h3><p>Ouvrez un média pour consulter, ajouter ou retirer ses images. Le compteur indique les variantes enregistrées dans Aether ; les affiches locales ne sont pas comptées.</p><ul class="works">{#each page.items as item (item.workId)}<li><span><strong>{item.title}</strong><small>{item.posterCount} variante{item.posterCount > 1 ? 's' : ''} enregistrée{item.posterCount > 1 ? 's' : ''}</small></span><button type="button" onclick={() => selected = item}>Gérer les images</button></li>{:else}<li>Aucun média dans cette bibliothèque.</li>{/each}</ul><div class="actions"><button type="button" disabled={pageLoading || !previousCursors.length} onclick={() => void previousPage()}>Page précédente</button><button type="button" disabled={pageLoading || !page.nextCursor} onclick={() => void nextPage()}>Page suivante</button></div>{/if}
  </div>
  <div class="panel">
    <h3>Collecter les images</h3>
    <p>La collecte parcourt les œuvres une par une à partir de leurs identifiants externes. Les doublons sont écartés et les images ajoutées manuellement restent prioritaires. Les résultats dépendent des fournisseurs configurés pour la bibliothèque.</p>
    <fieldset disabled={busy || (started && !finished)}>
      <label>Langues par ordre de préférence<input bind:value={languages} placeholder="fr, en" maxlength="100" /></label>
      <label>Maximum d’images par œuvre<input type="number" min="1" max="12" required bind:value={maxImages} /></label>
      <label class="check"><input type="checkbox" bind:checked={includeNeutral} /> Inclure les images sans langue indiquée</label>
    </fieldset>
    <div class="actions">
      {#if !busy && !finished}<button class="primary" type="button" disabled={!libraryId || maxImages < 1 || maxImages > 12} onclick={() => void run()}>{started ? 'Reprendre la collecte' : 'Lancer la collecte'}</button>{/if}
      {#if busy}<button type="button" disabled={stopRequested} onclick={() => stopRequested = true}>{stopRequested ? 'Arrêt demandé…' : 'Arrêter après cette œuvre'}</button>{/if}
      {#if started && !busy}<button type="button" onclick={reset}>Nouvelle collecte</button>{/if}
    </div>
    {#if busy}<p role="status">{stopRequested ? 'Fin de l’œuvre en cours…' : 'Collecte en cours. Vous pouvez demander un arrêt après l’œuvre en cours.'}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if started}<div class="results" aria-live="polite"><strong>{finished ? 'Parcours terminé' : busy ? 'Progression' : 'Collecte interrompue'}</strong><p>{processed} œuvre{processed > 1 ? 's' : ''} examinée{processed > 1 ? 's' : ''} · {downloaded} image{downloaded > 1 ? 's' : ''} ajoutée{downloaded > 1 ? 's' : ''} · {errors} erreur{errors > 1 ? 's' : ''} · {partial} collecte{partial > 1 ? 's' : ''} partielle{partial > 1 ? 's' : ''}</p><p>La progression est conservée dans cette page uniquement. Une collecte partielle peut être relancée depuis la fiche du média.</p></div>{/if}
    {#if recent.length}<details><summary>Derniers résultats</summary><ul>{#each recent as item (item.workId)}<li><strong>{item.title || item.workId}</strong> — {item.error ? collectionError(item.error) : `${item.result?.downloaded ?? 0} image(s) ajoutée(s)${item.result?.partial ? ' · résultat partiel' : ''}`} <button type="button" onclick={() => selected = item}>Gérer les images</button></li>{/each}</ul></details>{/if}
    {#if problems.length}<details><summary>Œuvres à vérifier ({problems.length}{omittedProblems ? ` et ${omittedProblems} autres` : ''})</summary><p>Les résultats partiels ou en erreur peuvent être relancés individuellement dans « Gérer les images ».</p><ul>{#each problems as item (item.workId)}<li><strong>{item.title || item.workId}</strong> — {item.error ? collectionError(item.error) : item.result?.partial ? 'résultat partiel' : 'erreur de fournisseur'} <button type="button" onclick={() => selected = item}>Gérer les images</button></li>{/each}</ul></details>{/if}
  </div>
  <div class="panel"><h3>Affichage et gestion des affiches</h3><p>Chaque utilisateur choisit la rotation dans ses préférences d’affichage. Les affiches sont sélectionnées parmi les variantes disponibles ; la collecte ne modifie pas les images choisies manuellement.</p><p>Pour gérer les images d’un média à tout moment, ouvrez sa fiche dans le catalogue puis choisissez « Modifier les images » dans son menu.</p></div>
</section>
{#if selected}<ImageManager title={selected.title || selected.workId} workId={selected.workId} onclose={() => selected = null} onchanged={async () => { invalidateCatalog(libraryId); await loadPage(pageCursor); }} />{/if}
<style>
  .artwork-admin { display:grid; gap:20px; max-width:1000px; }
  h2,h3 { margin:0; } h2 { font-size:24px; } h3 { font-size:18px; }
  p { color:var(--color-muted); font-size:14px; line-height:1.6; }
  .panel { border:1px solid var(--color-line); border-radius:var(--radius-md); background:var(--color-surface); padding:24px; }
  fieldset { border:0; padding:0; margin:20px 0; display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:16px; }
  label { display:grid; gap:8px; font-size:14px; } input:not([type=checkbox]),select { box-sizing:border-box; width:100%; min-width:0; min-height:44px; padding:10px; border:1px solid var(--color-line-strong); border-radius:var(--radius-sm); background:var(--color-surface); color:var(--color-text); font:inherit; }
  .check { display:flex; align-items:center; gap:10px; } .check input { width:20px; height:20px; }
  .actions { display:flex; flex-wrap:wrap; gap:10px; } button { min-height:44px; padding:10px 16px; font:inherit; border:1px solid var(--color-line-strong); border-radius:var(--radius-sm); background:var(--color-surface); color:var(--color-text); cursor:pointer; } button.primary { background:var(--color-play); color:var(--color-on-play, #111); border-color:transparent; } button:disabled { opacity:.5; cursor:not-allowed; }
  .error { color:var(--status-error); } .results { margin-top:20px; padding:16px; border:1px solid var(--color-line); border-radius:var(--radius-sm); } .results p { margin-bottom:0; }
  details { margin-top:18px; } summary { cursor:pointer; } li { margin:8px 0; overflow-wrap:anywhere; font-size:13px; }
  .works { padding:0; list-style:none; } .works li { display:flex; align-items:center; justify-content:space-between; gap:12px; border-top:1px solid var(--color-line); padding:12px 0; } .works small { display:block; color:var(--color-muted); margin-top:4px; }
  .filters { display:grid; grid-template-columns:2fr 1fr auto; gap:12px; align-items:end; margin:20px 0; }
  @media(max-width:650px) { .panel { padding:16px; } fieldset,.filters { grid-template-columns:minmax(0,1fr); } .actions button { width:100%; } }
</style>

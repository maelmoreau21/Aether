<script lang="ts">
  import { resolve } from '$app/paths';
  import type { Library, AdminNode, Scan, RefreshMode } from './types';
  import { scanLabels } from './types';
  import Icon from '../ui/Icon.svelte';
  let { libraries, nodes, scans, pending, availableRootsForNode, saving, editing, error,
    name = $bindable(), kind = $bindable(), nodeId = $bindable(), root = $bindable(), enabled = $bindable(),
    excludedPatterns = $bindable(''),
    onsave, onscan, onedit, onreset, onnodechange }: {
      libraries: Library[]; nodes: AdminNode[]; scans: Record<string, Scan>; pending: string[];
      availableRootsForNode: string[]; saving: boolean; editing: string | null; error: string;
      name: string; kind: 'films' | 'series' | 'music'; nodeId: string; root: string; enabled: boolean;
      excludedPatterns: string;
      onsave: (event: SubmitEvent) => Promise<boolean>; onscan: (library: Library, mode?: RefreshMode) => Promise<void>;
      onedit: (library: Library) => void; onreset: () => void; onnodechange: () => void;
  } = $props();
  let editor = $state<HTMLDialogElement>(), refresher = $state<HTMLDialogElement>();
  let target = $state<Library | null>(null), mode = $state<RefreshMode>('incremental'), refreshing = $state(false);
  const activeNodes = $derived(nodes.filter(n => n.status !== 'revoked'));
  function closeMenu(event: MouseEvent) { const details = (event.currentTarget as HTMLElement).closest('details'); details?.removeAttribute('open'); details?.querySelector('summary')?.focus(); }
  function manage(library?: Library) { if (library) onedit(library); else onreset(); editor?.showModal(); }
  function refreshLibrary(library: Library | null) { target = library; mode = 'incremental'; refresher?.showModal(); }
  async function submit(event: SubmitEvent) { if (await onsave(event)) editor?.close(); }
  async function refresh(event: SubmitEvent) {
    event.preventDefault(); if (refreshing) return; refreshing = true;
    try {
      const targets = target ? [target] : libraries.filter(l => l.enabled && !l.id.startsWith('fixture-'));
      for (const library of targets) { await onscan(library, mode); if (error) break; }
      if (!error) refresher?.close();
    } finally { refreshing = false; }
  }
</script>
<section id="libraries-section" aria-labelledby="libraries-heading">
  <h2 id="libraries-heading">Médiathèques</h2>
  <div class="toolbar"><button class="primary" onclick={() => manage()}><span aria-hidden="true">+</span> Ajouter une médiathèque</button><button onclick={() => refreshLibrary(null)} disabled={libraries.length === 0 || refreshing}><Icon name="refresh" size={18} /> Actualiser toutes les médiathèques</button></div>
  <div class="library-grid">
    {#each libraries as library (library.id)}<article class="library-card">
      <button class="library-cover" onclick={() => manage(library)} aria-label={`Gérer ${library.name}`}><Icon name={library.kind === 'films' ? 'film' : 'tv'} size={48} /><span>{library.name}</span></button>
      <div class="card-bottom"><div class="card-text"><h3>{library.name}</h3><p>{library.kind === 'music' ? 'Musique' : library.kind === 'films' ? 'Films' : 'Séries'}{library.enabled ? '' : ' · Désactivée'}</p></div>
        <details class="library-menu"><summary aria-label={`Actions ${library.name}`}>⋮</summary><div class="menu-actions">
          <button onclick={(event) => { closeMenu(event); manage(library); }}>Gérer la médiathèque</button>
          <button onclick={(event) => { closeMenu(event); manage(library); }}>Renommer</button>
          <button disabled={!library.enabled || pending.includes(library.id) || ['queued','running'].includes(scans[library.id]?.status)} onclick={(event) => { closeMenu(event); refreshLibrary(library); }}>Actualiser la médiathèque</button>
          <a href={resolve(`admin/libraries/#tasks-section?task=scan-${encodeURIComponent(library.id)}`)}>Planifier l’actualisation</a>
        </div></details>
      </div>
      {#if scans[library.id]}<p class="scan-status scan-counters" role="status">Scan : {scanLabels[scans[library.id].status]} · {scans[library.id].discovered} fichier(s) découvert(s) · {scans[library.id].probed} analysé(s) · {scans[library.id].reused} réutilisé(s)</p>{/if}
    </article>{:else}<p>Ajoutez votre première bibliothèque de films ou de séries.</p>{/each}
  </div>
</section>
<dialog bind:this={editor} aria-labelledby="library-form" oncancel={(event) => { if (saving) event.preventDefault(); }}>
  <h2 id="library-form">{editing ? 'Modifier la médiathèque' : 'Ajouter une médiathèque'}</h2>
  <form onsubmit={submit}><fieldset disabled={saving}>
    <label for="library-name">Nom de la bibliothèque</label><input id="library-name" bind:value={name} required maxlength="100" autocomplete="off" />
    <label for="library-kind">Type de contenu</label><select id="library-kind" bind:value={kind}><option value="films">Films</option><option value="series">Séries</option><option value="music">Musique</option></select>
    <label for="library-node">Media Node assigné</label><select id="library-node" bind:value={nodeId} onchange={onnodechange}><option value="local">Node local</option>{#each activeNodes.filter(n => n.id !== 'local') as node (node.id)}<option value={node.id}>{node.name}</option>{/each}</select>
    <label for="library-root">Racine autorisée sur le Node</label><select id="library-root" bind:value={root} required><option value="" disabled>Choisir une racine</option>{#each availableRootsForNode as item (item)}<option value={item}>{item}</option>{/each}</select>
    {#if availableRootsForNode.length === 0}<p>Aucune racine disponible sur ce Node.</p>{/if}
    <label for="library-excluded">Dossiers ou motifs à exclure (séparés par des virgules)</label>
    <input id="library-excluded" bind:value={excludedPatterns} placeholder="ex: bonus, trailers, #recycle" autocomplete="off" />
    <p class="field-hint">Les dossiers système (#recycle, @eaDir, dossiers cachés) et les dossiers contenant un fichier .nomedia ou .ignore sont exclus automatiquement.</p>
    <label class="check"><input type="checkbox" bind:checked={enabled} /> Bibliothèque active</label>
    <p>Les accès se règlent dans Utilisateurs et Groupes. Les médias restent en lecture seule.</p>
    <div class="actions"><button type="button" onclick={() => editor?.close()}>Annuler</button><button class="primary" type="submit" disabled={!availableRootsForNode.includes(root)}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button></div>
  </fieldset></form>
  {#if error}<p role="alert">{error}</p>{/if}
</dialog>
<dialog bind:this={refresher} aria-labelledby="refresh-heading" oncancel={(event) => { if (refreshing) event.preventDefault(); }}>
  <form onsubmit={refresh}>
    <h2 id="refresh-heading">Actualiser {target?.name ?? 'toutes les médiathèques'}</h2>
    <label for="refresh-mode">Mode d’actualisation</label><select id="refresh-mode" bind:value={mode} disabled={refreshing}>
      <option value="incremental">Actualisation rapide (recommandée)</option>
      <option value="new_only">Actualiser en gardant les descriptions existantes</option>
      <option value="missing_metadata">Compléter les descriptions manquantes</option>
      <option value="full">Tout réanalyser (plus lent)</option>
    </select>
    <p>{mode === 'new_only' ? 'Les métadonnées des médias déjà connus sont conservées. Les fichiers modifiés et les pistes restent vérifiés.' : mode === 'missing_metadata' ? 'Les métadonnées locales déjà validées sont conservées. Les médias sans métadonnées sont complétés depuis les fichiers NFO disponibles.' : mode === 'full' ? 'Tous les fichiers sont réanalysés. Les métadonnées locales sont relues, y compris celles des médias connus.' : 'Les analyses techniques inchangées sont réutilisées. Les fichiers et leurs pistes externes sont vérifiés.'}</p>
    <p>La recherche de métadonnées auprès de fournisseurs en ligne n’est pas encore disponible.</p>
    {#if error}<p role="alert">{error}</p>{/if}
    <div class="actions"><button type="button" disabled={refreshing} onclick={() => refresher?.close()}>Annuler</button><button class="primary" type="submit" disabled={refreshing}>{refreshing ? 'Lancement…' : 'Actualiser'}</button></div>
  </form>
</dialog>
<style>
  h2 { font-size: 28px; font-weight: 400; margin: 0 0 28px; overflow-wrap: anywhere; }
  .toolbar { display: flex; flex-wrap: wrap; gap: 16px; margin-bottom: 32px; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 10px; min-height: 44px; padding: 10px 18px; font: inherit; font-size: 14px; border: 1px solid var(--border-subtle); border-radius: 4px; background: var(--bg-surface); color: var(--text-primary); }
  button:hover:not(:disabled), .menu-actions a:hover { background: var(--bg-elevated); }
  .primary { border: 0; background: var(--brand-gradient); color: var(--color-on-accent); }
  .primary:hover:not(:disabled) { background: var(--brand-gradient); box-shadow: var(--shadow-sm); }
  .library-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 240px), 1fr)); gap: 24px; }
  .library-card { min-width: 0; background: var(--bg-surface); }
  .library-cover { width: 100%; min-height: 175px; border: 0; border-radius: 0; display: flex; flex-direction: column; gap: 16px; padding: 24px; background: var(--bg-elevated); color: var(--text-primary); }
  .library-cover span { font-size: clamp(22px, 2vw, 30px); font-weight: 650; overflow-wrap: anywhere; }
  .library-cover :global(svg) { color: var(--accent-secondary); flex-shrink: 0; }
  .card-bottom { display: flex; align-items: center; gap: 12px; padding: 18px; }
  .card-text { min-width: 0; flex: 1; } h3 { font-size: 17px; font-weight: 500; margin: 0; overflow-wrap: anywhere; }
  p { font-size: 14px; line-height: 1.6; color: var(--text-secondary); }
  .card-text p { margin: 8px 0 0; }
  .library-menu { position: relative; flex-shrink: 0; }
  summary { display: grid; place-items: center; cursor: pointer; list-style: none; width: 44px; height: 44px; font-size: 28px; }
  summary::-webkit-details-marker { display: none; }
  .menu-actions { position: absolute; right: 0; top: 44px; width: min(280px, calc(100vw - 48px)); padding: 8px; z-index: 20; background: var(--bg-elevated); border: 1px solid var(--border-subtle); box-shadow: var(--shadow-md); }
  .menu-actions button, .menu-actions a { display: flex; justify-content: flex-start; width: 100%; padding: 12px; border: 0; border-radius: 0; background: transparent; font-size: 14px; text-align: left; min-height: 44px; }
  .scan-status { margin: 0; padding: 0 18px 16px; color: var(--text-secondary); font-size: 12px; }
  dialog { width: min(640px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; padding: 28px; border: 1px solid var(--border-subtle); border-radius: 6px; background: var(--bg-main); color: var(--text-primary); }
  dialog::backdrop { background: rgb(0 0 0 / .65); }
  fieldset { min-width: 0; padding: 0; margin: 0; border: 0; }
  label { display: block; margin: 24px 0 10px; font-size: 14px; }
  input:not([type='checkbox']), select { width: 100%; min-width: 0; min-height: 44px; padding: 12px; background-color: var(--bg-surface); color: var(--text-primary); border: 1px solid var(--border-subtle); border-radius: 4px; }
  .field-hint { font-size: 12px; color: var(--text-secondary); margin: 6px 0 14px; }
  .check { display: flex; align-items: center; gap: 10px; }
  .actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 12px; margin-top: 24px; }
  [role='alert'] { color: var(--status-error); overflow-wrap: anywhere; }
  @media(max-width: 600px) { .library-grid { grid-template-columns: minmax(0, 1fr); } dialog { padding: 20px; } }
</style>

<script lang="ts">
  import { resolve } from '$app/paths';
  import type { Library, AdminNode, Scan, RefreshMode, LibraryOptions } from './types';
  import LibraryOptionsForm from './LibraryOptions.svelte';
  import LibraryImages from './LibraryImages.svelte';
  import PathPickerModal from './PathPickerModal.svelte';
  import { scanLabels } from './types';
  import Icon from '../ui/Icon.svelte';
  let { libraries, nodes, scans, pending, availableRootsForNode, saving, editing, error,
    name = $bindable(), kind = $bindable(), nodeId = $bindable(), root = $bindable(), folders = $bindable(), enabled = $bindable(),
    excludedPatterns = $bindable(''),
    options = $bindable(), onchanged,
    onsave, onscan, onedit, onreset, onnodechange }: {
      libraries: Library[]; nodes: AdminNode[]; scans: Record<string, Scan>; pending: string[];
      availableRootsForNode: string[]; saving: boolean; editing: string | null; error: string;
      name: string; kind: 'films' | 'series' | 'music'; nodeId: string; root: string; folders: { nodeId: string; root: string }[]; enabled: boolean;
      excludedPatterns: string;
      options: LibraryOptions; onchanged: () => Promise<void>;
      onsave: (event: SubmitEvent) => Promise<boolean>; onscan: (library: Library, mode?: RefreshMode) => Promise<void>;
      onedit: (library: Library) => void; onreset: () => void; onnodechange: () => void;
  } = $props();
  let editor = $state<HTMLDialogElement>(), refresher = $state<HTMLDialogElement>();
  let imageTarget = $state<Library | null>(null);
  let target = $state<Library | null>(null), mode = $state<RefreshMode>('incremental'), refreshing = $state(false);
  let showPathPicker = $state(false);
  const activeNodes = $derived(nodes.filter(n => n.status !== 'revoked'));
  function closeMenu(event: MouseEvent) { const details = (event.currentTarget as HTMLElement).closest('details'); details?.removeAttribute('open'); details?.querySelector('summary')?.focus(); }
  function manage(library?: Library) { if (library) onedit(library); else onreset(); editor?.showModal(); }
  function refreshLibrary(library: Library | null) { target = library; mode = 'incremental'; refresher?.showModal(); }
  async function submit(event: SubmitEvent) {
    if (folders.length === 0 && root.trim()) {
      folders = [{ nodeId, root: root.trim() }];
    }
    if (await onsave(event)) editor?.close();
  }
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
  <h2 id="libraries-heading">Bibliothèques</h2>
  <div class="toolbar"><button class="primary" onclick={() => manage()}><span aria-hidden="true">+</span> Ajouter une bibliothèque</button><button onclick={() => refreshLibrary(null)} disabled={libraries.length === 0 || refreshing}><Icon name="refresh" size={18} /> Actualiser toutes les bibliothèques</button></div>
  <div class="library-grid">
    {#each libraries as library (library.id)}
      {@const folderCount = library.folders?.length ?? 1}
      {@const serverCount = new Set((library.folders ?? [{nodeId:library.nodeId}]).map(folder => folder.nodeId)).size}
      <article class="library-card">
      <button class="library-cover" onclick={() => manage(library)} aria-label={`Gérer ${library.name}`}>
        {#if library.imageKinds?.includes('backdrop') || library.imageKinds?.includes('poster')}
          <img class="custom-cover" src={`/api/v1/admin/libraries/${encodeURIComponent(library.id)}/images/${library.imageKinds.includes('backdrop') ? 'backdrop' : 'poster'}?v=${library.imageVersions?.[library.imageKinds.includes('backdrop') ? 'backdrop' : 'poster'] ?? ''}`} alt="" loading="lazy" />
        {:else if library.coverUrls?.length}<div class="cover-collage" aria-hidden="true">{#each library.coverUrls as url (url)}<img src={url} alt="" loading="lazy" decoding="async" />{/each}</div>
        {:else}<Icon name={library.kind === 'films' ? 'film' : 'tv'} size={48} />{/if}
        <span>{library.name}</span>
      </button>
      <div class="card-bottom"><div class="card-text"><h3>{library.name}</h3><p>{library.kind === 'music' ? 'Musique' : library.kind === 'films' ? 'Films' : 'Séries'}{library.enabled ? '' : ' · Désactivée'}</p></div>
        <details class="library-menu"><summary aria-label={`Actions ${library.name}`}>⋮</summary><div class="menu-actions">
          <button onclick={(event) => { closeMenu(event); manage(library); }}>Gérer la bibliothèque</button>
          <button onclick={(event) => { closeMenu(event); manage(library); }}>Renommer</button>
          <button disabled={!library.enabled || pending.includes(library.id) || ['queued','running'].includes(scans[library.id]?.status)} onclick={(event) => { closeMenu(event); void onscan(library); }}>Scanner la bibliothèque</button>
          <button onclick={(event) => { closeMenu(event); imageTarget = library; }}>Modifier les images</button>
          <button disabled={!library.enabled || pending.includes(library.id) || ['queued','running'].includes(scans[library.id]?.status)} onclick={(event) => { closeMenu(event); refreshLibrary(library); }}>Actualiser la bibliothèque</button>
          <a href={resolve(`admin/libraries/#tasks-section?task=scan-${encodeURIComponent(library.id)}`)}>Planifier l’actualisation</a>
        </div></details>
      </div>
      <p class="folder-count">{folderCount} {folderCount > 1 ? 'dossiers' : 'dossier'} · {serverCount} {serverCount > 1 ? 'serveurs' : 'serveur'}</p>
      {#if scans[library.id]}<p class="scan-status scan-counters" role="status">Scan : {scanLabels[scans[library.id].status]} · {scans[library.id].discovered} fichier(s) découvert(s) · {scans[library.id].probed} analysé(s) · {scans[library.id].reused} réutilisé(s)</p>{/if}
      {#if ['queued','running'].includes(scans[library.id]?.status)}<progress aria-label={`Scan de ${library.name} en cours`}></progress>{/if}
    </article>{:else}<p>Ajoutez votre première bibliothèque de films ou de séries.</p>{/each}
  </div>
</section>
<dialog bind:this={editor} aria-labelledby="library-form" oncancel={(event) => { if (saving) event.preventDefault(); }}>
  <h2 id="library-form">{editing ? 'Modifier la bibliothèque' : 'Ajouter une bibliothèque'}</h2>
  <form onsubmit={submit}><fieldset disabled={saving}>
    <label for="library-name">Nom de la bibliothèque</label><input id="library-name" bind:value={name} required maxlength="100" autocomplete="off" />
    <label for="library-kind">Type de contenu</label><select id="library-kind" bind:value={kind}><option value="films">Films</option><option value="series">Séries</option><option value="music">Musique</option></select>
    <section class="folders" aria-labelledby="folders-heading">
      <h3 id="folders-heading">Dossiers de la bibliothèque</h3>
      <p>Regroupez vos dossiers de plusieurs serveurs, par exemple dans Films_UHD. Les accès et les réglages s’appliquent à toute la bibliothèque.</p>
      <ul>{#each folders as folder, index (`${folder.nodeId}:${folder.root}`)}
        <li><div><strong>{nodes.find(node => node.id === folder.nodeId)?.name ?? folder.nodeId}</strong><span>{folder.root}</span></div><button type="button" aria-label={`Retirer ${folder.root}`} onclick={() => folders = folders.filter((_, i) => i !== index)}>Retirer</button></li>
      {:else}<li class="empty-folders">Aucun dossier ajouté. Choisissez un serveur et un dossier ci-dessous.</li>{/each}</ul>
    <label for="library-node">Serveur du dossier</label><select id="library-node" bind:value={nodeId} onchange={onnodechange}><option value="local">Node local</option>{#each activeNodes.filter(n => n.id !== 'local') as node (node.id)}<option value={node.id}>{node.name}</option>{/each}</select>
    <label for="library-root">Dossier</label>
    <div class="folder-input-row">
      <input
        id="library-root"
        type="text"
        bind:value={root}
        placeholder="ex: /media/films ou C:\Films"
        autocomplete="off"
        spellcheck="false"
      />
      <button
        type="button"
        class="browse-btn"
        onclick={() => showPathPicker = true}
        title="Parcourir les dossiers"
      >
        <Icon name="search" size={16} /> Parcourir…
      </button>
    </div>

    <button
      type="button"
      class="add-folder-btn"
      disabled={folders.length >= 32 || !root.trim() || folders.some(folder => folder.nodeId === nodeId && folder.root === root.trim())}
      onclick={() => {
        if (root.trim()) {
          folders = [...folders, { nodeId, root: root.trim() }];
          root = '';
        }
      }}
    >
      <span aria-hidden="true">+</span> Ajouter ce dossier
    </button>
    </section>
    <label for="library-excluded">Dossiers ou motifs à exclure (séparés par des virgules)</label>
    <input id="library-excluded" bind:value={excludedPatterns} placeholder="ex: bonus, trailers, #recycle" autocomplete="off" />
    <p class="field-hint">Les dossiers système (#recycle, @eaDir, dossiers cachés) et les dossiers contenant un fichier .nomedia ou .ignore sont exclus automatiquement.</p>
    <label class="check"><input type="checkbox" bind:checked={enabled} /> Bibliothèque active</label>
    <LibraryOptionsForm bind:options {kind} />
    <p>Les accès se règlent dans Utilisateurs et Groupes. Les médias restent en lecture seule.</p>
    <div class="actions"><button type="button" onclick={() => editor?.close()}>Annuler</button><button class="primary" type="submit" disabled={folders.length === 0}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button></div>
  </fieldset></form>
  {#if error}<p role="alert">{error}</p>{/if}
</dialog>
<dialog bind:this={refresher} aria-labelledby="refresh-heading" oncancel={(event) => { if (refreshing) event.preventDefault(); }}>
  <form onsubmit={refresh}>
    <h2 id="refresh-heading">Actualiser {target?.name ?? 'toutes les bibliothèques'}</h2>
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
{#if imageTarget}<LibraryImages library={imageTarget} onclose={() => imageTarget = null} {onchanged} />{/if}
{#if showPathPicker}
  <PathPickerModal
    open={showPathPicker}
    initialPath={root}
    {nodeId}
    onselect={(selected) => { root = selected; }}
    onclose={() => { showPathPicker = false; }}
  />
{/if}
<style>
  .folder-count { margin:0; padding:0 18px 18px; font-size:12px; }
  .folders { margin:24px 0; padding:20px; border:1px solid var(--border-subtle); border-radius:12px; background:var(--bg-elevated); }
  .folders h3 { margin:0 0 8px; } .folders p { font-size:13px; line-height:1.6; color:var(--text-secondary); }
  .folders ul { list-style:none; padding:0; display:grid; gap:10px; }
  .folders li { display:flex; justify-content:space-between; align-items:center; gap:12px; padding:12px; background:var(--bg-surface); border-radius:var(--radius-md); }
  .folders li div { min-width:0; display:grid; gap:5px; } .folders li span { overflow-wrap:anywhere; font-size:13px; color:var(--text-secondary); }
  .folders li button { flex-shrink:0; white-space:nowrap; padding:8px 12px; }
  h2 { font-size: 28px; font-weight: 400; margin: 0 0 28px; overflow-wrap: anywhere; }
  .toolbar { display: flex; flex-wrap: wrap; gap: 16px; margin-bottom: 32px; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 10px; min-height: 44px; padding: 10px 18px; font: inherit; font-size: 14px; border: 1px solid var(--border-subtle); border-radius: var(--radius-md); background: var(--bg-surface); color: var(--text-primary); }
  button:hover:not(:disabled), .menu-actions a:hover { background: var(--bg-elevated); }
  .primary { border: 0; background: var(--color-play); color: var(--color-on-play); }
  .primary:hover:not(:disabled) { background: var(--color-play); box-shadow: var(--shadow-sm); }
  .library-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 240px), 1fr)); gap: 24px; }
  .library-card { min-width: 0; background: var(--bg-surface); border-radius:var(--radius-lg); box-shadow:var(--shadow-sm); }
  .library-cover { position:relative;overflow:hidden;isolation:isolate;width: 100%; min-height: 175px; border: 0; border-radius: var(--radius-lg) var(--radius-lg) 0 0; display: flex; flex-direction: column; gap: 16px; padding: 24px; background: var(--bg-elevated); color: var(--text-primary); }
  .custom-cover,.cover-collage { position:absolute;inset:0;width:100%;height:100%;object-fit:cover;z-index:-2; }
  .cover-collage { display:flex;transform:rotate(-12deg) scale(1.3);gap:5px; }
  .cover-collage img { flex:1;min-width:0;object-fit:cover; }
  .library-cover:has(img)::after { content:'';position:absolute;inset:0;background:linear-gradient(transparent,rgb(0 0 0 / .8));z-index:-1; }
  .library-cover:has(img) span { color:white;text-shadow:0 2px 5px black; }
  progress { display:block;width:calc(100% - 36px);margin:0 18px 16px;accent-color:var(--accent-primary); }
  .library-cover span { font-size: clamp(22px, 2vw, 30px); font-weight: 650; overflow-wrap: anywhere; }
  .library-cover :global(svg) { color: var(--accent-secondary); flex-shrink: 0; }
  .card-bottom { display: flex; align-items: center; gap: 12px; padding: 18px; }
  .card-text { min-width: 0; flex: 1; } h3 { font-size: 17px; font-weight: 500; margin: 0; overflow-wrap: anywhere; }
  p { font-size: 14px; line-height: 1.6; color: var(--text-secondary); }
  .card-text p { margin: 8px 0 0; }
  .library-menu { position: relative; flex-shrink: 0; }
  summary { display: grid; place-items: center; cursor: pointer; list-style: none; width: 44px; height: 44px; font-size: 28px; }
  summary::-webkit-details-marker { display: none; }
  .menu-actions { position: absolute; right: 0; top: 44px; width: min(280px, calc(100vw - 48px)); padding: 8px; border-radius:var(--radius-lg); z-index: 20; background: var(--bg-elevated); border: 1px solid var(--border-subtle); box-shadow: var(--shadow-md); }
  .menu-actions button, .menu-actions a { display: flex; justify-content: flex-start; width: 100%; padding: 12px; border: 0; border-radius: 0; background: transparent; font-size: 14px; text-align: left; min-height: 44px; }
  .scan-status { margin: 0; padding: 0 18px 16px; color: var(--text-secondary); font-size: 12px; }
  dialog { width: min(640px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; padding: 28px; border: 1px solid var(--border-subtle); border-radius: var(--radius-xl); background: var(--bg-main); color: var(--text-primary); }
  dialog::backdrop { background: rgb(0 0 0 / .65); }
  fieldset { min-width: 0; padding: 0; margin: 0; border: 0; }
  label { display: block; margin: 24px 0 10px; font-size: 14px; }
  input:not([type='checkbox']), select { width: 100%; min-width: 0; min-height: 44px; padding: 12px; background-color: var(--bg-surface); color: var(--text-primary); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); }
  .field-hint { font-size: 12px; color: var(--text-secondary); margin: 6px 0 14px; }
  .check { display: flex; align-items: center; gap: 10px; }
  .actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 12px; margin-top: 24px; }
  [role='alert'] { color: var(--status-error); overflow-wrap: anywhere; }
  .folder-input-row { display: flex; gap: 8px; align-items: center; }
  .folder-input-row input { flex: 1; min-width: 0; }
  .browse-btn { flex-shrink: 0; white-space: nowrap; height: 44px; }
  .add-folder-btn { margin-top: 10px; width: 100%; }
  @media(max-width: 600px) { .library-grid { grid-template-columns: minmax(0, 1fr); } dialog { padding: 20px; } }
</style>

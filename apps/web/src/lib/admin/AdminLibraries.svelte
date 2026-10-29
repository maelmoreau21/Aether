<script lang="ts">
  import { tick } from 'svelte';
  import { resolve } from '$app/paths';
  import type { Library, AdminNode, Scan, RefreshMode, LibraryOptions } from './types';
  import LibraryOptionsForm from './LibraryOptions.svelte';
  import LibraryImages from './LibraryImages.svelte';
  import PathPickerModal from './PathPickerModal.svelte';
  import { scanLabels } from './types';
  import Icon from '../ui/Icon.svelte';
  let { libraries, nodes, scans, pending, saving, editing, error,
    name = $bindable(), kind = $bindable(), nodeId = $bindable(), root = $bindable(), folders = $bindable(), enabled = $bindable(),
    excludedPatterns = $bindable(''),
    options = $bindable(), onchanged,
    onsave, onscan, onedit, onreset, onnodechange }: {
      libraries: Library[]; nodes: AdminNode[]; scans: Record<string, Scan>; pending: string[];
      saving: boolean; editing: string | null; error: string;
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
  const editorSections = [
    { id: 'general', label: 'Général' },
    { id: 'sources', label: 'Sources et dossiers' },
    { id: 'metadata', label: 'Métadonnées et images' },
    { id: 'indexing', label: 'Indexation' },
    { id: 'access', label: 'Accès' },
    { id: 'advanced', label: 'Avancé' }
  ] as const;
  type EditorSection = typeof editorSections[number]['id'];
  let activeSection = $state<EditorSection>('general');
  let formError = $state('');
  let editorContent: HTMLDivElement;
  function chooseEditorSection(section: EditorSection) {
    activeSection = section;
    if (editorContent) editorContent.scrollTop = 0;
  }
  function tabKeydown(event: KeyboardEvent, index: number) {
    const step = ['ArrowDown', 'ArrowRight'].includes(event.key) ? 1 : ['ArrowUp', 'ArrowLeft'].includes(event.key) ? -1 : 0;
    if (!step && event.key !== 'Home' && event.key !== 'End') return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? editorSections.length - 1 : (index + step + editorSections.length) % editorSections.length;
    chooseEditorSection(editorSections[next].id);
    editor?.querySelector<HTMLButtonElement>(`#library-tab-${editorSections[next].id}`)?.focus();
  }
  const activeNodes = $derived(nodes.filter(n => n.status !== 'revoked'));
  function closeMenu(event: MouseEvent) { const details = (event.currentTarget as HTMLElement).closest('details'); details?.removeAttribute('open'); details?.querySelector('summary')?.focus(); }
  function manage(library?: Library) { if (library) onedit(library); else onreset(); activeSection = 'general'; formError = ''; editor?.showModal(); }
  function refreshLibrary(library: Library | null) { target = library; mode = 'incremental'; refresher?.showModal(); }
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    formError = '';
    const form = event.currentTarget as HTMLFormElement;
    const invalid = form.querySelector<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>('input:invalid, select:invalid, textarea:invalid');
    if (invalid) {
      const panel = invalid.closest<HTMLElement>('[data-editor-panel]');
      chooseEditorSection((panel?.dataset.editorPanel ?? 'general') as EditorSection);
      for (let parent = invalid.parentElement; parent && parent !== editor; parent = parent.parentElement) {
        if (parent instanceof HTMLDetailsElement) parent.open = true;
      }
      await tick();
      invalid.focus(); invalid.reportValidity();
      return;
    }
    if (!folders.length) {
      formError = 'Ajoutez au moins un dossier dans Sources et dossiers.';
      chooseEditorSection('sources');
      await tick(); editor?.querySelector<HTMLInputElement>('#library-root')?.focus();
      return;
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
<dialog class="library-editor" bind:this={editor} aria-labelledby="library-form" oncancel={event => { if (saving) event.preventDefault(); }}>
  <header class="editor-header"><div><p class="eyebrow">{editing ? name : 'Nouvelle bibliothèque'}</p><h2 id="library-form">{editing ? 'Modifier la bibliothèque' : 'Ajouter une bibliothèque'}</h2></div><button type="button" disabled={saving} aria-label="Fermer les réglages de la bibliothèque" onclick={() => editor?.close()}><Icon name="close" size={20} /></button></header>
  <form onsubmit={submit} novalidate><fieldset disabled={saving}>
    <div class="editor-workspace">
      <div class="editor-navigation">
        <div class="editor-tabs" role="tablist" aria-label="Réglages de la bibliothèque" aria-orientation="vertical">
          {#each editorSections as section, index (section.id)}<button type="button" id={`library-tab-${section.id}`} role="tab" aria-selected={activeSection === section.id} aria-controls={`library-panel-${section.id}`} tabindex={activeSection === section.id ? 0 : -1} onclick={() => chooseEditorSection(section.id)} onkeydown={event => tabKeydown(event, index)}>{section.label}</button>{/each}
        </div>
        <label class="editor-select">Rubrique<select aria-label="Rubrique des réglages" value={activeSection} onchange={event => chooseEditorSection(event.currentTarget.value as EditorSection)}>{#each editorSections as section (section.id)}<option value={section.id}>{section.label}</option>{/each}</select></label>
      </div>
      <div class="editor-content" bind:this={editorContent}>
        <div id="library-panel-general" role="tabpanel" aria-labelledby="library-tab-general" data-editor-panel="general" hidden={activeSection !== 'general'} tabindex="0">
          <h3>Général</h3><p class="panel-description">Donnez un nom à la bibliothèque et choisissez le type de médias.</p>
          <div class="identity-fields"><div><label for="library-name">Nom de la bibliothèque</label><input id="library-name" bind:value={name} required maxlength="100" autocomplete="off" /></div><div><label for="library-kind">Type de contenu</label><select id="library-kind" bind:value={kind}><option value="films">Films</option><option value="series">Séries</option><option value="music">Musique</option></select></div></div>
          <label class="check"><input type="checkbox" bind:checked={enabled} /> Bibliothèque active</label><p class="field-hint">Une bibliothèque désactivée est masquée du catalogue et ne peut pas être lue.</p>
          <div class="source-summary"><strong>{folders.length} {folders.length > 1 ? 'dossiers configurés' : 'dossier configuré'}</strong><p>{folders.length ? 'Les médias restent en lecture seule.' : 'Ajoutez un dossier pour pouvoir enregistrer cette bibliothèque.'}</p><button type="button" onclick={() => chooseEditorSection('sources')}>{folders.length ? 'Consulter les dossiers' : 'Configurer les dossiers'}</button></div>
        </div>
        <div id="library-panel-sources" role="tabpanel" aria-labelledby="library-tab-sources" data-editor-panel="sources" hidden={activeSection !== 'sources'} tabindex="0">
          <div class="section-heading"><h3>Sources et dossiers</h3><span>{folders.length} / 32</span></div><p class="panel-description">Choisissez les dossiers sur les serveurs qui les hébergent. Aucun fichier d’origine n’est modifié.</p>
          <ul class="folders">{#each folders as folder (`${folder.nodeId}:${folder.root}`)}<li><div><strong>{nodes.find(node => node.id === folder.nodeId)?.name ?? folder.nodeId}</strong><span>{folder.root}</span></div><button type="button" aria-label={`Retirer ${folder.root}`} onclick={() => folders = folders.filter(item => item !== folder)}>Retirer</button></li>{:else}<li class="empty-folders">Aucun dossier ajouté.</li>{/each}</ul>
          <label for="library-node">Serveur du dossier</label><select id="library-node" bind:value={nodeId} onchange={onnodechange}><option value="local">Node local</option>{#each activeNodes.filter(node => node.id !== 'local') as node (node.id)}<option value={node.id}>{node.name}</option>{/each}</select>
          <label for="library-root">Dossier</label><div class="folder-input-row"><input id="library-root" type="text" bind:value={root} placeholder="ex: /media/films ou C:\Films" autocomplete="off" spellcheck="false" /><button type="button" class="browse-btn" onclick={() => showPathPicker = true}><Icon name="search" size={16} />Parcourir…</button></div>
          <button type="button" class="add-folder-btn" disabled={folders.length >= 32 || !root.trim() || folders.some(folder => folder.nodeId === nodeId && folder.root === root.trim())} onclick={() => { if (root.trim()) { folders = [...folders, { nodeId, root: root.trim() }]; root = ''; formError = ''; } }}><span aria-hidden="true">+</span> Ajouter ce dossier</button>
        </div>
        <div id="library-panel-metadata" role="tabpanel" aria-labelledby="library-tab-metadata" data-editor-panel="metadata" hidden={activeSection !== 'metadata'} tabindex="0"><LibraryOptionsForm bind:options {kind} section="metadata" /></div>
        <div id="library-panel-indexing" role="tabpanel" aria-labelledby="library-tab-indexing" data-editor-panel="indexing" hidden={activeSection !== 'indexing'} tabindex="0">
          <h3>Indexation</h3><p class="panel-description">Choisissez les fichiers à ignorer et les règles d’import.</p>
          <label for="library-excluded">Dossiers ou motifs à exclure</label><input id="library-excluded" bind:value={excludedPatterns} placeholder="ex: bonus, trailers, #recycle" autocomplete="off" /><p class="field-hint">Séparez les motifs par des virgules. Les dossiers système (#recycle, @eaDir, dossiers cachés), .nomedia et .ignore sont déjà exclus automatiquement.</p>
          <LibraryOptionsForm bind:options {kind} section="indexing" />
        </div>
        <div id="library-panel-access" role="tabpanel" aria-labelledby="library-tab-access" data-editor-panel="access" hidden={activeSection !== 'access'} tabindex="0"><h3>Accès à la bibliothèque</h3><p>Les autorisations se gèrent dans les rubriques <strong>Utilisateurs</strong> et <strong>Groupes</strong> de l’administration.</p><p>Ce formulaire configure les sources et l’import. Ajouter un dossier n’accorde aucun droit supplémentaire à un compte.</p><p>Les médias et les fichiers utilisés par Jellyfin restent en lecture seule.</p></div>
        <div id="library-panel-advanced" role="tabpanel" aria-labelledby="library-tab-advanced" data-editor-panel="advanced" hidden={activeSection !== 'advanced'} tabindex="0"><h3>Options avancées</h3><p class="panel-description">Les indications de chaque groupe distinguent les réglages disponibles des préférences préparées pour de futurs moteurs.</p><LibraryOptionsForm bind:options {kind} section="advanced" /></div>
      </div>
    </div>
    <footer class="editor-actions">
      {#if formError || error}<p class="editor-error" role="alert">{formError || error}</p>{/if}
      <p class="save-hint">{folders.length ? 'Les changements d’import s’appliqueront au prochain scan.' : 'Ajoutez un dossier dans Sources et dossiers.'}</p>
      <div class="footer-buttons"><button type="button" onclick={() => editor?.close()}>Annuler</button><button class="primary" type="submit" disabled={!folders.length}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button></div>
    </footer>
  </fieldset></form>
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
  dialog.library-editor { width:min(1040px,calc(100vw - 48px)); height:min(800px,calc(100dvh - 48px)); max-width:none; max-height:calc(100dvh - 48px); padding:0; overflow:hidden; }
  dialog.library-editor[open] { display:flex; flex-direction:column; }
  .editor-header { display:flex; align-items:center; justify-content:space-between; gap:16px; padding:20px 24px; background:var(--color-surface); border-bottom:1px solid var(--color-line); flex:none; }
  .editor-header h2 { margin:4px 0 0; font-size:24px; font-weight:650; }
  .editor-header button { width:44px; height:44px; padding:0; flex:none; }
  .eyebrow { margin:0; font-size:12px; color:var(--color-muted); max-width:70ch; overflow-wrap:anywhere; }
  .library-editor form,.library-editor fieldset { display:flex; flex-direction:column; flex:1; min-height:0; }
  .editor-workspace { display:grid; grid-template-columns:190px minmax(0,1fr); flex:1; min-height:0; }
  .editor-navigation { background:var(--color-surface); border-right:1px solid var(--color-line); padding:20px 12px; overflow-y:auto; }
  .editor-tabs { display:grid; gap:4px; }
  .editor-tabs button { justify-content:start; border:0; background:transparent; text-align:left; font-size:13px; line-height:1.5; padding:12px; }
  .editor-tabs button[aria-selected=true] { background:var(--color-accent-soft); color:var(--color-accent); font-weight:600; }
  .editor-select { display:none; }
  .editor-content { min-height:0; overflow-y:auto; overscroll-behavior:contain; padding:24px; }
  .editor-content h3 { font-size:20px; font-weight:600; letter-spacing:-.02em; }
  .panel-description { margin:8px 0 20px; }
  .identity-fields { display:grid; grid-template-columns:minmax(0,2fr) minmax(0,1fr); gap:20px; }
  .source-summary { border-top:1px solid var(--color-line); padding-top:20px; margin-top:24px; }
  .source-summary p { margin:8px 0 16px; }
  .section-heading { display:flex; align-items:center; justify-content:space-between; gap:12px; }
  .section-heading span { color:var(--color-muted); font-size:13px; }
  .folders { list-style:none; padding:0; margin:20px 0; display:grid; gap:8px; }
  .folders li { display:flex; justify-content:space-between; align-items:center; gap:12px; padding:12px 0; border-bottom:1px solid var(--color-line); }
  .folders li div { min-width:0; display:grid; gap:5px; }
  .folders li span { overflow-wrap:anywhere; font-size:13px; color:var(--color-muted); }
  .folders li button { flex:none; padding:8px 12px; }
  .folder-count { margin:0; padding:0 18px 18px; font-size:12px; }
  .editor-actions { flex:none; display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:12px 20px; padding:16px 24px; background:var(--color-surface); border-top:1px solid var(--color-line); }
  .save-hint { flex:1; min-width:160px; margin:0; font-size:12px; }
  .footer-buttons { display:flex; gap:12px; margin-left:auto; }
  .editor-error { flex-basis:100%; margin:0; font-size:13px; }
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
  @media(max-width:800px), (max-height:520px) and (max-width:1100px) {
    .editor-workspace { grid-template-columns:minmax(0,1fr); grid-template-rows:auto minmax(0,1fr); }
    .editor-navigation { padding:12px 20px; border-right:0; border-bottom:1px solid var(--color-line); overflow:visible; }
    .editor-tabs { display:none; }
    .editor-select { display:flex; align-items:center; gap:12px; margin:0; font-size:12px; }
    .editor-select select { flex:1; width:auto; min-width:0; min-height:44px; padding:8px 12px; font-size:16px; }
  }
  @media(max-width:600px) {
    .library-grid { grid-template-columns:minmax(0,1fr); }
    dialog { padding:20px; }
    dialog.library-editor { width:100vw; height:100dvh; max-height:100dvh; margin:0; border:0; border-radius:0; }
    .editor-header { padding:16px; }
    .editor-header h2 { font-size:21px; }
    .editor-navigation { padding:10px 16px; }
    .editor-content { padding:20px 16px; }
    .identity-fields { grid-template-columns:minmax(0,1fr); gap:0; }
    .folder-input-row { flex-wrap:wrap; }
    .folder-input-row input { flex-basis:100%; }
    .browse-btn { width:100%; }
    .editor-actions { padding:12px 16px calc(12px + env(safe-area-inset-bottom,0px)); gap:8px; }
    .save-hint { flex-basis:100%; }
    .footer-buttons { width:100%; }
    .footer-buttons button { flex:1; }
    .library-editor input:not([type=checkbox]),.library-editor select { font-size:16px; }
  }
  @media(max-height:520px) and (max-width:1100px) {
    dialog.library-editor { height:100dvh; max-height:100dvh; width:100vw; margin:0; border:0; border-radius:0; }
    .editor-header { padding:8px 16px; }
    .editor-header h2 { margin:0; font-size:20px; }
    .eyebrow,.save-hint { display:none; }
    .editor-navigation { padding:8px 16px; }
    .editor-actions { padding:8px 16px calc(8px + env(safe-area-inset-bottom,0px)); }
    .editor-content { padding:16px; }
  }
</style>

<script lang="ts">
  import { tick } from 'svelte';
  import { resolve } from '$app/paths';
  import type { Library, AdminNode, Scan, RefreshMode, LibraryOptions } from './types';
  import LibraryOptionsForm, { type LibraryOptionsSection } from './LibraryOptions.svelte';
  import LibraryAccess from './LibraryAccess.svelte';
  import LibraryImages from './LibraryImages.svelte';
  import PathPickerModal from './PathPickerModal.svelte';
  import { recommendedLibraryOptions, scanLabels } from './types';
  import type { AuthUser } from '../auth/session';
  import type { UserGroup } from './request';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import { t } from '../i18n';
  let { libraries, nodes, scans, pending, saving, editing, error, users = [], groups = [],
    name = $bindable(), kind = $bindable(), nodeId = $bindable(), root = $bindable(), folders = $bindable(), enabled = $bindable(),
    excludedPatterns = $bindable(''),
    options = $bindable(), onchanged,
    onsave, onscan, onedit, onreset, onnodechange }: {
      libraries: Library[]; nodes: AdminNode[]; scans: Record<string, Scan>; pending: string[];
      saving: boolean; editing: string | null; error: string; users?: AuthUser[]; groups?: UserGroup[];
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
  const allSections = [
    { id: 'general', label: 'Général' },
    { id: 'sources', label: 'Sources et dossiers' },
    { id: 'metadata', label: 'Métadonnées' },
    { id: 'images', label: 'Images' },
    { id: 'playback', label: 'Lecture' },
    { id: 'subtitles', label: 'Sous-titres' },
    { id: 'providers', label: 'Fournisseurs' },
    { id: 'access', label: 'Accès' }
  ] as const;
  type EditorSection = typeof allSections[number]['id'];
  const optionSections: LibraryOptionsSection[] = ['metadata', 'images', 'playback', 'subtitles', 'providers'];
  // Music has no artwork collection, video playback, subtitles nor video providers.
  const editorSections = $derived(allSections.filter(section => kind !== 'music' || !['images', 'playback', 'subtitles', 'providers'].includes(section.id)));
  let activeSection = $state<EditorSection>('general');
  let presetApplied = $state(false);
  const editedLibrary = $derived(editing ? libraries.find(library => library.id === editing) ?? null : null);
  $effect(() => { if (!editorSections.some(section => section.id === activeSection)) activeSection = 'general'; });
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
  function nodeName(id: string) { return nodes.find(node => node.id === id)?.name ?? (id === 'local' ? 'Node local' : id); }
  let deleting = $state(false), confirmDelete = $state(false), deleteError = $state('');
  async function deleteLibrary() {
    if (!editedLibrary || deleting) return;
    deleting = true; deleteError = '';
    try {
      await adminRequest(`/api/v1/admin/libraries/${encodeURIComponent(editedLibrary.id)}`, { method: 'DELETE' });
      editor?.close(); onreset(); await onchanged();
    } catch (cause) {
      deleteError = cause instanceof Error && !cause.message.startsWith('Ce nom') ? cause.message : 'Suppression impossible pendant une analyse. Réessayez après sa fin.';
    } finally { deleting = false; confirmDelete = false; }
  }
  function manage(library?: Library) { if (library) onedit(library); else onreset(); activeSection = 'general'; presetApplied = false; formError = ''; confirmDelete = false; deleteError = ''; editor?.showModal(); }
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
      formError = $t('no_folder_added');
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
  <h2 id="libraries-heading">{$t('libraries')}</h2>
  <div class="toolbar"><button class="primary" onclick={() => manage()}><span aria-hidden="true">+</span> Ajouter une bibliothèque</button><button onclick={() => refreshLibrary(null)} disabled={libraries.length === 0 || refreshing}><Icon name="refresh" size={18} /> Actualiser toutes les bibliothèques</button></div>
  {#if libraries.length}
  <div class="library-table-panel">
    <table class="library-table">
      <caption class="h-sr">{$t('configured_libraries')}</caption>
      <thead><tr><th scope="col">{$t('name')}</th><th scope="col">{$t('content_type')}</th><th scope="col">{$t('media_node')}</th><th scope="col" class="numeric">{$t('folders')}</th><th scope="col">{$t('last_scan')}</th><th scope="col"><span class="h-sr">{$t('actions')}</span></th></tr></thead>
      <tbody>
    {#each libraries as library (library.id)}
      {@const folderCount = library.folders?.length ?? 1}
      {@const serverNames = [...new Set((library.folders ?? [{ nodeId: library.nodeId }]).map(folder => folder.nodeId))].map(nodeName)}
      {@const kindLabel = library.kind === 'music' ? 'Musique' : library.kind === 'films' ? 'Films' : 'Séries'}
      {@const scan = scans[library.id]}
      {@const scanning = ['queued','running'].includes(scan?.status)}
      {@const unavailable = !library.enabled || pending.includes(library.id) || scanning}
      {@const scanErrors = scan ? scan.errorsCount ?? scan.errors.length : 0}
      <tr class="library-row" class:disabled-library={!library.enabled}>
        <th scope="row" class="name-cell">
          <button class="library-name" onclick={() => manage(library)} aria-label={`Gérer ${library.name}`}>
            <span class="kind-icon"><Icon name={library.kind === 'films' ? 'film' : library.kind === 'music' ? 'playlist' : 'tv'} size={20} /></span>
            <span class="name-text"><span class="name">{library.name}</span><span class="row-meta">{kindLabel}{library.enabled ? '' : ' · Désactivée'} · {folderCount} {folderCount > 1 ? 'dossiers' : 'dossier'} · {serverNames.join(', ')}</span></span>
          </button>
        </th>
        <td class="type-cell">{kindLabel}{#if !library.enabled}<span class="badge">{$t('disabled')}</span>{/if}</td>
        <td class="node-cell">{serverNames.join(', ')}</td>
        <td class="folders-cell numeric">{folderCount}<span class="h-sr"> {folderCount > 1 ? 'dossiers' : 'dossier'} · {serverNames.length} {serverNames.length > 1 ? 'serveurs' : 'serveur'}</span></td>
        <td class="status-cell">
          {#if scan}
            <div class="scan-status scan-counters" role="status">
              <span class="badge" class:ok={scan.status === 'completed' && !scanErrors} class:warn={scan.status === 'completed' && scanErrors > 0} class:err={scan.status === 'error'} class:ac={scanning}><span class="h-sr">Scan : </span>{scanLabels[scan.status]}{scan.status === 'completed' && scanErrors > 0 ? ` · ${scanErrors} erreur(s)` : ''}</span>
              <span class="counters">{scan.discovered} fichier(s) découvert(s) · {scan.probed} analysé(s) · {scan.reused} réutilisé(s)</span>
            </div>
            {#if scanning}<progress aria-label={`Scan de ${library.name} en cours`}></progress>{/if}
          {:else}<span class="no-scan">—</span>{/if}
        </td>
        <td class="actions-cell">
          <div class="row-actions">
            <button class="icon-action" disabled={unavailable} aria-label={`Scanner ${library.name}`} title="Scanner la bibliothèque" onclick={() => void onscan(library)}><Icon name="refresh" size={18} /></button>
            <button class="icon-action" aria-label={`Paramètres de ${library.name}`} title="Gérer la bibliothèque" onclick={() => manage(library)}><Icon name="settings" size={18} /></button>
          </div>
        </td>
      </tr>
    {/each}
      </tbody>
    </table>
  </div>
  {:else}<p class="empty-libraries">{$t('first_library_hint')}</p>{/if}
</section>
<dialog class="library-editor" bind:this={editor} aria-labelledby="library-form" oncancel={event => { if (saving) event.preventDefault(); }}>
  <header class="editor-header"><div><p class="eyebrow">{editing ? name : 'Nouvelle bibliothèque'}</p><h2 id="library-form">{editing ? 'Modifier la bibliothèque' : 'Ajouter une bibliothèque'}</h2></div><button type="button" disabled={saving} aria-label="Fermer les réglages de la bibliothèque" onclick={() => editor?.close()}><Icon name="close" size={20} /></button></header>
  <form onsubmit={submit} novalidate><fieldset disabled={saving}>
    <div class="editor-workspace">
      <div class="editor-navigation">
        <div class="editor-tabs" role="tablist" aria-label="Réglages de la bibliothèque" aria-orientation="vertical">
          {#each editorSections as section, index (section.id)}<button type="button" id={`library-tab-${section.id}`} role="tab" aria-selected={activeSection === section.id} aria-controls={`library-panel-${section.id}`} tabindex={activeSection === section.id ? 0 : -1} onclick={() => chooseEditorSection(section.id)} onkeydown={event => tabKeydown(event, index)}>{section.label}</button>{/each}
        </div>
        <label class="editor-select">{$t('settings_section')}<select aria-label={$t('settings_section')} value={activeSection} onchange={event => chooseEditorSection(event.currentTarget.value as EditorSection)}>{#each editorSections as section (section.id)}<option value={section.id}>{section.label}</option>{/each}</select></label>
      </div>
      <div class="editor-content" bind:this={editorContent}>
        <div id="library-panel-general" role="tabpanel" aria-labelledby="library-tab-general" data-editor-panel="general" hidden={activeSection !== 'general'} tabindex="0">
          <h3>{$t('library_general')}</h3><p class="panel-description">{$t('library_general_note')}</p>
          <div class="identity-fields"><div><label for="library-name">{$t('library_name')}</label><input id="library-name" bind:value={name} required maxlength="100" autocomplete="off" /></div><div><label for="library-kind">{$t('content_kind')}</label><select id="library-kind" bind:value={kind}><option value="films">{$t('films')}</option><option value="series">{$t('series')}</option><option value="music">{$t('music')}</option></select></div></div>
          <label class="check"><input type="checkbox" bind:checked={enabled} /> {$t('active_library')}</label><p class="field-hint">{$t('disabled_library_hint')}</p>
          {#if !folders.length}<p class="field-hint">Ajoutez un dossier dans <button type="button" class="inline-link" onclick={() => chooseEditorSection('sources')}>Sources et dossiers</button> pour pouvoir enregistrer.</p>{/if}
          <div class="preset-row"><p>Réinitialiser les réglages avec les valeurs recommandées pour ce type de contenu.</p><button type="button" onclick={() => { options = recommendedLibraryOptions(options); presetApplied = true; }}>Appliquer les réglages recommandés</button></div>
          {#if presetApplied}<p role="status" class="field-hint">Réglages recommandés appliqués. Enregistrez la bibliothèque pour les conserver.</p>{/if}
          <LibraryOptionsForm bind:options bind:excludedPatterns {kind} section="scan" />
          {#if editedLibrary}
            {@const busyLibrary = !editedLibrary.enabled || pending.includes(editedLibrary.id) || ['queued', 'running'].includes(scans[editedLibrary.id]?.status)}
            <div class="library-actions">
              <h4>Actions</h4>
              <div class="action-buttons">
                <button type="button" disabled={busyLibrary} onclick={() => refreshLibrary(editedLibrary)}><Icon name="refresh" size={16} /> Actualiser la bibliothèque</button>
                <button type="button" disabled={busyLibrary} onclick={() => { target = editedLibrary; mode = 'missing_metadata'; refresher?.showModal(); }}>Rechercher les métadonnées manquantes</button>
                <button type="button" onclick={() => imageTarget = editedLibrary}>Modifier les images</button>
                <a href={resolve(`admin/libraries/#tasks-section?task=scan-${encodeURIComponent(editedLibrary.id)}`)} onclick={() => editor?.close()}>{$t('schedule_refresh')}</a>
              </div>
            </div>
            <div class="danger-zone">
              <h4>Supprimer la bibliothèque</h4>
              <p>La bibliothèque disparaît d’Aether pour tous les comptes. Vos fichiers ne sont ni modifiés ni supprimés.</p>
              {#if confirmDelete}
                <div class="action-buttons"><button type="button" class="danger" disabled={deleting} onclick={() => void deleteLibrary()}>{deleting ? 'Suppression…' : `Supprimer définitivement « ${editedLibrary.name} »`}</button><button type="button" disabled={deleting} onclick={() => confirmDelete = false}>Annuler</button></div>
              {:else}<button type="button" class="danger" onclick={() => confirmDelete = true}><Icon name="trash" size={16} /> Supprimer la bibliothèque</button>{/if}
              {#if deleteError}<p role="alert" class="editor-error">{deleteError}</p>{/if}
            </div>
          {/if}
        </div>
        <div id="library-panel-sources" role="tabpanel" aria-labelledby="library-tab-sources" data-editor-panel="sources" hidden={activeSection !== 'sources'} tabindex="0">
          <div class="section-heading"><h3>{$t('sources_folders')}</h3><span>{folders.length} / 32</span></div><p class="panel-description">{$t('sources_folders_note')}</p>
          <ul class="folders">{#each folders as folder (`${folder.nodeId}:${folder.root}`)}<li><div><strong>{nodes.find(node => node.id === folder.nodeId)?.name ?? folder.nodeId}</strong><span>{folder.root}</span></div><button type="button" aria-label={`${$t('remove')} ${folder.root}`} onclick={() => folders = folders.filter(item => item !== folder)}>{$t('remove')}</button></li>{:else}<li class="empty-folders">{$t('no_folder_added')}</li>{/each}</ul>
          <label for="library-node">{$t('folder_server')}</label><select id="library-node" bind:value={nodeId} onchange={onnodechange}><option value="local">{$t('local_node')}</option>{#each activeNodes.filter(node => node.id !== 'local') as node (node.id)}<option value={node.id}>{node.name}</option>{/each}</select>
          <label for="library-root">{$t('folder')}</label><div class="folder-input-row"><input id="library-root" type="text" bind:value={root} placeholder="ex: /media/films ou C:\Films" autocomplete="off" spellcheck="false" /><button type="button" class="browse-btn" onclick={() => showPathPicker = true}><Icon name="search" size={16} />{$t('browse')}</button></div>
          <button type="button" class="add-folder-btn" disabled={folders.length >= 32 || !root.trim() || folders.some(folder => folder.nodeId === nodeId && folder.root === root.trim())} onclick={() => { if (root.trim()) { folders = [...folders, { nodeId, root: root.trim() }]; root = ''; formError = ''; } }}><span aria-hidden="true">+</span> {$t('add_folder')}</button>
        </div>
        {#each optionSections as section (section)}
          {#if editorSections.some(item => item.id === section)}
            <div id={`library-panel-${section}`} role="tabpanel" aria-labelledby={`library-tab-${section}`} data-editor-panel={section} hidden={activeSection !== section} tabindex="0"><LibraryOptionsForm bind:options bind:excludedPatterns {kind} {section} /></div>
          {/if}
        {/each}
        <div id="library-panel-access" role="tabpanel" aria-labelledby="library-tab-access" data-editor-panel="access" hidden={activeSection !== 'access'} tabindex="0"><LibraryAccess library={editedLibrary} {libraries} {users} {groups} {onchanged} /></div>
      </div>
    </div>
    <footer class="editor-actions">
      {#if formError || error}<p class="editor-error" role="alert">{formError || error}</p>{/if}
      <p class="save-hint">{folders.length ? 'Les réglages locaux s’appliquent au prochain scan ; les réglages des fournisseurs aux prochaines collectes.' : 'Ajoutez un dossier dans Sources et dossiers.'}</p>
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
    <p>{mode === 'new_only' ? 'Les métadonnées des médias déjà connus sont conservées. Les fichiers modifiés et les pistes restent vérifiés.' : mode === 'missing_metadata' ? 'Les métadonnées locales déjà validées sont conservées. Les médias sans métadonnées ou sans affiches sont complétés depuis les fournisseurs en ligne configurés et les fichiers NFO.' : mode === 'full' ? 'Tous les fichiers sont réanalysés. Les métadonnées locales sont relues, y compris celles des médias connus.' : 'Les analyses techniques inchangées sont réutilisées. Les fichiers et leurs pistes externes sont vérifiés.'}</p>

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
  /* Liste des bibliothèques */
  h2 { margin: 0 0 20px; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.15; overflow-wrap: anywhere; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; overflow-wrap: anywhere; }
  p { font-size: 14px; line-height: 1.6; color: var(--mu); }
  .toolbar { display: flex; flex-wrap: wrap; gap: 12px; margin-bottom: 28px; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 10px; min-height: 44px; padding: 0 18px; border: 1px solid var(--color-line); border-radius: var(--r-md); background: transparent; color: var(--tx); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; transition: background-color var(--t-fast) ease, color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--color-surface-hover); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .primary { min-height: 44px; border-color: transparent; background: var(--btn); color: var(--btn-tx); font-weight: 700; box-shadow: none; }
  .primary:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  .library-actions, .danger-zone { margin-top: 24px; padding-top: 18px; border-top: 1px solid var(--color-line-subtle); }
  .library-actions h4, .danger-zone h4 { margin: 0 0 10px; font-size: 13px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; color: var(--mu); }
  .action-buttons { display: flex; flex-wrap: wrap; gap: 10px; }
  .action-buttons a { display: inline-flex; align-items: center; min-height: 44px; padding: 0 18px; border: 1px solid var(--color-line); border-radius: var(--r-md); color: var(--tx); font-size: 14px; font-weight: 600; text-decoration: none; }
  .action-buttons a:hover { background: var(--color-surface-hover); }
  .danger { border-color: color-mix(in srgb, var(--err, #e5484d) 55%, transparent); color: var(--err, #e5484d); }
  .danger:hover:not(:disabled) { background: color-mix(in srgb, var(--err, #e5484d) 12%, transparent); }
  @media (max-width: 640px) { .action-buttons > * { width: 100%; justify-content: center; } }
  button:focus-visible, a:focus-visible, [role='tabpanel']:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .library-table-panel { overflow-x: auto; background: var(--color-surface); border-radius: var(--r-md); }
  .library-table { width: 100%; border-collapse: collapse; font-size: 14px; }
  .library-table thead th { padding: 14px 16px; border-bottom: 1px solid var(--color-line-subtle); color: var(--mu); font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-align: left; text-transform: uppercase; white-space: nowrap; }
  .library-table td, .library-table tbody th { padding: 12px 16px; border-bottom: 1px solid var(--color-line-subtle); color: var(--tx); font-weight: 500; text-align: left; vertical-align: middle; transition: background-color var(--t-fast) ease; }
  .library-table tbody tr:last-child > * { border-bottom: 0; }
  .library-table tbody tr:hover > * { background: var(--color-surface-hover); }
  .library-table .numeric { text-align: right; font-variant-numeric: tabular-nums; }
  .library-table tbody th.name-cell { min-width: 220px; padding-left: 8px; }
  .library-name { justify-content: flex-start; gap: 12px; width: 100%; min-height: 48px; padding: 4px 8px; border: 0; border-radius: var(--r-sm); background: transparent; text-align: left; }
  .library-name:hover:not(:disabled) { background: transparent; }
  .kind-icon { display: grid; flex: none; place-items: center; width: 36px; height: 36px; border-radius: var(--r-sm); background: var(--pn-2); color: var(--mu); }
  .name-text { display: grid; gap: 2px; min-width: 0; }
  .name { font-size: 15px; font-weight: 700; color: var(--tx); overflow-wrap: anywhere; }
  .row-meta { display: none; color: var(--mu); font-size: 13px; font-weight: 500; overflow-wrap: anywhere; }
  .type-cell { white-space: nowrap; }
  .type-cell .badge { margin-left: 8px; }
  .library-table td.node-cell { min-width: 120px; color: var(--mu); overflow-wrap: anywhere; }
  .disabled-library .kind-icon, .disabled-library .name { opacity: 0.7; }
  .badge { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 700; line-height: 1; white-space: nowrap; }
  .badge.ok { background: var(--ok-soft); color: var(--ok); }
  .badge.warn { background: var(--warn-soft); color: var(--warn); }
  .badge.err { background: var(--err-soft); color: var(--err); }
  .badge.ac { background: var(--ac-soft); color: var(--ac); }
  .status-cell { min-width: 220px; }
  .scan-status { display: grid; justify-items: start; gap: 4px; }
  .counters { color: var(--mu); font-size: 12px; font-variant-numeric: tabular-nums; }
  .no-scan { color: var(--mu); }
  .actions-cell { width: 1%; white-space: nowrap; }
  .row-actions { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .icon-action { width: 44px; padding: 0; border-radius: var(--r-sm); background: transparent; color: var(--tx); }
  .empty-libraries { margin: 0; padding: 40px 24px; border-radius: var(--r-md); background: var(--color-surface); text-align: center; }
  progress { display: block; width: 100%; max-width: 220px; height: 4px; margin: 8px 0 0; border: 0; border-radius: var(--r-pill); overflow: hidden; background: var(--pn-2); color: var(--ac); appearance: none; -webkit-appearance: none; }
  progress::-webkit-progress-bar { background: var(--pn-2); border-radius: var(--r-pill); }
  progress::-webkit-progress-value { background: var(--ac); border-radius: var(--r-pill); }
  progress::-moz-progress-bar { background: var(--ac); border-radius: var(--r-pill); }

  /* Dialogues */
  dialog { width: min(640px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; padding: 28px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-lg); background: var(--color-surface-elevated); color: var(--tx); font-family: var(--font); box-shadow: var(--shadow-md); }
  dialog::backdrop { background: var(--color-overlay); opacity: 0.85; }
  dialog > form > h2 { margin-bottom: 4px; }
  fieldset { min-width: 0; margin: 0; padding: 0; border: 0; }
  label { display: block; margin: 20px 0 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input:not([type='checkbox']), select { width: 100%; min-width: 0; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-sm); background-color: var(--color-surface); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  input:not([type='checkbox']):hover, select:hover:not(:disabled) { background-color: var(--color-surface-hover); border-color: var(--color-line); }
  input:not([type='checkbox']):focus, select:focus, select:focus-visible { outline: none; border-color: var(--ac); background-color: var(--color-surface); }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  .field-hint { margin: 8px 0 14px; font-size: 13px; color: var(--mu); }
  .check { display: flex; align-items: center; gap: 12px; min-height: 44px; margin-bottom: 0; font-weight: 600; cursor: pointer; }
  .actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 12px; margin-top: 24px; }
  [role='alert'] { color: var(--err); overflow-wrap: anywhere; }

  /* Éditeur de bibliothèque */
  dialog.library-editor { width: min(1040px, calc(100vw - 48px)); height: min(800px, calc(100dvh - 48px)); max-width: none; max-height: calc(100dvh - 48px); padding: 0; overflow: hidden; }
  dialog.library-editor[open] { display: flex; flex-direction: column; }
  .editor-header { display: flex; flex: none; align-items: center; justify-content: space-between; gap: 16px; padding: 20px 24px; border-bottom: 1px solid var(--color-line-subtle); }
  .editor-header h2 { margin: 4px 0 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  .editor-header button { flex: none; width: 44px; height: 44px; padding: 0; border-radius: var(--r-sm); background: transparent; }
  .eyebrow { max-width: 70ch; margin: 0; color: var(--mu); font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; overflow-wrap: anywhere; }
  .library-editor form, .library-editor fieldset { display: flex; flex: 1; flex-direction: column; min-height: 0; }
  .editor-workspace { display: grid; flex: 1; grid-template-columns: 210px minmax(0, 1fr); min-height: 0; }
  .editor-navigation { padding: 20px 12px; overflow-y: auto; border-right: 1px solid var(--color-line-subtle); }
  .editor-tabs { display: grid; gap: 2px; }
  .editor-tabs button { justify-content: flex-start; min-height: 42px; padding: 0 12px; border: 0; border-radius: var(--r-sm); background: transparent; color: var(--mu); font-size: 14px; font-weight: 500; line-height: 1.3; text-align: left; }
  .editor-tabs button:hover:not(:disabled) { background: var(--color-surface-hover); color: var(--tx); }
  .editor-tabs button[aria-selected='true'] { background: var(--color-accent-soft); color: var(--color-accent); font-weight: 600; }
  .editor-select { display: none; }
  .editor-content { min-height: 0; padding: 24px 28px; overflow-y: auto; overscroll-behavior: contain; }
  .editor-content h3 { font-size: 20px; font-weight: 800; letter-spacing: -0.02em; }
  .panel-description { margin: 6px 0 20px; }
  .identity-fields { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: 20px; }
  .identity-fields label { margin-top: 0; }
  .preset-row { margin-top: 24px; padding-top: 20px; border-top: 1px solid var(--color-line-subtle); }
  .preset-row p { margin: 0 0 12px; }
  .inline-link { display: inline; min-height: 0; padding: 0; border: 0; background: none; color: var(--ac); font-size: inherit; text-decoration: underline; }
  .section-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .section-heading span { display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--color-raised); color: var(--tx); font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .folders { display: grid; margin: 16px 0 20px; padding: 0; list-style: none; border-top: 1px solid var(--color-line-subtle); }
  .folders li { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 60px; padding: 10px 0; border-bottom: 1px solid var(--color-line-subtle); }
  .folders li div { display: grid; gap: 4px; min-width: 0; }
  .folders li strong { font-size: 14px; font-weight: 700; }
  .folders li span { font-family: var(--font-mono); font-size: 13px; color: var(--mu); overflow-wrap: anywhere; }
  .folders li button { flex: none; padding: 0 14px; border-color: transparent; background: transparent; color: var(--mu); }
  .folders li button:hover:not(:disabled) { background: var(--color-surface-hover); color: var(--tx); }
  .folders .empty-folders { min-height: 52px; color: var(--mu); font-size: 14px; }
  #library-root { font-family: var(--font-mono); font-size: 14px; }
  .folder-input-row { display: flex; align-items: center; gap: 8px; }
  .folder-input-row input { flex: 1; min-width: 0; }
  .browse-btn { flex-shrink: 0; height: 46px; white-space: nowrap; }
  .add-folder-btn { width: 100%; margin-top: 12px; }
  .editor-actions { display: flex; flex: none; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px 20px; padding: 16px 24px; border-top: 1px solid var(--color-line-subtle); }
  .save-hint { flex: 1; min-width: 160px; margin: 0; font-size: 13px; }
  .footer-buttons { display: flex; gap: 12px; margin-left: auto; }
  .editor-error { flex-basis: 100%; margin: 0; font-size: 14px; }

  @media (max-width: 800px), (max-height: 520px) and (max-width: 1100px) {
    .editor-workspace { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
    .editor-navigation { padding: 12px 20px; overflow: visible; border-right: 0; border-bottom: 1px solid var(--color-line-subtle); }
    .editor-tabs { display: none; }
    .editor-select { display: flex; align-items: center; gap: 12px; margin: 0; }
    .editor-select select { flex: 1; width: auto; min-width: 0; font-size: 16px; }
  }
  @media (max-width: 640px) {
    .library-table, .library-table tbody { display: block; }
    .library-table tbody tr { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 4px 8px; padding: 10px 8px 12px; border-bottom: 1px solid var(--color-line-subtle); }
    .library-table tbody tr:last-child { border-bottom: 0; }
    .library-table tbody tr > *, .library-table tbody tr:hover > * { display: block; padding: 0; border-bottom: 0; background: transparent; }
    .library-table tbody tr:hover { background: var(--color-surface-hover); }
    .library-table tbody th.name-cell { min-width: 0; padding: 0; }
    .library-table thead, .library-table tbody tr > .type-cell, .library-table tbody tr > .node-cell, .library-table tbody tr > .folders-cell { display: none; }
    .row-meta { display: block; }
    .actions-cell { grid-column: 2; grid-row: 1; width: auto; }
    .library-table td.status-cell { grid-column: 1 / -1; min-width: 0; padding: 0 8px; }
    .toolbar button { width: 100%; }
    dialog { padding: 20px; border-radius: var(--r-lg); }
    dialog.library-editor { width: 100vw; height: 100dvh; max-height: 100dvh; margin: 0; border: 0; border-radius: 0; }
    .editor-header { padding: 14px 16px; }
    .editor-header h2 { font-size: 20px; }
    .editor-navigation { padding: 10px 16px; }
    .editor-content { padding: 20px 16px; }
    .identity-fields { grid-template-columns: minmax(0, 1fr); gap: 0; }
    .identity-fields div + div label { margin-top: 20px; }
    .folder-input-row { flex-wrap: wrap; }
    .folder-input-row input { flex-basis: 100%; }
    .browse-btn { width: 100%; }
    .editor-actions { gap: 8px; padding: 12px 16px calc(12px + env(safe-area-inset-bottom, 0px)); }
    .save-hint { flex-basis: 100%; }
    .footer-buttons { width: 100%; }
    .footer-buttons button { flex: 1; }
    .actions button { flex: 1; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }
  @media (max-height: 520px) and (max-width: 1100px) {
    dialog.library-editor { width: 100vw; height: 100dvh; max-height: 100dvh; margin: 0; border: 0; border-radius: 0; }
    .editor-header { padding: 8px 16px; }
    .editor-header h2 { margin: 0; font-size: 20px; }
    .eyebrow, .save-hint { display: none; }
    .editor-navigation { padding: 8px 16px; }
    .editor-actions { padding: 8px 16px calc(8px + env(safe-area-inset-bottom, 0px)); }
    .editor-content { padding: 16px; }
  }
  @media (prefers-reduced-motion: reduce) {
    button, .library-table td, .library-table tbody th { transition: none; }
  }
</style>

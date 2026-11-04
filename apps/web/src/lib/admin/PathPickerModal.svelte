<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../ui/Icon.svelte';

  export interface DirectoryEntry {
    name: string;
    path: string;
  }

  export interface BrowseResult {
    path: string;
    parent: string | null;
    directories: DirectoryEntry[];
  }

  let {
    open = false,
    initialPath = '',
    nodeId = 'local',
    onselect,
    onclose
  }: {
    open: boolean;
    initialPath?: string;
    nodeId?: string;
    onselect: (path: string) => void;
    onclose: () => void;
  } = $props();

  let dialog = $state<HTMLDialogElement>();
  let currentPath = $state('');
  let parent = $state<string | null>(null);
  let directories = $state<DirectoryEntry[]>([]);
  let loading = $state(false);
  let error = $state('');
  let resolvedPath = $state('');
  let requestController: AbortController | undefined;

  $effect(() => {
    if (open) {
      dialog?.showModal();
      // Opening initializes the picker once. Reading currentPath here would
      // restart this effect on every navigation and reset it to initialPath.
      untrack(() => { void browse(initialPath || ''); });
    } else {
      requestController?.abort();
      dialog?.close();
    }
  });

  async function browse(targetPath: string) {
    requestController?.abort();
    const controller = new AbortController();
    requestController = controller;
    loading = true;
    error = '';
    try {
      const url = `/api/v1/admin/filesystem/browse?nodeId=${encodeURIComponent(nodeId)}&path=${encodeURIComponent(targetPath)}`;
      const res = await fetch(url, { credentials: 'same-origin', cache: 'no-store', signal: controller.signal });
      if (!res.ok) {
        const data = await res.json().catch(() => null);
        error = data?.error ?? 'Dossier introuvable ou inaccessible.';
        return;
      }
      const data: BrowseResult = await res.json();
      currentPath = data.path;
      resolvedPath = data.path;
      parent = data.parent;
      directories = data.directories;
    } catch {
      if (!controller.signal.aborted) error = 'Impossible de contacter le serveur.';
    } finally {
      if (requestController === controller) loading = false;
    }
  }

  function handleInputKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      void browse(currentPath);
    }
  }

  function handleCancel(event: Event) {
    event.preventDefault();
    onclose();
  }

  function handleConfirm() {
    if (loading || error || currentPath !== resolvedPath) return;
    const selected = currentPath.trim();
    if (selected) {
      onselect(selected);
    }
    onclose();
  }
</script>

<dialog bind:this={dialog} class="path-picker-dialog" oncancel={handleCancel} aria-labelledby="picker-title">
  <div class="picker-header">
    <button type="button" class="back-btn" onclick={onclose} aria-label="Retour">
      <Icon name="back" size={22} />
    </button>
    <h2 id="picker-title">Sélectionner un chemin</h2>
  </div>

  <div class="picker-body">
    <p class="server-hint">Parcourez les dossiers du serveur sélectionné. Dans Docker, seuls les dossiers montés dans le conteneur peuvent être ajoutés.</p>
    <label for="picker-path-input">Dossier</label>
    <div class="input-wrapper">
      <input
        id="picker-path-input"
        type="text"
        bind:value={currentPath}
        onkeydown={handleInputKeydown}
        placeholder="/"
        autocomplete="off"
        spellcheck="false"
      />
      <button
        type="button"
        class="search-btn"
        onclick={() => browse(currentPath)}
        title="Accéder au dossier"
        aria-label="Accéder au dossier"
      >
        <Icon name="search" size={18} />
      </button>
    </div>

    <div class="navigation-actions">
      <button type="button" onclick={() => browse('')} disabled={loading}>Racines / lecteurs</button>
      <button type="button" onclick={() => browse(parent ?? '')} disabled={loading || parent === null}>Dossier parent</button>
    </div>

    {#if error}
      <p class="error-msg" role="alert">{error}</p>
    {/if}

    <div class="dirs-container" class:is-loading={loading}>
      {#if parent !== null}
        <button type="button" class="dir-item parent-item" onclick={() => browse(parent ?? '')}>
          <span class="dir-name">.. — Dossier parent</span>
          <span class="dir-arrow"><Icon name="arrow" size={18} /></span>
        </button>
      {/if}

      {#each directories as dir (dir.path)}
        <button type="button" class="dir-item" onclick={() => browse(dir.path)}>
          <span class="dir-name">{dir.name}</span>
          <span class="dir-arrow"><Icon name="arrow" size={18} /></span>
        </button>
      {/each}

      {#if directories.length === 0 && !loading && !error}
        <div class="empty-hint">Ce dossier ne contient aucun sous-dossier. Vous pouvez le sélectionner ou revenir au dossier parent.</div>
      {/if}
    </div>

    <div class="picker-footer">
      <button type="button" class="ok-btn" onclick={handleConfirm} disabled={loading || !!error || !resolvedPath || currentPath !== resolvedPath}>
        {loading ? 'Chargement…' : 'Sélectionner ce dossier'}
      </button>
    </div>
  </div>
</dialog>

<style>
  dialog.path-picker-dialog {
    width: min(760px, calc(100vw - 32px));
    max-width: none;
    max-height: calc(100dvh - 48px);
    padding: 0;
    overflow: hidden auto;
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    background: var(--bg-3);
    color: var(--tx);
    font-family: var(--font);
    box-shadow: var(--shadow);
  }

  dialog.path-picker-dialog::backdrop {
    background: rgb(6 8 14 / 0.55);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
  }

  .picker-header {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 20px 24px 0;
  }

  .back-btn {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border: 0;
    border-radius: 14px;
    background: var(--pn);
    color: var(--tx);
    cursor: pointer;
    transition: background-color var(--t-fast) ease;
  }

  .back-btn:hover {
    background: var(--pn-2);
  }

  h2 {
    margin: 0;
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.2;
    color: var(--tx);
  }

  .picker-body {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 20px 28px 28px;
  }

  .server-hint { margin: 0; font-size: 14px; line-height: 1.6; color: var(--mu); }

  label {
    display: block;
    margin-bottom: -8px;
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }

  .input-wrapper {
    display: flex;
    align-items: center;
    overflow: hidden;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: var(--pn);
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }

  .input-wrapper:hover {
    background: var(--pn-2);
  }

  .input-wrapper:focus-within {
    border-color: var(--ac);
    background: var(--bg-3);
  }

  .input-wrapper input {
    flex: 1;
    min-width: 0;
    height: 46px;
    padding: 0 14px;
    border: 0;
    background: transparent;
    color: var(--tx);
    font-family: var(--font-mono);
    font-size: 14px;
  }

  .input-wrapper input:focus {
    outline: none;
  }

  .search-btn {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 46px;
    height: 46px;
    border: 0;
    background: transparent;
    color: var(--mu);
    cursor: pointer;
    transition: color var(--t-fast) ease, background-color var(--t-fast) ease;
  }

  .search-btn:hover {
    color: var(--tx);
    background: var(--pn);
  }

  .search-btn:focus-visible { outline-offset: -3px; }

  .navigation-actions { display: flex; flex-wrap: wrap; gap: 8px; }

  .navigation-actions button {
    display: inline-flex;
    align-items: center;
    min-height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }

  .navigation-actions button:hover:not(:disabled) { background: var(--pn-3); }

  button:active:not(:disabled) { transform: scale(0.98); }

  button:disabled { opacity: 0.45; cursor: not-allowed; }

  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  .error-msg {
    margin: 0;
    font-size: 14px;
    color: var(--err);
  }

  .dirs-container {
    display: flex;
    flex-direction: column;
    max-height: min(340px, 45dvh);
    overflow-y: auto;
    overscroll-behavior: contain;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
  }

  .dirs-container.is-loading {
    opacity: 0.6;
    pointer-events: none;
  }

  .dir-item {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    min-height: 48px;
    padding: 0 16px;
    border: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--t-fast) ease;
  }

  .dir-item:last-child {
    border-bottom: 0;
  }

  .dir-item:hover {
    background: var(--pn);
  }

  .dir-item:focus-visible { outline-offset: -3px; }

  .parent-item { color: var(--mu); }

  .dir-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dir-arrow {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    margin-left: 12px;
    color: var(--mu);
  }

  .empty-hint {
    padding: 24px 16px;
    font-size: 14px;
    line-height: 1.6;
    text-align: center;
    color: var(--mu);
  }

  .picker-footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }

  .ok-btn {
    min-width: 140px;
    height: 48px;
    padding: 0 22px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--btn);
    color: var(--btn-tx);
    font: inherit;
    font-size: 15px;
    font-weight: 700;
    box-shadow: none;
    cursor: pointer;
    transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }

  .ok-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  @media (max-width: 640px) {
    dialog.path-picker-dialog {
      width: 100vw;
      max-height: calc(100dvh - 24px);
      margin: auto 0 0;
      border-width: 1px 0 0;
      border-radius: var(--r-xl) var(--r-xl) 0 0;
    }
    .picker-header { padding: 16px 16px 0; }
    h2 { font-size: 20px; }
    .picker-body { padding: 16px 16px calc(20px + env(safe-area-inset-bottom, 0px)); }
    .input-wrapper input { font-size: 16px; }
    .navigation-actions button { flex: 1; justify-content: center; }
    .ok-btn { width: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    .back-btn, .input-wrapper, .search-btn, .navigation-actions button, .dir-item, .ok-btn { transition: none; }
  }
</style>

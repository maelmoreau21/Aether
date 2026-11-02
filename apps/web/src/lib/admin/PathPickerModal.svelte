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
    width: min(1000px, calc(100vw - 32px));
    max-height: calc(100dvh - 48px);
    padding: 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background: var(--bg-surface);
    color: var(--text-primary);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.6);
  }

  dialog.path-picker-dialog::backdrop {
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(2px);
  }

  .picker-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 20px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    background: var(--bg-elevated);
  }

  .back-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: var(--text-primary);
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .back-btn:hover {
    background: rgba(255, 255, 255, 0.1);
  }

  h2 {
    font-size: 20px;
    font-weight: 500;
    margin: 0;
    color: var(--text-primary);
  }

  .picker-body {
    display: flex;
    flex-direction: column;
    padding: 24px;
    gap: 16px;
  }

  .server-hint { margin: 0; color: var(--text-secondary); font-size: 14px; line-height: 1.5; }
  .navigation-actions { display: flex; gap: 8px; flex-wrap: wrap; }
  .navigation-actions button { padding: 8px 12px; border: 1px solid var(--border-subtle); border-radius: var(--radius-md); background: var(--bg-elevated); color: var(--text-primary); cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }

  label {
    display: block;
    font-size: 14px;
    color: var(--text-secondary);
    margin-bottom: -8px;
  }

  .input-wrapper {
    display: flex;
    align-items: center;
    background: var(--bg-elevated);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: var(--radius-md);
    overflow: hidden;
    transition: border-color 0.15s ease;
  }

  .input-wrapper:focus-within {
    border-color: var(--color-play, #00a4dc);
  }

  .input-wrapper input {
    flex: 1;
    min-width: 0;
    height: 44px;
    padding: 0 14px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font: inherit;
    font-size: 14px;
  }

  .input-wrapper input:focus {
    outline: none;
  }

  .search-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }

  .search-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.05);
  }

  .error-msg {
    margin: 0;
    font-size: 13px;
    color: var(--status-error, #f44336);
  }

  .dirs-container {
    display: flex;
    flex-direction: column;
    max-height: 340px;
    overflow-y: auto;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
  }

  .dirs-container.is-loading {
    opacity: 0.6;
    pointer-events: none;
  }

  .dir-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 12px 16px;
    background: transparent;
    border: none;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    color: var(--text-primary);
    font: inherit;
    font-size: 14px;
    text-align: left;
    cursor: pointer;
    transition: background 0.12s ease;
  }

  .dir-item:last-child {
    border-bottom: none;
  }

  .dir-item:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .dir-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dir-arrow {
    display: inline-flex;
    align-items: center;
    color: var(--text-secondary);
    margin-left: 12px;
    flex-shrink: 0;
  }

  .empty-hint {
    padding: 24px 16px;
    text-align: center;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .picker-footer {
    display: flex;
    justify-content: center;
    margin-top: 8px;
  }

  .ok-btn {
    min-width: 140px;
    height: 42px;
    padding: 0 24px;
    border: none;
    border-radius: var(--radius-md);
    background: var(--color-play, #00a4dc);
    color: #fff;
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s ease, filter 0.15s ease;
  }

  .ok-btn:hover {
    filter: brightness(1.1);
  }
</style>

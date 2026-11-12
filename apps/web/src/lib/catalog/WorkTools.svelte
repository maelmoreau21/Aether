<script lang="ts">
  import ImageManager from './ImageManager.svelte';
  import ReportButton from '../applications/ReportButton.svelte';
  import IdentifyModal from './IdentifyModal.svelte';
  import MetadataEditorModal from './MetadataEditorModal.svelte';
  import SubtitleDownloads from './SubtitleDownloads.svelte';
  import WatchPartyModal from '../syncplay/WatchPartyModal.svelte';
  import SegmentsDialog from './SegmentsDialog.svelte';
  import { session } from '../auth/session';
  import { playbackIds, type Work } from './api';
  import type { Media } from '../player/types';
  import Icon from '../ui/Icon.svelte';
  import { dismissibleDetails } from '../ui/dismissible-details';
  import { t } from '../i18n';
  import { watchTogetherEnabled } from '../config/presentation';
  import { startWatchParty } from '../syncplay/launch';
  import { fetchAssetDetails, type AssetDetails } from './media-details';
  import WorkInfoModal from './WorkInfoModal.svelte';

  let {
    work,
    selectedMedia
  }: {
    work: Work;
    selectedMedia?: Media;
  } = $props();

  let imagesOpen = $state(false);
  let imagesChanged = $state(false);
  let metadataOpen = $state(false);
  let identifyOpen = $state(false);
  let subtitlesOpen = $state(false);
  let infoOpen = $state(false);
  let listOpen = $state(false);
  let menuOpen = $state(false);
  let watchPartyOpen = $state(false);
  let segmentsOpen = $state(false);

  let lists = $state<{ id: string; name: string; owned?: boolean }[]>([]);
  let listName = $state('');
  let error = $state('');
  let message = $state('');
  let busy = $state(false);
  let copiedKey = $state('');
  let detailsById = $state<Record<string, { status: 'loading' | 'ready' | 'error'; data?: AssetDetails; error?: string }>>({});
  let detailsController: AbortController | null = null;

  const isAdmin = $derived($session?.user.role === 'administrator');

  // Every indexed file of this work (versions and multi-Node copies), selected one included.
  const assetIds = $derived.by(() => {
    const ids = playbackIds(work);
    return selectedMedia && !ids.includes(selectedMedia.id) ? [selectedMedia.id, ...ids] : ids;
  });

  function openInfo() {
    infoOpen = true;
    void loadDetails();
  }

  function closeInfo() {
    infoOpen = false;
    detailsController?.abort();
    detailsController = null;
  }

  async function loadDetails(only?: string) {
    if (!only) detailsController?.abort();
    const controller = only && detailsController ? detailsController : new AbortController();
    detailsController = controller;
    const ids = only ? [only] : assetIds.filter(id => detailsById[id]?.status !== 'ready');
    for (const id of ids) detailsById[id] = { status: 'loading' };
    await Promise.all(ids.map(async id => {
      try {
        const data = await fetchAssetDetails(id, controller.signal);
        if (!controller.signal.aborted) detailsById[id] = { status: 'ready', data };
      } catch (cause) {
        if (!controller.signal.aborted) detailsById[id] = { status: 'error', error: cause instanceof Error ? cause.message : $t('information_unavailable') };
      }
    }));
  }

  async function loadLists() {
    try {
      const response = await fetch('/api/v1/lists');
      if (!response.ok) throw new Error($t('lists_unavailable'));
      lists = (await response.json()).lists.filter((list: { owned?: boolean }) => list.owned !== false);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('lists_unavailable');
    }
  }

  async function mutate(path: string, method: string, body?: unknown, contentType = 'application/json') {
    busy = true;
    error = '';
    message = '';
    try {
      const response = await fetch(path, {
        method,
        headers: {
          'content-type': contentType,
          'x-csrf-token': $session?.csrfToken ?? ''
        },
        body: body instanceof File ? body : body === undefined ? undefined : JSON.stringify(body)
      });
      if (!response.ok) throw new Error($t('catalog_saving_failed_http', { status: response.status }));
      message = $t('saved_successfully');
      return response.status === 204 ? true : await response.json();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
      return false;
    } finally {
      busy = false;
    }
  }

  async function createList() {
    if (!listName.trim()) return;
    const result = await mutate('/api/v1/lists', 'POST', { name: listName.trim() });
    if (result) {
      if (await mutate(`/api/v1/lists/${encodeURIComponent(result.id)}/items/${encodeURIComponent(work.id)}`, 'PUT')) {
        listName = '';
        await loadLists();
        message = $t('list_created_and_media_added');
      }
    }
  }

  function copyText(key: string, text: string) {
    if (!navigator?.clipboard) return;
    void navigator.clipboard.writeText(text).then(() => {
      copiedKey = key;
      setTimeout(() => { if (copiedKey === key) copiedKey = ''; }, 2000);
    }).catch(() => {});
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      menuOpen = false;
      if (infoOpen) closeInfo();
      if (metadataOpen) metadataOpen = false;
      if (listOpen) listOpen = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if watchPartyOpen && $watchTogetherEnabled}
  <WatchPartyModal
    mediaId={work.id}
    mediaTitle={work.title}
    onClose={() => (watchPartyOpen = false)}
    onJoinParty={(partyId) => {
      watchPartyOpen = false;
      startWatchParty({ partyId, workId: work.id });
    }}
  />
{/if}

{#if imagesOpen}
  <ImageManager
    title={work.title}
    workId={work.id}
    onclose={() => {
      imagesOpen = false;
      if (imagesChanged) {
        imagesChanged = false;
        window.dispatchEvent(new CustomEvent('media-artwork-updated', { detail: { id: work.id } }));
      }
    }}
    onchanged={async () => { imagesChanged = true; }}
  />
{/if}

{#if identifyOpen && isAdmin}
  <IdentifyModal
    {work}
    onclose={() => (identifyOpen = false)}
    onsaved={() => {
      window.dispatchEvent(new CustomEvent('media-artwork-updated', { detail: { id: work.id } }));
      window.location.reload();
    }}
  />
{/if}

{#if segmentsOpen && isAdmin}
  <SegmentsDialog {work} assetId={selectedMedia?.id ?? null} onclose={() => (segmentsOpen = false)} />
{/if}

{#if subtitlesOpen && isAdmin}
  <SubtitleDownloads {work} {selectedMedia} onclose={() => (subtitlesOpen = false)} />
{/if}

<!-- Actions bar (Add to playlist + Three dots menu) -->
<div class="work-tools-container">
  <ReportButton workId={work.id} title={work.title} />
  <!-- Quick Playlist Dropdown -->
  <details
    class="list-picker"
    use:dismissibleDetails
    bind:open={listOpen}
    ontoggle={event => {
      if (event.currentTarget.open) {
        void loadLists();
      }
    }}
  >
    <summary
      class="tool-btn icon-btn"
      aria-label={$t('add_to_playlist')}
      title={$t('add_to_playlist')}
    >
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path d="M12 4v16M4 12h16" />
      </svg>
    </summary>

    <div class="list-dropdown-menu">
      <div class="list-dropdown-header">
        <Icon name="sparkles" size={16} />
        <strong>{$t('add_to_playlist')}</strong>
      </div>

      <div class="playlists-grid">
        {#each lists as list (list.id)}
          <button
            type="button"
            class="playlist-item-btn"
            disabled={busy}
            onclick={async () => {
              await mutate(`/api/v1/lists/${encodeURIComponent(list.id)}/items/${encodeURIComponent(work.id)}`, 'PUT');
              listOpen = false;
            }}
          >
            <span>{list.name}</span>
            <Icon name="check" size={16} />
          </button>
        {:else}
          <p class="empty-list-notice">{$t('no_playlists')}</p>
        {/each}
      </div>

      <form
        class="new-list-form"
        onsubmit={event => {
          event.preventDefault();
          void createList();
        }}
      >
        <label>{$t('new_playlist')}<input required bind:value={listName} maxlength="100" placeholder={$t('new_playlist')} /></label>
        <button type="submit" class="btn primary-btn" disabled={busy || !listName.trim()}>
          {$t('create_and_add')}
        </button>
      </form>
    </div>
  </details>

  {#if isAdmin}
    <button
      type="button"
      class="tool-btn icon-btn edit-meta-btn"
      class:active={metadataOpen}
      aria-label={$t('edit_metadata')}
      title={$t('edit_metadata')}
      onclick={() => (metadataOpen = true)}
    >
      <Icon name="edit" size={18} />
    </button>
  {/if}

  <!-- Three dots menu trigger -->
  <div class="menu-dropdown-wrapper">
    <button
      type="button"
      class="tool-btn icon-btn more-btn"
      class:active={menuOpen}
      aria-label={$t('more_actions')}
      title={$t('media_options')}
      aria-haspopup="menu"
      aria-expanded={menuOpen}
      onclick={() => (menuOpen = !menuOpen)}
    >
      <Icon name="more" size={20} />
    </button>

    {#if menuOpen}
      <button
        type="button"
        class="backdrop-dismiss"
        aria-label={$t('close_menu')}
        tabindex="-1"
        onclick={() => (menuOpen = false)}
      ></button>

      <div class="dropdown-menu" role="menu">
        {#if $watchTogetherEnabled}
        <button
          type="button"
          class="menu-item"
          role="menuitem"
          onclick={() => {
            menuOpen = false;
            watchPartyOpen = true;
          }}
        >
          <Icon name="users" size={16} />
          <span>{$t('watch_together')}</span>
        </button>
        {/if}

        <button
          type="button"
          class="menu-item"
          role="menuitem"
          onclick={() => {
            menuOpen = false;
            openInfo();
          }}
        >
          <Icon name="info" size={16} />
          <span>{$t('media_information')}</span>
        </button>

        <button
          type="button"
          class="menu-item"
          role="menuitem"
          onclick={() => {
            menuOpen = false;
            listOpen = true;
            void loadLists();
          }}
        >
          <Icon name="sparkles" size={16} />
          <span>{$t('add_to_a_playlist')}</span>
        </button>

        {#if isAdmin}
          <div class="menu-divider" role="separator"></div>
          <div class="menu-section-header">{$t('media_management')}</div>

          {#if work.kind !== 'series' && !work.synthetic}
            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={() => {
                menuOpen = false;
                subtitlesOpen = true;
              }}
            >
              <Icon name="search" size={16} />
              <span>{$t('search_subtitles')}</span>
            </button>
          {/if}

          {#if !work.synthetic && (work.kind === 'series' ? (work.seasons?.length ?? 0) > 0 : assetIds.length > 0)}
            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={() => {
                menuOpen = false;
                segmentsOpen = true;
              }}
            >
              <Icon name="film" size={16} />
              <span>{$t('segments_tab')}</span>
            </button>
          {/if}

          <button
            type="button"
            class="menu-item"
            role="menuitem"
            onclick={() => {
              menuOpen = false;
              imagesOpen = true;
            }}
          >
            <Icon name="image" size={16} />
            <span>{$t('edit_artwork')}</span>
          </button>

          <button
            type="button"
            class="menu-item"
            role="menuitem"
            onclick={() => {
              menuOpen = false;
              identifyOpen = true;
            }}
          >
            <Icon name="search" size={16} />
            <span>{$t('identify_media')}</span>
          </button>

          <button
            type="button"
            class="menu-item"
            role="menuitem"
            onclick={() => {
              menuOpen = false;
              metadataOpen = true;
            }}
          >
            <Icon name="edit" size={16} />
            <span>{$t('edit_metadata')}</span>
          </button>
        {/if}
      </div>
    {/if}
  </div>

  {#if error}<p class="status-msg error" role="alert">{error}</p>{/if}
  {#if message}<p class="status-msg success" role="status">{message}</p>{/if}
</div>

<!-- Modal: Informations média (fichiers par version : Media Node, chemin, pistes, conteneur) -->
{#if infoOpen}
  <WorkInfoModal {work} {selectedMedia} {copiedKey} {detailsById} {assetIds} {closeInfo} {copyText} {loadDetails} />
{/if}

<!-- Modal: Modifier les métadonnées (Admin) -->
{#if metadataOpen && isAdmin}
  <MetadataEditorModal
    {work}
    onclose={() => (metadataOpen = false)}
    onsaved={() => {
      window.location.reload();
    }}
  />
{/if}

<style>
  .work-tools-container {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    position: relative;
    z-index: 40;
    vertical-align: middle;
  }

  /* Details list-picker */
  details.list-picker {
    position: relative;
    display: inline-block;
  }

  details.list-picker summary {
    list-style: none;
  }

  details.list-picker summary::-webkit-details-marker {
    display: none;
  }

  /* Trigger buttons: same icon button as the hero action row */
  .tool-btn {
    display: inline-grid;
    place-items: center;
    width: 44px;
    height: 44px;
    min-height: 44px;
    box-sizing: border-box;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 14px;
    background: var(--pn);
    color: var(--tx);
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .tool-btn:hover {
    background: var(--pn-2);
  }

  .tool-btn:active {
    transform: scale(0.98);
  }

  .tool-btn:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  .tool-btn.active,
  details.list-picker[open] > .tool-btn {
    background: var(--pn-3);
  }

  .menu-dropdown-wrapper {
    position: relative;
  }

  .backdrop-dismiss {
    position: fixed;
    inset: 0;
    z-index: 290;
    background: transparent;
    border: none;
    cursor: default;
  }

  /* Menus */
  .dropdown-menu,
  .list-dropdown-menu {
    position: absolute;
    top: calc(100% + 8px);
    left: 0;
    z-index: 300;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    background: var(--bg-3);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    box-shadow: var(--shadow);
    color: var(--tx);
  }

  .dropdown-menu {
    min-width: 260px;
  }

  .list-dropdown-menu {
    min-width: 290px;
    max-width: min(340px, calc(100vw - 32px));
  }

  /* Téléphone : les boutons passent à la ligne, un menu ancré à son bouton sortait
     de l'écran. Feuille fixée au-dessus de la barre de navigation mobile. */
  @media (max-width: 760px) {
    .dropdown-menu,
    .list-dropdown-menu {
      position: fixed;
      top: auto;
      left: 12px;
      right: 12px;
      bottom: calc(78px + env(safe-area-inset-bottom, 0px));
      min-width: 0;
      max-width: none;
      max-height: min(70dvh, 520px);
      overflow-y: auto;
      overscroll-behavior: contain;
    }
  }

  .list-dropdown-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 10px;
    font-size: 15px;
    font-weight: 700;
    color: var(--tx);
  }

  .list-dropdown-header :global(svg) {
    color: var(--mu);
  }

  .menu-divider {
    height: 1px;
    background: var(--line);
    margin: 6px 4px;
  }

  .menu-section-header {
    padding: 8px 12px 4px;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--mu);
  }

  .menu-item,
  .playlist-item-btn {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 42px;
    padding: 0 12px;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: var(--tx);
    font-size: 14px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }

  .menu-item :global(svg),
  .playlist-item-btn :global(svg) {
    color: var(--mu);
    flex-shrink: 0;
  }

  .menu-item:hover,
  .playlist-item-btn:hover:not(:disabled) {
    background: var(--pn);
  }

  .menu-item:focus-visible,
  .playlist-item-btn:focus-visible {
    background: var(--pn);
    outline: 2px solid var(--ac);
    outline-offset: -2px;
  }

  /* Playlists */
  .playlists-grid {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 220px;
    overflow-y: auto;
  }

  .playlist-item-btn {
    justify-content: space-between;
  }

  .playlist-item-btn span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty-list-notice {
    margin: 4px 0;
    padding: 8px 12px;
    color: var(--mu);
    font-size: 13px;
  }

  .new-list-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 6px;
    padding: 12px 6px 6px;
    border-top: 1px solid var(--line);
  }

  .new-list-form label {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }

  .new-list-form label input {
    width: 100%;
  }

  input {
    box-sizing: border-box;
    width: 100%;
    min-height: 46px;
    padding: 0 14px;
    background: var(--pn);
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    box-shadow: none;
    transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease);
  }

  input:focus {
    outline: none;
    border-color: var(--ac);
    background: var(--bg-3);
    box-shadow: none;
  }

  /* Buttons */
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 18px;
    border-radius: var(--r-md);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    border: 1px solid transparent;
    transition: background-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease), opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .btn:active:not(:disabled) {
    transform: scale(0.98);
  }

  .primary-btn {
    background: var(--btn);
    color: var(--btn-tx);
  }

  .primary-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .status-msg {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
    margin: 0;
  }

  .status-msg.error { color: var(--err); }

  .status-msg.success { color: var(--ok); }
</style>

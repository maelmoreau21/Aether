<script lang="ts">
  import ImageManager from './ImageManager.svelte';
  import ReportButton from '../applications/ReportButton.svelte';
  import IdentifyModal from './IdentifyModal.svelte';
  import MetadataEditorModal from './MetadataEditorModal.svelte';
  import SubtitleDownloads from './SubtitleDownloads.svelte';
  import WatchPartyModal from '../syncplay/WatchPartyModal.svelte';
  import { session } from '../auth/session';
  import { playbackIds, type Work } from './api';
  import type { Media } from '../player/types';
  import Icon from '../ui/Icon.svelte';
  import { dismissibleDetails } from '../ui/dismissible-details';
  import { containerLabel } from '../player/track-labels';
  import LanguageLabel from '../ui/LanguageLabel.svelte';
  import { t, fmt } from '../i18n';
  import { startWatchParty } from '../syncplay/launch';
  import {
    audioSummary,
    codecLabel,
    fetchAssetDetails,
    formatBitrate,
    formatBytes,
    formatDuration,
    groupTracks,
    nodeKindLabel,
    nodeStatusLabel,
    trackRoles,
    versionLabel,
    videoSummary,
    type AssetDetails
  } from './media-details';

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

{#if watchPartyOpen}
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
          <span>{$t('watch_party')}</span>
        </button>

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
  <div class="modal-overlay" role="dialog" aria-modal="true" aria-labelledby="media-info-title">
    <button
      type="button"
      class="modal-backdrop-click"
      aria-label={$t('close_the_window')}
      tabindex="-1"
      onclick={closeInfo}
    ></button>
    <div class="modal-card wide info-card">
      <div class="modal-header">
        <div class="modal-title-group">
          <Icon name="info" size={20} />
          <h3 id="media-info-title">{$t('media_information')}</h3>
        </div>
        <button
          type="button"
          class="close-modal-btn"
          aria-label={$t('close')}
          onclick={closeInfo}
        >
          <Icon name="close" size={18} />
        </button>
      </div>

      <div class="modal-body info-body">
        {#if assetIds.length === 0}
          <p class="info-note">
            {work.kind === 'series'
              ? $t('files_are_described_episode_by_episode')
              : $t('no_indexed_file_is_available_for')}
          </p>
        {:else if assetIds.length > 1}
          <p class="info-note">{$t('files_available_for_this_media_item', { count: assetIds.length })}</p>
        {/if}

        {#each assetIds as id, position (id)}
          {@const entry = detailsById[id]}
          {@const info = entry?.data}
          <details class="version-card" open={id === (assetIds.includes(selectedMedia?.id ?? '') ? selectedMedia?.id : assetIds[0])}>
            <summary>
              <span class="version-title">{versionLabel(info, position)}</span>
              {#if id === selectedMedia?.id && assetIds.length > 1}<span class="tag-status online">{$t('selected')}</span>{/if}
              {#if info}
                {@const status = nodeStatusLabel(info.node.status)}
                <span class="version-node"><Icon name="server" size={14} />{info.node.name}<span class="status-dot {status.tone}" role="img" aria-label={status.label}></span></span>
              {/if}
            </summary>

            {#if !entry || entry.status === 'loading'}
              <p class="info-note" role="status">{$t('loading_file_information')}</p>
            {:else if entry.status === 'error'}
              <div class="info-error-row" role="alert">
                <span>{entry.error}</span>
                <button type="button" class="btn small outline-btn" onclick={() => void loadDetails(id)}>{$t('request_retry')}</button>
              </div>
            {:else if info}
              {@const status = nodeStatusLabel(info.node.status)}
              {@const groups = groupTracks(info.tracks)}
              <div class="version-body">
                <section class="info-section" aria-label={$t('file_location')}>
                  <h4>{$t('file_location')}</h4>
                  <dl class="info-grid">
                    <div class="info-item">
                      <dt class="info-label">{$t('media_node')}</dt>
                      <dd class="info-value">
                        {info.node.name}
                        <span class="tag-status {status.tone}">{status.label}</span>
                        <span class="info-sub">{nodeKindLabel(info.node.kind)} · <code>{info.node.id}</code>{info.node.version ? ` · v${info.node.version}` : ''}</span>
                      </dd>
                    </div>
                    <div class="info-item">
                      <dt class="info-label">{$t('library')}</dt>
                      <dd class="info-value">{info.library.name}</dd>
                    </div>
                    {#if info.pathsVisible && info.file.absolutePath}
                      <div class="info-item full">
                        <dt class="info-label">{$t('path_on_the_media_node')}</dt>
                        <dd class="info-value path-row">
                          <code class="path">{info.file.absolutePath}</code>
                          <button type="button" class="copy-btn" aria-label={$t('copy_full_path')} onclick={() => copyText(`abs-${id}`, info.file.absolutePath ?? '')}>
                            <Icon name={copiedKey === `abs-${id}` ? 'check' : 'copy'} size={14} />
                            <span>{copiedKey === `abs-${id}` ? 'Copié' : 'Copier'}</span>
                          </button>
                        </dd>
                      </div>
                      <div class="info-item full">
                        <dt class="info-label">{$t('path_in_the_library')}</dt>
                        <dd class="info-value path-row">
                          <code class="path">{info.file.relativePath}</code>
                          <button type="button" class="copy-btn" aria-label={$t('copy_relative_path')} onclick={() => copyText(`rel-${id}`, info.file.relativePath ?? '')}>
                            <Icon name={copiedKey === `rel-${id}` ? 'check' : 'copy'} size={14} />
                            <span>{copiedKey === `rel-${id}` ? 'Copié' : 'Copier'}</span>
                          </button>
                        </dd>
                        <span class="info-sub">{$t('library_root')} <code>{info.file.libraryRoot}</code></span>
                      </div>
                    {:else}
                      <div class="info-item full">
                        <dt class="info-label">{$t('path')}</dt>
                        <dd class="info-value muted">{$t('admin_only_path')}</dd>
                      </div>
                    {/if}
                    {#if !info.available}
                      <div class="info-item full"><dd class="info-value info-error">{$t('missing_file')}</dd></div>
                    {/if}
                  </dl>
                </section>

                <section class="info-section" aria-label={$t('video_tracks')}>
                  <h4>{$t('catalog_video', { count: groups.video.length })}</h4>
                  <ul class="track-list">
                    {#each groups.video as track (track.index)}
                      <li class="track-row">
                        <div class="track-head">
                          <span class="track-index">#{track.index}</span>
                          <strong>{codecLabel(track)}</strong>
                          {#if track.title}<span class="track-title">{track.title}</span>{/if}
                        </div>
                        <div class="track-chips">
                          {#each videoSummary(track) as part (part)}<span class="track-chip">{part}</span>{/each}
                          {#each trackRoles(track.disposition) as role (role)}<span class="track-chip role">{role}</span>{/each}
                        </div>
                      </li>
                    {:else}
                      <li class="track-row muted">{$t('no_video_track_analysed')}</li>
                    {/each}
                  </ul>
                </section>

                <section class="info-section" aria-label={$t('audio_tracks')}>
                  <h4>{$t('catalog_audio', { value: groups.audio.length + info.externalAudio.length })}</h4>
                  <ul class="track-list">
                    {#each groups.audio as track (track.index)}
                      <li class="track-row">
                        <div class="track-head">
                          <span class="track-index">#{track.index}</span>
                          <LanguageLabel language={track.language ?? 'und'} />
                          <strong>{codecLabel(track)}</strong>
                          {#if track.title}<span class="track-title">{track.title}</span>{/if}
                        </div>
                        <div class="track-chips">
                          {#each audioSummary(track) as part (part)}<span class="track-chip">{part}</span>{/each}
                          {#each trackRoles(track.disposition) as role (role)}<span class="track-chip role">{role}</span>{/each}
                        </div>
                      </li>
                    {/each}
                    {#each info.externalAudio as audio, index (index)}
                      <li class="track-row">
                        <div class="track-head">
                          <span class="track-index">{$t('external')}</span>
                          <LanguageLabel language={audio.language} />
                          {#if audio.track}<strong>{codecLabel(audio.track)}</strong>{/if}
                        </div>
                        <div class="track-chips">
                          {#if audio.track}{#each audioSummary(audio.track) as part (part)}<span class="track-chip">{part}</span>{/each}{/if}
                          <span class="track-chip">{formatBytes(audio.size)}</span>
                          {#if audio.offsetSeconds}<span class="track-chip">{$t('audio_offset_chip', { value: $fmt.number(audio.offsetSeconds) })}</span>{/if}
                        </div>
                        {#if audio.absolutePath}<code class="path small">{audio.absolutePath}</code>{/if}
                      </li>
                    {/each}
                    {#if groups.audio.length + info.externalAudio.length === 0}
                      <li class="track-row muted">{$t('no_audio_track_analysed')}</li>
                    {/if}
                  </ul>
                </section>

                <section class="info-section" aria-label={$t('subtitle_tracks_info')}>
                  <h4>{$t('catalog_subtitles', { value: groups.subtitle.length + info.externalSubtitles.length })}</h4>
                  <ul class="track-list">
                    {#each groups.subtitle as track (track.index)}
                      <li class="track-row">
                        <div class="track-head">
                          <span class="track-index">#{track.index}</span>
                          <LanguageLabel language={track.language ?? 'und'} />
                          <strong>{codecLabel(track)}</strong>
                          {#if track.title}<span class="track-title">{track.title}</span>{/if}
                        </div>
                        {#if trackRoles(track.disposition).length}
                          <div class="track-chips">
                            {#each trackRoles(track.disposition) as role (role)}<span class="track-chip role">{role}</span>{/each}
                          </div>
                        {/if}
                      </li>
                    {/each}
                    {#each info.externalSubtitles as subtitle, index (index)}
                      <li class="track-row">
                        <div class="track-head">
                          <span class="track-index">{subtitle.origin === 'downloaded' ? 'Téléchargé' : 'Externe'}</span>
                          <LanguageLabel language={subtitle.language} />
                          {#if subtitle.sourceFormat}<strong>{subtitle.sourceFormat.toUpperCase()}</strong>{/if}
                          {#if subtitle.label}<span class="track-title">{subtitle.label}</span>{/if}
                        </div>
                        {#if subtitle.forced || subtitle.sdh}
                          <div class="track-chips">
                            {#if subtitle.forced}<span class="track-chip role">{$t('forced')}</span>{/if}
                            {#if subtitle.sdh}<span class="track-chip role">{$t('hard_of_hearing_sdh')}</span>{/if}
                          </div>
                        {/if}
                      </li>
                    {/each}
                    {#if groups.subtitle.length + info.externalSubtitles.length === 0}
                      <li class="track-row muted">{$t('no_subtitles')}</li>
                    {/if}
                  </ul>
                </section>

                {#if groups.other.length}
                  <section class="info-section" aria-label={$t('other_streams')}>
                    <h4>{$t('catalog_other_streams', { count: groups.other.length })}</h4>
                    <ul class="track-list">
                      {#each groups.other as track (track.index)}
                        <li class="track-row">
                          <div class="track-head">
                            <span class="track-index">#{track.index}</span>
                            <strong>{codecLabel(track)}</strong>
                            <span class="track-title">{track.codecType ?? 'flux'}{track.title ? ` · ${track.title}` : ''}</span>
                          </div>
                        </li>
                      {/each}
                    </ul>
                  </section>
                {/if}

                <section class="info-section" aria-label={$t('container_and_technical_details')}>
                  <h4>{$t('container_and_technical_details')}</h4>
                  <dl class="info-grid">
                    <div class="info-item">
                      <dt class="info-label">{$t('container')}</dt>
                      <dd class="info-value">
                        {containerLabel(info.file.mime)}
                        {#if info.container.formatLongName}<span class="info-sub">{info.container.formatLongName}{info.container.formatName ? ` (${info.container.formatName})` : ''}</span>{/if}
                      </dd>
                    </div>
                    <div class="info-item">
                      <dt class="info-label">{$t('size')}</dt>
                      <dd class="info-value">{formatBytes(info.file.size)}</dd>
                    </div>
                    <div class="info-item">
                      <dt class="info-label">{$t('sched_duration')}</dt>
                      <dd class="info-value">{formatDuration(info.container.durationSeconds) ?? '—'}</dd>
                    </div>
                    <div class="info-item">
                      <dt class="info-label">{$t('overall_bitrate')}</dt>
                      <dd class="info-value">{formatBitrate(info.container.bitRate) ?? '—'}</dd>
                    </div>
                    <div class="info-item">
                      <dt class="info-label">{$t('chapters')}</dt>
                      <dd class="info-value">{info.chapterCount || 'Aucun'}</dd>
                    </div>
                    {#if info.file.modifiedAt}
                      <div class="info-item">
                        <dt class="info-label">{$t('modified_on')}</dt>
                        <dd class="info-value">{$fmt.dateTime(info.file.modifiedAt)}</dd>
                      </div>
                    {/if}
                    <div class="info-item">
                      <dt class="info-label">{$t('mime_type')}</dt>
                      <dd class="info-value"><code>{info.file.mime}</code></dd>
                    </div>
                    <div class="info-item full">
                      <dt class="info-label">{$t('file_identifier')}</dt>
                      <dd class="info-value path-row">
                        <code class="path">{info.assetId}</code>
                        <button type="button" class="copy-btn" aria-label={$t('copy_the_file_identifier')} onclick={() => copyText(`asset-${id}`, info.assetId)}>
                          <Icon name={copiedKey === `asset-${id}` ? 'check' : 'copy'} size={14} />
                          <span>{copiedKey === `asset-${id}` ? 'Copié' : 'Copier'}</span>
                        </button>
                      </dd>
                    </div>
                  </dl>
                </section>
              </div>
            {/if}
          </details>
        {/each}

        <section class="info-section" aria-label={$t('catalogue_and_provenance')}>
          <h4>{$t('catalogue_and_provenance')}</h4>
          <dl class="info-grid">
            <div class="info-item">
              <dt class="info-label">{$t('metadata_provenance')}</dt>
              <dd class="info-value">{work.metadataProvenance === 'nfo' ? $t('local_nfo_file') : $t('automatic_indexing')}</dd>
            </div>
            {#if work.addedAt}
              <div class="info-item">
                <dt class="info-label">{$t('added_on')}</dt>
                <dd class="info-value">{$fmt.date(work.addedAt)}</dd>
              </div>
            {/if}
            {#if work.premiered}
              <div class="info-item">
                <dt class="info-label">{$t('first_aired')}</dt>
                <dd class="info-value">{work.premiered}</dd>
              </div>
            {/if}
            {#if work.rating}
              <div class="info-item">
                <dt class="info-label">{$t('rating')}</dt>
                <dd class="info-value">{work.rating.toFixed(1)} / 10</dd>
              </div>
            {/if}
            {#if work.mpaa}
              <div class="info-item">
                <dt class="info-label">{$t('classification')}</dt>
                <dd class="info-value">{work.mpaa}</dd>
              </div>
            {/if}
            <div class="info-item full">
              <dt class="info-label">{$t('work_identifier')}</dt>
              <dd class="info-value path-row">
                <code class="path">{work.id}</code>
                <button type="button" class="copy-btn" aria-label={$t('copy_the_work_identifier')} onclick={() => copyText('work', work.id)}>
                  <Icon name={copiedKey === 'work' ? 'check' : 'copy'} size={14} />
                  <span>{copiedKey === 'work' ? 'Copié' : 'Copier'}</span>
                </button>
              </dd>
            </div>
            {#if work.globalMediaId}
              <div class="info-item full">
                <dt class="info-label">{$t('global_identity')}</dt>
                <dd class="info-value"><code class="path">{work.globalMediaId}</code></dd>
              </div>
            {/if}
          </dl>
        </section>
      </div>

      <div class="modal-footer">
        <button type="button" class="btn primary-btn" onclick={closeInfo}>{$t('close')}</button>
      </div>
    </div>
  </div>
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

  @media (max-width: 720px) {
    .dropdown-menu,
    .list-dropdown-menu {
      left: auto;
      right: 0;
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

  /* Modals */
  .modal-overlay {
    position: fixed;
    inset: 0;
    z-index: 2000;
    display: grid;
    place-items: center;
    padding: 24px;
  }

  .modal-backdrop-click {
    position: absolute;
    inset: 0;
    background: rgb(6 8 14 / 0.55);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
    border: none;
    cursor: default;
  }

  .modal-card {
    position: relative;
    z-index: 1;
    width: 100%;
    max-width: 600px;
    max-height: calc(100dvh - 48px);
    overflow-y: auto;
    overscroll-behavior: contain;
    background: var(--bg-3);
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow);
    color: var(--tx);
    display: flex;
    flex-direction: column;
  }

  .modal-card.wide {
    max-width: 800px;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 24px 28px 4px;
  }

  .modal-title-group {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    color: var(--mu);
  }

  .modal-title-group h3 {
    margin: 0;
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    color: var(--tx);
  }

  .close-modal-btn {
    display: grid;
    place-items: center;
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
    border: none;
    border-radius: 14px;
    background: var(--pn);
    color: var(--tx);
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }

  .close-modal-btn:hover {
    background: var(--pn-2);
  }

  .modal-body {
    padding: 20px 28px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 10px;
    padding: 8px 28px 24px;
  }

  .form-body .modal-footer {
    padding: 8px 0 4px;
    justify-content: space-between;
  }

  .modal-footer-right {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 10px;
    margin-left: auto;
  }

  /* Info modal styles */
  .modal-card.info-card {
    max-width: 860px;
  }

  .info-note {
    margin: 0;
    color: var(--mu);
    font-size: 14px;
    line-height: 1.5;
  }

  .version-card {
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    background: var(--pn);
    overflow: hidden;
  }

  .version-card > summary {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 12px;
    min-height: 52px;
    padding: 12px 16px;
    cursor: pointer;
    list-style: none;
  }

  .version-card > summary::-webkit-details-marker {
    display: none;
  }

  .version-card > summary::before {
    content: '';
    flex: none;
    width: 8px;
    height: 8px;
    border-right: 2px solid var(--mu);
    border-bottom: 2px solid var(--mu);
    transform: rotate(-45deg);
    transition: transform var(--t-fast) var(--ease);
  }

  .version-card[open] > summary::before {
    transform: rotate(45deg);
  }

  .version-card > summary:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: -2px;
  }

  .version-title {
    font-size: 15px;
    font-weight: 750;
    color: var(--tx);
    overflow-wrap: anywhere;
  }

  .version-node {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
    font-size: 13px;
    font-weight: 600;
    color: var(--mu);
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--err);
  }

  .status-dot.online { background: var(--ok); }
  .status-dot.revoked { background: var(--warn); }

  .version-body {
    display: flex;
    flex-direction: column;
    gap: 22px;
    padding: 6px 16px 18px;
    border-top: 1px solid var(--line);
    background: var(--bg-3);
  }

  .version-card > .info-note,
  .info-error-row {
    padding: 4px 16px 16px;
  }

  .info-error-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    color: var(--err);
    font-size: 14px;
  }

  .info-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }

  .info-section h4 {
    margin: 0;
    padding: 12px 0 8px;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--mu);
    border-bottom: 1px solid var(--line);
  }

  .info-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px 20px;
    margin: 0;
  }

  .info-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    font-size: 14px;
  }

  .info-item.full {
    grid-column: 1 / -1;
  }

  .info-label {
    font-size: 13px;
    color: var(--mu);
  }

  .info-value {
    margin: 0;
    color: var(--tx);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .info-value.muted,
  .track-row.muted {
    color: var(--mu);
    font-weight: 500;
  }

  .info-error {
    color: var(--err);
  }

  .info-sub {
    display: block;
    margin-top: 2px;
    color: var(--mu);
    font-size: 12.5px;
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .info-value code,
  .info-sub code,
  code.path {
    font-family: var(--font-mono);
    font-size: 12.5px;
    font-weight: 500;
  }

  .path-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  code.path {
    flex: 1 1 auto;
    min-width: 0;
    padding: 8px 10px;
    border-radius: var(--r-sm);
    background: var(--pn);
    color: var(--tx);
    line-height: 1.45;
    overflow-wrap: anywhere;
    word-break: break-all;
    user-select: all;
  }

  code.path.small {
    display: block;
    margin-top: 8px;
    font-size: 12px;
  }

  .copy-btn {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
    min-height: 36px;
    padding: 0 12px;
    font-size: 13px;
    font-weight: 700;
    border-radius: var(--r-sm);
    background: var(--pn);
    border: 1px solid transparent;
    color: var(--tx);
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }

  .copy-btn:hover {
    background: var(--pn-2);
  }

  .copy-btn:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 2px;
  }

  .tag-status {
    display: inline-block;
    padding: 2px 8px;
    border-radius: var(--r-pill);
    font-size: 12px;
    font-weight: 700;
    margin-left: 6px;
  }

  .tag-status.online {
    background: var(--ok-soft);
    color: var(--ok);
  }

  .tag-status.offline {
    background: var(--err-soft);
    color: var(--err);
  }

  .tag-status.revoked {
    background: var(--warn-soft);
    color: var(--warn);
  }

  .version-card > summary .tag-status {
    margin-left: 0;
  }

  .track-list {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .track-row {
    padding: 10px 12px;
    border-radius: var(--r-md);
    background: var(--pn);
    font-size: 14px;
    min-width: 0;
  }

  .track-head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    color: var(--tx);
    overflow-wrap: anywhere;
  }

  .track-index {
    padding: 1px 7px;
    border-radius: var(--r-pill);
    background: var(--pn-2);
    color: var(--mu);
    font-size: 11.5px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .track-title {
    color: var(--mu);
    font-weight: 500;
  }

  .track-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }

  .track-chip {
    padding: 4px 10px;
    border-radius: var(--r-pill);
    background: var(--bg-3);
    border: 1px solid var(--line);
    color: var(--tx);
    font-size: 12.5px;
    font-weight: 600;
  }

  .track-chip.role {
    background: var(--ac-soft);
    border-color: transparent;
    color: var(--ac);
  }

  /* TV and very large screens: readable from a distance. */
  @media (min-width: 1600px) {
    .modal-card.info-card { max-width: 1040px; }
    .info-item, .track-row, .info-note { font-size: 16px; }
    .info-label, .info-section h4 { font-size: 14px; }
    .track-chip, .info-value code, code.path { font-size: 14px; }
    .version-title { font-size: 18px; }
  }

  /* Form styles */
  .form-fieldset {
    border: none;
    margin: 0;
    padding: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .form-grid {
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    gap: 16px;
  }

  .col-12 { grid-column: span 12; }
  .col-8 { grid-column: span 8; }
  .col-6 { grid-column: span 6; }
  .col-4 { grid-column: span 4; }

  .form-field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .field-label {
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }

  input, textarea {
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

  textarea {
    padding: 12px 14px;
    line-height: 1.5;
    resize: vertical;
  }

  input:focus, textarea:focus {
    outline: none;
    border-color: var(--ac);
    background: var(--bg-3);
    box-shadow: none;
  }

  .code-area {
    font-family: var(--font-mono);
    font-size: 13px;
  }

  .form-divider {
    border: none;
    border-top: 1px solid var(--line);
    margin: 8px 0;
  }

  .subhead {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
  }

  .copy-group-row {
    display: flex;
    gap: 10px;
    align-items: flex-end;
    flex-wrap: wrap;
  }

  .flex-grow { flex: 1 1 240px; }
  .self-end { align-self: flex-end; }

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

  .secondary-btn {
    background: var(--pn);
    color: var(--tx);
  }

  .secondary-btn:hover:not(:disabled) {
    background: var(--pn-2);
  }

  .outline-btn {
    background: transparent;
    border-color: var(--line-2);
    color: var(--tx);
  }

  .outline-btn:hover:not(:disabled) {
    background: var(--pn);
  }

  .danger-btn {
    background: var(--err-soft);
    color: var(--err);
  }

  .danger-btn:hover:not(:disabled) {
    background: var(--pn-2);
  }

  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .btn.small {
    min-height: 36px;
    padding: 0 12px;
    font-size: 13px;
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

  .candidates-box {
    padding: 14px 16px;
    border-radius: var(--r-md);
    background: var(--pn);
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 14px;
  }

  .candidate-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    font-size: 13px;
  }

  .candidate-row code {
    font-family: var(--font-mono);
  }

  .candidate-actions {
    display: flex;
    gap: 6px;
  }

  .merges-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    border: 0;
  }

  @media (max-width: 639px) {
    .modal-overlay {
      padding: 0;
      place-items: stretch;
    }

    .modal-card,
    .modal-card.wide {
      max-width: none;
      max-height: 100dvh;
      height: 100dvh;
      border: 0;
      border-radius: 0;
    }

    .modal-header { padding: 20px 16px 4px; }
    .modal-body { padding: 16px; }
    .modal-footer { padding: 8px 16px calc(20px + env(safe-area-inset-bottom, 0px)); }
    .form-body .modal-footer { padding: 8px 0 calc(8px + env(safe-area-inset-bottom, 0px)); }

    .info-grid { grid-template-columns: minmax(0, 1fr); }
    .info-item.full { grid-column: auto; }
    .path-row { flex-wrap: wrap; }
    .path-row code.path { flex-basis: 100%; }
    .version-node { margin-left: 0; flex-basis: 100%; }
    .version-body { padding: 4px 12px 14px; }

    .col-8, .col-6, .col-4 { grid-column: span 12; }
  }
</style>

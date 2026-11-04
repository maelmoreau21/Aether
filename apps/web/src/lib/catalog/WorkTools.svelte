<script lang="ts">
  import ImageManager from './ImageManager.svelte';
  import IdentifyModal from './IdentifyModal.svelte';
  import SubtitleDownloads from './SubtitleDownloads.svelte';
  import WatchPartyModal from '../syncplay/WatchPartyModal.svelte';
  import { session } from '../auth/session';
  import type { Work } from './api';
  import type { Media } from '../player/types';
  import Icon from '../ui/Icon.svelte';
  import {
    videoResolutionBadge,
    containerLabel,
    colorProfileLabel,
    inspectedAudioTracks
  } from '../player/track-labels';
  import LanguageLabel from '../ui/LanguageLabel.svelte';

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
  let copiedId = $state(false);

  // Form metadata states
  let title = $state('');
  let plot = $state('');
  let year = $state<number | undefined>();
  let genres = $state('');
  let tags = $state('');
  let ids = $state('{}');
  let age = $state<number | undefined>();
  let copyId = $state('');

  let candidates = $state<{
    id: string;
    leftWorkId: string;
    leftTitle: string;
    rightWorkId: string;
    rightTitle: string;
    score: number;
    reason: string;
    status: string;
  }[]>([]);
  let merges = $state<{ id: string; groupId: string; origin: string }[]>([]);

  $effect(() => {
    title = work.title;
    plot = (work.plot ?? work.description ?? '').replace(/^Média local indexé$/i, '');
    year = work.year ?? undefined;
    genres = (work.genres ?? []).join(', ');
    tags = (work.tags ?? []).join(', ');
    ids = JSON.stringify(work.externalIds ?? {}, null, 2);
    age = work.minimumAge ?? undefined;
  });

  const isAdmin = $derived($session?.user.role === 'administrator');

  const currentVideoTrack = $derived(
    selectedMedia?.sources[0]?.tracks?.find(t => t.codec_type === 'video')
  );
  const currentAudioTracks = $derived(selectedMedia ? inspectedAudioTracks(selectedMedia) : []);

  async function loadLists() {
    try {
      const response = await fetch('/api/v1/lists');
      if (!response.ok) throw new Error('Listes indisponibles.');
      lists = (await response.json()).lists.filter((list: { owned?: boolean }) => list.owned !== false);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Listes indisponibles.';
    }
  }

  async function loadCandidates() {
    try {
      const response = await fetch('/api/v1/admin/media-groups/candidates?status=pending');
      if (!response.ok) return;
      const data = await response.json();
      candidates = (data.candidates ?? []).filter(
        (c: { leftWorkId: string; rightWorkId: string }) => c.leftWorkId === work.id || c.rightWorkId === work.id
      );
      merges = (data.merges ?? []).filter(
        (m: { groupId: string }) => m.groupId === work.id || m.groupId === work.globalMediaId
      );
    } catch {}
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
      if (!response.ok) throw new Error(`Enregistrement impossible (HTTP ${response.status}).`);
      message = 'Enregistré avec succès.';
      return response.status === 204 ? true : await response.json();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Enregistrement impossible.';
      return false;
    } finally {
      busy = false;
    }
  }

  async function saveMetadata(event: SubmitEvent) {
    event.preventDefault();
    let externalIds;
    try {
      externalIds = JSON.parse(ids);
    } catch {
      error = 'Les identifiants doivent être un objet JSON valide.';
      return;
    }
    const success = await mutate(`/api/v1/admin/works/${work.id}/metadata`, 'PUT', {
      title,
      plot,
      year: year ?? null,
      genres: genres.split(',').map(s => s.trim()).filter(Boolean),
      tags: tags.split(',').map(s => s.trim()).filter(Boolean),
      externalIds,
      minimumAge: age ?? null
    });
    if (success) {
      window.location.reload();
    }
  }

  async function createList() {
    if (!listName.trim()) return;
    const result = await mutate('/api/v1/lists', 'POST', { name: listName.trim() });
    if (result) {
      if (await mutate(`/api/v1/lists/${encodeURIComponent(result.id)}/items/${encodeURIComponent(work.id)}`, 'PUT')) {
        listName = '';
        await loadLists();
        message = 'Liste créée et média ajouté.';
      }
    }
  }

  function copyWorkId() {
    if (navigator?.clipboard) {
      void navigator.clipboard.writeText(work.id).then(() => {
        copiedId = true;
        setTimeout(() => { copiedId = false; }, 2000);
      });
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      menuOpen = false;
      if (infoOpen) infoOpen = false;
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
      window.dispatchEvent(new CustomEvent('start-watch-party', { detail: { partyId, workId: work.id } }));
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
  <!-- Quick Playlist Dropdown -->
  <details
    class="list-picker"
    bind:open={listOpen}
    ontoggle={event => {
      if (event.currentTarget.open) {
        void loadLists();
      }
    }}
  >
    <summary
      class="tool-btn icon-btn"
      aria-label="Ajouter à une playlist"
      title="Ajouter à une playlist"
    >
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path d="M12 4v16M4 12h16" />
      </svg>
    </summary>

    <div class="list-dropdown-menu">
      <div class="list-dropdown-header">
        <Icon name="sparkles" size={16} />
        <strong>Ajouter à une playlist</strong>
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
          <p class="empty-list-notice">Aucune playlist existante.</p>
        {/each}
      </div>

      <form
        class="new-list-form"
        onsubmit={event => {
          event.preventDefault();
          void createList();
        }}
      >
        <label>Nouvelle liste<input required bind:value={listName} maxlength="100" placeholder="Nom de la liste" /></label>
        <button type="submit" class="btn primary-btn" disabled={busy || !listName.trim()}>
          Créer et ajouter
        </button>
      </form>
    </div>
  </details>

  <!-- Three dots menu trigger -->
  <div class="menu-dropdown-wrapper">
    <button
      type="button"
      class="tool-btn icon-btn more-btn"
      class:active={menuOpen}
      aria-label="Plus d'actions et informations"
      title="Options et informations du média"
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
        aria-label="Fermer le menu"
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
          <span>Séance Watch Party</span>
        </button>

        <button
          type="button"
          class="menu-item"
          role="menuitem"
          onclick={() => {
            menuOpen = false;
            infoOpen = true;
          }}
        >
          <Icon name="info" size={16} />
          <span>Informations média</span>
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
          <span>Ajouter à une playlist…</span>
        </button>

        {#if isAdmin}
          <div class="menu-divider" role="separator"></div>
          <div class="menu-section-header">Gestion du média · admin</div>

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
              <span>Rechercher des sous-titres</span>
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
            <span>Modifier les images</span>
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
            <span>Identifier</span>
          </button>

          <button
            type="button"
            class="menu-item"
            role="menuitem"
            onclick={() => {
              menuOpen = false;
              metadataOpen = true;
              void loadCandidates();
            }}
          >
            <Icon name="edit" size={16} />
            <span>Modifier les métadonnées</span>
          </button>
        {/if}
      </div>
    {/if}
  </div>

  {#if error}<p class="status-msg error" role="alert">{error}</p>{/if}
  {#if message}<p class="status-msg success" role="status">{message}</p>{/if}
</div>

<!-- Modal: Informations média -->
{#if infoOpen}
  <div class="modal-overlay" role="dialog" aria-modal="true" aria-labelledby="media-info-title">
    <button
      type="button"
      class="modal-backdrop-click"
      aria-label="Fermer la fenêtre"
      tabindex="-1"
      onclick={() => (infoOpen = false)}
    ></button>
    <div class="modal-card">
      <div class="modal-header">
        <div class="modal-title-group">
          <Icon name="info" size={20} />
          <h3 id="media-info-title">Informations média</h3>
        </div>
        <button
          type="button"
          class="close-modal-btn"
          aria-label="Fermer"
          onclick={() => (infoOpen = false)}
        >
          <Icon name="close" size={18} />
        </button>
      </div>

      <div class="modal-body info-body">
        <div class="info-section">
          <h4>Emplacement et Media Node</h4>
          <div class="info-grid">
            <div class="info-item">
              <span class="info-label">Nœud source :</span>
              <span class="info-value">
                <code>{work.nodeId ?? 'local'}</code>
                {#if work.available === false}
                  <span class="tag-status offline">Hors ligne</span>
                {:else}
                  <span class="tag-status online">Connecté</span>
                {/if}
              </span>
            </div>
            <div class="info-item">
              <span class="info-label">Bibliothèque :</span>
              <span class="info-value">{work.libraryId}</span>
            </div>
            <div class="info-item full">
              <span class="info-label">Identifiant copie :</span>
              <span class="info-value id-row">
                <code>{work.id}</code>
                <button
                  type="button"
                  class="copy-btn"
                  title="Copier l'identifiant"
                  aria-label="Copier l'identifiant"
                  onclick={copyWorkId}
                >
                  <Icon name="copy" size={14} />
                  <span>{copiedId ? 'Copié !' : 'Copier'}</span>
                </button>
              </span>
            </div>
            {#if work.globalMediaId}
              <div class="info-item full">
                <span class="info-label">Identité globale :</span>
                <span class="info-value"><code>{work.globalMediaId}</code></span>
              </div>
            {/if}
          </div>
        </div>

        <div class="info-section">
          <h4>Catalogue et Provenance</h4>
          <div class="info-grid">
            <div class="info-item">
              <span class="info-label">Provenance métadonnées :</span>
              <span class="info-value">{work.metadataProvenance === 'nfo' ? 'Fichier NFO local' : 'Indexation automatique'}</span>
            </div>
            {#if work.addedAt}
              <div class="info-item">
                <span class="info-label">Ajouté le :</span>
                <span class="info-value">{new Date(work.addedAt * 1000).toLocaleDateString('fr-FR')}</span>
              </div>
            {/if}
            {#if work.premiered}
              <div class="info-item">
                <span class="info-label">Première diffusion :</span>
                <span class="info-value">{work.premiered}</span>
              </div>
            {/if}
            {#if work.rating}
              <div class="info-item">
                <span class="info-label">Note :</span>
                <span class="info-value">{work.rating.toFixed(1)} / 10</span>
              </div>
            {/if}
            {#if work.mpaa}
              <div class="info-item">
                <span class="info-label">Classification :</span>
                <span class="info-value">{work.mpaa}</span>
              </div>
            {/if}
          </div>
        </div>

        {#if selectedMedia}
          <div class="info-section">
            <h4>Pistes et spécifications techniques</h4>
            <div class="info-grid">
              <div class="info-item">
                <span class="info-label">Conteneur :</span>
                <span class="info-value">{containerLabel(selectedMedia.sources[0]?.mime)}</span>
              </div>
              <div class="info-item">
                <span class="info-label">Vidéo :</span>
                <span class="info-value">
                  {currentVideoTrack?.codec_name?.toUpperCase() ?? 'Inconnu'}
                  {#if currentVideoTrack?.width && currentVideoTrack?.height}
                    · {currentVideoTrack.width} × {currentVideoTrack.height}
                  {/if}
                  {#if videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)}
                    ({videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)})
                  {/if}
                  {#if colorProfileLabel(currentVideoTrack?.pix_fmt)}
                    · {colorProfileLabel(currentVideoTrack?.pix_fmt)}
                  {/if}
                </span>
              </div>
              <div class="info-item full">
                <span class="info-label">Pistes audio ({currentAudioTracks.length}) :</span>
                <div class="audio-list">
                  {#each currentAudioTracks as at, idx (idx)}
                    <span class="track-chip">
                      <LanguageLabel language={at.language} /> · {at.codec_name?.toUpperCase() ?? 'Inconnu'}{at.channel_layout || at.channels ? ` · ${at.channel_layout ?? `${at.channels} canaux`}` : ''}{at.tags?.title ? ` (${at.tags.title})` : ''}
                    </span>
                  {:else}
                    <span class="info-value">Aucune piste audio analysée</span>
                  {/each}
                </div>
              </div>
              {#if selectedMedia.subtitles.length > 0}
                <div class="info-item full">
                  <span class="info-label">Sous-titres ({selectedMedia.subtitles.length}) :</span>
                  <div class="sub-list">
                    {#each selectedMedia.subtitles as sub (sub.id)}
                      <span class="track-chip">
                        <LanguageLabel language={sub.language} />{sub.sourceFormat ? ` · ${sub.sourceFormat.toUpperCase()}` : ''}{sub.forced ? ' (Forcés)' : ''}{sub.sdh ? ' (SDH)' : ''}
                      </span>
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>

      <div class="modal-footer">
        <button type="button" class="btn primary-btn" onclick={() => (infoOpen = false)}>Fermer</button>
      </div>
    </div>
  </div>
{/if}

<!-- Modal: Modifier les métadonnées (Admin) -->
{#if metadataOpen && isAdmin}
  <div class="modal-overlay" role="dialog" aria-modal="true" aria-labelledby="metadata-edit-title">
    <button
      type="button"
      class="modal-backdrop-click"
      aria-label="Fermer la fenêtre"
      tabindex="-1"
      onclick={() => (metadataOpen = false)}
    ></button>
    <div class="modal-card wide">
      <div class="modal-header">
        <div class="modal-title-group">
          <Icon name="edit" size={20} />
          <h3 id="metadata-edit-title">Modifier les métadonnées</h3>
        </div>
        <button
          type="button"
          class="close-modal-btn"
          aria-label="Fermer"
          onclick={() => (metadataOpen = false)}
        >
          <Icon name="close" size={18} />
        </button>
      </div>

      <form onsubmit={saveMetadata} class="modal-body form-body">
        <fieldset disabled={busy} class="form-fieldset">
          <legend class="sr-only">Formulaire métadonnées</legend>

          <div class="form-grid">
            <label class="form-field col-8">
              <span class="field-label">Titre</span>
              <input required maxlength="512" bind:value={title} />
            </label>

            <label class="form-field col-4">
              <span class="field-label">Année</span>
              <input type="number" min="1800" max="2200" bind:value={year} />
            </label>

            <label class="form-field col-12">
              <span class="field-label">Description / Synopsis</span>
              <textarea bind:value={plot} maxlength="20000" rows="4"></textarea>
            </label>

            <label class="form-field col-6">
              <span class="field-label">Genres (séparés par des virgules)</span>
              <input bind:value={genres} placeholder="Action, Aventure, Drame" />
            </label>

            <label class="form-field col-6">
              <span class="field-label">Tags (séparés par des virgules)</span>
              <input bind:value={tags} maxlength="6400" />
            </label>

            <label class="form-field col-8">
              <span class="field-label">Identifiants externes (JSON : tmdb, imdb…)</span>
              <textarea bind:value={ids} rows="3" class="code-area"></textarea>
            </label>

            <label class="form-field col-4">
              <span class="field-label">Âge minimum (vide = non classé)</span>
              <input type="number" min="0" max="21" bind:value={age} />
            </label>
          </div>

          {#if work.kind === 'film' || work.kind === 'series'}
            <hr class="form-divider" />
            <h4 class="subhead">Regrouper des copies multi-nœuds</h4>
            <div class="copy-group-row">
              <label class="form-field flex-grow">
                <span class="field-label">Identifiant exact de la même œuvre sur un autre nœud ou bibliothèque</span>
                <input bind:value={copyId} placeholder="Identifiant Work ID" />
              </label>
              <button
                type="button"
                class="btn secondary-btn self-end"
                disabled={!copyId || copyId === work.id || busy}
                onclick={async () => {
                  if (await mutate('/api/v1/admin/media-groups', 'POST', { workIds: [work.id, copyId], label: work.title })) {
                    window.location.reload();
                  }
                }}
              >
                Associer les deux œuvres
              </button>
              <button
                type="button"
                class="btn danger-btn self-end"
                disabled={busy}
                onclick={async () => {
                  if (await mutate(`/api/v1/admin/media-groups/${work.id}`, 'DELETE')) {
                    window.location.reload();
                  }
                }}
              >
                Dissocier cette copie
              </button>
            </div>

            {#if merges.length > 0}
              <div class="merges-list">
                {#each merges as merge (merge.id)}
                  <button
                    type="button"
                    class="btn outline-btn"
                    onclick={async () => {
                      if (await mutate(`/api/v1/admin/media-groups/merges/${encodeURIComponent(merge.id)}/undo`, 'POST')) {
                        window.location.reload();
                      }
                    }}
                  >
                    Annuler la fusion ({merge.origin})
                  </button>
                {/each}
              </div>
            {/if}

            {#if candidates.length > 0}
              <div class="candidates-box">
                <strong>Correspondances probables trouvées :</strong>
                {#each candidates as candidate (candidate.id)}
                  <div class="candidate-row">
                    <span><code>{candidate.leftWorkId === work.id ? candidate.rightTitle : candidate.leftTitle}</code> ({candidate.score}% · {candidate.reason})</span>
                    <div class="candidate-actions">
                      <button
                        type="button"
                        class="btn small primary-btn"
                        onclick={async () => {
                          if (await mutate(`/api/v1/admin/media-groups/candidates/${encodeURIComponent(candidate.id)}/confirm`, 'POST')) {
                            window.location.reload();
                          }
                        }}
                      >
                        Confirmer
                      </button>
                      <button
                        type="button"
                        class="btn small outline-btn"
                        onclick={async () => {
                          if (await mutate(`/api/v1/admin/media-groups/candidates/${encodeURIComponent(candidate.id)}/reject`, 'POST')) {
                            await loadCandidates();
                          }
                        }}
                      >
                        Rejeter
                      </button>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
        </fieldset>

        <div class="modal-footer">
          <button
            type="button"
            class="btn outline-btn"
            disabled={busy}
            onclick={async () => {
              if (await mutate(`/api/v1/admin/works/${work.id}/metadata`, 'DELETE')) {
                window.location.reload();
              }
            }}
          >
            Revenir aux métadonnées du scan
          </button>
          <div class="modal-footer-right">
            <button type="button" class="btn secondary-btn" onclick={() => (metadataOpen = false)}>
              Annuler
            </button>
            <button type="submit" class="btn primary-btn" disabled={busy}>
              {busy ? 'Enregistrement…' : 'Enregistrer'}
            </button>
          </div>
        </div>
      </form>
    </div>
  </div>
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
  .info-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .info-section h4 {
    margin: 0;
    padding-bottom: 8px;
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
  }

  .info-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    font-size: 14px;
  }

  .info-item.full {
    grid-column: span 2;
  }

  .info-label {
    font-size: 13px;
    color: var(--mu);
  }

  .info-value {
    color: var(--tx);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .info-value code {
    font-family: var(--font-mono);
    font-size: 12.5px;
    font-weight: 500;
  }

  .id-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }

  .copy-btn {
    display: inline-flex;
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

  .audio-list, .sub-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 4px;
  }

  .track-chip {
    padding: 5px 10px;
    border-radius: var(--r-pill);
    background: var(--pn);
    color: var(--tx);
    font-size: 13px;
    font-weight: 600;
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

    .col-8, .col-6, .col-4 { grid-column: span 12; }
  }
</style>

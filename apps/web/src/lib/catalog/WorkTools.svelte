<script lang="ts">
  import ImageManager from './ImageManager.svelte';
  import IdentifyModal from './IdentifyModal.svelte';
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
  let infoOpen = $state(false);
  let listOpen = $state(false);
  let menuOpen = $state(false);

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
            infoOpen = true;
          }}
        >
          <Icon name="info" size={16} />
          <span>Informations média</span>
        </button>

        {#if isAdmin}
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
    margin-block: 8px;
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

  .list-dropdown-menu {
    position: absolute;
    top: calc(100% + 8px);
    left: 0;
    min-width: 280px;
    background: rgba(24, 24, 27, 0.95);
    backdrop-filter: blur(16px);
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5), 0 2px 6px rgba(0, 0, 0, 0.3);
    padding: 14px;
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .list-dropdown-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 600;
    color: var(--color-text-bright);
    padding-bottom: 4px;
    border-bottom: 1px solid var(--color-line);
  }

  .new-list-form {
    border-top: 1px solid var(--color-line);
    padding-top: 10px;
    margin-top: 4px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .new-list-form label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    color: var(--color-muted);
  }

  .new-list-form label input {
    width: 100%;
  }

  .tool-btn {
    display: inline-grid;
    place-items: center;
    width: 44px;
    height: 44px;
    box-sizing: border-box;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    color: var(--color-text-bright);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .tool-btn:hover {
    color: var(--color-accent);
    border-color: var(--color-accent);
    background: var(--color-surface-hover);
    transform: translateY(-1px);
  }

  .tool-btn:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  .tool-btn.active {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .menu-dropdown-wrapper {
    position: relative;
  }

  .backdrop-dismiss {
    position: fixed;
    inset: 0;
    z-index: 99;
    background: transparent;
    border: none;
    cursor: default;
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 8px);
    left: 0;
    min-width: 230px;
    background: rgba(24, 24, 27, 0.95);
    backdrop-filter: blur(16px);
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5), 0 2px 6px rgba(0, 0, 0, 0.3);
    padding: 6px;
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 10px 14px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-bright);
    font-size: 13.5px;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .menu-item:hover, .menu-item:focus-visible {
    background: var(--color-surface-hover);
    color: var(--color-accent);
    outline: none;
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
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(8px);
    border: none;
    cursor: default;
  }

  .modal-card {
    position: relative;
    z-index: 1;
    width: 100%;
    max-width: 580px;
    max-height: 90vh;
    overflow-y: auto;
    background: var(--color-glass-raised);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
  }

  .modal-card.wide {
    max-width: 780px;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 18px 24px;
    border-bottom: 1px solid var(--color-line);
  }

  .modal-title-group {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--color-accent);
  }

  .modal-title-group h3 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    color: var(--color-text-bright);
  }

  .close-modal-btn {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-muted);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .close-modal-btn:hover {
    color: var(--color-text-bright);
    background: var(--color-surface);
  }

  .modal-body {
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-top: 1px solid var(--color-line);
    background: rgba(0, 0, 0, 0.2);
  }

  .modal-footer-right {
    display: flex;
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
    font-size: 13.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-accent);
    border-bottom: 1px solid var(--color-line);
    padding-bottom: 6px;
  }

  .info-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
  }

  .info-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13.5px;
  }

  .info-item.full {
    grid-column: span 2;
  }

  .info-label {
    font-size: 12px;
    color: var(--color-muted);
  }

  .info-value {
    color: var(--color-text-bright);
    word-break: break-all;
  }

  .id-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .copy-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    font-size: 11.5px;
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    color: var(--color-text-bright);
    cursor: pointer;
  }

  .copy-btn:hover {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .tag-status {
    display: inline-block;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    font-weight: 600;
    margin-left: 6px;
  }

  .tag-status.online {
    background: rgba(34, 197, 94, 0.2);
    color: #4ade80;
  }

  .tag-status.offline {
    background: rgba(239, 68, 68, 0.2);
    color: #f87171;
  }

  .audio-list, .sub-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 4px;
  }

  .track-chip {
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    font-size: 12px;
  }

  /* Form styles */
  .form-fieldset {
    border: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .form-grid {
    display: grid;
    grid-template-columns: repeat(12, 1fr);
    gap: 14px;
  }

  .col-12 { grid-column: span 12; }
  .col-8 { grid-column: span 8; }
  .col-6 { grid-column: span 6; }
  .col-4 { grid-column: span 4; }

  .form-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-muted);
  }

  input, textarea {
    padding: 10px 12px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    color: var(--color-text-bright);
    font: inherit;
    font-size: 13.5px;
    transition: border-color var(--transition-fast);
  }

  input:focus, textarea:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .code-area {
    font-family: monospace;
    font-size: 12px;
  }

  .form-divider {
    border: none;
    border-top: 1px solid var(--color-line);
    margin: 12px 0;
  }

  .subhead {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
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
    padding: 9px 16px;
    border-radius: var(--radius-sm);
    font-size: 13.5px;
    font-weight: 500;
    cursor: pointer;
    border: 1px solid transparent;
    transition: all var(--transition-fast);
  }

  .primary-btn {
    background: var(--color-accent);
    color: #fff;
  }

  .primary-btn:hover:not(:disabled) {
    filter: brightness(1.1);
    transform: translateY(-1px);
  }

  .secondary-btn {
    background: var(--color-surface);
    border-color: var(--color-line);
    color: var(--color-text-bright);
  }

  .secondary-btn:hover:not(:disabled) {
    background: var(--color-surface-hover);
  }

  .outline-btn {
    background: transparent;
    border-color: var(--color-line);
    color: var(--color-muted);
  }

  .outline-btn:hover:not(:disabled) {
    color: var(--color-text-bright);
    border-color: var(--color-line-strong);
  }

  .danger-btn {
    background: rgba(239, 68, 68, 0.15);
    border-color: rgba(239, 68, 68, 0.3);
    color: #fca5a5;
  }

  .danger-btn:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.3);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Playlists */
  .playlists-grid {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 200px;
    overflow-y: auto;
  }

  .playlist-item-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    color: var(--color-text-bright);
    font-size: 13.5px;
    cursor: pointer;
    text-align: left;
    transition: all var(--transition-fast);
  }

  .playlist-item-btn:hover {
    border-color: var(--color-accent);
    color: var(--color-accent);
    background: var(--color-surface-hover);
  }



  .empty-list-notice {
    color: var(--color-muted);
    font-size: 13.5px;
    margin: 8px 0;
  }

  .status-msg {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    font-size: 12px;
    white-space: nowrap;
    margin: 0;
  }

  .status-msg.error { color: var(--status-error); }
  .status-msg.success { color: var(--status-success, #4ade80); }

  .candidates-box {
    padding: 12px;
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .candidate-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 12.5px;
  }

  .candidate-actions {
    display: flex;
    gap: 6px;
  }

  .btn.small {
    padding: 4px 10px;
    font-size: 12px;
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
</style>

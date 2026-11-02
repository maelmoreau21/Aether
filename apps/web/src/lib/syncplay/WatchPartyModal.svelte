<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { session } from '../auth/session';
  import type { ControlMode, PartySummary, UserInvitePayload } from './types';

  interface SelectedInvitee {
    id?: string;
    username: string;
    displayName: string;
  }

  interface CatalogWorkOption {
    id: string;
    title: string;
    year?: number | null;
  }

  let {
    mediaId,
    mediaTitle,
    onClose,
    onJoinParty
  }: {
    mediaId?: string;
    mediaTitle?: string;
    onClose: () => void;
    onJoinParty: (partyId: string, mediaId?: string) => void;
  } = $props();

  let selectedMediaId = $state<string | undefined>(undefined);
  let selectedMediaTitle = $state<string | undefined>(undefined);

  $effect(() => {
    selectedMediaId = mediaId;
    selectedMediaTitle = mediaTitle;
  });

  let mode = $state<'create' | 'list'>('create');
  let customPartyName = $state<string | null>(null);
  let partyName = $derived(
    customPartyName ?? (selectedMediaTitle ? `Séance : ${selectedMediaTitle}` : 'Ma séance privée')
  );
  let controlMode = $state<ControlMode>('host_only');
  let waitForBuffering = $state(true);

  // Invitees management (by pseudo & display name)
  let invitees = $state<SelectedInvitee[]>([]);
  let userSearchQuery = $state('');
  let userSearchResults = $state<{ id: string; login: string; displayName: string }[]>([]);
  let isSearchingUsers = $state(false);

  // Media selector (if opened without mediaId)
  let mediaSearchQuery = $state('');
  let mediaSearchResults = $state<CatalogWorkOption[]>([]);
  let isSearchingMedia = $state(false);

  let activeParties = $state<PartySummary[]>([]);
  let userInvites = $state<UserInvitePayload[]>([]);

  let loading = $state(false);
  let error = $state<string | null>(null);

  onMount(async () => {
    await Promise.all([loadParties(), loadInvites()]);
    if (!selectedMediaId && activeParties.length > 0) {
      mode = 'list';
    } else if (!selectedMediaId) {
      void searchCatalogMedia('');
    }
  });

  async function searchUsers(q: string) {
    if (!q.trim()) {
      userSearchResults = [];
      return;
    }
    isSearchingUsers = true;
    try {
      const cleanQ = q.trim().replace(/^@/, '');
      const mediaParam = selectedMediaId ? `&media_id=${encodeURIComponent(selectedMediaId)}` : '';
      const res = await fetch(
        `/api/v1/parties/eligible-users?q=${encodeURIComponent(cleanQ)}${mediaParam}`,
        { credentials: 'same-origin' }
      );
      if (res.ok) {
        const data = await res.json();
        userSearchResults = data.users ?? [];
      }
    } catch {
      // Ignored
    } finally {
      isSearchingUsers = false;
    }
  }

  function addInvitee(user: { id?: string; login: string; displayName?: string }) {
    const cleanLogin = user.login.trim().replace(/^@/, '');
    if (!cleanLogin) return;

    const exists = invitees.some(
      (i) => i.username.toLowerCase() === cleanLogin.toLowerCase() || (user.id && i.id === user.id)
    );
    if (!exists) {
      invitees = [
        ...invitees,
        {
          id: user.id,
          username: cleanLogin,
          displayName: user.displayName || cleanLogin
        }
      ];
    }
    userSearchQuery = '';
    userSearchResults = [];
  }

  function handleUserKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (userSearchResults.length > 0) {
        addInvitee(userSearchResults[0]);
      } else if (userSearchQuery.trim()) {
        addInvitee({ login: userSearchQuery.trim() });
      }
    }
  }

  function removeInvitee(username: string) {
    invitees = invitees.filter((i) => i.username.toLowerCase() !== username.toLowerCase());
  }

  async function searchCatalogMedia(q: string) {
    isSearchingMedia = true;
    try {
      const url = `/api/v1/catalog?${new URLSearchParams({ q: q.trim(), limit: '6' })}`;
      const res = await fetch(url, { credentials: 'same-origin' });
      if (res.ok) {
        const data = await res.json();
        mediaSearchResults = (data.items ?? []).map((w: CatalogWorkOption) => ({
          id: w.id,
          title: w.title,
          year: w.year
        }));
      }
    } catch {
      // Ignored
    } finally {
      isSearchingMedia = false;
    }
  }

  async function loadParties() {
    try {
      const res = await fetch('/api/v1/parties', { credentials: 'same-origin' });
      if (res.ok) {
        const data = await res.json();
        activeParties = data.parties ?? [];
      }
    } catch {}
  }

  async function loadInvites() {
    try {
      const res = await fetch('/api/v1/parties/invites', { credentials: 'same-origin' });
      if (res.ok) {
        const data = await res.json();
        userInvites = data.invites ?? [];
      }
    } catch {}
  }

  async function handleCreateParty() {
    if (!selectedMediaId) {
      error = 'Veuillez sélectionner une œuvre ou un épisode à regarder.';
      return;
    }
    loading = true;
    error = null;

    try {
      const userIds = invitees.map((i) => i.id).filter((id): id is string => Boolean(id));
      const usernames = invitees.filter((i) => !i.id).map((i) => i.username);

      const res = await fetch('/api/v1/parties', {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          'Content-Type': 'application/json',
          'x-csrf-token': $session?.csrfToken ?? ''
        },
        body: JSON.stringify({
          name: partyName.trim(),
          mediaId: selectedMediaId,
          controlMode,
          waitForBuffering,
          inviteUserIds: userIds.length > 0 ? userIds : undefined,
          inviteUsernames: usernames.length > 0 ? usernames : undefined
        })
      });

      if (!res.ok) {
        let msg = 'Impossible de créer la séance.';
        try {
          const body = await res.json();
          if (body.error) msg = body.error;
        } catch {}
        throw new Error(msg);
      }

      const party = await res.json();
      onJoinParty(party.id, selectedMediaId);
      onClose();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Erreur inconnue';
    } finally {
      loading = false;
    }
  }

  async function handleAcceptInvite(token: string, pId: string, mId: string) {
    loading = true;
    error = null;
    try {
      const res = await fetch(`/api/v1/parties/invites/${encodeURIComponent(token)}/accept`, {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          'x-csrf-token': $session?.csrfToken ?? ''
        }
      });
      if (res.ok) {
        onJoinParty(pId, mId);
        onClose();
      } else {
        let msg = "L'invitation a expiré ou est invalide.";
        try {
          const body = await res.json();
          if (body.error) msg = body.error;
        } catch {}
        throw new Error(msg);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : 'Erreur lors de l’acceptation';
    } finally {
      loading = false;
    }
  }

  async function handleJoin(pId: string, mId: string) {
    loading = true;
    error = null;
    try {
      const res = await fetch(`/api/v1/parties/${encodeURIComponent(pId)}/join`, {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          'x-csrf-token': $session?.csrfToken ?? ''
        }
      });
      if (res.ok) {
        onJoinParty(pId, mId);
        onClose();
      } else {
        let msg = 'Impossible de rejoindre cette séance.';
        try {
          const body = await res.json();
          if (body.error) msg = body.error;
        } catch {}
        throw new Error(msg);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : 'Erreur lors de l’accès';
    } finally {
      loading = false;
    }
  }
</script>

<div class="modal-backdrop" role="dialog" aria-modal="true" aria-label="Séances Watch Party">
  <div class="modal-card">
    <header class="modal-header">
      <div class="title-with-icon">
        <Icon name="users" size={24} />
        <div>
          <h2>Watch Party Aether</h2>
          <span class="sub-badge">Synchronisation multi-utilisateurs</span>
        </div>
      </div>
      <button type="button" class="btn-close" aria-label="Fermer" onclick={onClose}>
        <Icon name="close" size={20} />
      </button>
    </header>

    <!-- Navigation Tabs -->
    <nav class="modal-tabs">
      <button
        type="button"
        class="tab-btn"
        class:active={mode === 'create'}
        onclick={() => (mode = 'create')}
      >
        Créer une séance
      </button>
      <button
        type="button"
        class="tab-btn"
        class:active={mode === 'list'}
        onclick={() => (mode = 'list')}
      >
        Séances en cours ({activeParties.length})
      </button>
      {#if userInvites.length > 0}
        <button
          type="button"
          class="tab-btn badge-tab"
          class:active={mode === 'list'}
          onclick={() => (mode = 'list')}
        >
          Invitations reçues
          <span class="count-pill">{userInvites.length}</span>
        </button>
      {/if}
    </nav>

    {#if error}
      <div class="alert-error" role="alert">
        <Icon name="alert" size={16} />
        <span>{error}</span>
      </div>
    {/if}

    <div class="modal-body">
      {#if mode === 'create'}
        <!-- Security & Privacy Guarantee Banner -->
        <div class="privacy-banner">
          <div class="privacy-icon">🔒</div>
          <div class="privacy-text">
            <strong>Séance privée sur invitation uniquement</strong>
            <p>
              Chaque utilisateur invité regarde avec ses propres réglages audio (VF/VO), ses
              sous-titres et son débit, tout en restant parfaitement synchronisé.
            </p>
          </div>
        </div>

        <form
          class="create-form"
          onsubmit={(e) => {
            e.preventDefault();
            void handleCreateParty();
          }}
        >
          <!-- Media Selection if not preselected -->
          {#if !mediaId}
            <div class="field-group">
              <label for="media-search">Média à regarder</label>
              {#if selectedMediaId && selectedMediaTitle}
                <div class="selected-media-chip">
                  <span class="media-title">🎬 {selectedMediaTitle}</span>
                  <button
                    type="button"
                    class="btn-change-media"
                    onclick={() => {
                      selectedMediaId = undefined;
                      selectedMediaTitle = undefined;
                    }}
                  >
                    Changer
                  </button>
                </div>
              {:else}
                <div class="search-input-wrapper">
                  <input
                    id="media-search"
                    type="text"
                    bind:value={mediaSearchQuery}
                    oninput={() => void searchCatalogMedia(mediaSearchQuery)}
                    placeholder="Rechercher un film ou une série dans votre catalogue…"
                    autocomplete="off"
                  />
                  {#if isSearchingMedia}
                    <span class="spinner-small"></span>
                  {/if}
                </div>
                {#if mediaSearchResults.length > 0}
                  <div class="autocomplete-dropdown">
                    {#each mediaSearchResults as m (m.id)}
                      <button
                        type="button"
                        class="dropdown-row"
                        onclick={() => {
                          selectedMediaId = m.id;
                          selectedMediaTitle = m.title;
                          mediaSearchResults = [];
                          mediaSearchQuery = '';
                        }}
                      >
                        <span class="row-title">{m.title}</span>
                        {#if m.year}
                          <span class="row-year">({m.year})</span>
                        {/if}
                      </button>
                    {/each}
                  </div>
                {/if}
              {/if}
            </div>
          {:else if selectedMediaTitle}
            <div class="media-preview-bar">
              <Icon name="film" size={16} />
              <span>Programme : <strong>{selectedMediaTitle}</strong></span>
            </div>
          {/if}

          <!-- Party Name -->
          <div class="field-group">
            <label for="party-name">Nom de la séance</label>
            <input
              id="party-name"
              type="text"
              required
              value={partyName}
              oninput={(e) => {
                customPartyName = e.currentTarget.value;
              }}
              placeholder="Ex: Soirée cinéma entre amis"
            />
          </div>

          <!-- Invite users by username (@pseudo) -->
          <div class="field-group">
            <label for="invite-pseudo">Inviter des amis par pseudo</label>
            <div class="pseudo-input-row">
              <div class="search-input-wrapper">
                <input
                  id="invite-pseudo"
                  type="text"
                  bind:value={userSearchQuery}
                  oninput={() => void searchUsers(userSearchQuery)}
                  onkeydown={handleUserKeyDown}
                  placeholder="Tapez un @pseudo ou un nom…"
                  autocomplete="off"
                />
                {#if isSearchingUsers}
                  <span class="spinner-small"></span>
                {/if}
              </div>
              <button
                type="button"
                class="btn-add-pseudo"
                disabled={!userSearchQuery.trim()}
                onclick={() => {
                  if (userSearchResults.length > 0) {
                    addInvitee(userSearchResults[0]);
                  } else if (userSearchQuery.trim()) {
                    addInvitee({ login: userSearchQuery.trim() });
                  }
                }}
              >
                Ajouter
              </button>
            </div>

            {#if userSearchResults.length > 0}
              <div class="autocomplete-dropdown">
                {#each userSearchResults as u (u.id)}
                  <button type="button" class="dropdown-row" onclick={() => addInvitee(u)}>
                    <div class="user-avatar-small">
                      {u.displayName.slice(0, 2).toUpperCase()}
                    </div>
                    <div class="user-meta">
                      <span class="row-title">{u.displayName}</span>
                      <span class="row-login">@{u.login}</span>
                    </div>
                  </button>
                {/each}
              </div>
            {/if}

            <!-- Chips for invited users -->
            {#if invitees.length > 0}
              <div class="invitees-chips-list">
                {#each invitees as inv (inv.username)}
                  <div class="invitee-chip">
                    <span class="chip-avatar">{inv.displayName.slice(0, 1).toUpperCase()}</span>
                    <span class="chip-label">@{inv.username}</span>
                    <button
                      type="button"
                      class="btn-remove-chip"
                      aria-label="Retirer {inv.username}"
                      onclick={() => removeInvitee(inv.username)}
                    >
                      ×
                    </button>
                  </div>
                {/each}
              </div>
            {:else}
              <span class="hint-text">
                Vous pouvez ajouter des invités maintenant ou partager un lien d'invitation après le
                démarrage.
              </span>
            {/if}
          </div>

          <!-- Playback Control Mode -->
          <div class="field-group">
            <label for="control-mode">Mode de contrôle de lecture</label>
            <select id="control-mode" bind:value={controlMode}>
              <option value="host_only">Organisateur uniquement (recommandé)</option>
              <option value="shared">Contrôles partagés (tous les participants)</option>
              <option value="request">Contrôle sur demande</option>
            </select>
          </div>

          <label class="checkbox-row">
            <input type="checkbox" bind:checked={waitForBuffering} />
            <span>Mettre en pause automatique si un participant est en cours de chargement</span>
          </label>

          <div class="modal-actions">
            <button type="button" class="btn-secondary" onclick={onClose}>Annuler</button>
            <button
              type="submit"
              class="btn-primary"
              disabled={loading || !selectedMediaId || !partyName.trim()}
            >
              {loading ? 'Création en cours…' : 'Créer la séance privée'}
            </button>
          </div>
        </form>
      {:else}
        <!-- List Pane: Invites & Active Parties -->
        <div class="parties-list-pane">
          {#if userInvites.length > 0}
            <section class="section-block">
              <h3>Invitations reçues ({userInvites.length})</h3>
              <div class="cards-list">
                {#each userInvites as inv (inv.token)}
                  <div class="party-card invite-card">
                    <div class="party-card-info">
                      <div class="invite-header">
                        <strong>{inv.partyName}</strong>
                        <span class="badge-invite">Invitation privée</span>
                      </div>
                      <span class="sub-text">
                        Média : {inv.mediaTitle} · Invité par <strong>{inv.hostName}</strong>
                      </span>
                    </div>
                    <button
                      type="button"
                      class="btn-primary-small"
                      disabled={loading}
                      onclick={() => handleAcceptInvite(inv.token, inv.partyId, inv.mediaId)}
                    >
                      Rejoindre
                    </button>
                  </div>
                {/each}
              </div>
            </section>
          {/if}

          <section class="section-block">
            <h3>Vos séances actives ({activeParties.length})</h3>
            <div class="cards-list">
              {#each activeParties as p (p.id)}
                <div class="party-card">
                  <div class="party-card-info">
                    <div class="party-card-header">
                      <strong>{p.name}</strong>
                      <span class="badge-status" class:playing={p.state === 'playing'}>
                        {p.state === 'playing'
                          ? 'En lecture'
                          : p.state === 'paused'
                            ? 'En pause'
                            : 'En attente'}
                      </span>
                    </div>
                    <span class="sub-text">
                      {p.mediaTitle} · {p.membersCount} participant{p.membersCount > 1 ? 's' : ''} ·
                      Hôte : {p.hostName}
                    </span>
                  </div>
                  <button
                    type="button"
                    class="btn-secondary-small"
                    disabled={loading}
                    onclick={() => handleJoin(p.id, p.mediaId)}
                  >
                    Rejoindre
                  </button>
                </div>
              {:else}
                <div class="empty-state">
                  <p class="empty-text">Aucune séance communautaire en cours actuellement.</p>
                  <button
                    type="button"
                    class="btn-secondary-small"
                    onclick={() => (mode = 'create')}
                  >
                    Créer une séance privée
                  </button>
                </div>
              {/each}
            </div>
          </section>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.85);
    backdrop-filter: blur(20px);
    display: grid;
    place-items: center;
    z-index: 100;
    padding: 20px;
  }
  .modal-card {
    width: min(580px, 100%);
    max-height: 90vh;
    background: #141824;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 16px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.9);
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }
  .title-with-icon {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--accent-primary, #f97316);
  }
  .title-with-icon h2 {
    font-size: 18px;
    font-weight: 700;
    color: #fff;
    margin: 0;
  }
  .sub-badge {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.5);
  }
  .btn-close {
    background: transparent;
    border: 0;
    color: rgba(255, 255, 255, 0.6);
    cursor: pointer;
    padding: 6px;
    border-radius: 6px;
  }
  .btn-close:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.1);
  }

  .modal-tabs {
    display: flex;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    padding: 0 16px;
    background: rgba(0, 0, 0, 0.2);
  }
  .tab-btn {
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    color: rgba(255, 255, 255, 0.6);
    padding: 14px 16px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .tab-btn.active {
    color: var(--accent-primary, #f97316);
    border-bottom-color: var(--accent-primary, #f97316);
  }
  .count-pill {
    background: var(--accent-primary, #f97316);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 10px;
  }

  .alert-error {
    margin: 16px 24px 0;
    padding: 10px 14px;
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 8px;
    color: #fca5a5;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .modal-body {
    padding: 24px;
    overflow-y: auto;
  }

  .privacy-banner {
    display: flex;
    gap: 12px;
    padding: 12px 16px;
    background: rgba(249, 115, 22, 0.08);
    border: 1px solid rgba(249, 115, 22, 0.2);
    border-radius: 10px;
    margin-bottom: 18px;
  }
  .privacy-icon {
    font-size: 20px;
    line-height: 1;
  }
  .privacy-text strong {
    font-size: 13px;
    color: #fff;
    display: block;
    margin-bottom: 3px;
  }
  .privacy-text p {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.7);
    margin: 0;
    line-height: 1.4;
  }

  .media-preview-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.85);
  }

  .selected-media-chip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: rgba(249, 115, 22, 0.12);
    border: 1px solid rgba(249, 115, 22, 0.3);
    border-radius: 8px;
  }
  .selected-media-chip .media-title {
    font-weight: 600;
    color: #fff;
    font-size: 13px;
  }
  .btn-change-media {
    background: transparent;
    border: 0;
    color: var(--accent-primary, #f97316);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    text-decoration: underline;
  }

  .create-form {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .field-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    position: relative;
  }
  .field-group label {
    font-size: 13px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.85);
  }
  .field-group input,
  .field-group select {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 8px;
    color: #fff;
    padding: 10px 14px;
    font-size: 14px;
  }
  .field-group input:focus,
  .field-group select:focus {
    outline: none;
    border-color: var(--accent-primary, #f97316);
  }

  .pseudo-input-row {
    display: flex;
    gap: 8px;
  }
  .search-input-wrapper {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
  }
  .search-input-wrapper input {
    width: 100%;
  }
  .spinner-small {
    position: absolute;
    right: 12px;
    width: 14px;
    height: 14px;
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-top-color: var(--accent-primary, #f97316);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .btn-add-pseudo {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #fff;
    padding: 0 16px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s;
  }
  .btn-add-pseudo:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.18);
  }
  .btn-add-pseudo:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .autocomplete-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    background: #1b2130;
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 8px;
    z-index: 50;
    max-height: 190px;
    overflow-y: auto;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  .dropdown-row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    background: transparent;
    border: 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
    color: #fff;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s;
  }
  .dropdown-row:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .user-avatar-small {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--accent-primary, #f97316);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
    display: grid;
    place-items: center;
  }
  .user-meta {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .row-title {
    font-size: 13px;
    font-weight: 600;
  }
  .row-login {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.5);
  }
  .row-year {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
  }

  .invitees-chips-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 6px;
  }
  .invitee-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    background: rgba(249, 115, 22, 0.15);
    border: 1px solid rgba(249, 115, 22, 0.3);
    padding: 4px 8px 4px 6px;
    border-radius: 16px;
  }
  .chip-avatar {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--accent-primary, #f97316);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
    display: grid;
    place-items: center;
  }
  .chip-label {
    font-size: 12px;
    font-weight: 600;
    color: #fff;
  }
  .btn-remove-chip {
    background: transparent;
    border: 0;
    color: rgba(255, 255, 255, 0.6);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
  }
  .btn-remove-chip:hover {
    color: #fff;
  }

  .hint-text {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    margin-top: 12px;
  }
  .btn-primary {
    background: var(--accent-primary, #f97316);
    color: #fff;
    border: 0;
    padding: 10px 20px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: all 0.2s;
  }
  .btn-primary:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 4px 16px rgba(249, 115, 22, 0.4);
  }
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .btn-secondary {
    background: transparent;
    color: rgba(255, 255, 255, 0.7);
    border: 1px solid rgba(255, 255, 255, 0.2);
    padding: 10px 18px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  /* List Pane */
  .parties-list-pane {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .section-block {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .section-block h3 {
    margin: 0;
    font-size: 12px;
    font-weight: 700;
    color: rgba(255, 255, 255, 0.6);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .cards-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .party-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    padding: 12px 16px;
    border-radius: 10px;
  }
  .invite-card {
    background: rgba(249, 115, 22, 0.06);
    border-color: rgba(249, 115, 22, 0.2);
  }
  .party-card-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .party-card-header,
  .invite-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .party-card-header strong,
  .invite-header strong {
    font-size: 14px;
    color: #fff;
  }
  .badge-invite {
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 10px;
    background: rgba(249, 115, 22, 0.2);
    color: var(--accent-primary, #f97316);
    font-weight: 600;
  }
  .badge-status {
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.7);
  }
  .badge-status.playing {
    background: rgba(34, 197, 94, 0.2);
    color: #4ade80;
  }
  .sub-text {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.6);
  }
  .sub-text strong {
    color: rgba(255, 255, 255, 0.9);
  }
  .btn-primary-small {
    background: var(--accent-primary, #f97316);
    border: 0;
    color: #fff;
    padding: 6px 14px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .btn-secondary-small {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #fff;
    padding: 6px 14px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 24px 0;
  }
  .empty-text {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.4);
    margin: 0;
  }
</style>

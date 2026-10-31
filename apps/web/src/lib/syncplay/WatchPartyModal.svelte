<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import type { ControlMode, PartySummary, UserInvitePayload } from './types';

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

  let mode = $state<'create' | 'list'>('create');
  let customPartyName = $state<string | null>(null);
  let partyName = $derived(customPartyName ?? (mediaTitle ? `Séance : ${mediaTitle}` : 'Ma séance'));
  let controlMode = $state<ControlMode>('host_only');
  let waitForBuffering = $state(true);

  let eligibleUsers = $state<{ id: string; login: string; displayName: string }[]>([]);
  let selectedUserIds = $state<string[]>([]);

  let activeParties = $state<PartySummary[]>([]);
  let userInvites = $state<UserInvitePayload[]>([]);

  let loading = $state(false);
  let error = $state<string | null>(null);

  onMount(async () => {
    await Promise.all([loadEligibleUsers(), loadParties(), loadInvites()]);
    if (!mediaId && activeParties.length > 0) {
      mode = 'list';
    }
  });

  async function loadEligibleUsers() {
    try {
      const q = mediaId ? `?media_id=${encodeURIComponent(mediaId)}` : '';
      const res = await fetch(`/api/v1/parties/eligible-users${q}`);
      if (res.ok) {
        const data = await res.json();
        eligibleUsers = data.users ?? [];
      }
    } catch {}
  }

  async function loadParties() {
    try {
      const res = await fetch('/api/v1/parties');
      if (res.ok) {
        const data = await res.json();
        activeParties = data.parties ?? [];
      }
    } catch {}
  }

  async function loadInvites() {
    try {
      const res = await fetch('/api/v1/parties/invites');
      if (res.ok) {
        const data = await res.json();
        userInvites = data.invites ?? [];
      }
    } catch {}
  }

  function toggleUserSelection(id: string) {
    if (selectedUserIds.includes(id)) {
      selectedUserIds = selectedUserIds.filter(u => u !== id);
    } else {
      selectedUserIds = [...selectedUserIds, id];
    }
  }

  async function handleCreateParty() {
    if (!mediaId) return;
    loading = true;
    error = null;

    try {
      const res = await fetch('/api/v1/parties', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: partyName.trim(),
          mediaId,
          controlMode,
          waitForBuffering,
          inviteUserIds: selectedUserIds
        })
      });

      if (!res.ok) {
        throw new Error('Impossible de créer la séance.');
      }

      const party = await res.json();
      onJoinParty(party.id, mediaId);
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
        method: 'POST'
      });
      if (res.ok) {
        onJoinParty(pId, mId);
        onClose();
      } else {
        throw new Error("L'invitation a expiré ou est invalide.");
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
        method: 'POST'
      });
      if (res.ok) {
        onJoinParty(pId, mId);
        onClose();
      } else {
        throw new Error('Impossible de rejoindre cette séance.');
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
        <h2>Séances Watch Party (SyncPlay)</h2>
      </div>
      <button type="button" class="btn-close" aria-label="Fermer" onclick={onClose}>
        <Icon name="close" size={20} />
      </button>
    </header>

    <!-- Navigation Tabs -->
    <nav class="modal-tabs">
      {#if mediaId}
        <button
          type="button"
          class="tab-btn"
          class:active={mode === 'create'}
          onclick={() => (mode = 'create')}
        >
          Créer une séance
        </button>
      {/if}
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
          class="tab-btn"
          class:active={mode === 'list'}
          onclick={() => (mode = 'list')}
        >
          Invitations ({userInvites.length})
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
      {#if mode === 'create' && mediaId}
        <form class="create-form" onsubmit={(e) => { e.preventDefault(); void handleCreateParty(); }}>
          <div class="field-group">
            <label for="party-name">Nom de la séance</label>
            <input
              id="party-name"
              type="text"
              required
              value={partyName}
              oninput={(e) => { customPartyName = e.currentTarget.value; }}
              placeholder="Ex: Soirée cinéma entre amis"
            />
          </div>

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
            <span>Mettre en pause automatique si un participant est en buffering</span>
          </label>

          {#if eligibleUsers.length > 0}
            <div class="invite-users-section">
              <span class="section-label">Inviter des utilisateurs Aether</span>
              <div class="users-scroll-list">
                {#each eligibleUsers as user (user.id)}
                  {@const isSelected = selectedUserIds.includes(user.id)}
                  <button
                    type="button"
                    class="user-select-row"
                    class:selected={isSelected}
                    onclick={() => toggleUserSelection(user.id)}
                  >
                    <div class="user-avatar">{user.displayName.slice(0, 2).toUpperCase()}</div>
                    <span class="user-name">{user.displayName}</span>
                    <span class="check-box">{isSelected ? '✓' : ''}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          <div class="modal-actions">
            <button type="button" class="btn-secondary" onclick={onClose}>Annuler</button>
            <button type="submit" class="btn-primary" disabled={loading || !partyName.trim()}>
              {loading ? 'Création en cours…' : 'Lancer la séance'}
            </button>
          </div>
        </form>
      {:else}
        <div class="parties-list-pane">
          {#if userInvites.length > 0}
            <section class="section-block">
              <h3>Invitations reçues ({userInvites.length})</h3>
              <div class="cards-list">
                {#each userInvites as inv (inv.token)}
                  <div class="party-card invite-card">
                    <div class="party-card-info">
                      <strong>{inv.partyName}</strong>
                      <span class="sub-text">Média : {inv.mediaTitle} · Invité par {inv.hostName}</span>
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
            <h3>Séances actives ({activeParties.length})</h3>
            <div class="cards-list">
              {#each activeParties as p (p.id)}
                <div class="party-card">
                  <div class="party-card-info">
                    <div class="party-card-header">
                      <strong>{p.name}</strong>
                      <span class="badge-status" class:playing={p.state === 'playing'}>
                        {p.state === 'playing' ? 'En lecture' : p.state === 'paused' ? 'En pause' : 'En attente'}
                      </span>
                    </div>
                    <span class="sub-text">
                      {p.mediaTitle} · {p.membersCount} participant{p.membersCount > 1 ? 's' : ''} · Hôte : {p.hostName}
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
                <p class="empty-text">Aucune séance communautaire en cours actuellement.</p>
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
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(16px);
    display: grid;
    place-items: center;
    z-index: 100;
    padding: 20px;
  }
  .modal-card {
    width: min(580px, 100%);
    max-height: 90vh;
    background: rgba(22, 26, 38, 0.98);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 16px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.85);
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
  .btn-close {
    background: transparent;
    border: 0;
    color: rgba(255, 255, 255, 0.6);
    cursor: pointer;
    padding: 6px;
    border-radius: 6px;
  }
  .btn-close:hover { color: #fff; background: rgba(255, 255, 255, 0.1); }

  .modal-tabs {
    display: flex;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    padding: 0 16px;
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
  }
  .tab-btn.active {
    color: var(--accent-primary, #f97316);
    border-bottom-color: var(--accent-primary, #f97316);
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

  .create-form {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .field-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field-group label {
    font-size: 13px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.85);
  }
  .field-group input, .field-group select {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 8px;
    color: #fff;
    padding: 10px 14px;
    font-size: 14px;
  }
  .field-group input:focus, .field-group select:focus {
    outline: none;
    border-color: var(--accent-primary, #f97316);
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
  }

  .invite-users-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .section-label {
    font-size: 13px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.85);
  }
  .users-scroll-list {
    max-height: 180px;
    overflow-y: auto;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.2);
    display: flex;
    flex-direction: column;
  }
  .user-select-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: transparent;
    border: 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
    color: #fff;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s;
  }
  .user-select-row:hover {
    background: rgba(255, 255, 255, 0.05);
  }
  .user-select-row.selected {
    background: rgba(249, 115, 22, 0.15);
  }
  .user-avatar {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--accent-primary, #f97316);
    color: #fff;
    font-size: 11px;
    font-weight: 700;
    display: grid;
    place-items: center;
  }
  .user-name {
    flex: 1;
    font-size: 13px;
  }
  .check-box {
    width: 18px;
    height: 18px;
    border: 1px solid rgba(255, 255, 255, 0.3);
    border-radius: 4px;
    display: grid;
    place-items: center;
    font-size: 12px;
    color: var(--accent-primary, #f97316);
  }
  .user-select-row.selected .check-box {
    border-color: var(--accent-primary, #f97316);
    background: rgba(249, 115, 22, 0.2);
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
    font-size: 13px;
    font-weight: 700;
    color: rgba(255, 255, 255, 0.7);
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
  .party-card-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .party-card-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .party-card-header strong {
    font-size: 14px;
    color: #fff;
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
    color: rgba(255, 255, 255, 0.5);
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
  .empty-text {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.4);
    text-align: center;
    padding: 24px 0;
  }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { session } from '../auth/session';
  import type { ControlMode, PartySummary, UserInvitePayload } from './types';
  import {
    acceptPartyInvite, createParty, fetchActiveParties, fetchPartyInvites, initials, joinParty,
    searchCatalogWorks, searchEligibleUsers, type CatalogWorkOption
  } from './party-api';
  import { t } from '../i18n';

  interface SelectedInvitee {
    id?: string;
    username: string;
    displayName: string;
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
    customPartyName ?? (selectedMediaTitle ? `Séance : ${selectedMediaTitle}` : 'Ma séance')
  );
  let isPublic = $state(true);
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
      const users = await searchEligibleUsers(q, selectedMediaId);
      if (users) userSearchResults = users;
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
      const works = await searchCatalogWorks(q);
      if (works) mediaSearchResults = works;
    } finally {
      isSearchingMedia = false;
    }
  }

  async function loadParties() {
    const parties = await fetchActiveParties();
    if (parties) activeParties = parties;
  }

  async function loadInvites() {
    const invites = await fetchPartyInvites();
    if (invites) userInvites = invites;
  }

  async function handleCreateParty() {
    if (!selectedMediaId) {
      error = $t('please_select_a_work_or_an');
      return;
    }
    if (!isPublic && invitees.length === 0) {
      error = $t('party_private_needs_invitees');
      return;
    }
    loading = true;
    error = null;

    try {
      const userIds = invitees.map((i) => i.id).filter((id): id is string => Boolean(id));
      const usernames = invitees.filter((i) => !i.id).map((i) => i.username);

      const party = await createParty(
        {
          name: partyName.trim(),
          mediaId: selectedMediaId,
          controlMode,
          waitForBuffering,
          isPublic,
          inviteUserIds: userIds.length > 0 ? userIds : undefined,
          inviteUsernames: usernames.length > 0 ? usernames : undefined
        },
        $session?.csrfToken ?? ''
      );
      onJoinParty(party.id, selectedMediaId);
      onClose();
    } catch (e) {
      error = e instanceof Error ? e.message : $t('unknown_error');
    } finally {
      loading = false;
    }
  }

  async function handleAcceptInvite(token: string, pId: string, mId: string) {
    loading = true;
    error = null;
    try {
      await acceptPartyInvite(token, $session?.csrfToken ?? '');
      onJoinParty(pId, mId);
      onClose();
    } catch (e) {
      error = e instanceof Error ? e.message : $t('error_while_accepting');
    } finally {
      loading = false;
    }
  }

  async function handleJoin(pId: string, mId: string) {
    loading = true;
    error = null;
    try {
      // A private session is entered through its invitation.
      const invite = userInvites.find((i) => i.partyId === pId);
      if (invite) await acceptPartyInvite(invite.token, $session?.csrfToken ?? '');
      else await joinParty(pId, $session?.csrfToken ?? '');
      onJoinParty(pId, mId);
      onClose();
    } catch (e) {
      error = e instanceof Error ? e.message : $t('error_while_accessing');
    } finally {
      loading = false;
    }
  }
</script>

<div class="modal-backdrop" role="dialog" aria-modal="true" aria-label={$t('watch_party_sessions')}>
  <div class="modal-card">
    <header class="modal-header">
      <div class="title-with-icon">
        <Icon name="users" size={24} />
        <div>
          <h2>{$t('aether_watch_party')}</h2>
          <span class="sub-badge">{$t('multi_user_synchronisation')}</span>
        </div>
      </div>
      <button type="button" class="btn-close" aria-label={$t('close')} onclick={onClose}>
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
        {$t('create_a_session')}
      </button>
      <button
        type="button"
        class="tab-btn"
        class:active={mode === 'list'}
        onclick={() => (mode = 'list')}
      >
        {$t('sessions_in_progress', { count: activeParties.length })}
      </button>
      {#if userInvites.length > 0}
        <button
          type="button"
          class="tab-btn badge-tab"
          class:active={mode === 'list'}
          onclick={() => (mode = 'list')}
        >
          {$t('invitations_received')}
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
              <label for="media-search">{$t('media_to_watch')}</label>
              {#if selectedMediaId && selectedMediaTitle}
                <div class="selected-media-chip">
                  <span class="media-title"><Icon name="film" size={16} />{selectedMediaTitle}</span>
                  <button
                    type="button"
                    class="btn-change-media"
                    onclick={() => {
                      selectedMediaId = undefined;
                      selectedMediaTitle = undefined;
                    }}
                  >
                    {$t('change')}
                  </button>
                </div>
              {:else}
                <div class="search-input-wrapper">
                  <input
                    id="media-search"
                    type="text"
                    bind:value={mediaSearchQuery}
                    oninput={() => void searchCatalogMedia(mediaSearchQuery)}
                    placeholder={$t('search_for_a_film_or_series')}
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
              <span>{$t('programme')} <strong>{selectedMediaTitle}</strong></span>
            </div>
          {/if}

          <!-- Party Name -->
          <div class="field-group">
            <label for="party-name">{$t('session_name')}</label>
            <input
              id="party-name"
              type="text"
              required
              value={partyName}
              oninput={(e) => {
                customPartyName = e.currentTarget.value;
              }}
              placeholder={$t('e_g_movie_night_with_friends')}
            />
          </div>

          <!-- Visibility: public (listed, open) or private (invited users only) -->
          <fieldset class="field-group visibility">
            <legend>{$t('party_visibility')}</legend>
            <div class="visibility-options">
              <label class="visibility-option" class:active={isPublic}>
                <input type="radio" name="party-visibility" value="public" onchange={() => (isPublic = true)} checked={isPublic} />
                <span class="visibility-icon"><Icon name="users" size={18} /></span>
                <span class="visibility-text">
                  <strong>{$t('party_public')}</strong>
                  <small>{$t('party_public_hint')}</small>
                </span>
              </label>
              <label class="visibility-option" class:active={!isPublic}>
                <input type="radio" name="party-visibility" value="private" onchange={() => (isPublic = false)} checked={!isPublic} />
                <span class="visibility-icon"><Icon name="shield" size={18} /></span>
                <span class="visibility-text">
                  <strong>{$t('party_private')}</strong>
                  <small>{$t('party_private_hint')}</small>
                </span>
              </label>
            </div>
          </fieldset>

          <!-- Invite users by username (@pseudo) -->
          <div class="field-group">
            <label for="invite-pseudo">{$t('invite_friends_by_username')}</label>
            <div class="pseudo-input-row">
              <div class="search-input-wrapper">
                <input
                  id="invite-pseudo"
                  type="text"
                  bind:value={userSearchQuery}
                  oninput={() => void searchUsers(userSearchQuery)}
                  onkeydown={handleUserKeyDown}
                  placeholder={$t('type_a_username_or_a_name')}
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
                {$t('sched_add')}
              </button>
            </div>

            {#if userSearchResults.length > 0}
              <div class="autocomplete-dropdown">
                {#each userSearchResults as u (u.id)}
                  <button type="button" class="dropdown-row" onclick={() => addInvitee(u)}>
                    <div class="user-avatar-small">
                      {initials(u.displayName)}
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
                    <span class="chip-avatar">{initials(inv.displayName, 1)}</span>
                    <span class="chip-label">@{inv.username}</span>
                    <button
                      type="button"
                      class="btn-remove-chip"
                      aria-label={$t('player_remove', { username: inv.username })}
                      onclick={() => removeInvitee(inv.username)}
                    >
                      <Icon name="close" size={14} />
                    </button>
                  </div>
                {/each}
              </div>
            {:else}
              <span class="hint-text">
                {$t('you_can_add_guests_now_or')}
              </span>
            {/if}
          </div>

          <!-- Playback Control Mode -->
          <div class="field-group">
            <label for="control-mode">{$t('playback_control_mode')}</label>
            <select id="control-mode" bind:value={controlMode}>
              <option value="host_only">{$t('organiser_only_recommended')}</option>
              <option value="shared">{$t('shared_controls_all_participants')}</option>
              <option value="request">{$t('control_on_request')}</option>
            </select>
          </div>

          <label class="checkbox-row">
            <input type="checkbox" bind:checked={waitForBuffering} />
            <span>{$t('pause_automatically_if_a_participant_is')}</span>
          </label>

          <p class="expiry-note">{$t('party_expiry_note')}</p>
          <div class="modal-actions">
            <button type="button" class="btn-secondary" onclick={onClose}>{$t('sched_cancel')}</button>
            <button
              type="submit"
              class="btn-primary"
              disabled={loading || !selectedMediaId || !partyName.trim()}
            >
              {loading ? $t('creating') : $t('create_the_session')}
            </button>
          </div>
        </form>
      {:else}
        <!-- List Pane: Invites & Active Parties -->
        <div class="parties-list-pane">
          {#if userInvites.length > 0}
            <section class="section-block">
              <h3>{$t('player_invitations_received', { count: userInvites.length })}</h3>
              <div class="cards-list">
                {#each userInvites as inv (inv.token)}
                  <div class="party-card invite-card">
                    <div class="party-card-info">
                      <div class="invite-header">
                        <strong>{inv.partyName}</strong>
                        <span class="badge-invite">{$t('private_invitation')}</span>
                      </div>
                      <span class="sub-text">
                        {$t('media_invited_by', { mediaTitle: inv.mediaTitle })} <strong>{inv.hostName}</strong>
                      </span>
                    </div>
                    <button
                      type="button"
                      class="btn-primary-small"
                      disabled={loading}
                      onclick={() => handleAcceptInvite(inv.token, inv.partyId, inv.mediaId)}
                    >
                      {$t('join')}
                    </button>
                  </div>
                {/each}
              </div>
            </section>
          {/if}

          <section class="section-block">
            <h3>{$t('your_active_sessions', { count: activeParties.length })}</h3>
            <div class="cards-list">
              {#each activeParties as p (p.id)}
                <div class="party-card">
                  <div class="party-card-info">
                    <div class="party-card-header">
                      <strong>{p.name}</strong>
                      <span class="badge-visibility" class:private={p.isLocked}>
                        {p.isLocked ? $t('private_badge') : $t('public_badge')}
                      </span>
                      <span class="badge-status" class:playing={p.state === 'playing'}>
                        {p.state === 'playing'
                          ? $t('playing')
                          : p.state === 'paused'
                            ? $t('paused')
                            : $t('waiting')}
                      </span>
                    </div>
                    <span class="sub-text">
                      {$t('party_summary', { title: p.mediaTitle, count: p.onlineCount ?? p.membersCount, host: p.hostName })}
                    </span>
                  </div>
                  <button
                    type="button"
                    class="btn-secondary-small"
                    disabled={loading}
                    onclick={() => handleJoin(p.id, p.mediaId)}
                  >
                    {$t('join')}
                  </button>
                </div>
              {:else}
                <div class="empty-state">
                  <p class="empty-text">{$t('no_community_session_is_running_right')}</p>
                  <button
                    type="button"
                    class="btn-secondary-small"
                    onclick={() => (mode = 'create')}
                  >
                    {$t('create_a_session')}
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
  .expiry-note { margin: 4px 0 0; font-size: 13px; line-height: 1.5; color: var(--mu, #9aa3b2); }
  /* Halo — dialogue d'application (suit le thème clair/sombre) */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgb(6 8 14 / 0.55);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
    display: grid;
    place-items: center;
    z-index: 100;
    padding: 24px;
    font-family: var(--font);
  }
  .modal-card {
    width: min(600px, 100%);
    max-height: min(860px, calc(100dvh - 48px));
    background: var(--bg-3);
    color: var(--tx);
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: var(--shadow);
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 24px 28px 16px;
    flex: none;
  }
  .title-with-icon {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
    color: var(--mu);
  }
  .title-with-icon > div {
    min-width: 0;
  }
  .title-with-icon h2 {
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.2;
    color: var(--tx);
    margin: 0;
  }
  .sub-badge {
    display: block;
    margin-top: 2px;
    font-size: 13px;
    color: var(--mu);
  }
  .btn-close {
    flex: none;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    padding: 0;
    background: var(--pn);
    border: 0;
    border-radius: var(--r-pill);
    color: var(--tx);
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }
  .btn-close:hover {
    background: var(--pn-2);
  }

  /* Onglets segmentés */
  .modal-tabs {
    display: flex;
    flex: none;
    align-self: flex-start;
    max-width: calc(100% - 56px);
    gap: 4px;
    margin: 0 28px 8px;
    padding: 4px;
    border-radius: var(--r-pill);
    background: var(--pn);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .modal-tabs::-webkit-scrollbar {
    display: none;
  }
  .tab-btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 0 16px;
    background: transparent;
    border: 0;
    border-radius: var(--r-pill);
    color: var(--mu);
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }
  .tab-btn:hover {
    color: var(--tx);
  }
  .tab-btn.active,
  .tab-btn.active:hover {
    background: var(--tx);
    color: var(--bg);
  }
  .count-pill {
    min-width: 20px;
    padding: 1px 7px;
    border-radius: var(--r-pill);
    background: var(--ac);
    color: var(--on-ac);
    font-size: 12px;
    font-weight: 800;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .alert-error {
    flex: none;
    margin: 8px 28px 0;
    padding: 12px 16px;
    background: var(--err-soft);
    border: 0;
    border-radius: var(--r-sm);
    color: var(--err);
    font-size: 14px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .modal-body {
    padding: 16px 28px 28px;
    overflow-y: auto;
    overscroll-behavior: contain;
    min-height: 0;
  }

  /* Visibilité : publique ou privée */
  fieldset.visibility {
    border: 0;
    margin: 0;
    padding: 0;
    min-width: 0;
  }
  .visibility legend {
    padding: 0;
    margin-bottom: 8px;
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }
  .visibility-options {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .visibility-option {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    min-height: 44px;
    padding: 12px 14px;
    background: var(--pn);
    border: 2px solid transparent;
    border-radius: var(--r-md);
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      border-color var(--t-fast) var(--ease);
  }
  .visibility-option:hover {
    background: var(--pn-2);
  }
  .visibility-option.active {
    border-color: var(--ac);
  }
  .visibility-option input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .visibility-option:has(input:focus-visible) {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }
  .visibility-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: var(--r-sm);
    background: var(--pn-2);
    color: var(--tx);
  }
  .visibility-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .visibility-text strong {
    font-size: 15px;
    font-weight: 700;
    color: var(--tx);
  }
  .visibility-text small {
    font-size: 13px;
    line-height: 1.45;
    color: var(--mu);
  }
  .badge-visibility {
    display: inline-flex;
    align-items: center;
    min-height: 22px;
    padding: 0 8px;
    border-radius: var(--r-pill);
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
    background: var(--ok-soft);
    color: var(--ok);
  }
  .badge-visibility.private {
    background: var(--info-soft);
    color: var(--info);
  }

  .media-preview-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 46px;
    padding: 10px 16px;
    background: var(--pn);
    border: 0;
    border-radius: var(--r-sm);
    font-size: 14px;
    color: var(--mu);
  }
  .media-preview-bar strong {
    color: var(--tx);
    font-weight: 700;
  }

  .selected-media-chip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 56px;
    padding: 6px 6px 6px 16px;
    background: var(--pn);
    border: 2px solid var(--ac);
    border-radius: var(--r-sm);
  }
  .selected-media-chip .media-title {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    font-weight: 700;
    color: var(--tx);
    font-size: 15px;
    overflow-wrap: anywhere;
  }
  .btn-change-media {
    flex: none;
    min-height: 44px;
    padding: 0 16px;
    background: var(--pn-2);
    border: 0;
    border-radius: var(--r-md);
    color: var(--tx);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
  }
  .btn-change-media:hover {
    background: var(--pn-3);
  }

  .create-form {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .field-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    position: relative;
  }
  .field-group label {
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }
  .field-group input[type='text'],
  .field-group select {
    min-height: 46px;
    height: 46px;
    background-color: var(--pn);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    color: var(--tx);
    padding: 0 16px;
    font-size: 15px;
    box-shadow: none;
    transition:
      border-color var(--t-fast) var(--ease),
      background-color var(--t-fast) var(--ease);
  }
  .field-group select {
    padding-right: 40px;
  }
  .field-group input[type='text']::placeholder {
    color: var(--mu-2);
  }
  .field-group input[type='text']:hover,
  .field-group select:hover {
    background-color: var(--pn-2);
  }
  .field-group input[type='text']:focus,
  .field-group select:focus,
  .field-group select:focus-visible {
    outline: none;
    border-color: var(--ac);
    box-shadow: none;
  }

  .pseudo-input-row {
    display: flex;
    gap: 8px;
  }
  .search-input-wrapper {
    flex: 1;
    min-width: 0;
    position: relative;
    display: flex;
    align-items: center;
  }
  .search-input-wrapper input[type='text'] {
    width: 100%;
    padding-right: 40px;
  }
  .spinner-small {
    position: absolute;
    right: 16px;
    width: 16px;
    height: 16px;
    border: 2px solid var(--line-2);
    border-top-color: var(--ac);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .btn-add-pseudo {
    flex: none;
    min-height: 46px;
    background: var(--pn-2);
    border: 0;
    color: var(--tx);
    padding: 0 18px;
    border-radius: var(--r-md);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }
  .btn-add-pseudo:hover:not(:disabled) {
    background: var(--pn-3);
  }
  .btn-add-pseudo:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .autocomplete-dropdown {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    padding: 6px;
    background: var(--bg-3);
    border: 1px solid var(--line-2);
    border-radius: var(--r-md);
    z-index: 50;
    max-height: 240px;
    overflow-y: auto;
    box-shadow: var(--shadow);
  }
  .dropdown-row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 8px 12px;
    background: transparent;
    border: 0;
    border-radius: var(--r-sm);
    color: var(--tx);
    cursor: pointer;
    text-align: left;
    transition: background-color var(--t-fast) var(--ease);
  }
  .dropdown-row:hover {
    background: var(--pn-2);
  }
  .user-avatar-small {
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--pn-2);
    color: var(--tx);
    font-size: 13px;
    font-weight: 800;
    display: grid;
    place-items: center;
  }
  .user-meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .row-title {
    font-size: 15px;
    font-weight: 700;
  }
  .row-login {
    font-size: 13px;
    color: var(--mu);
  }
  .row-year {
    font-size: 13px;
    color: var(--mu);
    font-variant-numeric: tabular-nums;
  }

  .invitees-chips-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 4px;
  }
  .invitee-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    background: var(--pn-2);
    border: 0;
    padding: 4px 4px 4px 4px;
    border-radius: var(--r-pill);
  }
  .chip-avatar {
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--pn-3);
    color: var(--tx);
    font-size: 13px;
    font-weight: 800;
    display: grid;
    place-items: center;
  }
  .chip-label {
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }
  .btn-remove-chip {
    position: relative;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    background: transparent;
    border: 0;
    border-radius: 50%;
    color: var(--mu);
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }
  /* Zone tactile portée à 44 px sans grossir la puce */
  .btn-remove-chip::before {
    content: '';
    position: absolute;
    inset: -6px;
  }
  .btn-remove-chip:hover {
    color: var(--tx);
    background: var(--pn-3);
  }

  .hint-text {
    font-size: 13px;
    line-height: 1.5;
    color: var(--mu);
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    font-size: 14px;
    font-weight: 600;
    line-height: 1.4;
    color: var(--tx);
    cursor: pointer;
  }
  .checkbox-row input {
    flex: none;
    width: 20px;
    height: 20px;
    margin: 0;
    accent-color: var(--ac);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 8px;
  }
  .btn-primary,
  .btn-secondary,
  .btn-primary-small,
  .btn-secondary-small {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border: 0;
    border-radius: var(--r-md);
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }
  .btn-primary,
  .btn-secondary {
    min-height: 48px;
    padding: 0 22px;
    font-size: 15px;
  }
  .btn-primary,
  .btn-primary-small {
    background: var(--btn);
    color: var(--btn-tx);
  }
  .btn-secondary,
  .btn-secondary-small {
    background: var(--pn-2);
    color: var(--tx);
  }
  .btn-primary:hover:not(:disabled),
  .btn-primary-small:hover:not(:disabled) {
    transform: none;
    box-shadow: none;
    filter: brightness(1.08);
  }
  .btn-secondary:hover:not(:disabled),
  .btn-secondary-small:hover:not(:disabled) {
    background: var(--pn-3);
  }
  .btn-primary:active:not(:disabled),
  .btn-secondary:active:not(:disabled),
  .btn-primary-small:active:not(:disabled),
  .btn-secondary-small:active:not(:disabled) {
    transform: scale(0.98);
  }
  .btn-primary:disabled,
  .btn-primary-small:disabled,
  .btn-secondary-small:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .btn-primary-small,
  .btn-secondary-small {
    flex: none;
    min-height: 44px;
    padding: 0 18px;
    font-size: 14px;
  }

  .modal-card button:focus-visible,
  .modal-card input[type='checkbox']:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  /* Listes : invitations et séances */
  .parties-list-pane {
    display: flex;
    flex-direction: column;
    gap: 24px;
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
    color: var(--mu);
    text-transform: uppercase;
    letter-spacing: 0.07em;
  }
  .cards-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .party-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 60px;
    background: var(--pn);
    border: 0;
    padding: 10px 10px 10px 16px;
    border-radius: var(--r-md);
    transition: background-color var(--t-fast) var(--ease);
  }
  .party-card:hover {
    background: var(--pn-2);
  }
  .invite-card {
    background: var(--pn);
  }
  .party-card-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .party-card-header,
  .invite-header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .party-card-header strong,
  .invite-header strong {
    font-size: 15px;
    font-weight: 700;
    color: var(--tx);
    overflow-wrap: anywhere;
  }
  .badge-invite,
  .badge-status {
    display: inline-flex;
    align-items: center;
    min-height: 22px;
    padding: 0 8px;
    border-radius: var(--r-pill);
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
  }
  .badge-invite {
    background: var(--info-soft);
    color: var(--info);
  }
  .badge-status {
    background: var(--pn-2);
    color: var(--mu);
  }
  .badge-status.playing {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .sub-text {
    font-size: 13px;
    line-height: 1.45;
    color: var(--mu);
    overflow-wrap: anywhere;
  }
  .sub-text strong {
    color: var(--tx);
    font-weight: 700;
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 32px 16px;
    border-radius: var(--r-md);
    background: var(--pn);
    text-align: center;
  }
  .empty-text {
    font-size: 14px;
    color: var(--mu);
    margin: 0;
  }

  /* Téléphone : feuille plein écran */
  @media (max-width: 639px) {
    .modal-backdrop {
      padding: 0;
      place-items: stretch;
    }
    .modal-card {
      width: 100vw;
      height: 100dvh;
      max-height: 100dvh;
      border: 0;
      border-radius: 0;
      padding-top: env(safe-area-inset-top, 0px);
    }
    .modal-header {
      padding: 16px 16px 12px;
    }
    .title-with-icon h2 {
      font-size: 22px;
    }
    .modal-tabs {
      margin-inline: 16px;
      max-width: calc(100% - 32px);
    }
    .alert-error {
      margin-inline: 16px;
    }
    .modal-body {
      flex: 1;
      padding: 12px 16px calc(24px + env(safe-area-inset-bottom, 0px));
    }
    .modal-actions {
      flex-direction: column-reverse;
    }
    .visibility-options {
      grid-template-columns: 1fr;
    }
    .modal-actions > button {
      width: 100%;
    }
    .party-card {
      flex-wrap: wrap;
      padding: 12px 12px 12px 16px;
    }
    .party-card > button {
      width: 100%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner-small {
      animation: none;
    }
  }
</style>

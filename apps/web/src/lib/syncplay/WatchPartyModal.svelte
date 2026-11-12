<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { session } from '../auth/session';
  import type { PartySummary, UserInvitePayload } from './types';
  import { acceptPartyInvite, fetchActiveParties, fetchPartyInvites, joinParty } from './party-api';
  import { t } from '../i18n';
  import PartyCreateForm from './PartyCreateForm.svelte';

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

  let createForm = $state<ReturnType<typeof PartyCreateForm>>();
  let activeParties = $state<PartySummary[]>([]);
  let userInvites = $state<UserInvitePayload[]>([]);

  let loading = $state(false);
  let error = $state<string | null>(null);

  onMount(async () => {
    await Promise.all([loadParties(), loadInvites()]);
    if (!selectedMediaId && activeParties.length > 0) {
      mode = 'list';
    } else if (!selectedMediaId) {
      void createForm?.searchCatalogMedia('');
    }
  });

  async function loadParties() {
    const parties = await fetchActiveParties();
    if (parties) activeParties = parties;
  }

  async function loadInvites() {
    const invites = await fetchPartyInvites();
    if (invites) userInvites = invites;
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
      <!-- Always mounted (hidden on the list tab) so a half-filled form survives tab switches. -->
      <PartyCreateForm bind:this={createForm} active={mode === 'create'} {mediaId} bind:selectedMediaId bind:selectedMediaTitle bind:mode bind:loading bind:error {onJoinParty} {onClose} />
      {#if mode === 'list'}
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

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

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

  .btn-primary-small {
    background: var(--btn);
    color: var(--btn-tx);
  }

  .btn-secondary-small {
    background: var(--pn-2);
    color: var(--tx);
  }

  .btn-primary-small:hover:not(:disabled) {
    transform: none;
    box-shadow: none;
    filter: brightness(1.08);
  }

  .btn-secondary-small:hover:not(:disabled) {
    background: var(--pn-3);
  }

  .btn-primary-small:active:not(:disabled),
  .btn-secondary-small:active:not(:disabled) {
    transform: scale(0.98);
  }

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

  .modal-card button:focus-visible {
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
    .party-card {
      flex-wrap: wrap;
      padding: 12px 12px 12px 16px;
    }
    .party-card > button {
      width: 100%;
    }
  }
</style>

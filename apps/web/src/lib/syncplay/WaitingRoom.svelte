<script lang="ts">
  import { session } from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import type { SyncPartyClient } from './sync-client.svelte';
  import type { ControlMode } from './types';
  import { initials, partyInviteUrl } from './party-api';
  import { t } from '../i18n';
  import { resolve } from '$app/paths';

  let { client, isHost, onCloseParty }: { client: SyncPartyClient; isHost: boolean; onCloseParty?: () => void } = $props();

  const selfMember = $derived(client.members.find(m => m.userId === $session?.user.id));
  const isReady = $derived(selfMember?.isReady ?? false);
  const allReady = $derived(
    client.members.length > 0 && client.members.every(m => m.isReady || m.role === 'host')
  );

  function toggleReady() {
    client.setReady(!isReady);
  }

  function startParty() {
    if (!isHost) return;
    // Launch with a 3-second countdown
    client.play(0, 3);
  }

  function copyInviteLink() {
    if (typeof window === 'undefined' || !client.party) return;
    const url = partyInviteUrl(`${window.location.origin}${resolve('/')}`, client.party.id);
    if (navigator?.clipboard) {
      void navigator.clipboard.writeText(url);
    }
  }

  function updateControlMode(mode: ControlMode) {
    if (!isHost) return;
    client.updateSettings({ controlMode: mode });
  }

  function toggleLock() {
    if (!isHost || !client.party) return;
    client.updateSettings({ isLocked: !client.party.isLocked });
  }

  function toggleWaitForBuffering() {
    if (!isHost || !client.party) return;
    client.updateSettings({ waitForBuffering: !client.party.waitForBuffering });
  }
</script>

<div class="waiting-room-overlay" role="dialog" aria-label={$t('session_waiting_room')}>
  <div class="waiting-room-card">
    <header class="waiting-header">
      <div class="badge-live">
        <span class="pulse-dot"></span>
        <span>{$t('waiting_room')}</span>
      </div>
      <h2>{client.party?.name ?? $t('community_session')}</h2>
      <p class="waiting-media-title">{$t('film_episode')} <strong>{client.party?.mediaTitle}</strong></p>
    </header>

    <section class="members-lobby" aria-label={$t('participants_in_the_room')}>
      <div class="lobby-header">
        <h3>{$t('participants', { count: client.members.length })}</h3>
        <button type="button" class="btn-copy-invite" onclick={copyInviteLink}>
          <Icon name="copy" size={14} />
          <span>{$t('copy_the_invitation_link')}</span>
        </button>
      </div>

      <div class="members-grid">
        {#each client.members as member (member.userId)}
          <div class="member-pill" class:is-ready={member.isReady} class:is-host={member.role === 'host'}>
            <div class="member-avatar">
              {initials(member.displayName)}
            </div>
            <div class="member-info">
              <span class="member-name">{member.displayName}</span>
              <span class="member-role">
                {#if member.role === 'host'}
                  <Icon name="star" size={14} /> {$t('organiser')}
                {:else if member.isReady}
                  <Icon name="check" size={14} /> {$t('ready')}
                {:else}
                  <Icon name="clock" size={14} /> {$t('waiting')}
                {/if}
              </span>
            </div>
          </div>
        {/each}
      </div>
    </section>

    <!-- Settings for Host -->
    {#if isHost}
      <section class="host-settings-panel">
        <div class="settings-row">
          <label>
            {$t('player_playback_controls')}
            <select
              value={client.party?.controlMode ?? 'host_only'}
              onchange={(e) => updateControlMode(e.currentTarget.value as ControlMode)}
            >
              <option value="host_only">{$t('organiser_only')}</option>
              <option value="shared">{$t('shared_controls_all')}</option>
              <option value="request">{$t('control_on_request')}</option>
            </select>
          </label>

          <label class="checkbox-label">
            <input
              type="checkbox"
              checked={client.party?.waitForBuffering ?? true}
              onchange={toggleWaitForBuffering}
            />
            <span>{$t('wait_for_buffering_participants')}</span>
          </label>

          <label class="checkbox-label">
            <input
              type="checkbox"
              checked={client.party?.isLocked ?? false}
              onchange={toggleLock}
            />
            <span>{$t('lock_the_session_no_new_arrivals')}</span>
          </label>
        </div>
      </section>
    {/if}

    <footer class="waiting-actions">
      {#if !isHost}
        <button
          type="button"
          class="btn-ready"
          class:active={isReady}
          onclick={toggleReady}
        >
          <Icon name={isReady ? 'check' : 'clock'} size={18} />
          <span>{isReady ? $t('i_m_ready') : $t('mark_me_as_ready')}</span>
        </button>
      {/if}

      {#if isHost}
        <button
          type="button"
          class="btn-start"
          disabled={!allReady && client.members.length > 1}
          onclick={startParty}
        >
          <Icon name="play" size={20} />
          <span>{$t('start_the_session_for_everyone')}</span>
        </button>
      {/if}

      <button
        type="button"
        class="btn-leave"
        onclick={() => {
          client.leave();
          onCloseParty?.();
        }}
      >
        {$t('leave_the_session')}
      </button>
    </footer>
  </div>
</div>

<style>
  /* Salon d'attente */
  .waiting-room-overlay {
    position: absolute;
    inset: 0;
    background: rgb(6 8 14 / 0.8);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    display: grid;
    place-items: center;
    z-index: 25;
    padding: 24px;
    overflow-y: auto;
    font-family: var(--font);
  }

  .waiting-room-card {
    width: min(720px, 100%);
    background: rgb(17 21 31 / 0.92);
    border: 0;
    border-radius: 24px;
    padding: 28px;
    display: flex;
    flex-direction: column;
    gap: 24px;
    color: #eef1f8;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
  }

  .waiting-header {
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }

  .badge-live {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    background: rgb(255 255 255 / 0.09);
    color: #eef1f8;
    padding: 0 12px;
    border-radius: var(--r-pill);
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.07em;
  }

  .pulse-dot {
    width: 8px;
    height: 8px;
    background: var(--ac);
    border-radius: 50%;
    animation: blink 1.4s infinite;
  }

  .waiting-header h2 {
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.2;
    color: #eef1f8;
    margin: 0;
    overflow-wrap: anywhere;
  }

  .waiting-media-title {
    color: #a2a9ba;
    font-size: 14px;
    margin: 0;
  }

  .waiting-media-title strong {
    color: #eef1f8;
    font-weight: 700;
  }

  .members-lobby {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .lobby-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
  }

  .lobby-header h3 {
    font-size: 17px;
    font-weight: 700;
    color: #eef1f8;
    margin: 0;
  }

  .btn-copy-invite {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    background: rgb(255 255 255 / 0.14);
    border: 0;
    color: #eef1f8;
    padding: 0 16px;
    border-radius: 16px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }

  .btn-copy-invite:hover {
    background: rgb(255 255 255 / 0.24);
  }

  .members-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 8px;
  }

  .member-pill {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 60px;
    background: rgb(255 255 255 / 0.055);
    border: 0;
    padding: 10px 12px;
    border-radius: 16px;
  }

  .member-avatar {
    flex-shrink: 0;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: rgb(255 255 255 / 0.09);
    color: #eef1f8;
    display: grid;
    place-items: center;
    font-size: 14px;
    font-weight: 800;
  }

  .member-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .member-name {
    font-size: 15px;
    font-weight: 700;
    color: #eef1f8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .member-role {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: #a2a9ba;
  }

  .member-pill:not(.is-ready):not(.is-host) .member-role {
    color: var(--warn);
  }

  .member-pill.is-ready .member-role {
    color: var(--ok);
    font-weight: 700;
  }

  .host-settings-panel {
    background: rgb(255 255 255 / 0.055);
    border: 0;
    padding: 16px;
    border-radius: 16px;
  }

  .settings-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 14px;
    font-weight: 600;
    color: #eef1f8;
  }

  .settings-row > label:first-child {
    display: grid;
    gap: 8px;
    font-weight: 700;
  }

  .settings-row select {
    min-height: 46px;
    margin: 0;
    background-color: rgb(255 255 255 / 0.09);
    border: 1px solid rgb(255 255 255 / 0.09);
    border-radius: 12px;
    color: #eef1f8;
    padding: 0 40px 0 16px;
    font-size: 15px;
    box-shadow: none;
  }

  .settings-row select:hover {
    background-color: rgb(255 255 255 / 0.14);
  }

  .settings-row select:focus,
  .settings-row select:focus-visible {
    outline: none;
    border-color: var(--ac);
  }

  .settings-row option {
    background: #171c28;
    color: #eef1f8;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    cursor: pointer;
  }

  .checkbox-label input {
    flex: none;
    width: 20px;
    height: 20px;
    margin: 0;
    accent-color: var(--ac);
  }

  .waiting-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    justify-content: flex-end;
    border-top: 1px solid rgb(255 255 255 / 0.09);
    padding-top: 20px;
  }

  .btn-start,
  .btn-ready,
  .btn-leave {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 48px;
    padding: 0 22px;
    border: 0;
    border-radius: 16px;
    font-size: 15px;
    font-weight: 700;
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }

  .btn-start {
    background: #eef1f8;
    color: #0b0e18;
  }

  .btn-start:hover:not(:disabled) {
    background: #ffffff;
    transform: none;
    box-shadow: none;
  }

  .btn-start:active:not(:disabled),
  .btn-ready:active:not(:disabled),
  .btn-leave:active:not(:disabled) {
    transform: scale(0.98);
  }

  .btn-start:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .btn-ready {
    background: rgb(255 255 255 / 0.14);
    color: #eef1f8;
  }

  .btn-ready:hover {
    background: rgb(255 255 255 / 0.24);
  }

  .btn-ready.active {
    background: var(--ok-soft);
    color: var(--ok);
    box-shadow: inset 0 0 0 2px var(--ok);
  }

  .btn-leave {
    background: transparent;
    color: #a2a9ba;
  }

  .btn-leave:hover {
    background: rgb(255 255 255 / 0.14);
    color: #eef1f8;
  }

  .waiting-room-card button:focus-visible,
  .waiting-room-card input[type='checkbox']:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  @media (prefers-reduced-motion: reduce) {
  .pulse-dot {
          animation: none;
        }
}
</style>

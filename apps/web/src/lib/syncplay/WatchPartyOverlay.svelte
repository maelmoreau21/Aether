<script lang="ts">
  import Icon from '../ui/Icon.svelte';
  import type { SyncPartyClient } from './sync-client.svelte';
  import { t } from '../i18n';
  import CommunityDrawer from './CommunityDrawer.svelte';
  import WaitingRoom from './WaitingRoom.svelte';

  let {
    client,
    controlsVisible = true,
    onCloseParty
  }: {
    client: SyncPartyClient;
    /** The reaction bar follows the player controls: it hides with them during playback. */
    controlsVisible?: boolean;
    onCloseParty?: () => void;
  } = $props();

  let drawerOpen = $state(false);
  let muteOverlay = $state(false);

  const isHost = $derived(client.isHost);
  const isWaiting = $derived(client.party?.state === 'waiting');

  const onlineMembers = $derived(client.members.filter(m => m.isOnline));

  const quickEmojis = ['❤️', '😂', '👏', '🍿', '😱', '🔥'];

</script>

<!-- Floating Reactions Canvas -->
{#if !muteOverlay}
  <div class="reactions-canvas" aria-hidden="true">
    {#each client.recentReactions as reaction (reaction.id)}
      <div class="floating-reaction">
        <span class="reaction-emoji">{reaction.emoji}</span>
        <span class="reaction-name">{reaction.name}</span>
      </div>
    {/each}
  </div>
{/if}

<!-- Toast Notification -->
{#if client.lastNotification && !muteOverlay}
  <div class="sync-toast" role="status">
    <Icon name="sparkles" size={16} />
    <span>{client.lastNotification.text}</span>
  </div>
{/if}

<!-- Countdown Display -->
{#if client.countdownSeconds !== null}
  <div class="countdown-backdrop" role="alert">
    <div class="countdown-circle">
      <span class="countdown-number">{client.countdownSeconds}</span>
      <span class="countdown-label">{$t('starting_together')}</span>
    </div>
  </div>
{/if}

<!-- Salon d'attente (Waiting Room) -->
{#if isWaiting}
  <WaitingRoom {client} {isHost} {onCloseParty} />
{/if}

<!-- Top Right Trigger Pill (during active playback) -->
<div class="party-status-pill">
  <button
    type="button"
    class="pill-trigger"
    class:active={drawerOpen}
    aria-label={$t('open_the_community_panel')}
    onclick={() => (drawerOpen = !drawerOpen)}
  >
    <Icon name="users" size={16} />
    <span>{onlineMembers.length}</span>
  </button>
</div>

<!-- Bottom Quick Reactions Bar (when controls are visible or hover) -->
{#if !isWaiting && !muteOverlay && (controlsVisible || drawerOpen)}
  <div class="quick-reactions-bar" aria-label={$t('quick_reactions')}>
    {#each quickEmojis as emoji (emoji)}
      <button
        type="button"
        class="reaction-btn"
        aria-label={$t('react_with', { emoji })}
        onclick={() => client.sendReaction(emoji)}
      >
        {emoji}
      </button>
    {/each}
  </div>
{/if}

<!-- Community Drawer (Chat, Members, Queue, Polls) -->
{#if drawerOpen}
  <CommunityDrawer {client} {isHost} {onlineMembers} bind:drawerOpen bind:muteOverlay />
{/if}

<style>
  /* Halo — couche Watch Party posée sur le lecteur vidéo.
     Le lecteur est toujours sombre : les surfaces reprennent les valeurs
     littérales du panneau flottant de PlayerControls.svelte ; les états
     (accent, ok, warn, err) passent par les jetons, forcés en sombre sur
     .immersive-dialog. */

  /* Réactions flottantes */
  .reactions-canvas {
    position: absolute;
    inset: 0;
    pointer-events: none;
    overflow: hidden;
    z-index: 12;
  }

  .floating-reaction {
    position: absolute;
    bottom: 80px;
    right: 40px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    font-family: var(--font);
    animation: floatUp 3.2s var(--ease) forwards;
  }

  .reaction-emoji {
    font-size: 38px;
    line-height: 1;
  }

  .reaction-name {
    font-size: 12px;
    font-weight: 700;
    color: #eef1f8;
    background: rgb(17 21 31 / 0.92);
    padding: 3px 10px;
    border-radius: var(--r-pill);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
  }

  @keyframes floatUp {
    0% { transform: translateY(0) scale(0.6); opacity: 0; }
    15% { transform: translateY(-30px) scale(1.1); opacity: 1; }
    80% { opacity: 0.9; }
    100% { transform: translateY(-320px) scale(0.9); opacity: 0; }
  }

  /* Notification de synchronisation */
  .sync-toast {
    position: absolute;
    top: 75px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: calc(100% - 32px);
    min-height: 44px;
    padding: 10px 18px;
    background: rgb(17 21 31 / 0.92);
    border: 0;
    border-radius: var(--r-pill);
    color: #eef1f8;
    font-family: var(--font);
    font-size: 14px;
    font-weight: 600;
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    z-index: 15;
    animation: fadeInDown var(--t-med) var(--ease);
  }

  .sync-toast :global(svg) {
    flex: none;
    color: #a2a9ba;
  }

  @keyframes fadeInDown {
    from { opacity: 0; transform: translate(-50%, -12px); }
    to { opacity: 1; transform: translate(-50%, 0); }
  }

  /* Compte à rebours */
  .countdown-backdrop {
    position: absolute;
    inset: 0;
    background: rgb(6 8 14 / 0.72);
    display: grid;
    place-items: center;
    z-index: 20;
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    font-family: var(--font);
  }

  .countdown-circle {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .countdown-number {
    font-size: 96px;
    font-weight: 800;
    line-height: 1;
    letter-spacing: -0.035em;
    font-variant-numeric: tabular-nums;
    color: #eef1f8;
    animation: scalePulse 1s var(--ease) infinite;
  }

  .countdown-label {
    font-size: 17px;
    font-weight: 700;
    color: #a2a9ba;
  }

  @keyframes scalePulse {
    0% { transform: scale(0.9); opacity: 0.7; }
    50% { transform: scale(1.04); opacity: 1; }
    100% { transform: scale(0.96); opacity: 0.85; }
  }

  @keyframes blink { 50% { opacity: 0.35; } }

  /* Pastille d'ouverture du panneau (pendant la lecture) */
  .party-status-pill {
    position: absolute;
    top: max(24px, env(safe-area-inset-top));
    right: 90px;
    z-index: 10;
  }

  .pill-trigger {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px;
    background: rgb(17 21 31 / 0.92);
    border: 0;
    border-radius: var(--r-pill);
    color: #eef1f8;
    font-family: var(--font);
    font-size: 14px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .pill-trigger:hover {
    background: rgb(40 45 58 / 0.94);
  }

  .pill-trigger.active,
  .pill-trigger.active:hover {
    background: #eef1f8;
    color: #0b0e18;
  }

  /* Barre de réactions rapides */
  .quick-reactions-bar {
    position: absolute;
    bottom: 90px;
    right: 36px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    background: rgb(17 21 31 / 0.92);
    border: 0;
    border-radius: var(--r-pill);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    z-index: 8;
  }

  .reaction-btn {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    padding: 0;
    background: transparent;
    border: 0;
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    border-radius: 50%;
    transition:
      background-color var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }

  .reaction-btn:hover {
    background: rgb(255 255 255 / 0.14);
    transform: scale(1.08);
  }

  .reaction-btn:active {
    transform: scale(0.96);
  }

  @keyframes slideInRight {
    from { transform: translateX(24px); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }

  .pill-trigger:focus-visible,
  .reaction-btn:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  /* Téléphone : panneau en bas d'écran, barre de réactions recentrée */
  @media (max-width: 639px) {
    @keyframes slideUp {
      from { transform: translateY(24px); opacity: 0; }
      to { transform: translateY(0); opacity: 1; }
    }
    .quick-reactions-bar {
      right: 50%;
      transform: translateX(50%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
  .floating-reaction,
  .sync-toast,
  .countdown-number {
          animation: none;
        }
  .floating-reaction { opacity: 0; }
}
</style>

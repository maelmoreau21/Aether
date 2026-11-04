<script lang="ts">
  import { session } from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import type { SyncPartyClient } from './sync-client.svelte';
  import type { ControlMode } from './types';

  let {
    client,
    onCloseParty
  }: {
    client: SyncPartyClient;
    onCloseParty?: () => void;
  } = $props();

  let drawerOpen = $state(false);
  let activeTab = $state<'chat' | 'members' | 'queue' | 'polls'>('chat');
  let chatInput = $state('');
  let muteOverlay = $state(false);

  // Poll creation state
  let pollQuestion = $state('');
  let pollOption1 = $state('');
  let pollOption2 = $state('');
  let pollOption3 = $state('');
  let pollCreating = $state(false);

  const isHost = $derived(client.isHost);
  const isWaiting = $derived(client.party?.state === 'waiting');
  const selfMember = $derived(client.members.find(m => m.userId === $session?.user.id));
  const isReady = $derived(selfMember?.isReady ?? false);
  const allReady = $derived(
    client.members.length > 0 && client.members.every(m => m.isReady || m.role === 'host')
  );

  const onlineMembers = $derived(client.members.filter(m => m.isOnline));

  const quickEmojis = ['❤️', '😂', '👏', '🍿', '😱', '🔥'];

  function sendChatMessage() {
    if (!chatInput.trim()) return;
    client.sendChat(chatInput.trim());
    chatInput = '';
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendChatMessage();
    }
  }

  function toggleReady() {
    client.setReady(!isReady);
  }

  function startParty() {
    if (!isHost) return;
    // Launch with a 3-second countdown
    client.play(0, 3);
  }

  function handlePollSubmit() {
    const opts = [pollOption1.trim(), pollOption2.trim(), pollOption3.trim()].filter(Boolean);
    if (!pollQuestion.trim() || opts.length < 2) return;
    client.createPoll(pollQuestion.trim(), opts);
    pollQuestion = '';
    pollOption1 = '';
    pollOption2 = '';
    pollOption3 = '';
    pollCreating = false;
  }

  function copyInviteLink() {
    if (typeof window === 'undefined' || !client.party) return;
    const url = `${window.location.origin}/parties?id=${encodeURIComponent(client.party.id)}`;
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
      <span class="countdown-label">Départ collectif...</span>
    </div>
  </div>
{/if}

<!-- Salon d'attente (Waiting Room) -->
{#if isWaiting}
  <div class="waiting-room-overlay" role="dialog" aria-label="Salon d'attente de la séance">
    <div class="waiting-room-card">
      <header class="waiting-header">
        <div class="badge-live">
          <span class="pulse-dot"></span>
          <span>SALON D'ATTENTE</span>
        </div>
        <h2>{client.party?.name ?? 'Séance communautaire'}</h2>
        <p class="waiting-media-title">Film / Épisode : <strong>{client.party?.mediaTitle}</strong></p>
      </header>

      <section class="members-lobby" aria-label="Participants dans le salon">
        <div class="lobby-header">
          <h3>Participants ({client.members.length})</h3>
          <button type="button" class="btn-copy-invite" onclick={copyInviteLink}>
            <Icon name="copy" size={14} />
            <span>Copier le lien d'invitation</span>
          </button>
        </div>

        <div class="members-grid">
          {#each client.members as member (member.userId)}
            <div class="member-pill" class:is-ready={member.isReady} class:is-host={member.role === 'host'}>
              <div class="member-avatar">
                {member.displayName.slice(0, 2).toUpperCase()}
              </div>
              <div class="member-info">
                <span class="member-name">{member.displayName}</span>
                <span class="member-role">
                  {#if member.role === 'host'}
                    <Icon name="star" size={14} /> Organisateur
                  {:else if member.isReady}
                    <Icon name="check" size={14} /> Prêt
                  {:else}
                    <Icon name="clock" size={14} /> En attente
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
              Contrôles de lecture :
              <select
                value={client.party?.controlMode ?? 'host_only'}
                onchange={(e) => updateControlMode(e.currentTarget.value as ControlMode)}
              >
                <option value="host_only">Organisateur uniquement</option>
                <option value="shared">Contrôles partagés (tous)</option>
                <option value="request">Contrôle sur demande</option>
              </select>
            </label>

            <label class="checkbox-label">
              <input
                type="checkbox"
                checked={client.party?.waitForBuffering ?? true}
                onchange={toggleWaitForBuffering}
              />
              <span>Attendre les participants en buffering</span>
            </label>

            <label class="checkbox-label">
              <input
                type="checkbox"
                checked={client.party?.isLocked ?? false}
                onchange={toggleLock}
              />
              <span>Verrouiller la séance (interdire nouveaux arrivants)</span>
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
            <span>{isReady ? 'Je suis prêt' : 'Me marquer comme prêt'}</span>
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
            <span>Lancer la séance pour tous</span>
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
          Quitter la séance
        </button>
      </footer>
    </div>
  </div>
{/if}

<!-- Top Right Trigger Pill (during active playback) -->
<div class="party-status-pill">
  <button
    type="button"
    class="pill-trigger"
    class:active={drawerOpen}
    aria-label="Ouvrir le panneau communautaire"
    onclick={() => (drawerOpen = !drawerOpen)}
  >
    <Icon name="users" size={16} />
    <span>{onlineMembers.length}</span>
  </button>
</div>

<!-- Bottom Quick Reactions Bar (when controls are visible or hover) -->
{#if !isWaiting && !muteOverlay}
  <div class="quick-reactions-bar" aria-label="Réactions rapides">
    {#each quickEmojis as emoji (emoji)}
      <button
        type="button"
        class="reaction-btn"
        aria-label={`Réagir avec ${emoji}`}
        onclick={() => client.sendReaction(emoji)}
      >
        {emoji}
      </button>
    {/each}
  </div>
{/if}

<!-- Community Drawer (Chat, Members, Queue, Polls) -->
{#if drawerOpen}
  <aside class="community-drawer" aria-label="Panneau interactif de la séance">
    <header class="drawer-header">
      <div class="drawer-title">
        <Icon name="users" size={18} />
        <strong>Séance : {client.party?.name}</strong>
      </div>
      <div class="header-tools">
        <button
          type="button"
          class="btn-icon"
          title={muteOverlay ? 'Afficher les réactions' : 'Masquer les réactions'}
          onclick={() => (muteOverlay = !muteOverlay)}
        >
          <Icon name={muteOverlay ? 'eye-off' : 'eye'} size={16} />
        </button>
        <button
          type="button"
          class="btn-icon"
          aria-label="Fermer le panneau"
          onclick={() => (drawerOpen = false)}
        >
          <Icon name="close" size={18} />
        </button>
      </div>
    </header>

    <!-- Navigation Tabs -->
    <nav class="drawer-tabs" aria-label="Onglets communautaires">
      <button
        type="button"
        class="tab-btn"
        class:active={activeTab === 'chat'}
        onclick={() => (activeTab = 'chat')}
      >
        Chat
      </button>
      <button
        type="button"
        class="tab-btn"
        class:active={activeTab === 'members'}
        onclick={() => (activeTab = 'members')}
      >
        Participants ({onlineMembers.length})
      </button>
      <button
        type="button"
        class="tab-btn"
        class:active={activeTab === 'queue'}
        onclick={() => (activeTab = 'queue')}
      >
        File ({client.queue.length})
      </button>
      <button
        type="button"
        class="tab-btn"
        class:active={activeTab === 'polls'}
        onclick={() => (activeTab = 'polls')}
      >
        Sondage
      </button>
    </nav>

    <!-- Tab Contents -->
    <div class="drawer-content">
      {#if activeTab === 'chat'}
        <div class="chat-pane">
          <div class="chat-messages" role="log" aria-live="polite">
            {#each client.messages as msg (msg.id)}
              <div class="chat-bubble" class:is-self={msg.userId === $session?.user.id}>
                <div class="chat-author">
                  <strong>{msg.displayName}</strong>
                  <time>{new Date(msg.createdAt * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
                </div>
                <p class="chat-text">{msg.content}</p>
              </div>
            {:else}
              <p class="empty-state">Aucun message pour l'instant. Dites bonjour !</p>
            {/each}
          </div>

          <div class="chat-input-wrapper">
            <input
              type="text"
              maxlength="500"
              placeholder="Écrire un message…"
              bind:value={chatInput}
              onkeydown={handleKeydown}
            />
            <button
              type="button"
              class="btn-send"
              disabled={!chatInput.trim()}
              onclick={sendChatMessage}
            >
              Envoyer
            </button>
          </div>
        </div>
      {:else if activeTab === 'members'}
        <div class="members-pane">
          <div class="members-list">
            {#each client.members as m (m.userId)}
              <div class="member-card" class:offline={!m.isOnline}>
                <div class="card-avatar">
                  {m.displayName.slice(0, 2).toUpperCase()}
                </div>
                <div class="card-details">
                  <div class="name-line">
                    <strong>{m.displayName}</strong>
                    {#if m.role === 'host'}
                      <span class="badge-role"><Icon name="star" size={12} /> Organisateur</span>
                    {/if}
                  </div>
                  <div class="status-line">
                    <span
                      class="status-dot"
                      class:online={m.isOnline}
                      class:buffering={m.isOnline && m.isBuffering}
                    ></span>
                    <span>{m.isOnline ? (m.isBuffering ? 'Chargement…' : 'En ligne') : 'Absent'}</span>
                  </div>
                </div>

                <!-- Host Controls per member -->
                {#if isHost && m.userId !== $session?.user.id}
                  <div class="member-actions">
                    {#if client.party?.controlMode === 'request'}
                      <button
                        type="button"
                        class="btn-action-small"
                        title={m.hasControl ? 'Retirer le contrôle' : 'Donner le contrôle'}
                        onclick={() => m.hasControl ? client.revokeControl(m.userId) : client.grantControl(m.userId)}
                      >
                        {m.hasControl ? 'Retirer' : 'Autoriser'}
                      </button>
                    {/if}
                    <button
                      type="button"
                      class="btn-action-small"
                      title="Transférer le rôle d'organisateur"
                      aria-label="Transférer le rôle d'organisateur"
                      onclick={() => client.transferHost(m.userId)}
                    >
                      <Icon name="star" size={16} />
                    </button>
                    <button
                      type="button"
                      class="btn-action-small danger"
                      title="Exclure de la séance"
                      aria-label="Exclure de la séance"
                      onclick={() => client.kickMember(m.userId)}
                    >
                      <Icon name="close" size={16} />
                    </button>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {:else if activeTab === 'queue'}
        <div class="queue-pane">
          <div class="queue-list">
            {#each client.queue as item, idx (item.id)}
              <div class="queue-card">
                <div class="queue-info">
                  <strong>{item.mediaTitle}</strong>
                  <span>Ajouté par {item.addedByName}</span>
                </div>
                {#if client.canControl}
                  <div class="queue-btns">
                    <button
                      type="button"
                      class="btn-action-small"
                      title="Monter dans la file"
                      aria-label="Monter dans la file"
                      disabled={idx === 0}
                      onclick={() => client.moveQueueItem(item.id, idx - 1)}
                    >
                      <span class="chev-up"><Icon name="chevron" size={16} /></span>
                    </button>
                    <button
                      type="button"
                      class="btn-action-small"
                      title="Descendre dans la file"
                      aria-label="Descendre dans la file"
                      disabled={idx === client.queue.length - 1}
                      onclick={() => client.moveQueueItem(item.id, idx + 1)}
                    >
                      <span class="chev-down"><Icon name="chevron" size={16} /></span>
                    </button>
                    <button
                      type="button"
                      class="btn-action-small"
                      title="Lancer maintenant"
                      onclick={() => client.playQueueItem(item.id)}
                    >
                      <Icon name="play" size={14} />
                      <span>Lancer</span>
                    </button>
                    <button
                      type="button"
                      class="btn-action-small danger"
                      title="Retirer"
                      aria-label="Retirer"
                      onclick={() => client.removeFromQueue(item.id)}
                    >
                      <Icon name="close" size={16} />
                    </button>
                  </div>
                {/if}
              </div>
            {:else}
              <p class="empty-state">La file d'attente est vide.</p>
            {/each}
          </div>
        </div>
      {:else if activeTab === 'polls'}
        <div class="polls-pane">
          {#if client.poll}
            <div class="active-poll-card">
              <h4>{client.poll.question}</h4>
              <p class="poll-meta">
                {client.poll.totalVotes} vote{client.poll.totalVotes > 1 ? 's' : ''} ·
                {client.poll.isClosed ? 'Terminé' : 'En cours'}
              </p>

              <div class="poll-options">
                {#each client.poll.options as option, idx (option.text || idx)}
                  {@const pct = client.poll.totalVotes > 0 ? Math.round((option.votes / client.poll.totalVotes) * 100) : 0}
                  {@const hasVoted = option.voterIds.includes($session?.user.id ?? '')}
                  <button
                    type="button"
                    class="poll-option-btn"
                    class:voted={hasVoted}
                    disabled={client.poll.isClosed}
                    onclick={() => client.votePoll(client.poll!.id, idx)}
                  >
                    <div class="option-fill" style:width={`${pct}%`}></div>
                    <span class="option-text">{option.text}</span>
                    <span class="option-pct">{pct}%</span>
                  </button>
                {/each}
              </div>

              {#if isHost && !client.poll.isClosed}
                <button
                  type="button"
                  class="btn-close-poll"
                  onclick={() => client.closePoll(client.poll!.id)}
                >
                  Clôturer le sondage
                </button>
              {/if}
            </div>
          {:else if isHost}
            {#if pollCreating}
              <div class="create-poll-form">
                <h4>Créer un sondage</h4>
                <label>
                  Question :
                  <input type="text" placeholder="Ex: Quel film ensuite ?" bind:value={pollQuestion} />
                </label>
                <label>
                  Option 1 :
                  <input type="text" placeholder="Option 1" bind:value={pollOption1} />
                </label>
                <label>
                  Option 2 :
                  <input type="text" placeholder="Option 2" bind:value={pollOption2} />
                </label>
                <label>
                  Option 3 (facultatif) :
                  <input type="text" placeholder="Option 3" bind:value={pollOption3} />
                </label>
                <div class="poll-form-actions">
                  <button type="button" class="btn-send" onclick={handlePollSubmit}>Créer</button>
                  <button type="button" class="btn-cancel" onclick={() => (pollCreating = false)}>Annuler</button>
                </div>
              </div>
            {:else}
              <div class="empty-state">
                <p>Aucun sondage actif.</p>
                <button type="button" class="btn-send" onclick={() => (pollCreating = true)}>
                  Créer un sondage
                </button>
              </div>
            {/if}
          {:else}
            <p class="empty-state">Aucun sondage en cours.</p>
          {/if}
        </div>
      {/if}
    </div>
  </aside>
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
  @keyframes blink { 50% { opacity: 0.35; } }
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
  .member-avatar,
  .card-avatar {
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

  /* Panneau communautaire (chat, participants, file, sondage) */
  .community-drawer {
    position: absolute;
    top: 12px;
    right: 12px;
    bottom: 12px;
    width: min(400px, calc(100% - 24px));
    background: rgb(17 21 31 / 0.92);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    border: 0;
    border-radius: 24px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    z-index: 18;
    color: #eef1f8;
    font-family: var(--font);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    animation: slideInRight var(--t-med) var(--ease);
  }
  @keyframes slideInRight {
    from { transform: translateX(24px); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 14px 10px 20px;
    flex: none;
  }
  .drawer-title {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    color: #a2a9ba;
    font-size: 17px;
  }
  .drawer-title strong {
    font-weight: 800;
    letter-spacing: -0.01em;
    color: #eef1f8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .header-tools {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
  }
  .btn-icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    padding: 0;
    background: transparent;
    border: 0;
    color: #eef1f8;
    border-radius: 14px;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }
  .btn-icon:hover { background: rgb(255 255 255 / 0.1); }

  .drawer-tabs {
    display: flex;
    flex: none;
    gap: 4px;
    margin: 0 14px 6px;
    padding: 4px;
    border-radius: var(--r-pill);
    background: rgb(255 255 255 / 0.055);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .drawer-tabs::-webkit-scrollbar { display: none; }
  .tab-btn {
    flex: 1 0 auto;
    min-height: 36px;
    background: transparent;
    border: 0;
    border-radius: var(--r-pill);
    color: #a2a9ba;
    padding: 0 12px;
    font-size: 13px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }
  .tab-btn:hover { color: #eef1f8; }
  .tab-btn.active,
  .tab-btn.active:hover {
    background: #eef1f8;
    color: #0b0e18;
  }

  .drawer-content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    display: flex;
    flex-direction: column;
  }

  /* Focus visible commun */
  .community-drawer button:focus-visible,
  .community-drawer input:focus-visible,
  .waiting-room-card button:focus-visible,
  .waiting-room-card input[type='checkbox']:focus-visible,
  .pill-trigger:focus-visible,
  .reaction-btn:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  /* Chat */
  .chat-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }
  .chat-messages {
    flex: 1;
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
  }
  .chat-bubble {
    background: rgb(255 255 255 / 0.09);
    border: 0;
    padding: 10px 14px;
    border-radius: 16px 16px 16px 6px;
    align-self: flex-start;
    max-width: 85%;
  }
  .chat-bubble.is-self {
    background: rgb(255 255 255 / 0.16);
    border-radius: 16px 16px 6px 16px;
    align-self: flex-end;
  }
  .chat-author {
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-size: 12px;
    color: #a2a9ba;
    margin-bottom: 2px;
  }
  .chat-author strong {
    color: #eef1f8;
    font-weight: 700;
  }
  .chat-author time { font-variant-numeric: tabular-nums; }
  .chat-text {
    margin: 0;
    font-size: 14px;
    line-height: 1.45;
    color: #eef1f8;
    word-break: break-word;
  }
  .chat-input-wrapper {
    flex: none;
    padding: 12px 14px 14px;
    border-top: 1px solid rgb(255 255 255 / 0.09);
    display: flex;
    gap: 8px;
  }
  .chat-input-wrapper input[type='text'],
  .create-poll-form input[type='text'] {
    min-height: 46px;
    height: 46px;
    background: rgb(255 255 255 / 0.09);
    border: 1px solid rgb(255 255 255 / 0.09);
    border-radius: 12px;
    color: #eef1f8;
    padding: 0 14px;
    font-size: 15px;
    box-shadow: none;
    transition: border-color var(--t-fast) var(--ease);
  }
  .chat-input-wrapper input[type='text'] {
    flex: 1;
    min-width: 0;
  }
  .chat-input-wrapper input[type='text']::placeholder,
  .create-poll-form input[type='text']::placeholder {
    color: #8189a0;
  }
  .chat-input-wrapper input[type='text']:focus,
  .create-poll-form input[type='text']:focus {
    outline: none;
    border-color: var(--ac);
    box-shadow: none;
  }
  .btn-send,
  .btn-cancel,
  .btn-close-poll {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 46px;
    padding: 0 18px;
    border: 0;
    border-radius: 16px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }
  .btn-send {
    flex: none;
    background: #eef1f8;
    color: #0b0e18;
  }
  .btn-send:hover:not(:disabled) { background: #ffffff; }
  .btn-send:disabled { opacity: 0.45; cursor: not-allowed; }
  .btn-cancel,
  .btn-close-poll {
    background: rgb(255 255 255 / 0.14);
    color: #eef1f8;
  }
  .btn-cancel:hover,
  .btn-close-poll:hover { background: rgb(255 255 255 / 0.24); }

  /* Participants */
  .members-pane { padding: 8px 14px 16px; }
  .members-list { display: flex; flex-direction: column; gap: 4px; }
  .member-card {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 60px;
    background: transparent;
    border: 0;
    padding: 8px 8px 8px 10px;
    border-radius: 16px;
    transition: background-color var(--t-fast) var(--ease);
  }
  .member-card:hover { background: rgb(255 255 255 / 0.055); }
  .member-card.offline { opacity: 0.6; }
  .card-details {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name-line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px 8px;
    min-width: 0;
  }
  .name-line strong {
    font-size: 15px;
    font-weight: 700;
    color: #eef1f8;
    overflow-wrap: anywhere;
  }
  .badge-role {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-height: 22px;
    padding: 0 8px;
    border-radius: var(--r-pill);
    background: rgb(255 255 255 / 0.09);
    color: #eef1f8;
    font-size: 12px;
    font-weight: 700;
  }
  .status-line {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: #a2a9ba;
  }
  .status-dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #8189a0;
  }
  .status-dot.online { background: var(--ok); }
  .status-dot.online.buffering { background: var(--warn); }
  .member-actions,
  .queue-btns {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 4px;
    flex: none;
  }
  .btn-action-small {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 44px;
    min-height: 44px;
    padding: 0 12px;
    background: rgb(255 255 255 / 0.14);
    border: 0;
    color: #eef1f8;
    border-radius: 14px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    transition:
      background-color var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }
  .btn-action-small:hover:not(:disabled) { background: rgb(255 255 255 / 0.24); }
  .btn-action-small.danger { color: var(--err); }
  .btn-action-small.danger:hover:not(:disabled) { background: var(--err-soft); }
  .btn-action-small:disabled { opacity: 0.45; cursor: not-allowed; }
  .chev-up,
  .chev-down {
    display: grid;
    place-items: center;
  }
  .chev-up { transform: rotate(-90deg); }
  .chev-down { transform: rotate(90deg); }

  /* File d'attente */
  .queue-pane { padding: 8px 14px 16px; }
  .queue-list { display: flex; flex-direction: column; gap: 4px; }
  .queue-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px 12px;
    min-height: 60px;
    background: rgb(255 255 255 / 0.055);
    border: 0;
    padding: 8px 8px 8px 14px;
    border-radius: 16px;
  }
  .queue-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1 1 140px;
    min-width: 0;
  }
  .queue-info strong {
    font-size: 15px;
    font-weight: 700;
    color: #eef1f8;
    overflow-wrap: anywhere;
  }
  .queue-info span { font-size: 13px; color: #a2a9ba; }

  /* Sondage */
  .polls-pane { padding: 8px 14px 16px; }
  .active-poll-card {
    background: rgb(255 255 255 / 0.055);
    border: 0;
    padding: 16px;
    border-radius: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .active-poll-card h4 {
    margin: 0;
    color: #eef1f8;
    font-size: 17px;
    font-weight: 700;
    overflow-wrap: anywhere;
  }
  .poll-meta { margin: 0; font-size: 13px; color: #a2a9ba; }
  .poll-options { display: flex; flex-direction: column; gap: 6px; }
  .poll-option-btn {
    position: relative;
    overflow: hidden;
    min-height: 48px;
    background: rgb(255 255 255 / 0.09);
    border: 0;
    border-radius: 12px;
    padding: 0 14px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    color: #eef1f8;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }
  .poll-option-btn:hover:not(:disabled) { background: rgb(255 255 255 / 0.14); }
  .poll-option-btn.voted { box-shadow: inset 0 0 0 2px var(--ac); }
  .poll-option-btn:disabled { opacity: 1; cursor: default; }
  .option-fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--ac-soft);
    pointer-events: none;
    transition: width var(--t-slow) var(--ease);
  }
  .option-text {
    position: relative;
    z-index: 1;
    font-size: 14px;
    font-weight: 700;
    overflow-wrap: anywhere;
  }
  .option-pct {
    position: relative;
    z-index: 1;
    flex: none;
    font-size: 13px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
    color: #eef1f8;
  }
  .poll-option-btn.voted .option-pct { color: var(--ac); }

  .create-poll-form { display: flex; flex-direction: column; gap: 12px; }
  .create-poll-form h4 {
    margin: 0 0 4px;
    color: #eef1f8;
    font-size: 17px;
    font-weight: 700;
  }
  .create-poll-form label {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 14px;
    font-weight: 700;
    color: #eef1f8;
  }
  .poll-form-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 4px; }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    margin: 0;
    text-align: center;
    color: #a2a9ba;
    font-size: 14px;
    padding: 32px 16px;
  }
  .empty-state p { margin: 0; }

  /* Téléphone : panneau en bas d'écran, barre de réactions recentrée */
  @media (max-width: 639px) {
    .community-drawer {
      top: auto;
      left: 0;
      right: 0;
      bottom: 0;
      width: 100%;
      height: min(72dvh, 100%);
      border-radius: 24px 24px 0 0;
      padding-bottom: env(safe-area-inset-bottom, 0px);
      animation-name: slideUp;
    }
    @keyframes slideUp {
      from { transform: translateY(24px); opacity: 0; }
      to { transform: translateY(0); opacity: 1; }
    }
    .quick-reactions-bar {
      right: 50%;
      transform: translateX(50%);
    }
    .waiting-room-overlay {
      padding: 12px;
      place-items: start center;
    }
    .waiting-room-card {
      padding: 20px 16px;
      gap: 20px;
    }
    .members-grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .waiting-actions {
      flex-direction: column-reverse;
    }
    .waiting-actions > button {
      width: 100%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .floating-reaction,
    .sync-toast,
    .countdown-number,
    .pulse-dot,
    .community-drawer {
      animation: none;
    }
    .floating-reaction { opacity: 0; }
  }
</style>

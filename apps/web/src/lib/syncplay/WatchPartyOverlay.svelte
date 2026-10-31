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
                    👑 Organisateur
                  {:else if member.isReady}
                    ✓ Prêt
                  {:else}
                    ⏳ En attente
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
            {isReady ? '✓ Je suis prêt' : '⏳ Me marquer comme prêt'}
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
                      <span class="badge-role">👑 Organisateur</span>
                    {/if}
                  </div>
                  <div class="status-line">
                    <span class="status-dot" class:online={m.isOnline}></span>
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
                        {m.hasControl ? '🎮 Retirer' : '🎮 Autoriser'}
                      </button>
                    {/if}
                    <button
                      type="button"
                      class="btn-action-small"
                      title="Transférer le rôle d'organisateur"
                      onclick={() => client.transferHost(m.userId)}
                    >
                      👑
                    </button>
                    <button
                      type="button"
                      class="btn-action-small danger"
                      title="Exclure de la séance"
                      onclick={() => client.kickMember(m.userId)}
                    >
                      🚫
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
            {#each client.queue as item (item.id)}
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
                      title="Lancer maintenant"
                      onclick={() => client.changeMedia(item.mediaId)}
                    >
                      ▶ Lancer
                    </button>
                    <button
                      type="button"
                      class="btn-action-small danger"
                      title="Retirer"
                      onclick={() => client.removeFromQueue(item.id)}
                    >
                      ✕
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
  /* Floating Reactions */
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
    gap: 4px;
    animation: floatUp 3.2s cubic-bezier(0.2, 0.8, 0.2, 1) forwards;
  }
  .reaction-emoji {
    font-size: 38px;
    filter: drop-shadow(0 4px 12px rgba(0, 0, 0, 0.6));
  }
  .reaction-name {
    font-size: 11px;
    font-weight: 600;
    color: #fff;
    background: rgba(0, 0, 0, 0.6);
    padding: 2px 8px;
    border-radius: 12px;
    backdrop-filter: blur(8px);
  }
  @keyframes floatUp {
    0% { transform: translateY(0) scale(0.6); opacity: 0; }
    15% { transform: translateY(-30px) scale(1.15); opacity: 1; }
    80% { opacity: 0.9; }
    100% { transform: translateY(-320px) scale(0.9); opacity: 0; }
  }

  /* Sync Toast */
  .sync-toast {
    position: absolute;
    top: 75px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 18px;
    background: rgba(20, 22, 32, 0.92);
    border: 1px solid rgba(249, 115, 22, 0.4);
    border-radius: 20px;
    color: #fff;
    font-size: 13px;
    font-weight: 500;
    backdrop-filter: blur(16px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.6);
    z-index: 15;
    animation: fadeInDown 0.3s ease;
  }
  @keyframes fadeInDown {
    from { opacity: 0; transform: translate(-50%, -12px); }
    to { opacity: 1; transform: translate(-50%, 0); }
  }

  /* Countdown */
  .countdown-backdrop {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    display: grid;
    place-items: center;
    z-index: 20;
    backdrop-filter: blur(8px);
  }
  .countdown-circle {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }
  .countdown-number {
    font-size: 96px;
    font-weight: 900;
    color: var(--accent-primary, #f97316);
    text-shadow: 0 0 40px rgba(249, 115, 22, 0.6);
    animation: scalePulse 1s ease infinite;
  }
  .countdown-label {
    font-size: 18px;
    font-weight: 600;
    color: #fff;
    letter-spacing: 0.05em;
  }
  @keyframes scalePulse {
    0% { transform: scale(0.85); opacity: 0.7; }
    50% { transform: scale(1.08); opacity: 1; }
    100% { transform: scale(0.95); opacity: 0.8; }
  }

  /* Waiting Room */
  .waiting-room-overlay {
    position: absolute;
    inset: 0;
    background: rgba(8, 10, 16, 0.96);
    backdrop-filter: blur(20px);
    display: grid;
    place-items: center;
    z-index: 25;
    padding: 24px;
    overflow-y: auto;
  }
  .waiting-room-card {
    width: min(720px, 100%);
    background: rgba(22, 26, 38, 0.95);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 16px;
    padding: 32px;
    display: flex;
    flex-direction: column;
    gap: 24px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.8);
  }
  .waiting-header {
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .badge-live {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: rgba(249, 115, 22, 0.15);
    color: var(--accent-primary, #f97316);
    padding: 4px 12px;
    border-radius: 20px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
  }
  .pulse-dot {
    width: 8px;
    height: 8px;
    background: var(--accent-primary, #f97316);
    border-radius: 50%;
    box-shadow: 0 0 10px var(--accent-primary, #f97316);
    animation: blink 1.4s infinite;
  }
  @keyframes blink { 50% { opacity: 0.4; } }
  .waiting-header h2 {
    font-size: 24px;
    font-weight: 700;
    color: #fff;
    margin: 0;
  }
  .waiting-media-title {
    color: rgba(255, 255, 255, 0.7);
    font-size: 14px;
    margin: 0;
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
  }
  .lobby-header h3 {
    font-size: 14px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.9);
    margin: 0;
  }
  .btn-copy-invite {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    color: #fff;
    padding: 6px 12px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
  }
  .btn-copy-invite:hover {
    background: rgba(255, 255, 255, 0.15);
    border-color: var(--accent-primary, #f97316);
  }

  .members-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 10px;
  }
  .member-pill {
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    padding: 10px;
    border-radius: 10px;
    transition: all 0.2s;
  }
  .member-pill.is-ready {
    border-color: rgba(34, 197, 94, 0.4);
    background: rgba(34, 197, 94, 0.08);
  }
  .member-avatar {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--accent-primary, #f97316);
    color: #fff;
    display: grid;
    place-items: center;
    font-size: 13px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .member-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .member-name {
    font-size: 13px;
    font-weight: 600;
    color: #fff;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .member-role {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.6);
  }
  .member-pill.is-ready .member-role {
    color: #4ade80;
    font-weight: 600;
  }

  .host-settings-panel {
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.08);
    padding: 16px;
    border-radius: 12px;
  }
  .settings-row {
    display: flex;
    flex-direction: column;
    gap: 12px;
    font-size: 13px;
    color: #fff;
  }
  .settings-row select {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #fff;
    padding: 6px 10px;
    border-radius: 6px;
    margin-left: 8px;
  }
  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .waiting-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    justify-content: flex-end;
    border-top: 1px solid rgba(255, 255, 255, 0.1);
    padding-top: 20px;
  }
  .btn-start {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: var(--accent-primary, #f97316);
    color: #fff;
    border: 0;
    padding: 12px 24px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: all 0.2s;
  }
  .btn-start:hover:not(:disabled) {
    transform: translateY(-2px);
    box-shadow: 0 8px 24px rgba(249, 115, 22, 0.4);
  }
  .btn-start:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .btn-ready {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #fff;
    padding: 12px 20px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
  }
  .btn-ready.active {
    background: rgba(34, 197, 94, 0.2);
    border-color: #22c55e;
    color: #4ade80;
  }
  .btn-leave {
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: rgba(255, 255, 255, 0.7);
    padding: 12px 20px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  /* Status Pill in player */
  .party-status-pill {
    position: absolute;
    top: max(24px, env(safe-area-inset-top));
    right: 90px;
    z-index: 10;
  }
  .pill-trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    background: rgba(20, 24, 36, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 20px;
    color: #fff;
    font-size: 13px;
    font-weight: 600;
    backdrop-filter: blur(12px);
    cursor: pointer;
    transition: all 0.2s;
  }
  .pill-trigger:hover, .pill-trigger.active {
    background: rgba(249, 115, 22, 0.25);
    border-color: var(--accent-primary, #f97316);
  }

  /* Quick Reactions Bar */
  .quick-reactions-bar {
    position: absolute;
    bottom: 90px;
    right: 36px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: rgba(14, 18, 28, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 24px;
    backdrop-filter: blur(16px);
    z-index: 8;
  }
  .reaction-btn {
    background: transparent;
    border: 0;
    font-size: 20px;
    cursor: pointer;
    padding: 4px;
    border-radius: 50%;
    transition: transform 0.15s;
  }
  .reaction-btn:hover {
    transform: scale(1.3);
  }

  /* Community Drawer */
  .community-drawer {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(380px, 100%);
    background: rgba(16, 20, 30, 0.96);
    backdrop-filter: blur(24px);
    border-left: 1px solid rgba(255, 255, 255, 0.12);
    display: flex;
    flex-direction: column;
    z-index: 18;
    box-shadow: -12px 0 40px rgba(0, 0, 0, 0.8);
    animation: slideInRight 0.25s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  @keyframes slideInRight {
    from { transform: translateX(100%); }
    to { transform: translateX(0); }
  }
  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }
  .drawer-title {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #fff;
    font-size: 14px;
  }
  .header-tools {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .btn-icon {
    background: transparent;
    border: 0;
    color: rgba(255, 255, 255, 0.7);
    padding: 6px;
    border-radius: 6px;
    cursor: pointer;
  }
  .btn-icon:hover { color: #fff; background: rgba(255, 255, 255, 0.1); }

  .drawer-tabs {
    display: flex;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }
  .tab-btn {
    flex: 1;
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    color: rgba(255, 255, 255, 0.6);
    padding: 12px 8px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
  }
  .tab-btn.active {
    color: var(--accent-primary, #f97316);
    border-bottom-color: var(--accent-primary, #f97316);
  }

  .drawer-content {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  /* Chat Pane */
  .chat-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .chat-messages {
    flex: 1;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
  }
  .chat-bubble {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.06);
    padding: 8px 12px;
    border-radius: 10px;
    align-self: flex-start;
    max-width: 85%;
  }
  .chat-bubble.is-self {
    background: rgba(249, 115, 22, 0.15);
    border-color: rgba(249, 115, 22, 0.3);
    align-self: flex-end;
  }
  .chat-author {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.6);
    margin-bottom: 4px;
  }
  .chat-author strong { color: #fff; }
  .chat-text {
    margin: 0;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.95);
    word-break: break-word;
  }
  .chat-input-wrapper {
    padding: 12px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    gap: 8px;
  }
  .chat-input-wrapper input {
    flex: 1;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 8px;
    color: #fff;
    padding: 10px 12px;
    font-size: 13px;
  }
  .chat-input-wrapper input:focus {
    outline: none;
    border-color: var(--accent-primary, #f97316);
  }
  .btn-send {
    background: var(--accent-primary, #f97316);
    border: 0;
    color: #fff;
    padding: 10px 14px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-send:disabled { opacity: 0.5; cursor: not-allowed; }

  /* Members Pane */
  .members-pane { padding: 16px; }
  .members-list { display: flex; flex-direction: column; gap: 8px; }
  .member-card {
    display: flex;
    align-items: center;
    gap: 12px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    padding: 10px 12px;
    border-radius: 10px;
  }
  .member-card.offline { opacity: 0.6; }
  .card-avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--accent-primary, #f97316);
    color: #fff;
    display: grid;
    place-items: center;
    font-size: 12px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .card-details { flex: 1; min-width: 0; }
  .name-line { display: flex; align-items: center; gap: 6px; }
  .name-line strong { font-size: 13px; color: #fff; }
  .badge-role { font-size: 10px; color: #f59e0b; }
  .status-line { display: flex; align-items: center; gap: 6px; font-size: 11px; color: rgba(255, 255, 255, 0.5); }
  .status-dot { width: 6px; height: 6px; border-radius: 50%; background: #ef4444; }
  .status-dot.online { background: #22c55e; }
  .member-actions { display: flex; align-items: center; gap: 4px; }
  .btn-action-small {
    background: rgba(255, 255, 255, 0.1);
    border: 0;
    color: #fff;
    padding: 4px 8px;
    border-radius: 6px;
    font-size: 11px;
    cursor: pointer;
  }
  .btn-action-small.danger { color: #f87171; }
  .btn-action-small:hover { background: rgba(255, 255, 255, 0.2); }

  /* Queue Pane */
  .queue-pane { padding: 16px; }
  .queue-list { display: flex; flex-direction: column; gap: 8px; }
  .queue-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    padding: 10px 14px;
    border-radius: 10px;
  }
  .queue-info { display: flex; flex-direction: column; gap: 2px; }
  .queue-info strong { font-size: 13px; color: #fff; }
  .queue-info span { font-size: 11px; color: rgba(255, 255, 255, 0.5); }
  .queue-btns { display: flex; gap: 6px; }

  /* Polls Pane */
  .polls-pane { padding: 16px; }
  .active-poll-card {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    padding: 16px;
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .active-poll-card h4 { margin: 0; color: #fff; font-size: 15px; }
  .poll-meta { margin: 0; font-size: 11px; color: rgba(255, 255, 255, 0.5); }
  .poll-options { display: flex; flex-direction: column; gap: 8px; }
  .poll-option-btn {
    position: relative;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 8px;
    padding: 10px 14px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #fff;
    cursor: pointer;
  }
  .poll-option-btn.voted { border-color: var(--accent-primary, #f97316); }
  .option-fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(249, 115, 22, 0.25);
    pointer-events: none;
    transition: width 0.3s ease;
  }
  .option-text { position: relative; z-index: 1; font-size: 13px; font-weight: 500; }
  .option-pct { position: relative; z-index: 1; font-size: 12px; font-weight: 700; color: var(--accent-primary, #f97316); }
  .btn-close-poll {
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: rgba(255, 255, 255, 0.7);
    padding: 8px;
    border-radius: 6px;
    font-size: 12px;
    cursor: pointer;
  }

  .create-poll-form { display: flex; flex-direction: column; gap: 10px; }
  .create-poll-form h4 { margin: 0 0 4px; color: #fff; }
  .create-poll-form label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: rgba(255, 255, 255, 0.7); }
  .create-poll-form input {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 6px;
    color: #fff;
    padding: 8px 10px;
    font-size: 13px;
  }
  .poll-form-actions { display: flex; gap: 8px; margin-top: 6px; }
  .btn-cancel {
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #fff;
    padding: 8px 14px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }

  .empty-state {
    text-align: center;
    color: rgba(255, 255, 255, 0.5);
    font-size: 13px;
    padding: 32px 16px;
  }
</style>

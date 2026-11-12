<script lang="ts">
  import { session } from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import type { SyncPartyClient } from './sync-client.svelte';
  import { initials } from './party-api';
  import { t, fmt } from '../i18n';

  let { client, isHost, onlineMembers, drawerOpen = $bindable(), muteOverlay = $bindable() }: { client: SyncPartyClient; isHost: boolean; onlineMembers: SyncPartyClient['members']; drawerOpen: boolean; muteOverlay: boolean } = $props();

  let activeTab = $state<'chat' | 'members' | 'queue' | 'polls'>('chat');
  let chatInput = $state('');

  // Poll creation state
  let pollQuestion = $state('');
  let pollOption1 = $state('');
  let pollOption2 = $state('');
  let pollOption3 = $state('');
  let pollCreating = $state(false);

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
</script>

<aside class="community-drawer" aria-label={$t('interactive_session_panel')}>
  <header class="drawer-header">
    <div class="drawer-title">
      <Icon name="users" size={18} />
      <strong>{$t('session', { name: client.party?.name })}</strong>
    </div>
    <div class="header-tools">
      <button
        type="button"
        class="btn-icon"
        title={muteOverlay ? $t('show_reactions') : $t('hide_reactions')}
        onclick={() => (muteOverlay = !muteOverlay)}
      >
        <Icon name={muteOverlay ? 'eye-off' : 'eye'} size={16} />
      </button>
      <button
        type="button"
        class="btn-icon"
        aria-label={$t('close_the_panel')}
        onclick={() => (drawerOpen = false)}
      >
        <Icon name="close" size={18} />
      </button>
    </div>
  </header>

  <!-- Navigation Tabs -->
  <nav class="drawer-tabs" aria-label={$t('community_tabs')}>
    <button
      type="button"
      class="tab-btn"
      class:active={activeTab === 'chat'}
      onclick={() => (activeTab = 'chat')}
    >
      {$t('chat')}
    </button>
    <button
      type="button"
      class="tab-btn"
      class:active={activeTab === 'members'}
      onclick={() => (activeTab = 'members')}
    >
      {$t('participants', { count: onlineMembers.length })}
    </button>
    <button
      type="button"
      class="tab-btn"
      class:active={activeTab === 'queue'}
      onclick={() => (activeTab = 'queue')}
    >
      {$t('queue', { count: client.queue.length })}
    </button>
    <button
      type="button"
      class="tab-btn"
      class:active={activeTab === 'polls'}
      onclick={() => (activeTab = 'polls')}
    >
      {$t('poll')}
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
                <time>{$fmt.time(msg.createdAt)}</time>
              </div>
              <p class="chat-text">{msg.content}</p>
            </div>
          {:else}
            <p class="empty-state">{$t('no_messages_yet_say_hello')}</p>
          {/each}
        </div>

        <div class="chat-input-wrapper">
          <input
            type="text"
            maxlength="500"
            placeholder={$t('write_a_message')}
            bind:value={chatInput}
            onkeydown={handleKeydown}
          />
          <button
            type="button"
            class="btn-send"
            disabled={!chatInput.trim()}
            onclick={sendChatMessage}
          >
            {$t('send')}
          </button>
        </div>
      </div>
    {:else if activeTab === 'members'}
      <div class="members-pane">
        <div class="members-list">
          {#each client.members as m (m.userId)}
            <div class="member-card" class:offline={!m.isOnline}>
              <div class="card-avatar">
                {initials(m.displayName)}
              </div>
              <div class="card-details">
                <div class="name-line">
                  <strong>{m.displayName}</strong>
                  {#if m.role === 'host'}
                    <span class="badge-role"><Icon name="star" size={12} /> {$t('organiser')}</span>
                  {/if}
                </div>
                <div class="status-line">
                  <span
                    class="status-dot"
                    class:online={m.isOnline}
                    class:buffering={m.isOnline && m.isBuffering}
                  ></span>
                  <span>{m.isOnline ? (m.isBuffering ? 'Chargement…' : $t('online')) : 'Absent'}</span>
                </div>
              </div>

              <!-- Host Controls per member -->
              {#if isHost && m.userId !== $session?.user.id}
                <div class="member-actions">
                  {#if client.party?.controlMode === 'request'}
                    <button
                      type="button"
                      class="btn-action-small"
                      title={m.hasControl ? $t('remove_control') : $t('give_control')}
                      onclick={() => m.hasControl ? client.revokeControl(m.userId) : client.grantControl(m.userId)}
                    >
                      {m.hasControl ? 'Retirer' : 'Autoriser'}
                    </button>
                  {/if}
                  <button
                    type="button"
                    class="btn-action-small"
                    title={$t('transfer_the_organiser_role')}
                    aria-label={$t('transfer_the_organiser_role')}
                    onclick={() => client.transferHost(m.userId)}
                  >
                    <Icon name="star" size={16} />
                  </button>
                  <button
                    type="button"
                    class="btn-action-small danger"
                    title={$t('remove_from_the_session')}
                    aria-label={$t('remove_from_the_session')}
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
                <span>{$t('added_by', { addedByName: item.addedByName })}</span>
              </div>
              {#if client.canControl}
                <div class="queue-btns">
                  <button
                    type="button"
                    class="btn-action-small"
                    title={$t('move_up_in_the_queue')}
                    aria-label={$t('move_up_in_the_queue')}
                    disabled={idx === 0}
                    onclick={() => client.moveQueueItem(item.id, idx - 1)}
                  >
                    <span class="chev-up"><Icon name="chevron" size={16} /></span>
                  </button>
                  <button
                    type="button"
                    class="btn-action-small"
                    title={$t('move_down_in_the_queue')}
                    aria-label={$t('move_down_in_the_queue')}
                    disabled={idx === client.queue.length - 1}
                    onclick={() => client.moveQueueItem(item.id, idx + 1)}
                  >
                    <span class="chev-down"><Icon name="chevron" size={16} /></span>
                  </button>
                  <button
                    type="button"
                    class="btn-action-small"
                    title={$t('start_now')}
                    onclick={() => client.playQueueItem(item.id)}
                  >
                    <Icon name="play" size={14} />
                    <span>{$t('start')}</span>
                  </button>
                  <button
                    type="button"
                    class="btn-action-small danger"
                    title={$t('remove')}
                    aria-label={$t('remove')}
                    onclick={() => client.removeFromQueue(item.id)}
                  >
                    <Icon name="close" size={16} />
                  </button>
                </div>
              {/if}
            </div>
          {:else}
            <p class="empty-state">{$t('the_queue_is_empty')}</p>
          {/each}
        </div>
      </div>
    {:else if activeTab === 'polls'}
      <div class="polls-pane">
        {#if client.poll}
          <div class="active-poll-card">
            <h4>{client.poll.question}</h4>
            <p class="poll-meta">
              {$t('poll_summary', { count: client.poll.totalVotes, state: client.poll.isClosed ? $t('done') : $t('stats_kpi_active') })}
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
                {$t('close_the_poll')}
              </button>
            {/if}
          </div>
        {:else if isHost}
          {#if pollCreating}
            <div class="create-poll-form">
              <h4>{$t('create_a_poll')}</h4>
              <label>
                {$t('question')}
                <input type="text" placeholder={$t('e_g_which_film_next')} bind:value={pollQuestion} />
              </label>
              <label>
                {$t('option_1')}
                <input type="text" placeholder={$t('player_option_1')} bind:value={pollOption1} />
              </label>
              <label>
                {$t('option_2')}
                <input type="text" placeholder={$t('player_option_2')} bind:value={pollOption2} />
              </label>
              <label>
                {$t('option_3_optional')}
                <input type="text" placeholder={$t('option_3')} bind:value={pollOption3} />
              </label>
              <div class="poll-form-actions">
                <button type="button" class="btn-send" onclick={handlePollSubmit}>{$t('create')}</button>
                <button type="button" class="btn-cancel" onclick={() => (pollCreating = false)}>{$t('sched_cancel')}</button>
              </div>
            </div>
          {:else}
            <div class="empty-state">
              <p>{$t('no_active_poll')}</p>
              <button type="button" class="btn-send" onclick={() => (pollCreating = true)}>
                {$t('create_a_poll')}
              </button>
            </div>
          {/if}
        {:else}
          <p class="empty-state">{$t('no_poll_in_progress')}</p>
        {/if}
      </div>
    {/if}
  </div>
</aside>

<style>
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

  .community-drawer input:focus-visible {
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

  @media (prefers-reduced-motion: reduce) {
  .community-drawer {
        animation: none;
      }
}
</style>

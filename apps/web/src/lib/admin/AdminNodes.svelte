<script lang="ts">
  import type { AdminNode, EnrollmentToken } from './types';
  import { nodeStatusLabels, connectionModeLabels } from './types';
  import Icon from '../ui/Icon.svelte';
  import { t, fmt } from '../i18n';

  interface Props {
    nodes: AdminNode[];
    pairing: boolean;
    nodeActionBusy: string | null;
    pairName: string;
    pairBaseUrl: string;
    pairPublicUrl: string;
    pairToken: string;
    onpair: (e: SubmitEvent) => void;
    onnodeaction: (node: AdminNode, action: 'reconnect' | 'rotate-secret' | 'revoke') => void;
    ongeneratetoken?: () => Promise<EnrollmentToken>;
  }

  let {
    nodes,
    pairing,
    nodeActionBusy,
    pairName = $bindable(),
    pairBaseUrl = $bindable(),
    pairPublicUrl = $bindable(),
    pairToken = $bindable(),
    onpair,
    onnodeaction,
    ongeneratetoken
  }: Props = $props();

  let onlineCount = $derived(nodes.filter(n => n.status === 'connected').length);
  let offlineCount = $derived(nodes.filter(n => n.status === 'disconnected').length);
  let libraryCount = $derived(nodes.reduce((sum, n) => sum + n.libraries.length, 0));

  let showEnrollModal = $state(false);
  let generatingToken = $state(false);
  let activeToken = $state<EnrollmentToken | null>(null);
  let tokenError = $state('');
  let copiedToken = $state(false);
  let copiedCompose = $state(false);

  function formatTimestamp(ts?: number | null): string {
    if (!ts) return $t('never');
    try {
      return $fmt.dateTime(ts);
    } catch {
      return String(ts);
    }
  }

  async function handleOpenAddNode() {
    showEnrollModal = true;
    tokenError = '';
    copiedToken = false;
    copiedCompose = false;
    if (!activeToken || activeToken.expiresAt * 1000 < Date.now()) {
      await generateNewToken();
    }
  }

  async function generateNewToken() {
    if (!ongeneratetoken) return;
    generatingToken = true;
    tokenError = '';
    copiedToken = false;
    copiedCompose = false;
    try {
      activeToken = await ongeneratetoken();
    } catch (e) {
      tokenError = e instanceof Error ? e.message : $t('error_while_generating_the_token');
    } finally {
      generatingToken = false;
    }
  }

  async function copyToClipboard(text: string, type: 'token' | 'compose') {
    try {
      await navigator.clipboard.writeText(text);
      if (type === 'token') {
        copiedToken = true;
        setTimeout(() => (copiedToken = false), 2500);
      } else {
        copiedCompose = true;
        setTimeout(() => (copiedCompose = false), 2500);
      }
    } catch {
      // Fallback if clipboard API is blocked
    }
  }

  function getDockerComposeSnippet(token: EnrollmentToken | null): string {
    const coreUrl = token?.coreUrl || 'https://aether.example.com';
    const tokenVal = token?.token || 'mnet_...';
    return `services:
  aether-node:
    image: ghcr.io/aether/node:latest
    container_name: aether-node
    restart: unless-stopped
    environment:
      MEDIA_CORE_URL: ${coreUrl}
      MEDIA_NODE_NAME: Media-Node-Distant
      MEDIA_NODE_ENROLLMENT_TOKEN: ${tokenVal}
      MEDIA_NODE_ROOTS: /media/films;/media/series
    volumes:
      - /opt/aether-node/data:/var/lib/aether-node
      - /mnt/storage/films:/media/films:ro
      - /mnt/storage/series:/media/series:ro`;
  }
</script>

<section id="nodes-section" class="dashboard-section" aria-labelledby="nodes-heading">
  <div class="section-header">
    <div class="header-left-part">
      <div class="header-icon-wrap">
        <Icon name="server" size={18} />
      </div>
      <div>
        <h2 id="nodes-heading" class="section-title">{$t('manage_nodes')}</h2>
        <p class="section-subtitle">
          {$t('monitor_your_remote_media_nodes_nat')}
        </p>
      </div>
    </div>
    {#if ongeneratetoken}
      <button class="primary add-node-btn" onclick={handleOpenAddNode}>
        <Icon name="server" size={14} />
        <span>{$t('add_node')}</span>
      </button>
    {/if}
  </div>

  {#if showEnrollModal}
    <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) showEnrollModal = false; }} role="presentation">
      <div class="modal-card" role="dialog" aria-modal="true" aria-labelledby="enroll-title">
        <div class="modal-header">
          <div class="modal-title-wrap">
            <div class="header-icon-wrap">
              <Icon name="shield" size={16} />
            </div>
            <div>
              <h3 id="enroll-title" class="modal-title">{$t('add_media_node')}</h3>
              <p class="modal-desc">{$t('automatic_pairing')}</p>
            </div>
          </div>
          <button class="icon-close-btn" onclick={() => (showEnrollModal = false)} aria-label={$t('close')}>✕</button>
        </div>

        <div class="modal-body">
          <div class="info-callout">
            <Icon name="info" size={16} />
            <span>
              {$t('node_https_note')} {$t('invitation_token_is')} <strong>{$t('unique_temporary_and_single_use')}</strong>.
            </span>
          </div>

          {#if tokenError}
            <div class="token-error" role="alert">{tokenError}</div>
          {/if}

          {#if generatingToken}
            <div class="token-loading">{$t('secure_token_generating')}</div>
          {:else if activeToken}
            <div class="token-section">
              <label for="enroll-token-display">{$t('invitation_token_valid_until', { value: formatTimestamp(activeToken.expiresAt) })}</label>
              <div class="token-input-row">
                <input
                  id="enroll-token-display"
                  class="token-field"
                  readonly
                  value={activeToken.token}
                  onclick={(e) => (e.target as HTMLInputElement).select()}
                />
                <button
                  class="copy-btn"
                  onclick={() => copyToClipboard(activeToken!.token, 'token')}
                >
                  <Icon name={copiedToken ? 'check' : 'server'} size={14} />
                  <span>{copiedToken ? $t('copied') : 'Copier'}</span>
                </button>
              </div>
            </div>

            <div class="compose-section">
              <div class="compose-header">
                <label for="compose-snippet">{$t('recommended_docker_compose_configuration')}</label>
                <button
                  class="copy-btn copy-compose"
                  onclick={() => copyToClipboard(getDockerComposeSnippet(activeToken), 'compose')}
                >
                  <Icon name={copiedCompose ? 'check' : 'film'} size={14} />
                  <span>{copiedCompose ? $t('copied') : $t('copy_the_yaml')}</span>
                </button>
              </div>
              <textarea
                id="compose-snippet"
                class="compose-textarea"
                readonly
                rows={14}
                value={getDockerComposeSnippet(activeToken)}
              ></textarea>
            </div>

            <div class="modal-steps">
              <p><strong>{$t('setup_steps')}</strong></p>
              <ol>
                <li>{$t('start_the_container_with_the_core')}</li>
                <li>{$t('the_node_contacts_the_core_mutually')}</li>
                <li>{$t('the_token_is_invalidated_immediately_the')}</li>
              </ol>
            </div>
          {/if}
        </div>

        <div class="modal-footer">
          <button class="action-btn" onclick={generateNewToken} disabled={generatingToken}>
            <Icon name="refresh" size={12} />
                <span>{$t('generate_another_token')}</span>
          </button>
          <button class="primary" onclick={() => (showEnrollModal = false)}>
                <span>{$t('done')}</span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <div class="nodes-layout">
    <!-- Summary Strip -->
    <div class="nodes-summary">
      <div class="summary-tile">
        <span class="summary-label">{$t('nodes')}</span>
        <strong class="summary-value">{nodes.length}</strong>
      </div>
      <div class="summary-tile">
        <span class="summary-label">{nodeStatusLabels.connected}</span>
        <strong class="summary-value ok">{onlineCount}</strong>
      </div>
      <div class="summary-tile">
        <span class="summary-label">{nodeStatusLabels.disconnected}</span>
        <strong class="summary-value" class:warn={offlineCount > 0}>{offlineCount}</strong>
      </div>
      <div class="summary-tile">
        <span class="summary-label">{$t('attached_libraries')}</span>
        <strong class="summary-value">{libraryCount}</strong>
      </div>
    </div>

    <!-- Registered Media Nodes List (Full Width) -->
    <section class="card" aria-labelledby="node-list">
      <div class="card-header">
        <h3 id="node-list">{$t('registered_media_nodes', { count: nodes.length })}</h3>
        <span class="badge count-badge">
          {$t('nodes_online_total', { online: onlineCount, total: nodes.length })}
        </span>
      </div>

      {#if nodes.length === 0}
        <p class="muted">{$t('no_nodes')}</p>
      {:else}
        <div class="nodes-list">
          {#each nodes as node (node.id)}
            <article class="node-card" data-status={node.status}>
              <div class="node-card-header">
                <div class="node-title-group">
                  <span class="node-status-icon" data-status={node.status} aria-hidden="true">
                    <Icon name="server" size={18} />
                  </span>
                  <div class="node-title-meta">
                    <div class="item-title-row">
                      <h4>{node.name}</h4>
                      <span class="badge" data-status={node.status}>
                        {nodeStatusLabels[node.status] || node.status}
                      </span>
                      <span class="badge kind-badge">
                        {node.kind === 'local' ? $t('local_built_in') : $t('remote_standalone')}
                      </span>
                      {#if node.kind === 'remote'}
                        <span class="badge mode-badge" data-mode={node.connectionMode || 'outbound'}>
                          {connectionModeLabels[node.connectionMode || 'outbound'] || node.connectionMode || $t('outbound_relay')}
                        </span>
                      {/if}
                      {#if node.version}
                        <span class="badge version-badge" title={$t('media_node_version_v', { version: node.version })}>
                          v{node.version}
                        </span>
                      {:else}
                        <span class="badge version-badge version-unknown" title={$t('version_not_reported')}>
                          v?
                        </span>
                      {/if}
                    </div>
                  </div>
                </div>

                {#if node.kind === 'remote' && node.status !== 'revoked'}
                  <div class="node-actions">
                    <button
                      class="action-btn compact-btn"
                      disabled={!!nodeActionBusy}
                      onclick={() => onnodeaction(node, 'reconnect')}
                      aria-label={$t('admin_reconnect', { name: node.name })}
                    >
                      <Icon name="refresh" size={14} />
                      <span>{$t('reconnect')}</span>
                    </button>
                    <button
                      class="action-btn compact-btn"
                      disabled={!!nodeActionBusy}
                      onclick={() => onnodeaction(node, 'rotate-secret')}
                      aria-label={$t('renew_the_secret_of', { name: node.name })}
                    >
                      <Icon name="shield" size={14} />
                      <span>{$t('renew_secret')}</span>
                    </button>
                    <button
                      class="action-btn compact-btn danger-btn"
                      disabled={!!nodeActionBusy}
                      onclick={() => onnodeaction(node, 'revoke')}
                      aria-label={$t('admin_revoke', { name: node.name })}
                    >
                      <span>{$t('revoke')}</span>
                    </button>
                  </div>
                {/if}
              </div>

              <dl class="node-specs">
                <div class="spec-item">
                  <dt>{$t('identifier')}</dt>
                  <dd><code>{node.id}</code></dd>
                </div>
                <div class="spec-item">
                  <dt>{$t('key')}</dt>
                  <dd><code>{node.keyId}</code></dd>
                </div>
                <div class="spec-item">
                  <dt>{$t('last_activity')}</dt>
                  <dd>{formatTimestamp(node.lastSeenAt)}</dd>
                </div>
                {#if node.baseUrl}
                  <div class="spec-item">
                    <dt>{$t('internal_url_core_node')}</dt>
                    <dd><code>{node.baseUrl}</code></dd>
                  </div>
                {/if}
                {#if node.publicUrl}
                  <div class="spec-item">
                    <dt>{$t('direct_public_url')}</dt>
                    <dd><code>{node.publicUrl}</code></dd>
                  </div>
                {/if}
              </dl>

              <div class="node-tags-row">
                {#if node.roots.length > 0}
                  <div class="tag-group">
                    <span class="tag-group-label">{$t('media_folders')}</span>
                    <div class="tag-list">
                      {#each node.roots as root (root)}
                        <span class="tag-pill mono">{root}</span>
                      {/each}
                    </div>
                  </div>
                {/if}
                <div class="tag-group">
                  <span class="tag-group-label">{$t('attached_libraries')}</span>
                  <div class="tag-list">
                    {#if node.libraries.length}
                      {#each node.libraries as lib (lib.id)}
                        <span class="tag-pill">{lib.name} <small>({lib.kind})</small></span>
                      {/each}
                    {:else}
                      <span class="tag-pill muted-pill">{$t('no_attached_libraries')}</span>
                    {/if}
                  </div>
                </div>
              </div>
            </article>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Legacy Inbound Pairing (Collapsible) -->
    <details class="card legacy-panel">
      <summary class="legacy-summary" id="node-legacy-form">
        <span class="legacy-summary-title">
          <Icon name="shield" size={16} />
          <span>{$t('direct_pairing_legacy')}</span>
        </span>
        <Icon name="chevron" size={16} />
      </summary>

      <div class="legacy-body">
        <p class="muted">
          {$t('for_nodes_with_an_open_port')} <strong>{$t('add_node')}</strong> {$t('at_the_top')}
        </p>

        <form onsubmit={onpair}>
          <fieldset disabled={pairing}>
            <div class="legacy-form-grid">
              <div>
                <label for="pair-node-name">{$t('node_name')}</label>
                <input
                  id="pair-node-name"
                  bind:value={pairName}
                  required
                  maxlength="100"
                  placeholder={$t('e_g_films_nas_node')}
                  autocomplete="off"
                />
              </div>

              <div>
                <label for="pair-node-token">{$t('bootstrap_token')}</label>
                <input
                  id="pair-node-token"
                  type="password"
                  bind:value={pairToken}
                  required
                  autocomplete="off"
                />
              </div>

              <div>
                <label for="pair-node-base-url">{$t('internal_node_url')}</label>
                <input
                  id="pair-node-base-url"
                  bind:value={pairBaseUrl}
                  required
                  placeholder="http://aether-node:3101"
                  autocomplete="off"
                />
              </div>

              <div>
                <label for="pair-node-public-url">{$t('public_stream_url')}</label>
                <input
                  id="pair-node-public-url"
                  bind:value={pairPublicUrl}
                  placeholder="https://node.example.com:3101"
                  autocomplete="off"
                />
              </div>
            </div>

            <div class="actions">
              <button class="primary" type="submit">
                {pairing ? $t('pairing_in_progress') : $t('pair_live')}
              </button>
            </div>
          </fieldset>
        </form>
      </div>
    </details>
  </div>
</section>

<style>
  .dashboard-section { margin-bottom: 48px; scroll-margin-top: 24px; color: var(--tx); font-family: var(--font); }

  /* En-tête de section : titre + phrase d'explication, action principale à droite */
  .section-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; margin-bottom: 24px; }
  .header-left-part { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .header-icon-wrap { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: var(--r-sm); background: var(--color-surface); color: var(--tx); }
  .section-title { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  .section-subtitle { margin: 4px 0 0; font-size: 14px; line-height: 1.5; color: var(--mu); }

  .nodes-layout { display: grid; gap: 20px; }

  /* Résumé */
  .nodes-summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 12px; }
  .summary-tile { display: flex; flex-direction: column; gap: 4px; padding: 16px 18px; background: var(--color-surface); border: 1px solid var(--color-line-subtle); border-radius: var(--r-md); }
  .summary-label { font-size: 12px; font-weight: 600; color: var(--mu); }
  .summary-value { font-size: 24px; font-weight: 800; font-variant-numeric: tabular-nums; color: var(--tx); }
  .summary-value.ok { color: var(--ok); }
  .summary-value.warn { color: var(--warn); }

  /* Panneaux */
  .card { min-width: 0; padding: 24px; background: var(--color-surface); border: 1px solid var(--color-line-subtle); border-radius: var(--r-md); }
  .card-header { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; margin-bottom: 16px; color: var(--mu); }
  .card-header h3 { margin: 0; font-size: 17px; font-weight: 700; color: var(--tx); }

  /* Badges */
  .badge { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 700; white-space: nowrap; vertical-align: middle; }
  .count-badge { color: var(--mu); font-variant-numeric: tabular-nums; }
  .badge[data-status='connected'] { background: var(--ok-soft); color: var(--ok); }
  .badge[data-status='disconnected'] { background: var(--warn-soft); color: var(--warn); }
  .badge[data-status='revoked'] { background: var(--err-soft); color: var(--err); }
  .kind-badge { color: var(--mu); }
  .mode-badge[data-mode='outbound'] { background: var(--info-soft); color: var(--info); }
  .mode-badge[data-mode='direct'] { background: var(--ok-soft); color: var(--ok); }
  .mode-badge[data-mode='inbound'] { color: var(--mu); }
  .version-badge { font-family: var(--font-mono); font-weight: 600; }
  .version-badge.version-unknown { color: var(--mu); }

  /* Cartes des nœuds */
  .nodes-list { display: grid; gap: 14px; }
  .node-card { display: grid; gap: 16px; min-width: 0; padding: 18px 20px; background: var(--pn); border: 1px solid var(--color-line-subtle); border-radius: var(--r-md); }
  .node-card-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .node-title-group { display: flex; align-items: center; gap: 12px; min-width: 0; flex: 1; }
  .node-status-icon { display: grid; place-items: center; flex: none; width: 40px; height: 40px; border-radius: var(--r-sm); background: var(--pn-2); color: var(--mu); }
  .node-status-icon[data-status='connected'] { background: var(--ok-soft); color: var(--ok); }
  .node-status-icon[data-status='disconnected'] { background: var(--warn-soft); color: var(--warn); }
  .node-status-icon[data-status='revoked'] { background: var(--err-soft); color: var(--err); }
  .node-title-meta { min-width: 0; flex: 1; }
  .item-title-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .item-title-row h4 { margin: 0 4px 0 0; font-size: 17px; font-weight: 700; color: var(--tx); }

  .node-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .compact-btn { height: 36px; padding: 0 12px; font-size: 13px; border-radius: var(--r-sm); }

  .node-specs { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 10px 18px; margin: 0; padding: 14px 16px; background: var(--pn-2); border-radius: var(--r-sm); }
  .spec-item { display: grid; gap: 2px; min-width: 0; }
  .spec-item dt { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.05em; color: var(--mu); }
  .spec-item dd { margin: 0; font-size: 13px; color: var(--tx); overflow-wrap: anywhere; }
  .spec-item code { font-family: var(--font-mono); font-size: 12px; color: var(--tx); }

  .node-tags-row { display: grid; gap: 10px; }
  .tag-group { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .tag-group-label { font-size: 12px; font-weight: 700; color: var(--mu); min-width: 150px; }
  .tag-list { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .tag-pill { display: inline-flex; align-items: center; gap: 4px; padding: 4px 10px; border-radius: var(--r-pill); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 600; }
  .tag-pill.mono { font-family: var(--font-mono); font-size: 11px; }
  .tag-pill small { color: var(--mu); font-weight: 500; }
  .tag-pill.muted-pill { color: var(--mu); }

  /* Panneau legacy repliable */
  .legacy-panel { padding: 0; overflow: hidden; }
  .legacy-summary { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 18px 24px; cursor: pointer; list-style: none; font-size: 15px; font-weight: 700; color: var(--tx); }
  .legacy-summary::-webkit-details-marker { display: none; }
  .legacy-summary-title { display: inline-flex; align-items: center; gap: 10px; color: var(--mu); }
  .legacy-summary-title span { color: var(--tx); }
  .legacy-panel[open] .legacy-summary { border-bottom: 1px solid var(--color-line-subtle); }
  .legacy-body { padding: 20px 24px 24px; }
  .legacy-form-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 18px; }

  /* Boutons */
  .action-btn,
  .copy-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 44px;
    padding: 0 16px;
    border: 1px solid var(--color-line);
    border-radius: var(--r-md);
    background: transparent;
    color: var(--tx);
    font-family: var(--font);
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  .action-btn:hover:not(:disabled),
  .copy-btn:hover:not(:disabled) { background: var(--color-surface-hover); }
  .danger-btn { border-color: var(--color-danger); background: transparent; color: var(--err); }
  .danger-btn:hover:not(:disabled) { background: var(--err-soft); }

  .primary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 44px;
    padding: 0 22px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--btn);
    color: var(--btn-tx);
    font-family: var(--font);
    font-size: 14px;
    font-weight: 700;
    box-shadow: none;
    cursor: pointer;
    transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  .primary:hover:not(:disabled) { opacity: 0.9; }
  .add-node-btn { flex: none; }

  .action-btn:active:not(:disabled),
  .copy-btn:active:not(:disabled),
  .primary:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button:focus-visible,
  input:focus-visible,
  textarea:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  /* Formulaire */
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  label { display: block; margin: 18px 0 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input {
    width: 100%;
    height: 46px;
    padding: 0 14px;
    border: 1px solid var(--color-line-subtle);
    border-radius: var(--r-sm);
    background: var(--color-surface);
    color: var(--tx);
    font-family: var(--font);
    font-size: 14px;
    box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }
  input:hover { background: var(--color-surface-hover); }
  input:focus { outline: none; border-color: var(--ac); background: var(--color-surface); }
  .actions { display: flex; justify-content: flex-end; gap: 12px; margin-top: 24px; }

  .muted { margin: 0 0 4px; color: var(--mu); font-size: 14px; line-height: 1.55; }
  .muted strong { color: var(--tx); font-weight: 700; }

  /* Dialogue d'ajout */
  .modal-backdrop { position: fixed; inset: 0; z-index: 1000; display: flex; align-items: center; justify-content: center; padding: 20px; background: var(--color-overlay); }
  .modal-card { display: flex; flex-direction: column; width: 100%; max-width: 680px; max-height: 90vh; overflow: hidden; background: var(--color-surface-elevated); color: var(--tx); border: 1px solid var(--color-line-subtle); border-radius: var(--r-lg); box-shadow: var(--shadow-md); }
  .modal-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 24px 28px 16px; }
  .modal-title-wrap { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .modal-title { margin: 0; font-size: 20px; font-weight: 800; letter-spacing: -0.02em; }
  .modal-desc { margin: 4px 0 0; font-size: 13px; line-height: 1.5; color: var(--mu); }
  .icon-close-btn { display: grid; place-items: center; flex: none; width: 44px; height: 44px; padding: 0; border: 1px solid var(--color-line); border-radius: var(--r-sm); background: transparent; color: var(--mu); font-size: 16px; cursor: pointer; transition: background-color var(--t-fast) ease, color var(--t-fast) ease; }
  .icon-close-btn:hover { background: var(--color-surface-hover); color: var(--tx); }
  .modal-body { flex: 1; overflow-y: auto; padding: 8px 28px 24px; }

  .info-callout { display: flex; align-items: flex-start; gap: 12px; margin-bottom: 20px; padding: 14px 16px; border-radius: var(--r-md); background: var(--info-soft); color: var(--tx); font-size: 14px; line-height: 1.55; }
  .info-callout :global(svg) { flex: none; margin-top: 2px; color: var(--info); }
  .token-error { margin-bottom: 16px; padding: 12px 16px; border-radius: var(--r-md); background: var(--err-soft); color: var(--err); font-size: 14px; line-height: 1.5; }
  .token-loading { padding: 32px; text-align: center; color: var(--mu); font-size: 14px; }

  .token-section, .compose-section { margin-bottom: 20px; }
  .token-section label { margin-top: 0; }
  .token-input-row { display: flex; gap: 8px; }
  .token-field { flex: 1; min-width: 0; font-family: var(--font-mono); font-size: 13px; letter-spacing: 0.02em; }
  .compose-header { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; margin-bottom: 8px; }
  .compose-header label { margin: 0; }
  .compose-textarea { display: block; width: 100%; padding: 14px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-sm); background: var(--color-surface); color: var(--tx); font-family: var(--font-mono); font-size: 12px; line-height: 1.6; resize: vertical; }
  .compose-textarea:focus { outline: none; border-color: var(--ac); background: var(--color-surface); }

  .modal-steps { padding: 14px 16px; border-radius: var(--r-md); background: var(--color-surface); font-size: 13px; line-height: 1.6; color: var(--mu); }
  .modal-steps p { margin: 0 0 6px; color: var(--tx); }
  .modal-steps ol { margin: 0; padding-left: 20px; }

  .modal-footer { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 8px; padding: 16px 28px 24px; border-top: 1px solid var(--color-line-subtle); }

  @media (max-width: 767px) {
    .legacy-form-grid { grid-template-columns: minmax(0, 1fr); }
  }

  @media (max-width: 640px) {
    .card { padding: 16px; }
    .node-card { padding: 14px; }
    .node-actions { width: 100%; }
    .node-actions .action-btn { flex: 1 1 130px; }
    .legacy-summary { padding: 14px 16px; }
    .legacy-body { padding: 16px; }
    .add-node-btn { width: 100%; }
    .modal-backdrop { align-items: flex-end; padding: 0; }
    .modal-card { max-height: 92dvh; border-radius: var(--r-xl) var(--r-xl) 0 0; }
    .modal-header { padding: 20px 16px 12px; }
    .modal-body { padding: 8px 16px 16px; }
    .modal-footer { padding: 12px 16px calc(16px + env(safe-area-inset-bottom, 0px)); }
    .modal-footer button { flex: 1 1 auto; }
    .token-input-row { flex-wrap: wrap; }
    .token-input-row .copy-btn { flex: 1 1 auto; }
    input { font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .action-btn, .copy-btn, .primary, .icon-close-btn, input { transition: none; }
  }
</style>

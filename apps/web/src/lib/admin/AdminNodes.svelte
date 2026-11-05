<script lang="ts">
  import type { AdminNode, EnrollmentToken } from './types';
  import { nodeStatusLabels, connectionModeLabels } from './types';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';

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

  let showEnrollModal = $state(false);
  let generatingToken = $state(false);
  let activeToken = $state<EnrollmentToken | null>(null);
  let tokenError = $state('');
  let copiedToken = $state(false);
  let copiedCompose = $state(false);

  function formatTimestamp(ts?: number | null): string {
    if (!ts) return 'Jamais';
    try {
      return new Date(ts * 1000).toLocaleString('fr-FR');
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
      tokenError = e instanceof Error ? e.message : 'Erreur lors de la génération du jeton';
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
          Supervisez vos nœuds multimédias distants (connexions sortantes NAT-proof) et locaux.
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
          <button class="icon-close-btn" onclick={() => (showEnrollModal = false)} aria-label="Fermer">✕</button>
        </div>

        <div class="modal-body">
          <div class="info-callout">
            <Icon name="info" size={16} />
            <span>
              {$t('node_https_note')} Le jeton d'invitation est <strong>unique, temporaire et à usage unique</strong>.
            </span>
          </div>

          {#if tokenError}
            <div class="token-error" role="alert">{tokenError}</div>
          {/if}

          {#if generatingToken}
            <div class="token-loading">{$t('secure_token_generating')}</div>
          {:else if activeToken}
            <div class="token-section">
              <label for="enroll-token-display">Jeton d'invitation (valide jusqu'à {formatTimestamp(activeToken.expiresAt)}) :</label>
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
                  <span>{copiedToken ? 'Copié !' : 'Copier'}</span>
                </button>
              </div>
            </div>

            <div class="compose-section">
              <div class="compose-header">
                <label for="compose-snippet">Configuration Docker Compose recommandée :</label>
                <button
                  class="copy-btn copy-compose"
                  onclick={() => copyToClipboard(getDockerComposeSnippet(activeToken), 'compose')}
                >
                  <Icon name={copiedCompose ? 'check' : 'film'} size={14} />
                  <span>{copiedCompose ? 'Copié !' : 'Copier le YAML'}</span>
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
              <p><strong>Étapes de mise en service :</strong></p>
              <ol>
                <li>Lancez le conteneur avec l'URL du Core et ce jeton.</li>
                <li>Le Node contacte le Core, valide mutuellement l'identité et sauvegarde ses clés durables dans son volume persistant.</li>
                <li>Le jeton est immédiatement invalidé. Le Node se reconnectera automatiquement à chaque démarrage.</li>
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

  <div class="columns">
    <!-- Registered Media Nodes List -->
    <section class="card" aria-labelledby="node-list">
      <div class="card-header">
        <h3 id="node-list">Media Nodes enregistrés ({nodes.length})</h3>
        <span class="badge count-badge">
          {nodes.filter(n => n.status === 'connected').length} en ligne / {nodes.length} total
        </span>
      </div>

      {#if nodes.length === 0}
        <p class="muted">{$t('no_nodes')}</p>
      {/if}

      <div class="items-list">
        {#each nodes as node (node.id)}
          <article class="item-card">
            <div class="item-content">
              <div class="item-title-row">
                <h4>{node.name}</h4>
                <span class="badge" data-status={node.status}>
                  {nodeStatusLabels[node.status] || node.status}
                </span>
                <span class="badge kind-badge">
                  {node.kind === 'local' ? 'Local (intégré)' : 'Distant (autonome)'}
                </span>
                {#if node.kind === 'remote'}
                  <span class="badge mode-badge" data-mode={node.connectionMode || 'outbound'}>
                    {connectionModeLabels[node.connectionMode || 'outbound'] || node.connectionMode || 'Relais sortant'}
                  </span>
                {/if}
                {#if node.version}
                  <span class="badge version-badge" title={`Version du Media Node : v${node.version}`}>
                    v{node.version}
                  </span>
                {:else}
                  <span class="badge version-badge version-unknown" title="Version non transmise">
                    v?
                  </span>
                {/if}
              </div>

              <p class="path"><strong>Identifiant :</strong> <code>{node.id}</code></p>
              {#if node.baseUrl}
                <p class="path"><strong>URL interne (Core → Node) :</strong> <code>{node.baseUrl}</code></p>
              {/if}
              {#if node.publicUrl}
                <p class="path"><strong>URL publique directe :</strong> <code>{node.publicUrl}</code></p>
              {/if}

              <p class="node-meta">
                <strong>Dernière activité :</strong> {formatTimestamp(node.lastSeenAt)} ·
                <strong>Clé :</strong> <code>{node.keyId}</code>
              </p>

              {#if node.roots.length > 0}
                <p class="path"><strong>Dossiers multimédias :</strong> {node.roots.join(', ')}</p>
              {/if}

              <p class="node-meta">
                <strong>Bibliothèques rattachées :</strong>
                {node.libraries.length ? node.libraries.map(l => `${l.name} (${l.kind})`).join(', ') : 'Aucune'}
              </p>
            </div>

            {#if node.kind === 'remote'}
              <div class="item-actions-vertical">
                {#if node.status !== 'revoked'}
                  <button
                    class="action-btn"
                    disabled={!!nodeActionBusy}
                    onclick={() => onnodeaction(node, 'reconnect')}
                    aria-label={`Reconnecter ${node.name}`}
                  >
                    <Icon name="refresh" size={12} />
                    <span>{$t('reconnect')}</span>
                  </button>
                  <button
                    class="action-btn"
                    disabled={!!nodeActionBusy}
                    onclick={() => onnodeaction(node, 'rotate-secret')}
                    aria-label={`Renouveler le secret de ${node.name}`}
                  >
                    <Icon name="shield" size={12} />
                    <span>{$t('renew_secret')}</span>
                  </button>
                  <button
                    class="action-btn danger-btn"
                    disabled={!!nodeActionBusy}
                    onclick={() => onnodeaction(node, 'revoke')}
                    aria-label={`Révoquer ${node.name}`}
                  >
                    <span>{$t('revoke')}</span>
                  </button>
                {/if}
              </div>
            {/if}
          </article>
        {/each}
      </div>
    </section>

    <!-- Legacy Inbound Pairing (Collapsible / Backward-Compatible) -->
    <section class="card" aria-labelledby="node-legacy-form">
      <div class="card-header">
        <h3 id="node-legacy-form">{$t('direct_pairing_legacy')}</h3>
        <Icon name="shield" size={16} />
      </div>

      <p class="muted">
        Pour les nœuds disposant d'un port ouvert et accessible directement par le Core. Pour les machines derrière un NAT, utilisez le bouton <strong>Ajouter un Node</strong> en haut.
      </p>

      <form onsubmit={onpair}>
        <fieldset disabled={pairing}>
          <label for="pair-node-name">{$t('node_name')}</label>
          <input
            id="pair-node-name"
            bind:value={pairName}
            required
            maxlength="100"
            placeholder="Ex. Node NAS Films"
            autocomplete="off"
          />

          <label for="pair-node-base-url">{$t('internal_node_url')}</label>
          <input
            id="pair-node-base-url"
            bind:value={pairBaseUrl}
            required
            placeholder="http://aether-node:3101"
            autocomplete="off"
          />

          <label for="pair-node-public-url">{$t('public_stream_url')}</label>
          <input
            id="pair-node-public-url"
            bind:value={pairPublicUrl}
            placeholder="https://node.example.com:3101"
            autocomplete="off"
          />

          <label for="pair-node-token">{$t('bootstrap_token')}</label>
          <input
            id="pair-node-token"
            type="password"
            bind:value={pairToken}
            required
            autocomplete="off"
          />

          <div class="actions">
            <button class="primary" type="submit">
              {pairing ? 'Appairage en cours…' : 'Appairer en direct'}
            </button>
          </div>
        </fieldset>
      </form>
    </section>
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

  .columns { display: grid; grid-template-columns: minmax(0, 1.15fr) minmax(0, 0.85fr); gap: 20px; align-items: start; }

  /* Panneaux */
  .card { min-width: 0; padding: 24px; background: var(--color-surface); border-radius: var(--r-md); }
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

  /* Liste des nœuds : lignes séparées, pas de carte dans la carte */
  .items-list { display: grid; }
  .item-card { display: flex; justify-content: space-between; gap: 20px; min-width: 0; padding: 18px 0; border-top: 1px solid var(--color-line-subtle); }
  .item-card:first-child { border-top: 0; padding-top: 4px; }
  .item-content { flex: 1; min-width: 0; }
  .item-title-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 10px; }
  .item-title-row h4 { margin: 0 4px 0 0; font-size: 17px; font-weight: 700; color: var(--tx); }
  .item-content p { margin: 4px 0; font-size: 13px; line-height: 1.55; color: var(--mu); }
  .item-content strong { font-weight: 600; color: var(--tx); }
  .item-content code { font-family: var(--font-mono); font-size: 12px; color: var(--tx); }
  .path, .node-meta { overflow-wrap: anywhere; }

  .item-actions-vertical { display: flex; flex-direction: column; gap: 8px; flex: none; }

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

  @media (max-width: 1023px) {
    .columns { grid-template-columns: minmax(0, 1fr); }
  }

  @media (max-width: 640px) {
    .card { padding: 16px; }
    .item-card { flex-direction: column; gap: 12px; }
    .item-actions-vertical { flex-direction: row; flex-wrap: wrap; }
    .item-actions-vertical .action-btn { flex: 1 1 140px; }
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

<script lang="ts">
  import type { AdminNode, EnrollmentToken } from './types';
  import { nodeStatusLabels, connectionModeLabels } from './types';
  import Icon from '../ui/Icon.svelte';

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
        <h2 id="nodes-heading" class="section-title">Gestion des Media Nodes</h2>
        <p class="section-subtitle">
          Supervisez vos nœuds multimédias distants (connexions sortantes NAT-proof) et locaux.
        </p>
      </div>
    </div>
    {#if ongeneratetoken}
      <button class="primary add-node-btn" onclick={handleOpenAddNode}>
        <Icon name="server" size={14} />
        <span>Ajouter un Node</span>
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
              <h3 id="enroll-title" class="modal-title">Ajouter un Media Node</h3>
              <p class="modal-desc">Appairage automatique par connexion sortante initiée par le Node (compatible NAT / sans port ouvert)</p>
            </div>
          </div>
          <button class="icon-close-btn" onclick={() => (showEnrollModal = false)} aria-label="Fermer">✕</button>
        </div>

        <div class="modal-body">
          <div class="info-callout">
            <Icon name="info" size={16} />
            <span>
              Le Media Node contacte lui-même le Core en HTTPS. Aucun port entrant n'a besoin d'être ouvert sur la machine distante. Le jeton d'invitation est <strong>unique, temporaire et à usage unique</strong>.
            </span>
          </div>

          {#if tokenError}
            <div class="token-error" role="alert">{tokenError}</div>
          {/if}

          {#if generatingToken}
            <div class="token-loading">Génération du jeton sécurisé…</div>
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
            <span>Générer un autre jeton</span>
          </button>
          <button class="primary" onclick={() => (showEnrollModal = false)}>
            <span>Terminé</span>
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
        <p class="muted">Aucun Media Node enregistré.</p>
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
                    <span>Reconnecter</span>
                  </button>
                  <button
                    class="action-btn"
                    disabled={!!nodeActionBusy}
                    onclick={() => onnodeaction(node, 'rotate-secret')}
                    aria-label={`Renouveler le secret de ${node.name}`}
                  >
                    <Icon name="shield" size={12} />
                    <span>Renouveler secret</span>
                  </button>
                  <button
                    class="action-btn danger-btn"
                    disabled={!!nodeActionBusy}
                    onclick={() => onnodeaction(node, 'revoke')}
                    aria-label={`Révoquer ${node.name}`}
                  >
                    <span>Révoquer</span>
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
        <h3 id="node-legacy-form">Appairage direct entrant (Legacy)</h3>
        <Icon name="shield" size={16} />
      </div>

      <p class="muted">
        Pour les nœuds disposant d'un port ouvert et accessible directement par le Core. Pour les machines derrière un NAT, utilisez le bouton <strong>Ajouter un Node</strong> en haut.
      </p>

      <form onsubmit={onpair}>
        <fieldset disabled={pairing}>
          <label for="pair-node-name">Nom du Media Node</label>
          <input
            id="pair-node-name"
            bind:value={pairName}
            required
            maxlength="100"
            placeholder="Ex. Node NAS Films"
            autocomplete="off"
          />

          <label for="pair-node-base-url">URL interne du Node (Core → Node)</label>
          <input
            id="pair-node-base-url"
            bind:value={pairBaseUrl}
            required
            placeholder="http://aether-node:3101"
            autocomplete="off"
          />

          <label for="pair-node-public-url">URL publique de streaming direct (Optionnel)</label>
          <input
            id="pair-node-public-url"
            bind:value={pairPublicUrl}
            placeholder="https://node.example.com:3101"
            autocomplete="off"
          />

          <label for="pair-node-token">Jeton d’amorçage statique (MEDIA_NODE_BOOTSTRAP_TOKEN)</label>
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
  .dashboard-section {
    margin-bottom: 48px;
    scroll-margin-top: 24px;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 24px;
    padding-bottom: 14px;
    border-bottom: 1px solid var(--color-line);
    flex-wrap: wrap;
  }

  .header-left-part {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .add-node-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    font-size: 13px;
  }

  .header-icon-wrap {
    width: 36px;
    height: 36px;
    border-radius: var(--radius-sm);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .section-title {
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.02em;
    margin: 0;
    color: var(--color-text);
  }

  .section-subtitle {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--color-muted);
  }

  .columns {
    display: grid;
    grid-template-columns: 1.15fr 0.85fr;
    gap: 28px;
  }

  .card {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: 24px;
  }

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 20px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-line);
  }

  .card-header h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }

  .count-badge {
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .items-list {
    display: grid;
    gap: 16px;
  }

  .item-card {
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 18px;
    display: flex;
    justify-content: space-between;
    gap: 16px;
    transition: border-color 0.2s ease;
  }

  .item-card:hover {
    border-color: var(--color-border-glow);
  }

  .item-content {
    flex: 1;
    min-width: 0;
  }

  .item-title-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  .item-title-row h4 {
    margin: 0;
    font-size: 16px;
    font-weight: 650;
  }

  .item-content p {
    margin: 4px 0;
    font-size: 12px;
    color: var(--color-muted);
  }

  .path {
    overflow-wrap: anywhere;
  }

  .node-meta {
    font-size: 12px;
    color: var(--color-subtle);
  }

  .item-actions-vertical {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-shrink: 0;
  }

  .action-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 6px 12px;
    font-size: 12px;
    color: var(--color-text);
    background: var(--color-surface);
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease;
  }

  .action-btn:hover:not(:disabled) {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
  }

  .danger-btn {
    color: var(--color-danger);
    border-color: #f3aaa944;
  }

  .danger-btn:hover:not(:disabled) {
    background: #f3aaa922;
  }

  .info-callout {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    background: var(--color-accent-soft);
    border: 1px solid #b4a1f844;
    border-radius: var(--radius-sm);
    padding: 14px;
    margin-bottom: 20px;
    font-size: 13px;
    line-height: 1.6;
    color: var(--color-text);
  }

  .primary {
    background: var(--color-accent);
    color: #0f0d14;
    font-weight: 650;
    border: none;
    border-radius: var(--radius-sm);
    padding: 10px 18px;
    min-height: 40px;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  fieldset {
    border: 0;
    padding: 0;
    margin: 0;
    min-width: 0;
  }

  label {
    display: block;
    font-size: 13px;
    font-weight: 500;
    margin: 16px 0 6px;
    color: var(--color-text);
  }

  input {
    width: 100%;
    min-height: 44px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 10px 14px;
    color: var(--color-text);
    background: var(--color-raised);
    font-size: 13px;
  }

  input:focus {
    border-color: var(--color-accent);
    outline: none;
  }

  .actions {
    display: flex;
    gap: 12px;
    margin-top: 24px;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 500;
    padding: 2px 8px;
    border-radius: 99px;
    border: 1px solid var(--color-line);
    vertical-align: middle;
  }

  .badge[data-status='connected'] {
    color: #9be7b4;
    border-color: #2e6b44;
    background: #14281c;
  }

  .badge[data-status='disconnected'] {
    color: #f5c97b;
    border-color: #75541e;
    background: #2b200e;
  }

  .badge[data-status='revoked'] {
    color: var(--color-danger);
    border-color: var(--color-danger);
    background: #331518;
  }

  .kind-badge {
    background: var(--color-raised);
    color: #d1cadf;
  }

  .mode-badge {
    font-weight: 600;
    font-size: 11px;
  }

  .mode-badge[data-mode='outbound'] {
    background: #1e1b38;
    color: #a78bfa;
    border-color: #4c1d95;
  }

  .mode-badge[data-mode='direct'] {
    background: #0f2e24;
    color: #34d399;
    border-color: #065f46;
  }

  .mode-badge[data-mode='inbound'] {
    background: var(--color-raised);
    color: #94a3b8;
    border-color: var(--color-line);
  }

  .version-badge {
    background: #1e1b2e;
    color: var(--color-accent);
    border-color: #55447a;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 11px;
    font-weight: 600;
  }

  .version-badge.version-unknown {
    color: var(--color-muted);
    border-color: var(--color-line);
    background: var(--color-raised);
  }

  .muted {
    color: var(--color-muted);
    font-size: 13px;
    line-height: 1.5;
  }

  /* Modal styling */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(8px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 20px;
  }

  .modal-card {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    width: 100%;
    max-width: 680px;
    max-height: 90vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px;
    border-bottom: 1px solid var(--color-line);
  }

  .modal-title-wrap {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .modal-title {
    margin: 0;
    font-size: 18px;
    font-weight: 650;
  }

  .modal-desc {
    margin: 2px 0 0;
    font-size: 12px;
    color: var(--color-muted);
  }

  .icon-close-btn {
    background: transparent;
    border: none;
    color: var(--color-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
  }

  .icon-close-btn:hover {
    color: var(--color-text);
    background: var(--color-raised);
  }

  .modal-body {
    padding: 24px;
    overflow-y: auto;
    flex: 1;
  }

  .token-section {
    margin-bottom: 20px;
  }

  .token-input-row {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }

  .token-field {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 13px;
    letter-spacing: 0.04em;
    background: var(--color-raised);
    color: var(--color-accent);
    border: 1px solid var(--color-line);
  }

  .copy-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    color: var(--color-text);
    cursor: pointer;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    transition: all 0.15s ease;
  }

  .copy-btn:hover {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
  }

  .compose-section {
    margin-bottom: 20px;
  }

  .compose-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }

  .compose-header label {
    margin: 0;
  }

  .compose-textarea {
    width: 100%;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
    line-height: 1.5;
    background: #0c0b11;
    color: #e2e8f0;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 12px;
    resize: vertical;
  }

  .modal-steps {
    font-size: 12px;
    color: var(--color-muted);
    background: var(--color-raised);
    border-radius: var(--radius-sm);
    padding: 12px 16px;
  }

  .modal-steps p {
    margin: 0 0 6px;
    color: var(--color-text);
  }

  .modal-steps ol {
    margin: 0;
    padding-left: 20px;
    line-height: 1.6;
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-top: 1px solid var(--color-line);
    background: var(--color-surface);
  }

  .token-error {
    background: #331518;
    color: var(--color-danger);
    border: 1px solid var(--color-danger);
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    margin-bottom: 16px;
    font-size: 13px;
  }

  .token-loading {
    padding: 30px;
    text-align: center;
    color: var(--color-muted);
    font-size: 13px;
  }

  @media(max-width: 950px) {
    .columns {
      grid-template-columns: 1fr;
    }
  }
</style>

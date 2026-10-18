<script lang="ts">
  import type { AdminNode } from './types';
  import { nodeStatusLabels } from './types';
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
    onnodeaction
  }: Props = $props();

  function formatTimestamp(ts?: number | null): string {
    if (!ts) return 'Jamais';
    try {
      return new Date(ts * 1000).toLocaleString('fr-FR');
    } catch {
      return String(ts);
    }
  }
</script>

<section id="nodes-section" class="dashboard-section" aria-labelledby="nodes-heading">
  <div class="section-header">
    <div class="header-icon-wrap">
      <Icon name="server" size={18} />
    </div>
    <div>
      <h2 id="nodes-heading" class="section-title">Gestion des Media Nodes</h2>
      <p class="section-subtitle">Supervisez l'architecture distribuée, l'appairage sécurisé HMAC et l'état des nœuds de stockage</p>
    </div>
  </div>

  <div class="columns">
    <!-- Registered Media Nodes -->
    <section class="card" aria-labelledby="node-list">
      <div class="card-header">
        <h3 id="node-list">Media Nodes enregistrés ({nodes.length})</h3>
        <span class="badge count-badge">{nodes.filter(n => n.status === 'connected').length} en ligne</span>
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
                  {nodeStatusLabels[node.status]}
                </span>
                <span class="badge kind-badge">
                  {node.kind === 'local' ? 'Local (intégré)' : 'Distant (autonome)'}
                </span>
                {#if node.version}
                  <span class="badge version-badge" title={`Version du Media Node : v${node.version}`}>
                    v{node.version}
                  </span>
                {:else}
                  <span class="badge version-badge version-unknown" title="Version non transmise (nœud antérieur)">
                    v?
                  </span>
                {/if}
              </div>

              <p class="path"><strong>Identifiant :</strong> <code>{node.id}</code></p>
              {#if node.baseUrl}
                <p class="path"><strong>URL interne (Core → Node) :</strong> <code>{node.baseUrl}</code></p>
              {/if}
              {#if node.publicUrl}
                <p class="path"><strong>URL publique directe (Navigateur → Node) :</strong> <code>{node.publicUrl}</code></p>
              {/if}

              <p class="node-meta">
                <strong>Dernier contact :</strong> {formatTimestamp(node.lastSeenAt)} ·
                <strong>Clé :</strong> <code>{node.keyId}</code>
              </p>

              {#if node.roots.length > 0}
                <p class="path"><strong>Racines exposées :</strong> {node.roots.join(', ')}</p>
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

    <!-- Pair Remote Node Form -->
    <section class="card" aria-labelledby="node-pair-form">
      <div class="card-header">
        <h3 id="node-pair-form">Appairer un Media Node distant</h3>
        <Icon name="shield" size={16} />
      </div>

      <div class="info-callout">
        <Icon name="info" size={16} />
        <span>
          L'appairage établit une connexion sécurisée par échange cryptographique HMAC-SHA256, sans exposer les sessions ni les mots de passe utilisateurs.
        </span>
      </div>

      <form onsubmit={onpair}>
        <fieldset disabled={pairing}>
          <label for="pair-node-name">Nom du Media Node</label>
          <input
            id="pair-node-name"
            bind:value={pairName}
            required
            maxlength="100"
            placeholder="Ex. Node Séries NAS"
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
            placeholder="http://192.168.1.100:3101"
            autocomplete="off"
          />

          <label for="pair-node-token">Jeton d’amorçage / appairage (défini au démarrage du Node)</label>
          <input
            id="pair-node-token"
            type="password"
            bind:value={pairToken}
            required
            autocomplete="off"
          />

          <div class="actions">
            <button class="primary" type="submit">
              {pairing ? 'Appairage en cours…' : 'Appairer le Node'}
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
    gap: 14px;
    margin-bottom: 24px;
    padding-bottom: 14px;
    border-bottom: 1px solid var(--color-line);
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
  }

  @media(max-width: 950px) {
    .columns {
      grid-template-columns: 1fr;
    }
  }
</style>

<script lang="ts">
  import type { Library, AdminNode, Scan } from './types';
  import { scanLabels } from './types';
  import type { AuthUser } from '../auth/session';
  import Icon from '../ui/Icon.svelte';

  interface Props {
    libraries: Library[];
    nodes: AdminNode[];
    users: AuthUser[];
    scans: Record<string, Scan>;
    pending: string[];
    availableRootsForNode: string[];
    saving: boolean;
    editing: string | null;
    name: string;
    kind: 'films' | 'series';
    nodeId: string;
    root: string;
    enabled: boolean;
    userIds: string[];
    onsave: (e: SubmitEvent) => void;
    onscan: (lib: Library) => void;
    onedit: (lib: Library) => void;
    onreset: () => void;
    onnodechange: () => void;
  }

  let {
    libraries,
    nodes,
    users,
    scans,
    pending,
    availableRootsForNode,
    saving,
    editing,
    name = $bindable(),
    kind = $bindable(),
    nodeId = $bindable(),
    root = $bindable(),
    enabled = $bindable(),
    userIds = $bindable(),
    onsave,
    onscan,
    onedit,
    onreset,
    onnodechange
  }: Props = $props();

  const activeNodes = $derived(nodes.filter(n => n.status !== 'revoked'));

  function nodeLabel(id: string): string {
    if (id === 'local') return 'Node local';
    const found = nodes.find(n => n.id === id);
    return found ? `${found.name} (${found.id})` : id;
  }
</script>

<section id="libraries-section" class="dashboard-section" aria-labelledby="libraries-heading">
  <div class="section-header">
    <div class="header-icon-wrap">
      <Icon name="film" size={18} />
    </div>
    <div>
      <h2 id="libraries-heading" class="section-title">Gestion des bibliothèques & Scans</h2>
      <p class="section-subtitle">Configurez vos répertoires de films et séries et surveillez l'indexation</p>
    </div>
  </div>

  <div class="columns">
    <!-- List of Libraries -->
    <section class="card" aria-labelledby="library-list">
      <div class="card-header">
        <h3 id="library-list">Bibliothèques configurées ({libraries.length})</h3>
        <span class="badge count-badge">{libraries.length} active(s)</span>
      </div>

      {#if libraries.length === 0}
        <div class="empty-libraries">
          <Icon name="film" size={32} />
          <p>Aucune bibliothèque configurée pour le moment.</p>
          <span class="muted-note">Utilisez le formulaire ci-contre pour ajouter votre premier répertoire de médias.</span>
        </div>
      {/if}

      <div class="items-list">
        {#each libraries as library (library.id)}
          {@const nodeObj = nodes.find(n => n.id === library.nodeId)}
          {@const isNodeOffline = nodeObj && nodeObj.status !== 'connected'}
          <article class="item-card" class:offline-library={isNodeOffline}>
            <div class="item-content">
              <div class="item-title-row">
                <h4>{library.name}</h4>
                <span class="badge kind-badge">
                  <Icon name={library.kind === 'films' ? 'film' : 'tv'} size={12} />
                  {library.kind === 'films' ? 'Films' : 'Séries'}
                </span>
                <span class="badge" data-status={library.enabled ? 'connected' : 'disconnected'}>
                  {library.enabled ? 'Active' : 'Désactivée'}
                </span>
                {#if isNodeOffline}
                  <span class="badge warn-badge">Node hors ligne</span>
                {/if}
              </div>

              <p class="path"><strong>Racine :</strong> <code>{library.root}</code></p>
              <p class="node-info"><strong>Media Node :</strong> {nodeLabel(library.nodeId)}</p>

              {#if isNodeOffline}
                <p class="node-warning" role="alert">
                  ⚠️ Média temporairement indisponible : le Media Node associé ({library.nodeId}) est actuellement hors ligne.
                </p>
              {/if}

              <!-- Scan status and metrics -->
              {#if scans[library.id]}
                {@const scanData = scans[library.id]}
                <div class="scan-state" class:scanning={scanData.status === 'queued' || scanData.status === 'running'} role="status">
                  <div class="scan-header-row">
                    <span class="scan-indicator-badge" data-scan={scanData.status}>
                      {#if scanData.status === 'running' || scanData.status === 'queued'}
                        <span class="pulse-icon" aria-hidden="true">⏳</span>
                      {/if}
                      Scan : {scanLabels[scanData.status]}
                    </span>
                  </div>
                  <div class="scan-counters">
                    <span><strong>{scanData.discovered}</strong> découverts</span>
                    <span><strong>{scanData.probed}</strong> analysés</span>
                    <span><strong>{scanData.reused}</strong> réutilisés</span>
                  </div>

                  {#if scanData.errors.length > 0}
                    <details class="scan-errors-detail">
                      <summary class="error-summary">
                        ⚠️ {scanData.errors.length} anomalie(s) répertoriée(s)
                      </summary>
                      <ul class="error-list">
                        {#each scanData.errors as issue, idx (idx)}
                          <li>{issue}</li>
                        {/each}
                      </ul>
                    </details>
                  {/if}
                </div>
              {/if}

              {#if (library.kind === 'films' || library.kind === 'series') && library.enabled && !library.id.startsWith('fixture-')}
                <div class="item-actions">
                  <button
                    class="action-btn scan-btn"
                    disabled={isNodeOffline || pending.includes(library.id) || scans[library.id]?.status === 'queued' || scans[library.id]?.status === 'running'}
                    onclick={() => onscan(library)}
                    aria-label={`Scanner ${library.name}`}
                  >
                    <Icon name="refresh" size={13} />
                    <span>{pending.includes(library.id) || scans[library.id]?.status === 'running' ? 'Scan en cours…' : `Scanner ${library.name}`}</span>
                  </button>
                </div>
              {/if}
            </div>

            <div class="item-side-action">
              <button class="secondary-btn" disabled={saving} onclick={() => onedit(library)} aria-label={`Modifier ${library.name}`}>
                Modifier
              </button>
            </div>
          </article>
        {/each}
      </div>
    </section>

    <!-- Add / Edit Library Form -->
    <section class="card" aria-labelledby="library-form">
      <div class="card-header">
        <h3 id="library-form">{editing ? 'Modifier la bibliothèque' : 'Ajouter une bibliothèque'}</h3>
        {#if editing}
          <span class="badge editing-badge">Édition en cours</span>
        {/if}
      </div>

      <div class="info-callout">
        <Icon name="info" size={16} />
        <span>
          <strong>Multi-Node & Polyvalence :</strong> Vous pouvez associer des bibliothèques <em>Films</em> et <em>Séries</em> distinctes au même Media Node en sélectionnant simplement ses racines exposées correspondantes.
        </span>
      </div>

      <form onsubmit={onsave}>
        <fieldset disabled={saving}>
          <label for="library-name">Nom de la bibliothèque</label>
          <input
            id="library-name"
            bind:value={name}
            required
            maxlength="100"
            autocomplete="off"
            placeholder="Ex. Mes Films 4K"
          />

          <label for="library-kind">Type de contenu</label>
          <select id="library-kind" bind:value={kind}>
            <option value="films">Films</option>
            <option value="series">Séries</option>
          </select>

          <label for="library-node">Media Node assigné</label>
          {#if activeNodes.length > 1}
            <select id="library-node" bind:value={nodeId} onchange={onnodechange}>
              <option value="local">Node local</option>
              {#each activeNodes.filter(n => n.id !== 'local') as node (node.id)}
                <option value={node.id}>{node.name} ({node.id})</option>
              {/each}
            </select>
          {:else}
            <input id="library-node" value="Node local" readonly />
          {/if}

          <label for="library-root">Racine autorisée sur le Node</label>
          <select id="library-root" bind:value={root} required>
            <option value="" disabled>Choisir une racine</option>
            {#each availableRootsForNode as item (item)}
              <option value={item}>{item}</option>
            {/each}
          </select>

          {#if availableRootsForNode.length === 0}
            <p class="warning-msg">Aucune racine autorisée n’est configurée pour ce Media Node.</p>
          {/if}
          {#if editing && !availableRootsForNode.includes(root)}
            <p class="error-msg">Cette racine n’est plus autorisée sur ce Node. Choisissez une racine disponible avant d’enregistrer.</p>
          {/if}

          <label class="check">
            <input type="checkbox" bind:checked={enabled} />
            <span>Bibliothèque active (visible et diffusable)</span>
          </label>

          <p class="muted-note">
            💡 Les autorisations d'accès des comptes utilisateurs sont attribuées directement dans la rubrique <strong>Utilisateurs</strong> ou via les <strong>Groupes d’utilisateurs</strong>.
          </p>

          <div class="actions">
            <button class="primary" type="submit" disabled={!availableRootsForNode.includes(root)}>
              {saving ? 'Enregistrement…' : 'Enregistrer'}
            </button>
            <button class="secondary-btn" type="button" onclick={onreset}>
              {editing ? 'Annuler' : 'Réinitialiser'}
            </button>
          </div>
        </fieldset>
      </form>
      <p class="muted footer-note">
        Les fichiers médias restent toujours en lecture seule (<code>:ro</code>). Aucun fichier n'est altéré.
      </p>
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

  .editing-badge {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border-color: var(--color-accent);
  }

  .empty-libraries {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 40px 20px;
    text-align: center;
    color: var(--color-muted);
    gap: 10px;
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
    transition: border-color 0.2s ease, transform 0.2s ease;
  }

  .item-card:hover {
    border-color: var(--color-border-glow);
  }

  .offline-library {
    border-color: var(--color-warning);
    background: var(--color-warning-soft);
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
    margin-bottom: 10px;
  }

  .item-title-row h4 {
    margin: 0;
    font-size: 16px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .item-content p {
    margin: 4px 0;
    font-size: 12px;
    color: var(--color-muted);
    line-height: 1.5;
  }

  .path {
    overflow-wrap: anywhere;
  }

  .info-callout {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    background: var(--color-accent-soft);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 14px;
    margin-bottom: 20px;
    font-size: 13px;
    line-height: 1.6;
    color: var(--color-text);
  }

  .scan-state {
    margin-top: 14px;
    padding: 12px 14px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
  }

  .scan-state.scanning {
    border-color: var(--color-accent);
    box-shadow: 0 0 12px var(--color-accent-soft);
  }

  .scan-header-row {
    margin-bottom: 8px;
  }

  .scan-indicator-badge {
    font-size: 12px;
    font-weight: 600;
  }

  .scan-indicator-badge[data-scan='completed'] {
    color: var(--color-success);
  }

  .scan-indicator-badge[data-scan='running'],
  .scan-indicator-badge[data-scan='queued'] {
    color: var(--color-accent);
  }

  .scan-indicator-badge[data-scan='error'] {
    color: var(--color-danger);
  }

  .pulse-icon {
    display: inline-block;
    animation: pulse 1s infinite alternate;
  }

  @keyframes pulse {
    from { opacity: 0.6; transform: scale(0.95); }
    to { opacity: 1; transform: scale(1.05); }
  }

  .scan-counters {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    font-size: 12px;
    color: var(--color-muted);
  }

  .scan-counters strong {
    color: var(--color-text);
  }

  .scan-errors-detail {
    margin-top: 10px;
    font-size: 12px;
  }

  .error-summary {
    color: var(--color-danger);
    cursor: pointer;
    font-weight: 500;
  }

  .error-list {
    margin: 8px 0 0;
    padding-left: 20px;
    color: var(--color-danger);
    line-height: 1.6;
  }

  .item-actions {
    margin-top: 14px;
  }

  .item-side-action {
    flex-shrink: 0;
  }

  .action-btn, .secondary-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 8px 14px;
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text);
    background: var(--color-raised);
    min-height: 38px;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }

  .action-btn:hover:not(:disabled), .secondary-btn:hover:not(:disabled) {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
  }

  .scan-btn {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border-color: var(--color-line);
  }

  .scan-btn:hover:not(:disabled) {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }

  .primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
    font-weight: 650;
    border: none;
    border-radius: var(--radius-sm);
    padding: 10px 18px;
    min-height: 40px;
    cursor: pointer;
    transition: background 0.15s ease, transform 0.15s ease;
  }

  .primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
    transform: translateY(-1px);
  }

  .primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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

  input:not([type='checkbox']), select {
    width: 100%;
    min-height: 44px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 10px 14px;
    color: var(--color-text);
    background: var(--color-raised);
    font-size: 13px;
    transition: border-color 0.15s ease;
  }

  input:focus, select:focus {
    border-color: var(--color-accent);
    outline: none;
  }

  input[readonly] {
    color: var(--color-muted);
  }

  .check {
    display: flex;
    gap: 10px;
    align-items: center;
    min-height: 32px;
    cursor: pointer;
    font-size: 13px;
    color: var(--color-text);
    margin: 12px 0;
  }

  .check input {
    accent-color: var(--color-accent);
    width: 18px;
    height: 18px;
    cursor: pointer;
  }

  .actions {
    display: flex;
    gap: 12px;
    margin-top: 24px;
    flex-wrap: wrap;
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
    color: var(--color-success);
    border-color: var(--color-success);
    background: var(--color-success-soft);
  }

  .badge[data-status='disconnected'] {
    color: var(--color-warning);
    border-color: var(--color-warning);
    background: var(--color-warning-soft);
  }

  .kind-badge {
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .warn-badge {
    background: var(--color-warning-soft);
    color: var(--color-warning);
    border-color: var(--color-warning);
  }

  .warning-msg {
    color: var(--color-warning);
    font-size: 12px;
    margin-top: 6px;
  }

  .error-msg {
    color: var(--color-danger);
    font-size: 12px;
    margin-top: 6px;
  }

  .node-warning {
    color: var(--color-warning);
    font-size: 12px;
    margin: 6px 0 !important;
  }

  .muted-note {
    font-size: 12px;
    color: var(--color-muted);
    line-height: 1.5;
  }

  .footer-note {
    font-size: 12px;
    color: var(--color-subtle);
    margin-top: 16px;
  }

  @media(max-width: 950px) {
    .columns {
      grid-template-columns: 1fr;
    }
  }
</style>

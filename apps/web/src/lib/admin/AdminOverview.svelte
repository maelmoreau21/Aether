<script lang="ts">
  import type { CoreHealth, AdminNode, Library, Scan } from './types';
  import Icon from '../ui/Icon.svelte';

  interface Props {
    coreHealth: CoreHealth | null;
    nodes: AdminNode[];
    libraries: Library[];
    scans: Record<string, Scan>;
    adminLogin?: string;
  }

  const {
    coreHealth,
    nodes,
    libraries,
    scans,
    adminLogin = 'admin'
  }: Props = $props();

  const connectedNodesCount = $derived(nodes.filter(n => n.status === 'connected').length);
  const disconnectedNodesCount = $derived(nodes.filter(n => n.status === 'disconnected').length);
  const filmLibrariesCount = $derived(libraries.filter(l => l.kind === 'films').length);
  const seriesLibrariesCount = $derived(libraries.filter(l => l.kind === 'series').length);

  const totalDiscoveredFiles = $derived(
    Object.values(scans).reduce((acc, s) => acc + (s.discovered || 0), 0)
  );
  const totalProbedFiles = $derived(
    Object.values(scans).reduce((acc, s) => acc + (s.probed || 0), 0)
  );
  const totalErrorsCount = $derived(
    Object.values(scans).reduce((acc, s) => acc + (s.errorsCount ?? s.errors?.length ?? 0), 0)
  );
</script>

<section id="overview" class="dashboard-section" aria-labelledby="overview-heading">
  <div class="section-header">
    <div class="header-icon-wrap">
      <Icon name="sparkles" size={18} />
    </div>
    <div>
      <h2 id="overview-heading" class="section-title">Vue d'ensemble du système</h2>
      <p class="section-subtitle">Surveillance en temps réel des services Core, Media Nodes et indexations</p>
    </div>
  </div>

  <div class="kpi-grid">
    <div class="kpi-card">
      <div class="kpi-top">
        <span class="kpi-label">Statut du Core</span>
        <div class="kpi-badge ok">
          <span class="status-dot connected" aria-hidden="true"></span>
          <span>{coreHealth?.status === 'ok' ? 'Opérationnel' : 'En ligne'}</span>
        </div>
      </div>
      <div class="kpi-value-row">
        <span class="kpi-value">Actif</span>
      </div>
      <span class="kpi-meta">Mode d'exécution : <code>{coreHealth?.mode ?? 'local'}</code></span>
    </div>

    <div class="kpi-card">
      <div class="kpi-top">
        <span class="kpi-label">Media Nodes</span>
        <Icon name="server" size={16} />
      </div>
      <div class="kpi-value-row">
        <span class="kpi-value">{connectedNodesCount} <span class="kpi-total">/ {nodes.length}</span></span>
      </div>
      <span class="kpi-meta">
        {#if disconnectedNodesCount > 0}
          <span class="warn-text">⚠️ {disconnectedNodesCount} nœud(s) hors ligne</span>
        {:else}
          <span class="ok-text">✓ Tous les nœuds connectés</span>
        {/if}
      </span>
    </div>

    <div class="kpi-card">
      <div class="kpi-top">
        <span class="kpi-label">Bibliothèques</span>
        <Icon name="film" size={16} />
      </div>
      <div class="kpi-value-row">
        <span class="kpi-value">{libraries.length}</span>
      </div>
      <span class="kpi-meta">{filmLibrariesCount} films · {seriesLibrariesCount} séries</span>
    </div>

    <div class="kpi-card">
      <div class="kpi-top">
        <span class="kpi-label">Médias indexés</span>
        <Icon name="check" size={16} />
      </div>
      <div class="kpi-value-row">
        <span class="kpi-value">{totalDiscoveredFiles}</span>
      </div>
      <span class="kpi-meta">
        {totalProbedFiles} analysés
        {#if totalErrorsCount > 0}
          · <span class="error-text">{totalErrorsCount} erreur(s)</span>
        {/if}
      </span>
    </div>
  </div>

  <!-- Core Diagnostic Details -->
  <div class="card core-details-card">
    <div class="card-header">
      <div class="card-title-wrap">
        <Icon name="shield" size={16} />
        <h3>Informations d'exécution du Core</h3>
      </div>
      <span class="badge" data-status="connected">Prêt pour streaming AV1/Opus</span>
    </div>
    <div class="details-grid">
      <div class="detail-item">
        <span class="detail-label">Base de données</span>
        <span class="detail-value">SQLite WAL intégrée (transactions sécurisées)</span>
      </div>
      <div class="detail-item">
        <span class="detail-label">Session administrateur</span>
        <span class="detail-value"><code>{adminLogin}</code> (rôle <code>administrator</code>)</span>
      </div>
      <div class="detail-item">
        <span class="detail-label">Stockage médias</span>
        <span class="detail-value">Lecture seule stricte (<code>:ro</code>)</span>
      </div>
      <div class="detail-item">
        <span class="detail-label">Protection CSRF</span>
        <span class="detail-value">Jeton HMAC lié à la session navigateur</span>
      </div>
    </div>
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

  .kpi-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 20px;
    margin-bottom: 24px;
  }

  .kpi-card {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: 22px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    transition: transform 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease;
  }

  .kpi-card:hover {
    border-color: var(--color-border-glow);
    transform: translateY(-2px);
    box-shadow: var(--shadow-sm);
  }

  .kpi-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    color: var(--color-muted);
  }

  .kpi-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .kpi-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 99px;
  }

  .kpi-badge.ok {
    background: #14281c;
    border: 1px solid #2e6b44;
    color: #9be7b4;
  }

  .kpi-value-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  .kpi-value {
    font-size: 30px;
    font-weight: 700;
    color: var(--color-text);
    letter-spacing: -0.02em;
  }

  .kpi-total {
    font-size: 16px;
    color: var(--color-muted);
    font-weight: 400;
  }

  .kpi-meta {
    font-size: 12px;
    color: var(--color-subtle);
  }

  .ok-text {
    color: #4ade80;
  }

  .warn-text {
    color: #f5c97b;
  }

  .error-text {
    color: var(--color-danger);
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

  .card-title-wrap {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .card-header h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }

  .details-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 20px;
  }

  .detail-item {
    padding: 12px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
  }

  .detail-label {
    display: block;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-muted);
    margin-bottom: 6px;
    font-weight: 500;
  }

  .detail-value {
    font-size: 13px;
    color: var(--color-text);
    line-height: 1.5;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    display: inline-block;
  }

  .status-dot.connected {
    background: #4ade80;
    box-shadow: 0 0 8px #4ade80aa;
  }

  .badge {
    font-size: 11px;
    font-weight: 500;
    padding: 3px 10px;
    border-radius: 99px;
    border: 1px solid var(--color-line);
  }

  .badge[data-status='connected'] {
    color: #9be7b4;
    border-color: #2e6b44;
    background: #14281c;
  }
</style>

<script module lang="ts">
  const scanLabels = {
    idle: 'Jamais scanné',
    queued: 'En attente',
    running: 'En cours',
    completed: 'Terminé',
    error: 'Erreur'
  } as const;
  const nodeStatusLabels = {
    connected: 'Connecté',
    disconnected: 'Hors ligne',
    revoked: 'Révoqué'
  } as const;
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session, type AuthUser } from '../../../lib/auth/session';
  import ProfileMenu from '../../../lib/ui/ProfileMenu.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';

  interface Library {
    id: string;
    name: string;
    kind: 'films' | 'series';
    nodeId: string;
    root: string;
    enabled: boolean;
    userIds: string[];
  }

  interface Scan {
    status: 'idle' | 'queued' | 'running' | 'completed' | 'error';
    discovered: number;
    probed: number;
    reused: number;
    errors: string[];
    errorsCount?: number;
    warningsCount?: number;
  }

  interface NodeLibraryRef {
    id: string;
    name: string;
    kind: string;
    root: string;
    enabled: boolean;
  }

  interface NodeScanSummary {
    libraryId: string;
    status: string;
    discovered: number;
    probed: number;
    reused: number;
    errorsCount: number;
    warningsCount: number;
  }

  interface AdminNode {
    id: string;
    name: string;
    kind: 'local' | 'remote';
    baseUrl?: string | null;
    publicUrl?: string | null;
    status: 'connected' | 'disconnected' | 'revoked';
    keyId: string;
    roots: string[];
    pairedAt: number;
    lastSeenAt?: number | null;
    revokedAt?: number | null;
    libraries: NodeLibraryRef[];
    scans: NodeScanSummary[];
  }

  interface OidcAdminStatus {
    enabled: boolean;
    issuer?: string | null;
    client_id?: string | null;
    redirect_uri?: string | null;
    user_groups?: string[];
    local_login_enabled?: boolean;
  }

  interface CoreHealth {
    status: string;
    mode: string;
  }

  let scans = $state<Record<string, Scan>>({});
  let pending = $state<string[]>([]);
  let polling = false;
  let libraries = $state<Library[]>([]);
  let users = $state<AuthUser[]>([]);
  let roots = $state<string[]>([]);
  let nodes = $state<AdminNode[]>([]);
  let coreHealth = $state<CoreHealth | null>(null);
  let oidcStatus = $state<OidcAdminStatus | null>(null);

  let loading = $state(true);
  let saving = $state(false);
  let pairing = $state(false);
  let nodeActionBusy = $state<string | null>(null);
  let error = $state('');
  let notice = $state('');

  let editing = $state<string | null>(null);
  let name = $state('');
  let kind = $state<'films' | 'series'>('films');
  let nodeId = $state('local');
  let root = $state('');
  let enabled = $state(true);
  let userIds = $state<string[]>([]);

  let pairName = $state('');
  let pairBaseUrl = $state('');
  let pairPublicUrl = $state('');
  let pairToken = $state('');

  const administrator = $derived($session?.user.role === 'administrator');
  const activeNodes = $derived(nodes.filter(n => n.status !== 'revoked'));
  const selectedNode = $derived(nodes.find(n => n.id === nodeId));
  const availableRootsForNode = $derived.by(() => {
    if (nodeId === 'local') {
      const localNode = nodes.find(n => n.id === 'local');
      return localNode && localNode.roots.length > 0 ? localNode.roots : roots;
    }
    return selectedNode?.roots ?? [];
  });

  // KPI Calculations
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

  function nodeLabel(id: string): string {
    if (id === 'local') return 'Node local';
    const found = nodes.find(n => n.id === id);
    return found ? `${found.name} (${found.id})` : id;
  }

  function formatTimestamp(ts?: number | null): string {
    if (!ts) return 'Jamais';
    try {
      return new Date(ts * 1000).toLocaleString('fr-FR');
    } catch {
      return String(ts);
    }
  }

  async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await fetch(path, {
      ...init,
      credentials: 'same-origin',
      cache: 'no-store',
      signal: AbortSignal.timeout(10000)
    });
    if (response.status === 401) throw new Error('Votre session a expiré. Reconnectez-vous.');
    if (response.status === 403) throw new Error('Accès refusé. Vérifiez votre session et vos droits.');
    if (response.status === 400 || response.status === 409) {
      throw new Error('Configuration refusée. Vérifiez le nom, la racine et les utilisateurs.');
    }
    if (response.status === 502) {
      throw new Error('Le Media Node distant est injoignable ou le jeton d’appairage est invalide.');
    }
    if (!response.ok) throw new Error('Le serveur est indisponible. Réessayez.');
    return response.json() as Promise<T>;
  }

  function reset() {
    editing = null;
    name = '';
    kind = 'films';
    nodeId = 'local';
    root = roots[0] ?? '';
    enabled = true;
    userIds = [];
  }

  function handleNodeChange() {
    const candidateRoots = availableRootsForNode;
    if (!candidateRoots.includes(root)) {
      root = candidateRoots[0] ?? '';
    }
  }

  function edit(library: Library) {
    editing = library.id;
    name = library.name;
    kind = library.kind;
    nodeId = library.nodeId;
    root = library.root;
    enabled = library.enabled;
    userIds = [...library.userIds];
    error = '';
    notice = '';
  }

  async function load() {
    if (!administrator) {
      loading = false;
      return;
    }
    loading = true;
    error = '';
    try {
      const [data, accounts, configured, nodeList, health, oidc] = await Promise.all([
        request<{ libraries: Library[] }>('/api/v1/admin/libraries'),
        request<{ users: AuthUser[] }>('/api/v1/admin/users'),
        request<{ roots: string[] }>('/api/v1/admin/roots'),
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes').catch(() => ({ nodes: [] as AdminNode[] })),
        request<CoreHealth>('/api/v1/health').catch(() => null),
        request<OidcAdminStatus>('/api/v1/admin/oidc').catch(() => null)
      ]);
      libraries = data.libraries;
      users = accounts.users;
      roots = configured.roots;
      nodes = nodeList.nodes;
      coreHealth = health;
      oidcStatus = oidc;
      reset();
      await refreshScans();
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Chargement impossible.';
    } finally {
      loading = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!$session || !administrator || saving) return;
    saving = true;
    error = '';
    notice = '';
    try {
      await request(editing ? `/api/v1/admin/libraries/${encodeURIComponent(editing)}` : '/api/v1/admin/libraries', {
        method: editing ? 'PUT' : 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
        body: JSON.stringify({ name: name.trim(), kind, nodeId, root, enabled, userIds })
      });
      const [data, nodeList] = await Promise.all([
        request<{ libraries: Library[] }>('/api/v1/admin/libraries'),
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes').catch(() => ({ nodes }))
      ]);
      libraries = data.libraries;
      nodes = nodeList.nodes;
      reset();
      notice = 'Bibliothèque enregistrée. Les permissions sont appliquées aux prochaines requêtes.';
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Enregistrement impossible.';
    } finally {
      saving = false;
    }
  }

  async function pairNode(event: SubmitEvent) {
    event.preventDefault();
    if (!$session || !administrator || pairing) return;
    pairing = true;
    error = '';
    notice = '';
    try {
      const created = await request<AdminNode>('/api/v1/admin/nodes/pair', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
        body: JSON.stringify({
          name: pairName.trim(),
          baseUrl: pairBaseUrl.trim(),
          publicUrl: pairPublicUrl.trim() || null,
          pairingToken: pairToken.trim()
        })
      });
      const [nodeList, configured] = await Promise.all([
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes'),
        request<{ roots: string[] }>('/api/v1/admin/roots')
      ]);
      nodes = nodeList.nodes;
      roots = configured.roots;
      pairName = '';
      pairBaseUrl = '';
      pairPublicUrl = '';
      pairToken = '';
      nodeId = created.id;
      root = created.roots[0] ?? root;
      notice = `Media Node « ${created.name} » (${created.id}) appairé avec succès.`;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Appairage du Media Node impossible.';
    } finally {
      pairing = false;
    }
  }

  async function nodeAction(targetNode: AdminNode, action: 'reconnect' | 'rotate-secret' | 'revoke') {
    if (!$session || !administrator || nodeActionBusy) return;
    nodeActionBusy = `${targetNode.id}:${action}`;
    error = '';
    notice = '';
    try {
      await request<AdminNode>(`/api/v1/admin/nodes/${encodeURIComponent(targetNode.id)}/${action}`, {
        method: 'POST',
        headers: { 'x-csrf-token': $session.csrfToken }
      });
      const [nodeList, configured] = await Promise.all([
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes'),
        request<{ roots: string[] }>('/api/v1/admin/roots').catch(() => ({ roots }))
      ]);
      nodes = nodeList.nodes;
      roots = configured.roots;
      notice = action === 'revoke'
        ? `Media Node « ${targetNode.name} » révoqué.`
        : action === 'rotate-secret'
          ? `Secret du Media Node « ${targetNode.name} » renouvelé.`
          : `Media Node « ${targetNode.name} » reconnecté.`;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Action sur le Media Node impossible.';
    } finally {
      nodeActionBusy = null;
    }
  }

  async function refreshScans() {
    if (polling || !administrator) return;
    polling = true;
    try {
      const scannable = libraries.filter(
        library => (library.kind === 'films' || library.kind === 'series') && library.enabled && !library.id.startsWith('fixture-')
      );
      const results = await Promise.all(
        scannable.map(async library => [
          library.id,
          await request<Scan>(`/api/v1/admin/libraries/${encodeURIComponent(library.id)}/scan`)
        ] as const)
      );
      for (const [id, scan] of results) scans[id] = scan;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'État du scan indisponible.';
    } finally {
      polling = false;
    }
  }

  async function scan(library: Library) {
    if (!$session || !administrator || pending.includes(library.id)) return;
    pending = [...pending, library.id];
    error = '';
    notice = '';
    try {
      const started = await request<Pick<Scan, 'status'>>(
        `/api/v1/admin/libraries/${encodeURIComponent(library.id)}/scan`,
        { method: 'POST', headers: { 'x-csrf-token': $session.csrfToken } }
      );
      scans[library.id] = {
        status: started.status,
        discovered: 0,
        probed: 0,
        reused: 0,
        errors: [],
        errorsCount: 0,
        warningsCount: 0
      };
      await refreshScans();
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Lancement du scan impossible.';
    } finally {
      pending = pending.filter(id => id !== library.id);
    }
  }

  onMount(() => {
    void load();
    const timer = setInterval(() => {
      if (Object.values(scans).some(scan => scan.status === 'queued' || scan.status === 'running')) {
        void refreshScans();
      }
    }, 1500);
    return () => clearInterval(timer);
  });
</script>

<svelte:head>
  <title>Administration — Aether</title>
</svelte:head>

<header class="admin-header">
  <div class="header-left">
    <a class="back-link" href={resolve('/')}>← Retour à l’application</a>
    <span class="header-badge">Console Administrateur</span>
  </div>
  <div class="header-right">
    <ProfileMenu />
  </div>
</header>

<main>
  <div class="admin-hero">
    <p class="eyebrow">AETHER CORE · GESTION DE LA PLATEFORME</p>
    <h1>Tableau de bord administrateur</h1>
    <p class="hero-desc">
      Supervisez le serveur Core, surveillez l'état des Media Nodes distribués, organisez vos bibliothèques et configurez les droits d'accès.
    </p>
  </div>

  {#if !administrator}
    <div class="error-banner" role="alert">
      <strong>Accès refusé</strong>
      <p>Cette console est strictement réservée aux comptes disposant du rôle <code>administrator</code>.</p>
    </div>
  {:else}
    {#if error}
      <div class="error-banner" role="alert">
        <p>{error}</p>
        <button class="small-btn" disabled={saving} onclick={load}>Réessayer</button>
      </div>
    {/if}

    {#if notice}
      <div class="notice-banner" role="status">
        <p>{notice}</p>
      </div>
    {/if}

    <!-- Quick Navigation Anchor Bar -->
    <nav class="admin-nav" aria-label="Sections d’administration">
      <a href="#overview">📊 Vue d’ensemble</a>
      <a href="#libraries-section">📁 Bibliothèques & Scans</a>
      <a href="#nodes-section">🖥️ Media Nodes</a>
      <a href="#users-section">👥 Utilisateurs & Droits</a>
      <a href="#sso-section">🔐 SSO Authentik (OIDC)</a>
    </nav>

    {#if loading}
      <div class="loading-state" role="status">
        <div class="spinner" aria-hidden="true"></div>
        <p>Chargement des données administratives…</p>
      </div>
    {:else}
      <!-- Section 1: Dashboard Overview -->
      <section id="overview" class="dashboard-section" aria-labelledby="overview-heading">
        <h2 id="overview-heading" class="section-title">Vue d'ensemble du système</h2>
        
        <div class="kpi-grid">
          <div class="kpi-card">
            <span class="kpi-label">Statut du Core</span>
            <div class="kpi-value-row">
              <span class="status-dot connected" aria-hidden="true"></span>
              <span class="kpi-value">{coreHealth?.status === 'ok' ? 'Opérationnel' : 'En ligne'}</span>
            </div>
            <span class="kpi-meta">Mode : <code>{coreHealth?.mode ?? 'local'}</code></span>
          </div>

          <div class="kpi-card">
            <span class="kpi-label">Media Nodes</span>
            <div class="kpi-value-row">
              <span class="kpi-value">{connectedNodesCount} / {nodes.length}</span>
            </div>
            <span class="kpi-meta">
              {#if disconnectedNodesCount > 0}
                <span class="warn-text">{disconnectedNodesCount} hors ligne</span>
              {:else}
                Tous les nœuds sont en ligne
              {/if}
            </span>
          </div>

          <div class="kpi-card">
            <span class="kpi-label">Bibliothèques</span>
            <div class="kpi-value-row">
              <span class="kpi-value">{libraries.length}</span>
            </div>
            <span class="kpi-meta">{filmLibrariesCount} films · {seriesLibrariesCount} séries</span>
          </div>

          <div class="kpi-card">
            <span class="kpi-label">Indexation médias</span>
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
            <h3>Informations d'exécution du Core</h3>
            <span class="badge" data-status="connected">Actif</span>
          </div>
          <div class="details-grid">
            <div>
              <span class="detail-label">Base de données</span>
              <span class="detail-value">SQLite WAL intégrée (transactions sécurisées)</span>
            </div>
            <div>
              <span class="detail-label">Session administrateur</span>
              <span class="detail-value"><code>{$session?.user.login}</code> (rôle <code>administrator</code>)</span>
            </div>
            <div>
              <span class="detail-label">Stockage médias</span>
              <span class="detail-value">Lecture seule stricte (<code>:ro</code>)</span>
            </div>
            <div>
              <span class="detail-label">Protection CSRF</span>
              <span class="detail-value">Jeton HMAC lié à la session navigateur</span>
            </div>
          </div>
        </div>
      </section>

      <!-- Section 2: Libraries & Scans -->
      <section id="libraries-section" class="dashboard-section" aria-labelledby="libraries-heading">
        <h2 id="libraries-heading" class="section-title">Gestion des bibliothèques & Scans</h2>

        <div class="columns">
          <section class="card" aria-labelledby="library-list">
            <h3 id="library-list">Bibliothèques configurées ({libraries.length})</h3>
            {#if libraries.length === 0}
              <p class="muted">Aucune bibliothèque configurée pour le moment.</p>
            {/if}

            <div class="items-list">
              {#each libraries as library (library.id)}
                {@const nodeObj = nodes.find(n => n.id === library.nodeId)}
                {@const isNodeOffline = nodeObj && nodeObj.status !== 'connected'}
                <article class="item-card" class:offline-library={isNodeOffline}>
                  <div class="item-content">
                    <div class="item-title-row">
                      <h4>{library.name}</h4>
                      <span class="badge kind-badge">{library.kind === 'films' ? 'Films' : 'Séries'}</span>
                      <span class="badge" data-status={library.enabled ? 'connected' : 'disconnected'}>
                        {library.enabled ? 'Active' : 'Désactivée'}
                      </span>
                      {#if isNodeOffline}
                        <span class="badge warn-badge">Node hors ligne</span>
                      {/if}
                    </div>

                    <p class="path"><strong>Racine :</strong> <code>{library.root}</code></p>
                    <p class="node-info"><strong>Media Node :</strong> {nodeLabel(library.nodeId)}</p>
                    <p class="grants-info"><strong>Accès :</strong> {library.userIds.length} utilisateur(s) autorisé(s)</p>

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
                              ⚠️ {scanData.errors.length} erreur(s) de scan répertoriée(s)
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
                          onclick={() => scan(library)}
                          aria-label={`Scanner ${library.name}`}
                        >
                          {pending.includes(library.id) || scans[library.id]?.status === 'running' ? 'Scan en cours…' : `Scanner ${library.name}`}
                        </button>
                      </div>
                    {/if}
                  </div>

                  <div class="item-side-action">
                    <button class="secondary-btn" disabled={saving} onclick={() => edit(library)} aria-label={`Modifier ${library.name}`}>
                      Modifier
                    </button>
                  </div>
                </article>
              {/each}
            </div>
          </section>

          <!-- Add / Edit Library Form -->
          <section class="card" aria-labelledby="library-form">
            <h3 id="library-form">{editing ? 'Modifier la bibliothèque' : 'Ajouter une bibliothèque'}</h3>

            <div class="info-callout">
              <Icon name="film" size={16} />
              <span>
                <strong>Multi-Node & Polyvalence :</strong> Vous pouvez associer des bibliothèques <em>Films</em> et <em>Séries</em> distinctes au même Media Node en sélectionnant simplement ses racines exposées correspondantes.
              </span>
            </div>

            <form onsubmit={save}>
              <fieldset disabled={saving}>
                <label for="library-name">Nom de la bibliothèque</label>
                <input id="library-name" bind:value={name} required maxlength="100" autocomplete="off" placeholder="Ex. Mes Films 4K" />

                <label for="library-kind">Type de contenu</label>
                <select id="library-kind" bind:value={kind}>
                  <option value="films">Films</option>
                  <option value="series">Séries</option>
                </select>

                <label for="library-node">Media Node assigné</label>
                {#if activeNodes.length > 1}
                  <select id="library-node" bind:value={nodeId} onchange={handleNodeChange}>
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
                  <p class="muted warning-msg">Aucune racine autorisée n’est configurée pour ce Media Node.</p>
                {/if}
                {#if editing && !availableRootsForNode.includes(root)}
                  <p class="error warning-msg">Cette racine n’est plus autorisée sur ce Node. Choisissez une racine disponible avant d’enregistrer.</p>
                {/if}

                <label class="check">
                  <input type="checkbox" bind:checked={enabled} />
                  <span>Bibliothèque active (visible et diffusable)</span>
                </label>

                <fieldset class="grants">
                  <legend>Autorisations utilisateurs</legend>
                  <p class="muted">
                    Les administrateurs ont accès à toutes les bibliothèques. Les comptes utilisateurs n'accèdent qu'aux bibliothèques expressément cochées.
                  </p>
                  <div class="users-checkbox-grid">
                    {#each users.filter(user => user.role === 'user') as user (user.id)}
                      <label class="check">
                        <input type="checkbox" bind:group={userIds} value={user.id} />
                        <span>{user.login}</span>
                      </label>
                    {/each}
                  </div>
                  {#if !users.some(user => user.role === 'user')}
                    <p class="muted">Aucun compte utilisateur ordinaire n’est enregistré.</p>
                  {/if}
                </fieldset>

                <div class="actions">
                  <button class="primary" type="submit" disabled={!availableRootsForNode.includes(root)}>
                    {saving ? 'Enregistrement…' : 'Enregistrer'}
                  </button>
                  <button class="secondary-btn" type="button" onclick={reset}>
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

      <!-- Section 3: Media Nodes -->
      <section id="nodes-section" class="dashboard-section" aria-labelledby="nodes-heading">
        <h2 id="nodes-heading" class="section-title">Gestion des Media Nodes</h2>

        <div class="columns">
          <section class="card" aria-labelledby="node-list">
            <h3 id="node-list">Media Nodes enregistrés ({nodes.length})</h3>
            {#if nodes.length === 0}
              <p class="muted">Aucun Media Node enregistré.</p>
            {/if}

            <div class="items-list">
              {#each nodes as node (node.id)}
                <article class="item-card">
                  <div class="item-content">
                    <div class="item-title-row">
                      <h4>{node.name}</h4>
                      <span class="badge" data-status={node.status}>{nodeStatusLabels[node.status]}</span>
                      <span class="badge kind-badge">{node.kind === 'local' ? 'Local' : 'Distant'}</span>
                    </div>

                    <p class="path"><strong>Identifiant :</strong> <code>{node.id}</code></p>
                    {#if node.baseUrl}
                      <p class="path"><strong>URL interne (Core → Node) :</strong> <code>{node.baseUrl}</code></p>
                    {/if}
                    {#if node.publicUrl}
                      <p class="path"><strong>URL directe (Navigateur → Node) :</strong> <code>{node.publicUrl}</code></p>
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
                          onclick={() => nodeAction(node, 'reconnect')}
                          aria-label={`Reconnecter ${node.name}`}
                        >
                          Reconnecter
                        </button>
                        <button
                          class="action-btn"
                          disabled={!!nodeActionBusy}
                          onclick={() => nodeAction(node, 'rotate-secret')}
                          aria-label={`Renouveler le secret de ${node.name}`}
                        >
                          Renouveler secret
                        </button>
                        <button
                          class="action-btn danger-btn"
                          disabled={!!nodeActionBusy}
                          onclick={() => nodeAction(node, 'revoke')}
                          aria-label={`Révoquer ${node.name}`}
                        >
                          Révoquer
                        </button>
                      {/if}
                    </div>
                  {/if}
                </article>
              {/each}
            </div>
          </section>

          <!-- Pair Node Form -->
          <section class="card" aria-labelledby="node-pair-form">
            <h3 id="node-pair-form">Appairer un Media Node distant</h3>
            <p class="muted">
              L'appairage établit une connexion sécurisée par échange cryptographique HMAC-SHA256, sans exposer les sessions utilisateurs.
            </p>

            <form onsubmit={pairNode}>
              <fieldset disabled={pairing}>
                <label for="pair-node-name">Nom du Media Node</label>
                <input id="pair-node-name" bind:value={pairName} required maxlength="100" placeholder="Ex. Node Séries NAS" autocomplete="off" />

                <label for="pair-node-base-url">URL interne du Node (Core → Node)</label>
                <input id="pair-node-base-url" bind:value={pairBaseUrl} required placeholder="http://aether-node:3101" autocomplete="off" />

                <label for="pair-node-public-url">URL publique de streaming direct (Optionnel)</label>
                <input id="pair-node-public-url" bind:value={pairPublicUrl} placeholder="http://192.168.1.100:3101" autocomplete="off" />

                <label for="pair-node-token">Jeton d’amorçage / appairage (défini au démarrage du Node)</label>
                <input id="pair-node-token" type="password" bind:value={pairToken} required autocomplete="off" />

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

      <!-- Section 4: Users & RBAC Permissions -->
      <section id="users-section" class="dashboard-section" aria-labelledby="users-heading">
        <h2 id="users-heading" class="section-title">Comptes utilisateurs & Autorisations</h2>
        
        <div class="card">
          <div class="card-header">
            <h3>Comptes enregistrés ({users.length})</h3>
            <span class="muted">Rôles stricts : <code>administrator</code> et <code>user</code> uniquement.</span>
          </div>

          <div class="users-table-wrapper">
            <table class="admin-table">
              <thead>
                <tr>
                  <th>Identifiant / Login</th>
                  <th>Type de compte</th>
                  <th>Rôle</th>
                  <th>Bibliothèques autorisées</th>
                </tr>
              </thead>
              <tbody>
                {#each users as user (user.id)}
                  {@const isSso = user.login.startsWith('sso-')}
                  {@const userLibraries = libraries.filter(lib => lib.userIds.includes(user.id))}
                  <tr>
                    <td>
                      <strong>{user.login}</strong>
                      {#if user.id === $session?.user.id}
                        <span class="current-user-badge">Vous</span>
                      {/if}
                    </td>
                    <td>
                      <span class="badge" data-status={isSso ? 'disconnected' : 'connected'}>
                        {isSso ? 'SSO Authentik' : 'Compte local'}
                      </span>
                    </td>
                    <td>
                      <span class="role-badge" class:admin-role={user.role === 'administrator'}>
                        {user.role === 'administrator' ? 'Administrateur' : 'Utilisateur'}
                      </span>
                    </td>
                    <td>
                      {#if user.role === 'administrator'}
                        <span class="all-access-badge">Toutes les bibliothèques (administrateur)</span>
                      {:else if userLibraries.length > 0}
                        <div class="lib-badges-list">
                          {#each userLibraries as lib (lib.id)}
                            <span class="badge kind-badge">{lib.name}</span>
                          {/each}
                        </div>
                      {:else}
                        <span class="muted">Aucune bibliothèque autorisée</span>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <!-- Section 5: SSO Authentik (OIDC) -->
      <section id="sso-section" class="dashboard-section" aria-labelledby="sso-heading">
        <h2 id="sso-heading" class="section-title">Authentification SSO (OpenID Connect / Authentik)</h2>
        
        <div class="card">
          <div class="card-header">
            <h3>Statut du fournisseur OIDC</h3>
            <span class="badge" data-status={oidcStatus?.enabled ? 'connected' : 'disconnected'}>
              {oidcStatus?.enabled ? 'OIDC Configuré' : 'OIDC Non activé'}
            </span>
          </div>

          {#if oidcStatus?.enabled}
            <div class="details-grid">
              <div>
                <span class="detail-label">Fournisseur (Issuer)</span>
                <span class="detail-value"><code>{oidcStatus.issuer ?? 'Non renseigné'}</code></span>
              </div>
              <div>
                <span class="detail-label">Identifiant client (Client ID)</span>
                <span class="detail-value"><code>{oidcStatus.client_id ?? 'Non renseigné'}</code></span>
              </div>
              <div>
                <span class="detail-label">URI de redirection Core</span>
                <span class="detail-value"><code>{oidcStatus.redirect_uri ?? 'Non renseigné'}</code></span>
              </div>
              <div>
                <span class="detail-label">Groupes autorisés (claim groups)</span>
                <span class="detail-value">
                  {oidcStatus.user_groups?.length ? oidcStatus.user_groups.join(', ') : 'Tous les utilisateurs'}
                </span>
              </div>
            </div>
          {:else}
            <p class="muted">
              Le protocole OIDC n'est pas encore configuré sur cette instance. Pour activer l'authentification avec Authentik, définissez les variables d'environnement requises sur le Core :
            </p>
            <pre class="env-code"><code>MEDIA_OIDC_ISSUER_URL=https://authentik.lan/application/o/aether/
MEDIA_OIDC_CLIENT_ID=votre-client-id
MEDIA_OIDC_CLIENT_SECRET=votre-secret
MEDIA_OIDC_REDIRECT_URI=https://votre-domaine/api/v1/auth/oidc/callback
MEDIA_OIDC_USER_GROUPS=aether-users</code></pre>
          {/if}

          <div class="security-note">
            <Icon name="check" size={16} />
            <div>
              <strong>Sécurité RBAC garantie :</strong>
              <p>
                L'authentification OIDC utilise le flux sécurisé <em>Authorization Code + PKCE</em>. Les utilisateurs authentifiés via Authentik sont automatiquement créés avec le rôle <code>user</code>, sans aucun droit d'accès implicite sur les bibliothèques. L'administrateur local conserve toujours un accès direct de secours.
              </p>
            </div>
          </div>
        </div>
      </section>
    {/if}
  {/if}
</main>

<style>
  .admin-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 16px var(--page-gutter);
    border-bottom: 1px solid var(--color-line);
    background: var(--color-surface);
  }
  .header-left {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .back-link {
    font-size: 13px;
    color: var(--color-muted);
  }
  .back-link:hover {
    color: var(--color-text);
  }
  .header-badge {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    padding: 4px 10px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: 99px;
    color: var(--color-accent);
  }

  main {
    max-width: 1440px;
    margin: auto;
    padding: 40px var(--page-gutter) 80px;
  }

  .admin-hero {
    margin-bottom: 32px;
  }
  .admin-hero h1 {
    font-size: clamp(28px, 4vw, 42px);
    margin: 10px 0 12px;
    letter-spacing: -0.04em;
  }
  .hero-desc {
    color: var(--color-muted);
    font-size: 14px;
    line-height: 1.7;
    max-width: 820px;
    margin: 0;
  }

  .admin-nav {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 36px;
    padding: 12px 16px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
  }
  .admin-nav a {
    font-size: 13px;
    padding: 8px 14px;
    border-radius: var(--radius-sm);
    color: var(--color-muted);
    background: var(--color-raised);
    border: 1px solid transparent;
  }
  .admin-nav a:hover {
    color: var(--color-text);
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
  }

  .dashboard-section {
    margin-bottom: 56px;
    scroll-margin-top: 24px;
  }
  .section-title {
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.02em;
    margin: 0 0 20px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--color-line);
    color: var(--color-text);
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
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .kpi-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .kpi-value-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .kpi-value {
    font-size: 26px;
    font-weight: 650;
    color: var(--color-text);
  }
  .kpi-meta {
    font-size: 12px;
    color: var(--color-subtle);
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
    font-size: 17px;
  }

  .details-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 20px;
  }
  .detail-label {
    display: block;
    font-size: 12px;
    color: var(--color-muted);
    margin-bottom: 4px;
  }
  .detail-value {
    font-size: 13px;
    color: var(--color-text);
  }

  .columns {
    display: grid;
    grid-template-columns: 1.15fr 0.85fr;
    gap: 28px;
  }

  .items-list {
    display: grid;
    gap: 16px;
    margin-top: 16px;
  }
  .item-card {
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 18px;
    display: flex;
    justify-content: space-between;
    gap: 16px;
  }
  .offline-library {
    border-color: #f5c97b55;
    background: #1e1b15;
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
    font-weight: 600;
  }
  .item-content p {
    margin: 4px 0;
    font-size: 12px;
    color: var(--color-muted);
  }
  .path {
    overflow-wrap: anywhere;
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

  .scan-state {
    margin-top: 12px;
    padding: 12px 14px;
    background: #101017;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
  }
  .scan-state.scanning {
    border-color: var(--color-accent);
  }
  .scan-header-row {
    margin-bottom: 8px;
  }
  .scan-indicator-badge {
    font-size: 12px;
    font-weight: 600;
  }
  .scan-indicator-badge[data-scan='completed'] {
    color: #9be7b4;
  }
  .scan-indicator-badge[data-scan='running'],
  .scan-indicator-badge[data-scan='queued'] {
    color: #b4a1f8;
  }
  .scan-indicator-badge[data-scan='error'] {
    color: var(--color-danger);
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
  .item-actions-vertical {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-shrink: 0;
  }
  .item-side-action {
    flex-shrink: 0;
  }

  .action-btn, .secondary-btn, button {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 8px 14px;
    font-size: 12px;
    color: var(--color-text);
    background: var(--color-raised);
    min-height: 38px;
  }
  .action-btn:hover:not(:disabled), .secondary-btn:hover:not(:disabled) {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
  }
  .scan-btn {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border-color: #b4a1f844;
  }
  .scan-btn:hover:not(:disabled) {
    background: var(--color-accent);
    color: #16121d;
  }
  .danger-btn {
    color: var(--color-danger);
    border-color: #f3aaa944;
  }
  .danger-btn:hover:not(:disabled) {
    background: #f3aaa922;
  }

  .primary {
    background: var(--color-accent);
    color: #16121d;
    font-weight: 600;
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
  input:not([type='checkbox']), select {
    width: 100%;
    min-height: 44px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 10px 14px;
    color: var(--color-text);
    background: var(--color-raised);
    font-size: 13px;
  }
  input:focus, select:focus {
    border-color: var(--color-accent);
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
  }
  .check input {
    accent-color: var(--color-accent);
    width: 18px;
    height: 18px;
  }

  .grants {
    margin: 24px 0 16px;
    padding: 16px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    background: #101017;
  }
  .grants legend {
    font-size: 13px;
    font-weight: 600;
    padding: 0 6px;
  }
  .users-checkbox-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 10px;
    margin-top: 12px;
  }

  .actions {
    display: flex;
    gap: 12px;
    margin-top: 24px;
    flex-wrap: wrap;
  }

  .badge {
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
  .warn-badge {
    background: #38240b;
    color: #f5c97b;
    border-color: #75541e;
  }

  .status-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    display: inline-block;
  }
  .status-dot.connected {
    background: #4ade80;
    box-shadow: 0 0 10px #4ade8088;
  }

  .error-banner, .notice-banner {
    padding: 16px 20px;
    border-radius: var(--radius-sm);
    margin-bottom: 24px;
    font-size: 13px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .error-banner {
    background: #361719;
    border: 1px solid var(--color-danger);
    color: var(--color-danger);
  }
  .notice-banner {
    background: #14281c;
    border: 1px solid #2e6b44;
    color: #9be7b4;
  }

  .users-table-wrapper {
    overflow-x: auto;
  }
  .admin-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    text-align: left;
  }
  .admin-table th {
    padding: 12px 14px;
    border-bottom: 1px solid var(--color-line);
    color: var(--color-muted);
    font-weight: 500;
    font-size: 12px;
  }
  .admin-table td {
    padding: 14px;
    border-bottom: 1px solid var(--color-line);
    vertical-align: middle;
  }
  .current-user-badge {
    font-size: 10px;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    padding: 2px 6px;
    border-radius: 4px;
    margin-left: 6px;
  }
  .role-badge {
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 4px;
    background: var(--color-raised);
    color: var(--color-muted);
  }
  .admin-role {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-weight: 600;
  }
  .all-access-badge {
    color: #9be7b4;
    font-size: 12px;
  }
  .lib-badges-list {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .env-code {
    background: #101017;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 16px;
    overflow-x: auto;
    font-size: 12px;
    color: #e5e0f0;
    line-height: 1.6;
  }

  .security-note {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-top: 20px;
    padding: 16px;
    background: var(--color-raised);
    border-radius: var(--radius-sm);
    font-size: 13px;
    line-height: 1.6;
    color: var(--color-muted);
  }
  .security-note strong {
    color: var(--color-text);
  }
  .security-note p {
    margin: 4px 0 0;
  }

  .node-warning {
    color: #f5c97b;
    font-size: 12px;
    margin: 6px 0 !important;
  }
  .footer-note {
    font-size: 12px;
    margin-top: 16px;
  }
  .warn-text {
    color: #f5c97b;
  }
  .error-text {
    color: var(--color-danger);
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 60px 0;
    color: var(--color-muted);
  }
  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--color-line);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  @media(max-width: 950px) {
    .columns {
      grid-template-columns: 1fr;
    }
  }
</style>

<script module lang="ts">
  export { scanLabels, nodeStatusLabels } from '../../../lib/admin/types';
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session, type AuthUser } from '../../../lib/auth/session';
  import ProfileMenu from '../../../lib/ui/ProfileMenu.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import type { Library, Scan, AdminNode, OidcAdminStatus, CoreHealth, EnrollmentToken } from '../../../lib/admin/types';
  import AdminOverview from '../../../lib/admin/AdminOverview.svelte';
  import AdminLibraries from '../../../lib/admin/AdminLibraries.svelte';
  import AdminNodes from '../../../lib/admin/AdminNodes.svelte';
  import AdminUsers from '../../../lib/admin/AdminUsers.svelte';
  import AdminOidc from '../../../lib/admin/AdminOidc.svelte';
  import AdminTasks from '../../../lib/admin/AdminTasks.svelte';
  import AdminMaintenance from '../../../lib/admin/AdminMaintenance.svelte';
  import AdminCapabilities from '../../../lib/admin/AdminCapabilities.svelte';
  import AdminGroups from '../../../lib/admin/AdminGroups.svelte';
  import AdminApiKeys from '../../../lib/admin/AdminApiKeys.svelte';
  import type { UserGroup } from '../../../lib/admin/request';
  import { t } from '../../../lib/i18n';

  let scans = $state<Record<string, Scan>>({});
  let pending = $state<string[]>([]);
  let polling = false;
  let libraries = $state<Library[]>([]);
  let users = $state<AuthUser[]>([]);
  let groups = $state<UserGroup[]>([]);
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
  const selectedNode = $derived(nodes.find(n => n.id === nodeId));
  const availableRootsForNode = $derived.by(() => {
    if (nodeId === 'local') {
      const localNode = nodes.find(n => n.id === 'local');
      return localNode && localNode.roots.length > 0 ? localNode.roots : roots;
    }
    return selectedNode?.roots ?? [];
  });

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

  async function refreshAccess() {
    const [accounts, memberships] = await Promise.all([
      request<{ users: AuthUser[] }>('/api/v1/admin/users'),
      request<{ groups: UserGroup[] }>('/api/v1/admin/groups')
    ]);
    users = accounts.users;
    groups = memberships.groups;
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
      await refreshAccess();
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

  async function generateEnrollmentToken(): Promise<EnrollmentToken> {
    if (!$session || !administrator) throw new Error('Action non autorisée.');
    return await request<EnrollmentToken>('/api/v1/admin/nodes/enrollment-tokens', {
      method: 'POST',
      headers: {
        'x-csrf-token': $session.csrfToken,
        'content-type': 'application/json'
      },
      body: JSON.stringify({ expiresInSecs: 900 })
    });
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

  let currentHash = $state('#overview');
  const sections = new Set(['#overview', '#libraries-section', '#nodes-section', '#users-section', '#sso-section', '#tasks-section', '#backups-section', '#groups-section', '#api-keys-section', '#extensions-section']);
  function selectedSection() {
    return sections.has(window.location.hash) ? window.location.hash : '#overview';
  }

  onMount(() => {
    void load();
    if (typeof window !== 'undefined' && window.location.hash) {
      currentHash = selectedSection();
    }
    const hashHandler = () => {
      currentHash = selectedSection();
    };
    window.addEventListener('hashchange', hashHandler);
    const timer = setInterval(() => {
      if (Object.values(scans).some(scan => scan.status === 'queued' || scan.status === 'running')) {
        void refreshScans();
      }
    }, 1500);
    return () => {
      clearInterval(timer);
      window.removeEventListener('hashchange', hashHandler);
    };
  });
</script>

<svelte:head>
  <title>Administration — Aether</title>
</svelte:head>

<header class="admin-header">
  <div class="header-left">
    <a class="back-link" href={resolve('/')}>
      <Icon name="chevron-left" size={14} />
      <span>Retour à l’application</span>
    </a>
    <span class="header-separator">/</span>
    <span class="header-badge">Console Administrateur</span>
  </div>
  <div class="header-right">
    <ProfileMenu />
  </div>
</header>

<main>
  {#if !administrator}
    <div class="error-banner" role="alert">
      <div class="banner-content">
        <Icon name="shield" size={18} />
        <div>
          <strong>Accès refusé</strong>
          <p>Cette console est strictement réservée aux comptes disposant du rôle <code>administrator</code>.</p>
        </div>
      </div>
    </div>
  {:else}
    {#if error}
      <div class="error-banner" role="alert">
        <div class="banner-content">
          <Icon name="info" size={18} />
          <p>{error}</p>
        </div>
        <button class="small-btn" disabled={saving} onclick={load}>Réessayer</button>
      </div>
    {/if}

    {#if notice}
      <div class="notice-banner" role="status">
        <div class="banner-content">
          <Icon name="check" size={18} />
          <p>{notice}</p>
          <div hidden={currentHash !== '#tasks-section'}>
            <AdminTasks {libraries} {nodes} {scans} {pending} onscan={scan} />
          </div>
          <div hidden={currentHash !== '#backups-section'}>
            <AdminMaintenance />
          </div>
          <div hidden={currentHash !== '#groups-section'}>
            <AdminGroups {groups} {users} {libraries} onchanged={refreshAccess} />
          </div>
          <div hidden={currentHash !== '#api-keys-section'}>
            <AdminApiKeys {users} {libraries} />
          </div>
          <AdminCapabilities section={currentHash.slice(1)} />
        </div>
      </div>
    {/if}

    {#if loading}
      <div class="loading-state" role="status">
        <div class="spinner" aria-hidden="true"></div>
        <p>Chargement des données administratives…</p>
      </div>
    {:else}
      <div class="admin-dashboard-container">
        <!-- Jellyfin-Style Navigation Sidebar -->
        <aside class="admin-sidebar" aria-label="Menu d’administration">
          <div class="sidebar-header">
            <span class="sidebar-brand-title">CONSOLE SERVEUR</span>
          </div>

          <div class="sidebar-group">
            <span class="group-label">{$t('admin_category_server')}</span>
            <a
              href="#overview"
              class="sidebar-nav-item"
              class:active={currentHash === '#overview' || currentHash === ''}
              onclick={() => currentHash = '#overview'}
            >
              <span class="nav-icon"><Icon name="sparkles" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_overview')}</span>
              {#if coreHealth?.status === 'ok'}
                <span class="status-indicator online" title="Core opérationnel"></span>
              {/if}
            </a>
          </div>

          <div class="sidebar-group">
            <span class="group-label">{$t('admin_category_media')}</span>
            <a
              href="#libraries-section"
              class="sidebar-nav-item"
              class:active={currentHash === '#libraries-section'}
              onclick={() => currentHash = '#libraries-section'}
            >
              <span class="nav-icon"><Icon name="film" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_libraries')}</span>
              <span class="count-badge">{libraries.length}</span>
            </a>
            <a
              href="#nodes-section"
              class="sidebar-nav-item"
              class:active={currentHash === '#nodes-section'}
              onclick={() => currentHash = '#nodes-section'}
            >
              <span class="nav-icon"><Icon name="server" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_nodes')}</span>
              <span class="count-badge">{nodes.length}</span>
            </a>
          </div>

          <div class="sidebar-group">
            <span class="group-label">{$t('admin_category_access')}</span>
            <a
              href="#users-section"
              class="sidebar-nav-item"
              class:active={currentHash === '#users-section'}
              onclick={() => currentHash = '#users-section'}
            >
              <span class="nav-icon"><Icon name="users" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_users')}</span>
              <span class="count-badge">{users.length}</span>
            </a>
            <a
              href="#sso-section"
              class="sidebar-nav-item"
              class:active={currentHash === '#sso-section'}
              onclick={() => currentHash = '#sso-section'}
            >
              <span class="nav-icon"><Icon name="shield" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_sso')}</span>
              {#if oidcStatus?.enabled}
                <span class="sso-badge">{$t('active')}</span>
              {/if}
            </a>
          </div>
          <div class="sidebar-group">
            <span class="group-label">Maintenance &amp; intégrations</span>
            {#each [
              ['#tasks-section', 'Tâches', 'server'],
              ['#backups-section', 'Sauvegardes', 'server'],
              ['#groups-section', 'Groupes', 'users'],
              ['#api-keys-section', 'Clés API', 'shield'],
              ['#extensions-section', 'Extensions', 'sparkles']
            ] as [hash, label, icon] (hash)}
              <a href={resolve(`admin/libraries/#${hash.slice(1)}`)} class="sidebar-nav-item" class:active={currentHash === hash}
                aria-current={currentHash === hash ? 'page' : undefined}
                onclick={() => currentHash = hash}>
                <span class="nav-icon"><Icon name={icon as 'server' | 'users' | 'shield' | 'sparkles'} size={16} /></span>
                <span class="nav-text">{label}</span>
              </a>
            {/each}
          </div>
        </aside>

        <!-- Main Admin Content Area -->
        <div class="admin-content-area">
          <div class="admin-hero">
            <p class="eyebrow">AETHER CORE · GESTION DE LA PLATEFORME</p>
            <h1>{$t('admin_title')}</h1>
            <p class="hero-desc">
              {$t('admin_subtitle')}
            </p>
          </div>

          <!-- Section 1: Dashboard Overview -->
          <div hidden={currentHash !== "#overview"}>
          <AdminOverview
            {coreHealth}
            {nodes}
            {libraries}
            {scans}
            adminLogin={$session?.user.login}
          />
          </div>

          <!-- Section 2: Libraries & Scans -->
          <div hidden={currentHash !== "#libraries-section"}>
          <AdminLibraries
            {libraries}
            {nodes}
            {users}
            {scans}
            {pending}
            {availableRootsForNode}
            {saving}
            {editing}
            bind:name
            bind:kind
            bind:nodeId
            bind:root
            bind:enabled
            bind:userIds
            onsave={save}
            onscan={scan}
            onedit={edit}
            onreset={reset}
            onnodechange={handleNodeChange}
          />
          </div>

          <!-- Section 3: Media Nodes -->
          <div hidden={currentHash !== "#nodes-section"}>
          <AdminNodes
            {nodes}
            {pairing}
            {nodeActionBusy}
            bind:pairName
            bind:pairBaseUrl
            bind:pairPublicUrl
            bind:pairToken
            onpair={pairNode}
            onnodeaction={nodeAction}
            ongeneratetoken={generateEnrollmentToken}
          />
          </div>

          <!-- Section 4: Users & RBAC Permissions -->
          <div hidden={currentHash !== "#users-section"}>
          <AdminUsers
            {users}
            {libraries}
            {groups}
            onchanged={refreshAccess}
            currentUserId={$session?.user.id}
          />
          </div>

          <!-- Section 5: SSO OpenID Connect (OIDC) -->
          <div hidden={currentHash !== "#sso-section"}>
          <AdminOidc
            {oidcStatus}
          />
          </div>
          <div hidden={currentHash !== '#tasks-section'}>
            <AdminTasks {libraries} {nodes} {scans} {pending} onscan={scan} />
          </div>
          <div hidden={currentHash !== '#backups-section'}>
            <AdminMaintenance />
          </div>
          <div hidden={currentHash !== '#groups-section'}>
            <AdminGroups {groups} {users} {libraries} onchanged={refreshAccess} />
          </div>
          <div hidden={currentHash !== '#api-keys-section'}>
            <AdminApiKeys {users} {libraries} />
          </div>
          <AdminCapabilities section={currentHash.slice(1)} />
        </div>
      </div>
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
    background: var(--color-glass);
    backdrop-filter: blur(20px);
    -webkit-backdrop-filter: blur(20px);
    position: sticky;
    top: 0;
    z-index: 100;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .back-link {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 500;
    color: var(--color-muted);
    transition: color 0.15s ease;
  }

  .back-link:hover {
    color: var(--color-text);
  }

  .header-separator {
    color: var(--color-subtle);
    font-size: 13px;
  }

  .header-badge {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    padding: 4px 10px;
    background: var(--color-accent-soft);
    border: 1px solid var(--color-line);
    border-radius: 99px;
    color: var(--color-accent);
  }

  main {
    max-width: 1600px;
    margin: auto;
    padding: 28px var(--page-gutter) 80px;
  }

  .admin-dashboard-container {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: 36px;
    align-items: start;
  }

  /* Jellyfin-style Navigation Sidebar */
  .admin-sidebar {
    position: sticky;
    top: 86px;
    display: flex;
    flex-direction: column;
    gap: 20px;
    max-height: calc(100dvh - 112px);
    overflow-y: auto;
    padding: 20px 14px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-sm);
  }

  .sidebar-header {
    padding: 0 8px 10px;
    border-bottom: 1px solid var(--color-line);
  }

  .sidebar-brand-title {
    font-size: 10.5px;
    font-weight: 750;
    letter-spacing: 0.12em;
    color: var(--color-muted);
  }

  .sidebar-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .group-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--color-subtle);
    padding: 4px 8px;
    text-transform: uppercase;
  }

  .sidebar-nav-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    color: var(--color-muted);
    font-size: 13px;
    font-weight: 500;
    text-decoration: none;
    transition: all 0.15s ease;
    border-left: 3px solid transparent;
  }

  .sidebar-nav-item:hover {
    color: var(--color-text-bright);
    background: var(--color-raised);
  }

  .sidebar-nav-item.active {
    color: var(--color-text-bright);
    font-weight: 600;
    background: var(--color-accent-soft);
    border-left-color: var(--color-accent);

  }

  .nav-icon {
    display: grid;
    place-items: center;
    color: inherit;
    flex-shrink: 0;
  }

  .nav-text {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .count-badge {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 99px;
    background: var(--color-raised);
    color: var(--color-muted);
    border: 1px solid var(--color-line);
  }

  .sidebar-nav-item.active .count-badge {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border-color: var(--color-line);
  }

  .sso-badge {
    font-size: 10px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 99px;
    background: #14281c;
    color: #9be7b4;
    border: 1px solid #2e6b44;
  }

  .status-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #10b981;
    box-shadow: 0 0 8px #10b981;
    flex-shrink: 0;
  }

  .admin-content-area {
    display: flex;
    flex-direction: column;
    gap: 40px;
    min-width: 0;
  }

  .admin-hero {
    margin-bottom: 8px;
  }

  .eyebrow {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--color-accent);
    margin: 0 0 8px;
  }

  .admin-hero h1 {
    font-size: clamp(26px, 4vw, 36px);
    font-weight: 700;
    margin: 0 0 10px;
    letter-spacing: -0.03em;
  }

  .hero-desc {
    color: var(--color-muted);
    font-size: 14px;
    line-height: 1.6;
    max-width: 780px;
    margin: 0;
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

  .banner-content {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .banner-content p {
    margin: 0;
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

  .small-btn {
    border: 1px solid currentColor;
    background: transparent;
    color: inherit;
    border-radius: var(--radius-sm);
    padding: 6px 14px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .small-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.1);
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

  @media (max-width: 960px) {
    .admin-dashboard-container {
      grid-template-columns: 1fr;
      gap: 24px;
    }

    .admin-sidebar {
      max-height: none;
      position: static;
      top: 72px;
      z-index: 50;
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 8px;
      padding: 10px 12px;
    }

    .sidebar-header, .group-label {
      display: none;
    }

    .sidebar-group {
      display: contents;
    }

    .sidebar-nav-item {
      border-left: none;
      border-bottom: 2px solid transparent;
      padding: 8px 12px;
      font-size: 12px;
    }

    .sidebar-nav-item.active {
      border-left: none;
      border-bottom-color: var(--color-accent);
    }
  }
</style>

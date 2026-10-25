<script module lang="ts">
  export { scanLabels, nodeStatusLabels } from '../../../lib/admin/types';
</script>

<script lang="ts">
  import { invalidateCatalog } from '../../../lib/catalog/page-cache';
  import { onMount } from 'svelte';
  import { asset, resolve } from '$app/paths';
  import { goto } from '$app/navigation';
  import { session, type AuthUser } from '../../../lib/auth/session';
  import ProfileMenu from '../../../lib/ui/ProfileMenu.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import type { Library, Scan, AdminNode, CoreHealth, EnrollmentToken, RefreshMode } from '../../../lib/admin/types';
  import AdminSettings from '../../../lib/admin/AdminSettings.svelte';
  import { DEFAULT_NAME } from '../../../lib/config/brand';
  import { serverPresentation } from '../../../lib/config/presentation';
  import AdminOverview from '../../../lib/admin/AdminOverview.svelte';
  import { defaultLibraryOptions } from '../../../lib/admin/types';
  import AdminLibraries from '../../../lib/admin/AdminLibraries.svelte';
  import AdminNodes from '../../../lib/admin/AdminNodes.svelte';
  import AdminUsers from '../../../lib/admin/AdminUsers.svelte';
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

  let loading = $state(true);
  let saving = $state(false);
  let pairing = $state(false);
  let nodeActionBusy = $state<string | null>(null);
  let error = $state('');
  let notice = $state('');

  let editing = $state<string | null>(null);
  let name = $state('');
  let kind = $state<'films' | 'series' | 'music'>('films');
  let nodeId = $state('local');
  let root = $state('');
  let folders = $state<{ nodeId: string; root: string }[]>([]);
  let enabled = $state(true);
  let excludedPatterns = $state('');
  let libraryOptions = $state(defaultLibraryOptions());
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
      return [...new Set([...(localNode?.roots ?? []), ...roots])];
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
    if (init.method && init.method !== 'GET') {
      const library = path.match(/\/libraries\/([^/]+)/)?.[1];
      invalidateCatalog(library ? decodeURIComponent(library) : undefined);
    }
    return response.json() as Promise<T>;
  }

  function reset() {
    editing = null;
    name = '';
    kind = 'films';
    nodeId = 'local';
    root = roots[0] ?? '';
    folders = [];
    enabled = true;
    excludedPatterns = '';
    libraryOptions = defaultLibraryOptions();
    userIds = [];
  }

  function handleNodeChange() {
    const candidateRoots = availableRootsForNode;
    if (!root && candidateRoots.length > 0) {
      root = candidateRoots[0] ?? '';
    }
  }

  function edit(library: Library) {
    editing = library.id;
    name = library.name;
    kind = library.kind;
    nodeId = library.nodeId;
    root = library.root;
    folders = (library.folders ?? [{ nodeId: library.nodeId, root: library.root }]).map(folder => ({ ...folder }));
    enabled = library.enabled;
    excludedPatterns = library.excludedPatterns ?? '';
    libraryOptions = { ...defaultLibraryOptions(), ...library.options };
    userIds = [...library.userIds];
    error = '';
    notice = '';
  }

  async function refreshAccess() {
    const [accounts, memberships, libList] = await Promise.all([
      request<{ users: AuthUser[] }>('/api/v1/admin/users'),
      request<{ groups: UserGroup[] }>('/api/v1/admin/groups'),
      request<{ libraries: Library[] }>('/api/v1/admin/libraries')
    ]);
    users = accounts.users;
    groups = memberships.groups;
    libraries = libList.libraries;
  }

  async function load() {
    if (!administrator) {
      loading = false;
      return;
    }
    loading = true;
    error = '';
    try {
      // Independent diagnostics must not delay the library administration screen.
      void request<CoreHealth>('/api/v1/health').then(value => coreHealth = value).catch(() => {});
      const [data, accounts, configured, nodeList, memberships] = await Promise.all([
        request<{ libraries: Library[] }>('/api/v1/admin/libraries'),
        request<{ users: AuthUser[] }>('/api/v1/admin/users'),
        request<{ roots: string[]; localRoots?: string[] }>('/api/v1/admin/roots'),
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes').catch(() => ({ nodes: [] as AdminNode[] })),
        request<{ groups: UserGroup[] }>('/api/v1/admin/groups')
      ]);
      libraries = data.libraries;
      users = accounts.users;
      groups = memberships.groups;
      roots = configured.localRoots ?? configured.roots;
      nodes = nodeList.nodes;
      reset();
      // Scan status fills in after the forms are usable, without repeating access queries.
      void refreshScans();
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Chargement impossible.';
    } finally {
      loading = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!$session || !administrator || saving) return false;
    saving = true;
    error = '';
    notice = '';
    try {
      await request(editing ? `/api/v1/admin/libraries/${encodeURIComponent(editing)}` : '/api/v1/admin/libraries', {
        method: editing ? 'PUT' : 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
        body: JSON.stringify({ name: name.trim(), kind, nodeId: folders[0]?.nodeId ?? nodeId, root: folders[0]?.root ?? root, folders, enabled, excludedPatterns: excludedPatterns.trim(), userIds, options: libraryOptions })
      });
      const [data, nodeList] = await Promise.all([
        request<{ libraries: Library[] }>('/api/v1/admin/libraries'),
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes').catch(() => ({ nodes }))
      ]);
      libraries = data.libraries;
      nodes = nodeList.nodes;
      reset();
      notice = 'Bibliothèque enregistrée. Les autorisations d’accès se gèrent dans Utilisateurs ou Groupes.';
      return true;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Enregistrement impossible.';
      return false;
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
        request<{ roots: string[]; localRoots?: string[] }>('/api/v1/admin/roots')
      ]);
      nodes = nodeList.nodes;
      roots = configured.localRoots ?? configured.roots;
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
        library => (library.kind === 'films' || library.kind === 'series' || library.kind === 'music') && library.enabled && !library.id.startsWith('fixture-')
      );
      const results = await Promise.all(
        scannable.map(async library => [
          library.id,
          await request<Scan>(`/api/v1/admin/libraries/${encodeURIComponent(library.id)}/scan`)
        ] as const)
      );
      for (const [id, scan] of results) {
        if (scans[id]?.status !== scan.status && !['queued', 'running'].includes(scan.status)) invalidateCatalog(id);
        scans[id] = scan;
      }
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'État du scan indisponible.';
    } finally {
      polling = false;
    }
  }

  async function scan(library: Library, mode: RefreshMode = 'incremental') {
    if (!$session || !administrator || pending.includes(library.id)) return;
    pending = [...pending, library.id];
    error = '';
    notice = '';
    try {
      const started = await request<Pick<Scan, 'status'>>(
        `/api/v1/admin/libraries/${encodeURIComponent(library.id)}/scan`,
        { method: 'POST', headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken }, body: JSON.stringify({ mode }) }
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

  let menuOpen = $state(false);
  let navigationDialog = $state<HTMLDialogElement>();
  let menuButton = $state<HTMLButtonElement>();
  function openMenu() {
    if (!navigationDialog || navigationDialog.open) return;
    menuOpen = true;
    navigationDialog.showModal();
    navigationDialog.querySelector<HTMLAnchorElement>('[aria-current="page"]')?.focus();
  }
  function closeMenu() { navigationDialog?.close(); menuOpen = false; menuButton?.focus(); }
  $effect(() => {
    if (!menuOpen) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => { document.body.style.overflow = previous; };
  });
  let selectedTaskId = $state<string | null>(null);
  let currentHash = $state('#libraries-section');
  const sectionTitle = $derived(({
    '#overview': $t('admin_nav_overview'), '#general-section': 'Général', '#network-section': 'Réseau', '#libraries-section': $t('admin_nav_libraries'),
    '#nodes-section': $t('admin_nav_nodes'), '#users-section': $t('admin_nav_users'),
    '#groups-section': 'Groupes',
    '#tasks-section': $t('admin_nav_tasks'), '#backups-section': 'Sauvegardes',
    '#api-keys-section': 'Clés API', '#extensions-section': 'Extensions'
  } as Record<string, string>)[currentHash]);
  function chooseSection(hash: string) { currentHash = hash; closeMenu(); }
  function navigateHome(event: MouseEvent) {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    void goto(resolve('/'));
  }

  const sections = new Set(['#overview', '#general-section', '#network-section', '#libraries-section', '#nodes-section', '#users-section', '#tasks-section', '#backups-section', '#groups-section', '#api-keys-section', '#extensions-section']);
  function selectedSection() {
    const section = window.location.hash.split('?')[0];
    return sections.has(section) ? section : '#libraries-section';
  }

  onMount(() => {
    void load();
    if (typeof window !== 'undefined' && window.location.hash) {
      currentHash = selectedSection();
      selectedTaskId = new URLSearchParams(window.location.hash.split('?')[1] ?? '').get('task');
    }
    const hashHandler = () => {
      currentHash = selectedSection();
      selectedTaskId = new URLSearchParams(window.location.hash.split('?')[1] ?? '').get('task');
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
  <title>{sectionTitle} — Administration — {DEFAULT_NAME}</title>
</svelte:head>

<header class="admin-header">
  <div class="header-left">
    <a class="brand admin-brand" href={resolve('/')} onclick={navigateHome} title="Retour à l’accueil" aria-label={`${$serverPresentation.serverName || DEFAULT_NAME}, accueil`}><img src={asset('images/brand-mark.svg')} width="28" height="28" alt="" /><span class="brand-text">{$serverPresentation.serverName || DEFAULT_NAME}</span></a>
    {#if administrator}<button class="mobile-menu-toggle" bind:this={menuButton} type="button" aria-expanded={menuOpen} aria-controls="admin-navigation" disabled={loading} onclick={openMenu}><Icon name="menu" size={18} /><span>Menu</span></button>{/if}
    <a class="back-link" href={resolve('/')} onclick={navigateHome}><Icon name="chevron-left" size={14} /><span>Retour</span></a>
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
        </div>
      </div>
    {/if}

    {#if loading}
      <div class="loading-state" role="status">
        <img
          src={asset('images/logo-loading.svg')}
          alt=""
          class="logo-spinner"
          width="48"
          height="48"
        />
        <p>Chargement des données administratives…</p>
      </div>
    {:else}
      <div class="admin-dashboard-container">
        <dialog bind:this={navigationDialog} id="admin-navigation" class="admin-nav-dialog admin-sidebar" aria-label="Menu d’administration" onclose={() => { menuOpen=false;menuButton?.focus(); }} onpointerdown={event => { if(event.target===event.currentTarget) closeMenu(); }}>
          <div class="drawer-heading"><a class="admin-brand" href={resolve('/')} onclick={navigateHome} aria-label="Accueil"><img src={asset('images/brand-mark.svg')} width="28" height="28" alt="" /><span>{$serverPresentation.serverName || DEFAULT_NAME}</span></a><a class="back-link" href={resolve('/')} onclick={navigateHome}>Retour</a><button type="button" aria-label="Fermer le menu" onclick={closeMenu}><Icon name="close" size={20} /></button></div>
          <nav class="sidebar-sections" aria-label="Sections d’administration">
          <div class="sidebar-group">
            <span class="group-label">{$t('admin_category_server')}</span>
            <a
              href="#overview"
              class="sidebar-nav-item"
              class:active={currentHash === '#overview'}
              aria-current={currentHash === '#overview' ? 'page' : undefined}
              onclick={() => chooseSection('#overview')}
            >
              <span class="nav-icon"><Icon name="sparkles" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_overview')}</span>
            </a>
          </div>

          <div class="sidebar-group server-settings-nav">
            {#each [['#general-section', 'Général', 'settings'], ['#network-section', 'Réseau', 'globe']] as [hash, label, icon] (hash)}
              <a href={resolve(`admin/libraries/#${hash.slice(1)}`)} class="sidebar-nav-item" class:active={currentHash === hash} aria-current={currentHash === hash ? 'page' : undefined} onclick={() => chooseSection(hash)}><span class="nav-icon"><Icon name={icon as 'settings' | 'globe'} size={18} /></span><span class="nav-text">{label}</span></a>
            {/each}
          </div>
          <div class="sidebar-group">
            <span class="group-label">{$t('admin_category_media')}</span>
            <a
              href="#libraries-section"
              class="sidebar-nav-item"
              class:active={currentHash === '#libraries-section'}
              aria-current={currentHash === '#libraries-section' ? 'page' : undefined}
              onclick={() => chooseSection('#libraries-section')}
            >
              <span class="nav-icon"><Icon name="film" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_libraries')}</span>
            </a>
            <a
              href="#nodes-section"
              class="sidebar-nav-item"
              class:active={currentHash === '#nodes-section'}
              aria-current={currentHash === '#nodes-section' ? 'page' : undefined}
              onclick={() => chooseSection('#nodes-section')}
            >
              <span class="nav-icon"><Icon name="server" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_nodes')}</span>
            </a>
          </div>

          <div class="sidebar-group">
            <span class="group-label">{$t('admin_category_access')}</span>
            <a
              href="#users-section"
              class="sidebar-nav-item"
              class:active={currentHash === '#users-section'}
              aria-current={currentHash === '#users-section' ? 'page' : undefined}
              onclick={() => chooseSection('#users-section')}
            >
              <span class="nav-icon"><Icon name="users" size={16} /></span>
              <span class="nav-text">{$t('admin_nav_users')}</span>
            </a>
            <a href="#groups-section" class="sidebar-nav-item" class:active={currentHash === '#groups-section'}
              aria-current={currentHash === '#groups-section' ? 'page' : undefined}
              onclick={() => chooseSection('#groups-section')}>
              <span class="nav-icon"><Icon name="users" size={16} /></span>
              <span class="nav-text">Groupes d’utilisateurs</span>
            </a>
          </div>
          <div class="sidebar-group">
            <span class="group-label">Maintenance</span>
            {#each [
              ['#tasks-section', $t('admin_nav_tasks'), 'clock'],
              ['#backups-section', 'Sauvegardes', 'server'],
              ['#api-keys-section', 'Clés API', 'shield'],
              ['#extensions-section', 'Extensions', 'sparkles']
            ] as [hash, label, icon] (hash)}
              <a href={resolve(`admin/libraries/#${hash.slice(1)}`)} class="sidebar-nav-item" class:active={currentHash === hash}
                aria-current={currentHash === hash ? 'page' : undefined}
                onclick={() => chooseSection(hash)}>
                <span class="nav-icon"><Icon name={icon as 'server' | 'users' | 'shield' | 'sparkles' | 'clock'} size={16} /></span>
                <span class="nav-text">{label}</span>
              </a>
            {/each}
          </div>
          </nav>
        </dialog>

        <!-- Main Admin Content Area -->
        <div class="admin-content-area">
          <h1 class="sr-only">Administration</h1>
          <AdminSettings section={currentHash} />

          <!-- Section 1: Dashboard Overview -->
          <div hidden={currentHash !== "#overview"}>
          <AdminOverview active={currentHash === "#overview"}
            {coreHealth}
            {nodes}
            {libraries}
            {scans}
          />
          </div>

          <!-- Section 2: Libraries & Scans -->
          <div hidden={currentHash !== "#libraries-section"}>
          <AdminLibraries
            {libraries}
            {nodes}
            {scans}
            {pending}
            {saving}
            {editing}
            {error}
            bind:name
            bind:kind
            bind:nodeId
            bind:root
            bind:folders
            bind:enabled
            bind:excludedPatterns
            bind:options={libraryOptions}
            onchanged={refreshAccess}
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

          <div hidden={currentHash !== '#tasks-section'}>
            <AdminTasks {libraries} {nodes} {scans} {pending} onscan={scan} selectedId={selectedTaskId} active={currentHash === '#tasks-section'} />
          </div>
          <div hidden={currentHash !== '#backups-section'}>
            <AdminMaintenance visible={currentHash === '#backups-section'} />
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
  .admin-header{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px var(--page-gutter);border-bottom:1px solid var(--color-line);background:var(--color-surface);position:sticky;top:0;z-index:100;min-height:60px;box-sizing:border-box;}
  .header-left{display:flex;align-items:center;gap:16px;min-width:0;}.admin-brand{display:inline-flex;align-items:center;gap:8px;text-decoration:none;color:var(--color-text);font-weight:700;font-size:16px;margin-right:6px;cursor:pointer;}.admin-brand:hover{opacity:.9;}.back-link{display:inline-flex;align-items:center;gap:6px;font-size:13px;color:var(--color-muted);text-decoration:none;}.back-link:hover{color:var(--color-text);}.mobile-menu-toggle{display:inline-flex;align-items:center;gap:8px;border:1px solid var(--color-line);border-radius:var(--radius-sm);background:var(--color-raised);color:var(--color-text);padding:8px 12px;min-height:40px;font:inherit;font-size:13px;cursor:pointer;}.mobile-menu-toggle:disabled{opacity:.5;cursor:default;}
  main{max-width:1600px;margin:0 auto;padding:24px var(--page-gutter) 64px;}.admin-dashboard-container{display:block;min-width:0;}.admin-content-area{display:flex;flex-direction:column;gap:20px;min-width:0;}
  .admin-nav-dialog{position:fixed;inset:0 auto 0 0;width:min(280px,90vw);height:100dvh;max-height:100dvh;max-width:none;box-sizing:border-box;margin:0;padding:16px;border:0;border-right:1px solid var(--color-line);background:var(--color-surface);color:var(--color-text);overflow-y:auto;overscroll-behavior:contain;}.admin-nav-dialog::backdrop{background:rgb(0 0 0 / .4);}.drawer-heading{display:flex;align-items:center;justify-content:space-between;padding:0 8px 16px;border-bottom:1px solid var(--color-line);}.drawer-heading button{display:grid;place-items:center;width:36px;height:36px;border:0;border-radius:var(--radius-sm);background:transparent;color:var(--color-text);cursor:pointer;}.drawer-heading button:hover{background:var(--color-raised);}.sidebar-sections{display:grid;gap:14px;padding-top:16px;}.sidebar-group{display:grid;gap:3px;}.server-settings-nav{margin-top:-12px;}.group-label{font-size:10px;font-weight:700;letter-spacing:.08em;color:var(--color-muted);padding:0 10px 4px;text-transform:uppercase;}.sidebar-nav-item{display:flex;align-items:center;gap:10px;min-height:40px;padding:8px 10px;border-radius:var(--radius-sm);color:var(--color-muted);text-decoration:none;font-size:13px;box-sizing:border-box;}.sidebar-nav-item:hover{background:var(--color-raised);color:var(--color-text);}.sidebar-nav-item.active{background:var(--color-accent-soft);color:var(--color-accent);font-weight:600;}.nav-icon{display:inline-flex;flex-shrink:0;}.nav-text{min-width:0;overflow-wrap:anywhere;}.sidebar-nav-item:focus-visible,.mobile-menu-toggle:focus-visible,.drawer-heading button:focus-visible{outline:2px solid var(--color-accent);outline-offset:2px;}
  .error-banner,.notice-banner{padding:12px 16px;border-radius:var(--radius-sm);margin-bottom:20px;font-size:13px;display:flex;align-items:center;justify-content:space-between;gap:12px;}.banner-content{display:flex;align-items:center;gap:10px;min-width:0;overflow-wrap:anywhere;}.banner-content p{margin:0;}.error-banner{background:var(--color-danger-soft);border:1px solid var(--color-danger);color:var(--color-danger);}.notice-banner{background:var(--color-success-soft);border:1px solid var(--color-success);color:var(--color-success);}.small-btn{border:1px solid currentColor;background:transparent;color:inherit;border-radius:var(--radius-sm);padding:6px 12px;font:inherit;cursor:pointer;}.loading-state{display:flex;flex-direction:column;align-items:center;gap:16px;padding:60px 0;color:var(--color-muted);}.logo-spinner{width:48px;height:48px;display:block;margin:0 auto;}
  .admin-content-area :global(:where(section,article,form,fieldset,header,label,details,.card)){min-width:0;}.admin-content-area :global(:where(h2,h3,p,dd,code,label,button,summary)){overflow-wrap:anywhere;}.admin-content-area :global(:where(input,select,button)){max-width:100%;}
  @media(max-width:480px){main{padding:20px 16px 48px;}.admin-header{padding:8px 16px;}.header-left{gap:10px;}.admin-brand .brand-text{display:none;}}
</style>

<script module lang="ts">
  export { scanLabels, nodeStatusLabels } from '../../../lib/admin/types';
</script>

<script lang="ts">
  import { invalidateCatalog } from '../../../lib/catalog/page-cache';
  import { onMount } from 'svelte';
  import { asset, resolve } from '$app/paths';
  import { replaceState } from '$app/navigation';
  import { page } from '$app/state';

  import { session, type AuthUser } from '../../../lib/auth/session';
  import AdminShell from '../../../lib/admin/AdminShell.svelte';
  import { adminDestinations, adminDestinationLabel, parseAdminHash } from '../../../lib/admin/navigation';
  import Icon from '../../../lib/ui/Icon.svelte';
  import type { Library, Scan, AdminNode, CoreHealth, EnrollmentToken, RefreshMode } from '../../../lib/admin/types';
  import AdminSettings from '../../../lib/admin/AdminSettings.svelte';
  import AdminPlayback from '../../../lib/admin/AdminPlayback.svelte';
  import { DEFAULT_NAME } from '../../../lib/config/brand';

  import AdminOverview from '../../../lib/admin/AdminOverview.svelte';
  import Statistics from '../../../lib/statistics/Statistics.svelte';
  import { defaultLibraryOptions } from '../../../lib/admin/types';
  import AdminLibraries from '../../../lib/admin/AdminLibraries.svelte';
  import AdminNodes from '../../../lib/admin/AdminNodes.svelte';
  import AdminUsers from '../../../lib/admin/AdminUsers.svelte';
  import AdminTasks from '../../../lib/admin/AdminTasks.svelte';
  import AdminMaintenance from '../../../lib/admin/AdminMaintenance.svelte';
  import AdminServerLogs from '../../../lib/admin/AdminServerLogs.svelte';
  import AdminLogFiles from '../../../lib/admin/AdminLogFiles.svelte';
  import AutomationTasks from '../../../lib/admin/AutomationTasks.svelte';
  import AdminCapabilities from '../../../lib/admin/AdminCapabilities.svelte';
  import Requests from '../../../lib/applications/Requests.svelte';
  import Reports from '../../../lib/applications/Reports.svelte';
  import AcquisitionAdmin from '../../../lib/applications/AcquisitionAdmin.svelte';
  import AdminGroups from '../../../lib/admin/AdminGroups.svelte';
  import AdminApiKeys from '../../../lib/admin/AdminApiKeys.svelte';
  import AvatarCollections from '../../../lib/admin/AvatarCollections.svelte';
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
  let nodesError = $state('');

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

  async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await fetch(path, {
      ...init,
      credentials: 'same-origin',
      cache: 'no-store',
      signal: AbortSignal.timeout(10000)
    });
    if (response.status === 401) throw new Error($t('your_session_has_expired_sign_in'));
    if (response.status === 403) throw new Error($t('access_denied_check_your_session_and'));
    if (response.status === 400) {
      throw new Error($t('configuration_refused_check_the_name_the'));
    }
    if (response.status === 409) {
      throw new Error($t('library_conflict_scan_in_progress'));
    }
    if (response.status === 502) {
      throw new Error($t('the_remote_media_node_is_unreachable'));
    }
    if (!response.ok) throw new Error($t('admin_the_server_is_unavailable_try_again'));
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
    // A new folder is an explicit choice. The Core's empty inventory root is
    // an internal startup detail, not the starting point of the file picker.
    root = '';
    folders = [];
    enabled = true;
    excludedPatterns = '';
    libraryOptions = defaultLibraryOptions();
    userIds = [];
  }

  function handleNodeChange() {
    // A path from another server must not be reused on the newly selected Node.
    root = '';
  }

  function edit(library: Library) {
    editing = library.id;
    name = library.name;
    kind = library.kind;
    nodeId = library.nodeId;
    root = '';
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
    nodesError = '';
    try {
      // Independent diagnostics must not delay the library administration screen.
      void request<CoreHealth>('/api/v1/health').then(value => coreHealth = value).catch(() => {});
      const [data, accounts, configured, nodeList, memberships] = await Promise.all([
        request<{ libraries: Library[] }>('/api/v1/admin/libraries'),
        request<{ users: AuthUser[] }>('/api/v1/admin/users'),
        request<{ roots: string[]; localRoots?: string[] }>('/api/v1/admin/roots'),
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes').catch(() => { nodesError = $t('media_node_status_unavailable_try_again'); return { nodes: [] as AdminNode[] }; }),
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
      error = failure instanceof Error ? failure.message : $t('loading_impossible');
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
      const targetId = editing;
      const saved = await request<Library>(targetId ? `/api/v1/admin/libraries/${encodeURIComponent(targetId)}` : '/api/v1/admin/libraries', {
        method: targetId ? 'PUT' : 'POST',
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
      notice = $t('library_saved_access_permissions_are_managed');
      return targetId ?? saved?.id ?? true;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : $t('saving_failed');
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
      root = '';
      notice = $t('media_node_paired_successfully', { name: created.name, id: created.id });
    } catch (failure) {
      error = failure instanceof Error ? failure.message : $t('unable_to_pair_the_media_node');
    } finally {
      pairing = false;
    }
  }

  async function generateEnrollmentToken(): Promise<EnrollmentToken> {
    if (!$session || !administrator) throw new Error($t('action_not_allowed'));
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
        ? $t('admin_media_node_revoked', { name: targetNode.name })
        : action === 'rotate-secret'
          ? $t('secret_of_media_node_renewed', { name: targetNode.name })
          : $t('media_node_reconnected', { name: targetNode.name });
    } catch (failure) {
      error = failure instanceof Error ? failure.message : $t('media_node_action_failed');
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
      error = failure instanceof Error ? failure.message : $t('scan_status_unavailable');
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
      error = failure instanceof Error ? failure.message : $t('unable_to_start_the_scan');
    } finally {
      pending = pending.filter(id => id !== library.id);
    }
  }

  let selectedTaskId = $state<string | null>(null);
  let currentHash = $state('#overview');
  const sectionTitle = $derived(adminDestinationLabel(adminDestinations.find(link => link.hash === currentHash) ?? adminDestinations[0], $t));
  function chooseSection(hash: string) { currentHash = hash; selectedTaskId = null; }
  function syncSection() {
    const selected = parseAdminHash(window.location.hash);
    if (window.location.hash.split('?', 1)[0] === '#trickplay-section') {
      replaceState(resolve('admin/libraries/#libraries-section'), page.state);
    }
    currentHash = selected.section;
    selectedTaskId = selected.task;
  }

  onMount(() => {
    void load();
    if (typeof window !== 'undefined' && window.location.hash) {
      syncSection();
    }
    const hashHandler = () => {
      syncSection();
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
  <title>{$t('administration_2', { sectionTitle, DEFAULT_NAME })}</title>
</svelte:head>

<AdminShell title={sectionTitle} {currentHash} {administrator} onnavigate={chooseSection}>
  {#if !administrator}
    <div class="error-banner" role="alert">
      <div class="banner-content">
        <Icon name="shield" size={18} />
        <div>
          <strong>{$t('access_denied')}</strong>
          <p>{$t('this_console_is_strictly_reserved_for')} <code>administrator</code>.</p>
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
        <button class="small-btn" disabled={saving} onclick={load}>{$t('request_retry')}</button>
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
        <p>{$t('loading_administrative_data')}</p>
      </div>
    {:else}
          <AdminSettings section={currentHash} />
          {#if currentHash === '#playback-section'}<AdminPlayback />{/if}

          <!-- Section 1: Dashboard Overview -->
          <div hidden={currentHash !== '#statistics-section'}><Statistics administrator embedded active={currentHash === '#statistics-section'} /></div>
          <div hidden={currentHash !== "#overview"}>
          <AdminOverview active={currentHash === "#overview"}
            {coreHealth}
            {nodes}
            {libraries}
            {scans}
            {nodesError}
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
            {groups}
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
          {#if nodesError}<div class="error-banner" role="alert"><p>{nodesError}</p><button class="small-btn" onclick={load}>{$t('request_retry')}</button></div>{/if}
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
          {#if currentHash === '#logs-section'}
            <AdminLogFiles active={true} />
          {/if}
          {#if currentHash === '#logs-live-section'}
            <AdminServerLogs active={true} />
          {/if}
          {#if currentHash === '#automation-section'}
            <AutomationTasks active={true} {libraries} />
          {/if}
          <div hidden={currentHash !== '#backups-section'}>
            <AdminMaintenance visible={currentHash === '#backups-section'} />
          </div>
          <div hidden={currentHash !== '#groups-section'}>
            <AdminGroups {groups} {users} {libraries} onchanged={refreshAccess} />
          </div>
          <div hidden={currentHash !== '#api-keys-section'}>
            <AdminApiKeys {users} {libraries} />
          </div>
          {#if currentHash === '#avatars-section'}
            <div>
              <AvatarCollections />
            </div>
          {/if}
          <AdminCapabilities section={currentHash.slice(1)} />
          {#if currentHash === '#requests-section'}<div class="extension-application"><Requests embedded /></div>{/if}
          {#if currentHash === '#reports-section'}<div class="extension-application"><Reports embedded /></div>{/if}
          {#if currentHash === '#acquisition-section'}<div class="extension-application"><AcquisitionAdmin embedded /></div>{/if}
    {/if}
  {/if}
</AdminShell>

<style>
  .extension-application :global(.application-page) { max-width: none; padding: 0; }
.error-banner,
  .notice-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 56px;
    margin-bottom: 4px;
    padding: 12px 16px;
    border-radius: var(--r-md);
    color: var(--tx);
    font-size: 14px;
    line-height: 1.5;
  }
  .error-banner { background: var(--err-soft); }
  .notice-banner { background: var(--ok-soft); }
  .banner-content { display: flex; align-items: flex-start; gap: 12px; min-width: 0; overflow-wrap: anywhere; }
  .banner-content :global(svg) { flex: none; margin-top: 2px; }
  .error-banner .banner-content :global(svg) { color: var(--err); }
  .notice-banner .banner-content :global(svg) { color: var(--ok); }
  .banner-content p { margin: 0; }
  .banner-content strong { display: block; margin-bottom: 2px; font-weight: 700; }
  .banner-content code { font-family: var(--font-mono); font-size: 13px; }
  .error-banner > p { margin: 0; min-width: 0; overflow-wrap: anywhere; }
  .small-btn {
    flex: none;
    height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font: inherit;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }
  .small-btn:hover:not(:disabled) { background: var(--pn-3); }
  .small-btn:disabled { opacity: 0.45; cursor: not-allowed; }
  .small-btn:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 64px 24px;
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    background: var(--bg-2);
    color: var(--mu);
    font-size: 14px;
    text-align: center;
  }
  .loading-state p { margin: 0; }
  .logo-spinner { display: block; width: 48px; height: 48px; margin: 0 auto; }
  @media (max-width: 640px) {
    .error-banner, .notice-banner { flex-wrap: wrap; }
    .loading-state { padding: 48px 16px; }
  }
</style>

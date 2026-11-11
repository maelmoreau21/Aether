<script lang="ts">
  import { displayManifest } from './plugin-text';
  import ExtensionSettings from './ExtensionSettings.svelte';
  import { settingsFields, settingsDefaults } from './extension-settings';
  import AvatarCollections from './AvatarCollections.svelte';
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import { addedAccess, needsApproval, type AccessChange } from './extension-access';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import { extensionPins, hasOwnPage, isPinnableExtension, toggleExtensionPin } from './extension-pins';
  import { loadFeatures } from '../applications/api';
  import { t, fmt, translatedLabels, tr } from '../i18n';
  type ConfigOption = { label: string; value: string };
  type ConfigField = {
    key: string;
    label: string;
    description?: string;
    required?: boolean;
    secret?: boolean;
    fieldType: string; // 'text' | 'password' | 'boolean' | 'select'
    defaultValue?: string;
    options?: ConfigOption[];
  };
  type Manifest = {
    id: string;
    name: string;
    version: string;
    description: string;
    author: string;
    permissions: string[];
    httpHosts?: string[];
    category?: string;
    targetAbi?: string;
    minServerVersion?: string;
    maxServerVersion?: string;
    checksum?: string;
    banner?: string;
    repository?: string;
    changelog?: string;
    releaseDate?: string;
    configSchema?: ConfigField[];
  };
  type SettingValue = string | number | boolean;
  type ExtensionSettings = Record<string, SettingValue>;
  type Installed = { manifest: Manifest; publisherId: string; sha256: string; enabled: boolean; libraryIds: string[]; allLibraries?: boolean; settings?: ExtensionSettings; rollbackVersion?: string | null };
  type Repository = { id: string; name: string; url: string; refreshedAt: number };
  // One entry per published version. Versioned catalogues (apiVersion 2) also give
  // the release date, changelog, owner and whether an image can be fetched.
  import { isSafeHttpUrl } from '../safe-url';
  type CatalogueEntry = { repositoryId: string; repositoryName: string; compatible: boolean; manifest: Manifest; publisherId: string; publicKey: string; version?: string; timestamp?: string | null; changelog?: string | null; owner?: string | null; overview?: string | null; hasImage?: boolean };
  let tab = $state<'installed' | 'catalogue' | 'repositories'>('installed');
  let category = $state('all');
  let catalogueId = $state<string | null>(null);
  let addingRepository = $state(false);
  const categoryOf = (manifest: Manifest) => manifest.category || (manifest.permissions.includes('avatars') ? tr('admin_avatars_tab') : manifest.permissions.includes('metadata.read') ? tr('metadata') : tr('admin_general'));
  const hueOf = (id: string) => ([...id].reduce((sum, char) => (sum * 31 + char.charCodeAt(0)) % 360, 0) * 137.508) % 360;
  let managedId = $state<string | null>(null);
  let settingsOpen = $state(false);
  let repositories = $state<Repository[]>([]);
  let catalogue = $state<CatalogueEntry[]>([]);
  let repositoryName = $state('');
  let repositoryUrl = $state('');
  const catalogueGroups = $derived.by(() => {
    const groups = new SvelteMap<string,CatalogueEntry[]>();
    for (const entry of catalogue) {
      const key = `${entry.repositoryId}:${entry.manifest.id}`;
      groups.set(key,[...(groups.get(key) ?? []),entry]);
    }
    return [...groups].map(([key, entries]) => ({key,entries:entries.sort((a,b) => {
      const left=a.manifest.version.split('.').map(Number),right=b.manifest.version.split('.').map(Number);
      return right[0]-left[0] || right[1]-left[1] || right[2]-left[2];
    })}));
  });
  // Catalogue images arrive as data: URLs from the Core (the page only loads local images).
  const images = new SvelteMap<string,string>();
  const requestedImages = new SvelteSet<string>();
  async function loadImages() {
    for (const group of catalogueGroups) {
      const entry = group.entries[0];
      if (!entry.hasImage || requestedImages.has(group.key)) continue;
      requestedImages.add(group.key);
      try {
        const image = await adminRequest<{ dataUrl: string }>(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/image/${encodeURIComponent(entry.manifest.id)}`);
        if (image.dataUrl.startsWith('data:image/')) images.set(group.key, image.dataUrl);
      } catch { /* The generated artwork stays in place. */ }
    }
  }
  const imageFor = (id: string) => {
    const group = catalogueGroups.find(item => item.entries[0].manifest.id === id && images.has(item.key));
    return group ? images.get(group.key) : undefined;
  };
  const formatDate = (value?: string | null) => {
    if (!value) return null;
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : $fmt.date(date);
  };
  const changelogLines = (text?: string | null) => (text ?? '').split('\n').map(line => line.trim()).filter(Boolean);
  let installed = $state<Installed[]>([]);
  const categories = $derived([...new Set([...catalogue.map(entry => categoryOf(entry.manifest)), ...installed.map(entry => categoryOf(entry.manifest))])].sort());
  let editingSettings = $state<Record<string, ExtensionSettings>>({});
  const managed=$derived(installed.find(extension=>extension.manifest.id===managedId));
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let rollbackConfirmedFor = $state('');
  let downgradeConfirmedFor = $state('');
  let filter = $state('all');
  let search = $state('');
  let catalogueFilter = $state<'all' | 'installed' | 'available'>('all');
  const matchesSearch = (manifest: Manifest) => `${manifest.name} ${manifest.description}`.toLocaleLowerCase().includes(search.toLocaleLowerCase());
  const visibleExtensions = $derived(installed.filter(extension =>
    (category === 'all' || categoryOf(extension.manifest) === category) &&
    (filter === 'all' || extension.enabled === (filter === 'enabled')) &&
    matchesSearch(extension.manifest)
  ));
  const visibleCatalogue = $derived(catalogueGroups.filter(group => {
    const manifest = group.entries[0].manifest;
    const isInstalled = installed.some(item => item.manifest.id === manifest.id);
    return (category === 'all' || categoryOf(manifest) === category) &&
      (catalogueFilter === 'all' || isInstalled === (catalogueFilter === 'installed')) &&
      matchesSearch(manifest);
  }));
  // One detail page for an extension, whether it is installed or only in the catalogue.
  const detail = $derived.by(() => {
    if (tab === 'installed' && managed) {
      const id = managed.manifest.id;
      return { installed: managed, group: catalogueGroups.find(group => group.entries[0].manifest.id === id) ?? null };
    }
    if (tab === 'catalogue' && catalogueId) {
      const group = catalogueGroups.find(item => item.key === catalogueId);
      if (!group) return null;
      return { installed: installed.find(item => item.manifest.id === group.entries[0].manifest.id) ?? null, group };
    }
    return null;
  });
  function openCatalogue(key: string) { catalogueId = key; openRevisions = {}; }
  function closeDetail() { if (tab === 'catalogue') catalogueId = null; else managedId = null; settingsOpen = false; openRevisions = {}; }
  function openSettings(id: string) { tab = 'installed'; catalogueId = null; managedId = id; settingsOpen = true; }
  function uninstall(extension: Installed) {
    return perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}`, { method: 'DELETE', body: JSON.stringify({ deleteData: false }) });
      managedId = null;
    }, $t('extension_uninstalled'));
  }

  const permissionNames: Readonly<Record<string, string>> = translatedLabels({ 'metadata.read': 'view_metadata', 'metadata.write': 'edit_metadata', 'images': 'import_images', 'avatars': 'offer_avatars', 'providers': 'external_provider' });
  const displayName = (name: string) => name.replace(/\s+\([^)]*\)$/, '');
  const shortDescription = (description: string) => description.split(' (')[0];
  const accessLabels = (permissions: string[]) => permissions.map(permission => permissionNames[permission] ?? permission).join(' · ');
  function toBannerSrc(banner?: string): string | null {
    if (!banner) return null;
    const trimmed = banner.trim();
    if (trimmed.startsWith('<svg') || trimmed.includes('<svg')) {
      return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(trimmed)}`;
    }
    if (trimmed.startsWith('data:image/') || trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
      return trimmed;
    }
    return null;
  }
  async function reload(updateRepositories = false) {
    busy = true; error = '';
    try {
      await refresh(!updateRepositories);
      if (updateRepositories) await updateRepositoryCatalogues();
    }
    catch (e) { error = e instanceof Error ? e.message : $t('extensions_load_failed'); }
    finally { busy = false; }
  }
  async function refreshRepositories() {
    busy = true; error = ''; notice = '';
    try {
      await updateRepositoryCatalogues();
      if (!error) notice = $t('catalogue_updated_hint');
    } catch (e) { error = e instanceof Error ? e.message : $t('refresh_failed'); }
    finally { busy = false; }
  }
  async function updateRepositoryCatalogues() {
    // A failed source must not hide the cached catalogue or block other sources.
    const failures: string[] = [];
    // The Core allows two concurrent catalogue downloads. Leave image loading
    // until these complete, and keep one unavailable source independent of others.
    for (let offset = 0; offset < repositories.length; offset += 2) {
      const batch = repositories.slice(offset, offset + 2);
      const results = await Promise.allSettled(batch.map(repository =>
        adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}/refresh`, {method:'POST'})
      ));
      failures.push(...batch.filter((_, index) => results[index].status === 'rejected').map(repository => repository.name));
    }
    await refresh();
    if (failures.length) error = $t('unreachable_repositories', { repositories: failures.join(', ') });
  }
  async function refresh(loadArtwork = true) {
    const [extensions, sources, available] = await Promise.all([
      adminRequest<{ extensions: Installed[] }>('/api/v1/admin/extensions'),
      adminRequest<{ repositories: Repository[] }>('/api/v1/admin/extensions/repositories'),
      adminRequest<{ entries: CatalogueEntry[] }>('/api/v1/admin/extensions/catalogue')
    ]);
    installed = extensions.extensions.map(extension=>({...extension,manifest:displayManifest(extension.manifest)}));
    await loadFeatures(true).catch(() => {});
    repositories = sources.repositories; catalogue = available.entries.map(entry=>({...entry,manifest:displayManifest(entry.manifest)}));
    if (loadArtwork) void loadImages();

    for (const ext of installed) {
      if (!editingSettings[ext.manifest.id]) {
        const defaults = settingsDefaults(ext.manifest);
        editingSettings[ext.manifest.id] = { ...defaults, ...(ext.settings ?? {}) };
        if(defaults.base_url&&!editingSettings[ext.manifest.id].base_url)editingSettings[ext.manifest.id].base_url=defaults.base_url;
      }
    }
  }
  async function perform(action: () => Promise<unknown>, message: string) {
    busy = true; error = ''; notice = '';
    try { await action(); await refresh(); notice = message; }
    catch (e) { error = e instanceof Error ? e.message : $t('operation_failed'); }
    finally { busy = false; }
  }
  // A pinned extension in the menu opens its page: #extensions-section?extension=<id>.
  function openRequested() {
    const id = new URLSearchParams(window.location.hash.split('?', 2)[1] ?? '').get('extension');
    if (id && isPinnableExtension(id)) { tab = 'installed'; catalogueId = null; managedId = id; settingsOpen = false; }
  }
  onMount(() => {
    void reload(true); openRequested();
    window.addEventListener('hashchange', openRequested);
    return () => window.removeEventListener('hashchange', openRequested);
  });
  function chooseOfficialRepository(branch: 'main' | 'develop') {
    repositoryName = branch === 'develop' ? $t('admin_aether_plugins_develop') : $t('aether_plugins');
    repositoryUrl = `https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/${branch}/repository/manifest.json`;
  }
  function configure(extension: Installed, enabled: boolean) {
    return perform(() => adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}`, {
      method: 'PUT', body: JSON.stringify({ enabled, libraryIds: extension.libraryIds, allLibraries: extension.allLibraries === true || extension.libraryIds.length === 0 })
    }), enabled ? $t('extension_enabled_notice') : $t('extension_disabled_notice'));
  }
  async function saveSettings(id: string) {
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(id)}/settings`, {
        method: 'PUT',
        body: JSON.stringify(editingSettings[id] ?? {})
      });

    }, $t('settings_saved'));
  }
  async function rollback(extension: Installed) {
    if (rollbackConfirmedFor !== extension.manifest.id) return;
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}/rollback`, {
        method: 'POST', body: JSON.stringify({ expectedSha256: extension.sha256, expectedVersion: extension.manifest.version, confirmDataRestore: true })
      });
      delete editingSettings[extension.manifest.id];
      rollbackConfirmedFor = '';
    }, $t('rollback_done'));
  }
  async function addRepository(event: SubmitEvent) {
    event.preventDefault();
    await perform(async () => {
      await adminRequest('/api/v1/admin/extensions/repositories',{method:'POST',body:JSON.stringify({name:repositoryName.trim() || new URL(repositoryUrl).hostname.slice(0,80),url:repositoryUrl})});
      repositoryName=''; repositoryUrl=''; addingRepository=false; tab='catalogue';
    },$t('repository_saved'));
  }
  async function installFromCatalogue(entry: CatalogueEntry) {
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/install`,{method:'POST',body:JSON.stringify({extensionId:entry.manifest.id,version:entry.manifest.version})});
      tab='installed'; catalogueId=null; managedId=entry.manifest.id;
    },$t('extension_installed_notice'));
  }

  function compareVersions(v1: string, v2: string): number {
    const parse = (v: string) => {
      const [core, ...pre] = (v || '0').split('-');
      const nums = core.split('.').map(n => parseInt(n, 10) || 0);
      return { nums, pre: pre.join('-') };
    };
    const p1 = parse(v1);
    const p2 = parse(v2);
    const len = Math.max(p1.nums.length, p2.nums.length);
    for (let i = 0; i < len; i++) {
      const a = p1.nums[i] ?? 0;
      const b = p2.nums[i] ?? 0;
      if (a > b) return 1;
      if (a < b) return -1;
    }
    if (!p1.pre && p2.pre) return 1;
    if (p1.pre && !p2.pre) return -1;
    if (p1.pre && p2.pre) return p1.pre.localeCompare(p2.pre, undefined, { numeric: true });
    return 0;
  }

  function getUpdateEntry(ext: Installed): CatalogueEntry | null {
    const entries = catalogue.filter(e => e.manifest.id === ext.manifest.id && e.compatible);
    if (!entries.length) return null;
    entries.sort((a, b) => compareVersions(b.manifest.version, a.manifest.version));
    const latest = entries[0];
    return compareVersions(latest.manifest.version, ext.manifest.version) > 0 ? latest : null;
  }

  let openRevisions = $state<Record<string, boolean>>({});

  function getRevisions(extensionId: string, currentManifest?: Manifest, rollbackVersion?: string | null) {
    const matchingEntries = catalogue.filter(e => e.manifest.id === extensionId);
    const versionMap = new SvelteMap<string, CatalogueEntry>();
    for (const entry of matchingEntries) {
      if (!versionMap.has(entry.manifest.version)) {
        versionMap.set(entry.manifest.version, entry);
      }
    }

    const currentVersion = currentManifest?.version;
    const versions = [...versionMap.keys()];
    if (currentVersion && !versions.includes(currentVersion)) {
      versions.push(currentVersion);
    }

    if (rollbackVersion && !versions.includes(rollbackVersion)) versions.push(rollbackVersion);
    versions.sort((a, b) => compareVersions(b, a));

    return versions.map(v => {
      const entry = versionMap.get(v) ?? null;
      const manifest = entry ? entry.manifest : v === currentVersion ? currentManifest! : { ...currentManifest!, version: v, description: '', changelog: undefined, releaseDate: undefined };
      // Not installed: no version is current, newer or older.
      const cmp = currentVersion ? compareVersions(v, currentVersion) : NaN;

      const dateStr = formatDate(entry?.timestamp ?? manifest.releaseDate);

      return {
        version: v,
        entry,
        manifest,
        releaseDate: dateStr,
        changelog: entry?.changelog || manifest.changelog || null,
        isCurrent: cmp === 0,
        isNewer: cmp > 0,
        isOlder: cmp < 0,
        isRollback: v === rollbackVersion,
        compatible: entry ? entry.compatible : true
      };
    });
  }

  const updatableExtensions = $derived(
    installed
      .map(ext => ({ ext, upd: getUpdateEntry(ext) }))
      .filter((item): item is { ext: Installed; upd: CatalogueEntry } => item.upd !== null)
  );

  /** A version that adds access waits here for the administrator's explicit approval. */
  let pendingAccess = $state<{ entry: CatalogueEntry; change: AccessChange } | null>(null);
  const accessChangeOf = (entry: CatalogueEntry): AccessChange | null => {
    const current = installed.find(e => e.manifest.id === entry.manifest.id);
    if (!current) return null;
    const change = addedAccess(current.manifest, entry.manifest);
    return needsApproval(change) ? change : null;
  };

  async function switchVersion(entry: CatalogueEntry, showNotice = true, approved: AccessChange | null = null) {
    // The catalogue entry names the extension; the open detail page may be another one.
    const extId = entry.manifest.id;
    const currentInstalled = installed.find(e => e.manifest.id === extId);
    const change = accessChangeOf(entry);
    if (change && !approved && showNotice) {
      error = ''; notice = '';
      pendingAccess = { entry, change };
      return;
    }
    const action = async () => {
      await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/install`, {
        method: 'POST',
        body: JSON.stringify({
          extensionId: entry.manifest.id, version: entry.manifest.version, upgrade: !!currentInstalled,
          ...(approved ? { approvedAccess: approved } : {})
        })
      });
      if (showNotice) managedId = extId;
    };

    if (showNotice) {
      await perform(action, $t('version_installed_successfully', { version: entry.manifest.version }));
    } else {
      await action();
    }
  }

  async function downgrade(entry: CatalogueEntry, extension: Installed) {
    if (downgradeConfirmedFor !== entry.manifest.version) return;
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/install`, {
        method: 'POST', body: JSON.stringify({ extensionId: extension.manifest.id, version: entry.manifest.version, upgrade: true, confirmDowngrade: true, expectedVersion: extension.manifest.version, expectedSha256: extension.sha256 })
      });
      delete editingSettings[extension.manifest.id];
      downgradeConfirmedFor = '';
    }, $t('version_installed_and_disabled_check_its', { version: entry.manifest.version }));
  }

  async function updateAllExtensions() {
    if (updatableExtensions.length === 0) return;
    const pending = [...updatableExtensions];
    const failures: string[] = [];
    await perform(async () => {
      // One incompatible package must not block the remaining updates.
      for (const { upd } of pending) {
        // New access is never granted in bulk: it is approved one extension at a time.
        if (accessChangeOf(upd)) { failures.push(`${displayName(upd.manifest.name)} : ${$t('extension_new_access_update_alone')}`); continue; }
        try { await switchVersion(upd, false); }
        catch (cause) { failures.push(`${displayName(upd.manifest.name)} : ${cause instanceof Error ? cause.message : $t('operation_failed')}`); }
      }
      if (failures.length === pending.length) throw new Error(failures.join(' · '));
    }, $t('updated_successfully', { value: pending.length - failures.length }));
    if (failures.length && failures.length < pending.length) error = failures.join(' · ');
  }

</script>

{#snippet artwork(manifest: Manifest)}
  {@const bannerSrc = toBannerSrc(manifest.banner) ?? imageFor(manifest.id) ?? null}
  {#if bannerSrc}
    <!-- Any image ratio fills the frame: a blurred copy covers it, the image itself is shown whole. -->
    <span class="extension-artwork has-banner">
      <img class="banner-fill" src={bannerSrc} alt="" aria-hidden="true" />
      <img class="banner-img" src={bannerSrc} alt="" />
    </span>
  {:else}
    <span class="extension-artwork" style={`--art-hue:${hueOf(manifest.id)}`}>
      <span class="art-symbol"><Icon name={manifest.permissions.includes('avatars') ? 'profile' : manifest.permissions.includes('metadata.read') ? 'film' : 'globe'} size={36} /></span>
      <span class="art-text"><strong>{displayName(manifest.name)}</strong><span class="art-category">{categoryOf(manifest)}</span></span>
    </span>
  {/if}
{/snippet}

{#snippet extensionCard(manifest: Manifest, version: string, badges: { label: string; tone?: 'success' | 'update' }[], label: string, kind: 'installed-card' | 'catalogue-card', open: () => void)}
  <button type="button" class={`card plugin-card ${kind}`} aria-label={label} onclick={open}>
    {@render artwork(manifest)}
    <span class="plugin-body">
      <span class="plugin-title-row"><span class="plugin-name">{displayName(manifest.name)}</span><span class="plugin-version">v{version}</span></span>
      <span class="plugin-desc">{shortDescription(manifest.description)}</span>
      {#if badges.length}<span class="plugin-badges">{#each badges as badge (badge.label)}<span class="badge" class:success={badge.tone === 'success'} class:update={badge.tone === 'update'}>{badge.label}</span>{/each}</span>{/if}
    </span>
  </button>
{/snippet}

<section id="extensions-section" aria-labelledby="extensions-heading">
  {#if !detail}
  <div class="heading page-heading"><h2 id="extensions-heading">{$t('extensions_title')}</h2>{#if tab==='catalogue'}<button aria-label={$t('refresh_repository')} disabled={busy || repositories.length===0} onclick={refreshRepositories}><Icon name="refresh" size={16} /> {$t('refresh')}</button>{:else}<button disabled={busy} onclick={() => void reload(true)}><Icon name="refresh" size={16} /> {$t('refresh')}</button>{/if}</div>
  <p class="page-intro">{$t('extensions_intro')}</p>
  <nav class="extension-tabs" aria-label={$t('extensions_management')}>
    <button aria-label={$t('installed_extensions')} class:active={tab==='installed'} aria-current={tab==='installed' ? 'page' : undefined} onclick={() => tab='installed'}>{$t('installed_extensions')} <span>{installed.length}</span></button>
    <button aria-label={$t('extension_catalogue')} class:active={tab==='catalogue'} aria-current={tab==='catalogue' ? 'page' : undefined} onclick={() => tab='catalogue'}>{$t('extension_catalogue')} <span>{catalogueGroups.length}</span></button>
    <button aria-label={$t('extension_repositories')} class:active={tab==='repositories'} aria-current={tab==='repositories' ? 'page' : undefined} onclick={() => tab='repositories'}>{$t('extension_repositories')} <span>{repositories.length}</span></button>
  </nav>
  {#if tab !== 'repositories'}
    <!-- The same filters on both lists: categories, search, state. -->
    <div class="category-filters" aria-label={$t('categories')}>
      <button class:chosen={category === 'all'} aria-pressed={category === 'all'} onclick={() => category='all'}>{$t('all')}</button>
      {#each categories as item (item)}<button class:chosen={category === item} aria-pressed={category === item} onclick={() => category=item}>{item}</button>{/each}
    </div>
    <div class="list-filters">
      <label>{tab === 'catalogue' ? $t('catalogue_search') : $t('filter_extensions')}<input type="search" bind:value={search} placeholder={$t('name_or_description')} /></label>
      {#if tab === 'installed'}
        <label>{$t('state')}<select bind:value={filter}><option value="all">{$t('all_extensions_filter')}</option><option value="enabled">{$t('enabled_extensions')}</option><option value="disabled">{$t('disabled_extensions')}</option></select></label>
      {:else}
        <label>{$t('state')}<select bind:value={catalogueFilter}><option value="all">{$t('all_extensions_filter')}</option><option value="installed">{$t('catalogue_installed_filter')}</option><option value="available">{$t('catalogue_available_filter')}</option></select></label>
      {/if}
    </div>
  {/if}
  {/if}
  {#if busy}<p class="loading-notice" role="status">{$t('loading')}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if pendingAccess}
    {@const pending = pendingAccess}
    <section class="access-approval" role="alert" aria-labelledby="access-approval-title">
      <h4 id="access-approval-title">{$t('extension_new_access_title', { name: displayName(pending.entry.manifest.name), version: pending.entry.manifest.version })}</h4>
      <p>{$t('extension_new_access_intro')}</p>
      <ul>
        {#each pending.change.httpHosts as host (host)}<li>{$t('extension_new_access_host', { host })}</li>{/each}
        {#each pending.change.permissions as permission (permission)}<li>{$t('extension_new_access_permission', { permission: permissionNames[permission] ?? permission })}</li>{/each}
      </ul>
      <div class="actions">
        <button class="primary" disabled={busy} onclick={() => { const approved = pending.change; pendingAccess = null; void switchVersion(pending.entry, true, approved); }}>{$t('extension_new_access_approve')}</button>
        <button disabled={busy} onclick={() => pendingAccess = null}>{$t('cancel')}</button>
      </div>
    </section>
  {/if}
  {#if detail}
    {@const extension = detail.installed}
    {@const manifest = extension?.manifest ?? detail.group!.entries[0].manifest}
    {@const latest = detail.group?.entries.find(entry => entry.compatible) ?? detail.group?.entries[0] ?? null}
    {@const update = extension ? getUpdateEntry(extension) : null}
    {@const revisions = getRevisions(manifest.id, extension?.manifest, extension?.rollbackVersion)}
    {@const repositoryName = detail.group?.entries[0].repositoryName ?? catalogue.find(entry => entry.manifest.id === manifest.id)?.repositoryName}
    <div class="extension-detail">
      <div class="detail-top-nav">
        {#if settingsOpen}<button class="back-installed" onclick={()=>settingsOpen=false}><Icon name="chevron-left" size={16} /> {$t('back_to_extension')}</button>
        {:else}<button class="back-installed" onclick={closeDetail}><Icon name="chevron-left" size={16} /> {tab === 'catalogue' ? $t('extension_catalogue') : $t('all_extensions')}</button>{/if}
      </div>
      {#if settingsOpen && extension}
        <div class="settings-screen">
          <h2 id="extensions-heading">{$t('extension_settings_for', { name: displayName(extension.manifest.name) })}</h2>
          {#if extension.manifest.permissions.includes('metadata.read')}<p>{$t('metadata_language_note')}</p>{/if}
          {#if settingsFields(extension.manifest).length}
            <ExtensionSettings fields={settingsFields(extension.manifest)} bind:values={editingSettings[extension.manifest.id]} {busy} onsave={()=>void saveSettings(extension.manifest.id)} />
          {:else}<p>{$t('no_settings')}</p>{/if}
          {#if extension.manifest.permissions.includes('avatars')}<AvatarCollections extensionId={extension.manifest.id} />{/if}
        </div>
      {:else}
      <div class="detail-layout">
        <!-- Left: every published version, compact; a row opens its notes and its action. -->
        <aside class="versions-panel" aria-labelledby="extension-versions-heading">
          <div class="revisions-heading">
            <h3 id="extension-versions-heading">{$t('versions')}</h3>
            <span class="version-total">{revisions.length}</span>
          </div>
          {#if update}<p class="version-update"><span class="badge update">{$t('update_available', { version: update.manifest.version })}</span></p>{/if}
          <ol class="version-list">
            {#each revisions as rev (rev.version)}
              {@const isOpen = openRevisions[rev.version] ?? false}
              {@const label = `${displayName(manifest.name)} v${rev.version}`}
              <li class="version-row" class:current={rev.isCurrent} class:open={isOpen}>
                <button type="button" class="version-head" aria-expanded={isOpen} onclick={() => openRevisions[rev.version] = !isOpen}>
                  <span class="version-number">v{rev.version}</span>
                  {#if rev.releaseDate}<span class="revision-date">{rev.releaseDate}</span>{/if}
                  <span class="version-badges">
                    {#if rev.isCurrent}<span class="badge success">{$t('extension_installed')}</span>
                    {:else if rev.isRollback}<span class="badge">{$t('rollback_available')}</span>
                    {:else if rev.isNewer}<span class="badge update">{$t('update')}</span>
                    {:else if !extension && rev.entry === latest}<span class="badge update">{$t('latest_version')}</span>{/if}
                    {#if !rev.compatible}<span class="badge">{$t('incompatible')}</span>{/if}
                  </span>
                  <span class="revision-chevron" class:rotated={isOpen}><Icon name="chevron" size={14} /></span>
                </button>
                {#if isOpen}
                  <div class="version-body">
                    {#if rev.changelog}
                      <ul class="version-changelog">{#each changelogLines(rev.changelog) as line, index (index)}<li>{line.replace(/^[•*-]\s*/, '')}</li>{/each}</ul>
                    {:else}<p class="revision-fallback-desc">{$t('published_without_changelog')}</p>{/if}
                    {#if rev.manifest.targetAbi || rev.manifest.minServerVersion}
                      <div class="revision-specs">
                        {#if rev.manifest.targetAbi}<span class="spec-tag">{$t('target_api')} : <code>{rev.manifest.targetAbi}</code></span>{/if}
                        {#if rev.manifest.minServerVersion}<span class="spec-tag">{$t('required_server')} : <code>{rev.manifest.minServerVersion}+</code></span>{/if}
                      </div>
                    {/if}
                    <div class="revision-action-row">
                      {#if rev.isCurrent}
                        <span class="installed-tag"><Icon name="check" size={16} /> {$t('current_version_installed')}</span>
                      {:else if !extension && rev.entry}
                        <button aria-label={$t('labelled_action', { action: $t('install_this_version'), label })} disabled={busy || !rev.compatible} onclick={() => installFromCatalogue(rev.entry!)}><Icon name="download" size={16} /> {$t('install_this_version')}</button>
                      {:else if extension && rev.isRollback}
                        <div class="restore-action"><p>{$t('rollback_warning')}</p>
                          <label><input type="checkbox" checked={rollbackConfirmedFor === extension.manifest.id} onchange={(event) => rollbackConfirmedFor = event.currentTarget.checked ? extension.manifest.id : ''} /> {$t('confirm_restore')}</label>
                          <button type="button" disabled={busy || rollbackConfirmedFor !== extension.manifest.id} onclick={() => void rollback(extension)}>{$t('restore_version', { version: rev.version })}</button>
                        </div>
                      {:else if extension && rev.isOlder && rev.entry}
                        <div class="restore-action"><p>{$t('downgrade_warning')}</p>
                          <label><input type="checkbox" checked={downgradeConfirmedFor === rev.version} onchange={(event) => downgradeConfirmedFor = event.currentTarget.checked ? rev.version : ''} /> {$t('confirm_downgrade')}</label>
                          {#if extension.rollbackVersion}<p>{$t('existing_backup', { version: extension.rollbackVersion })}</p>{/if}
                          <button aria-label={$t('labelled_action', { action: $t('install_this_version'), label })} disabled={busy || !rev.compatible || downgradeConfirmedFor !== rev.version} onclick={() => void downgrade(rev.entry!, extension)}>{$t('install_version', { version: rev.version })}</button>
                        </div>
                      {:else if rev.isOlder}
                        <span class="local-only-text">{$t('no_restorable_backup')}</span>
                      {:else if rev.entry}
                        <button class:primary={rev.isNewer} aria-label={$t('labelled_action', { action: $t('update_to_version', { version: rev.version }), label })} disabled={busy || !rev.compatible} onclick={() => void switchVersion(rev.entry!)}><Icon name="download" size={16} /> {$t('update_to_version', { version: rev.version })}</button>
                      {:else}
                        <span class="local-only-text">{$t('local_version')}</span>
                      {/if}
                    </div>
                  </div>
                {/if}
              </li>
            {:else}
              <li class="empty-revisions">{$t('no_extension_results')}</li>
            {/each}
          </ol>
        </aside>

        <!-- Right: the extension itself, its actions, then its facts. -->
        <div class="detail-main">
          {@render artwork(manifest)}
          <div class="detail-title">
            <h2 id="extensions-heading">{displayName(manifest.name)}</h2>
            <p class="detail-description">{manifest.description}</p>
            {#if detail.group?.entries[0].overview}<p class="catalogue-overview">{detail.group.entries[0].overview}</p>{/if}
          </div>
          <div class="detail-actions">
            {#if extension}
              {#if update}<button class="primary" disabled={busy || !update.compatible} onclick={() => void switchVersion(update)}><Icon name="download" size={18} /> {$t('update_to_version', { version: update.manifest.version })}</button>{/if}
              <button type="button" class="enable-toggle" class:is-enabled={extension.enabled} role="switch" aria-checked={extension.enabled} aria-label={$t('enable', { value: displayName(extension.manifest.name) })} disabled={busy} onclick={() => void configure(extension, !extension.enabled)}>
                <span class="switch-track" aria-hidden="true"></span>{extension.enabled ? $t('disable_plugin') : $t('enable_plugin')}
              </button>
              <button type="button" class:primary={!update} onclick={() => openSettings(extension.manifest.id)}><Icon name="settings" size={18} /> {$t('extension_settings_button')}</button>
              {#if isPinnableExtension(extension.manifest.id)}
                <button type="button" aria-pressed={$extensionPins.includes(extension.manifest.id)} disabled={busy} onclick={() => toggleExtensionPin(extension.manifest.id, displayName(extension.manifest.name))}><Icon name="star" size={18} /> {$extensionPins.includes(extension.manifest.id) ? $t('unpin_menu') : $t('pin_menu')}</button>
              {/if}
              {#if extension.enabled && isPinnableExtension(extension.manifest.id) && hasOwnPage(extension.manifest.id)}<a class="button-link" href={`#${extension.manifest.id === 'media-requests' ? 'requests' : 'reports'}-section`}>{$t('open_extension', { name: displayName(extension.manifest.name) })}</a>{/if}
              <button type="button" class="danger" disabled={busy} onclick={() => void uninstall(extension)}><Icon name="trash" size={18} /> {$t('uninstall')}</button>
            {:else if latest}
              <button class="primary install-button" disabled={busy || !latest.compatible} onclick={() => installFromCatalogue(latest)}><Icon name="download" size={18} /> {$t('install')} {displayName(manifest.name)}</button>
              <p class="section-help">{$t('install_disabled_hint')}</p>
            {/if}
          </div>
          <dl class="detail-meta">
            <div><dt>{$t('state')}</dt><dd>{extension ? (extension.enabled ? $t('active_state') : $t('disabled_state')) : $t('extension_not_installed')}</dd></div>
            <div><dt>{$t('version')}</dt><dd>{manifest.version}</dd></div>
            <div><dt>{$t('developer')}</dt><dd>{detail.group?.entries[0].owner || manifest.author}</dd></div>
            <div><dt>{$t('repository')}</dt><dd>
              <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- External link -->
              {#if manifest.repository && isSafeHttpUrl(manifest.repository)}<a href={manifest.repository} target="_blank" rel="noopener noreferrer">{repositoryName ?? displayName(manifest.name)}</a>{:else}{repositoryName ?? '—'}{/if}
            </dd></div>
            {#if !extension && latest}<div><dt>{$t('compatibility')}</dt><dd>{latest.compatible ? $t('compatible') : $t('incompatible')}</dd></div>{/if}
            <div><dt>{$t('permissions')}</dt><dd>{accessLabels(manifest.permissions) || $t('none')}</dd></div>
            <div><dt>{$t('services_contacted')}</dt><dd>{manifest.httpHosts?.join(', ') || $t('none').toLowerCase()}</dd></div>
          </dl>
        </div>
      </div>
      {/if}
    </div>
  {:else if tab==='repositories'}
    <p class="repository-warning">{$t('third_party_warning')}</p>
    <div class="section-title"><h3>{$t('your_repositories')}</h3><button class="primary" aria-expanded={addingRepository} onclick={() => addingRepository=!addingRepository}>{addingRepository ? $t('close') : `+ ${$t('new_repository')}`}</button></div>
    {#if addingRepository}
    <form class="repository-form" onsubmit={addRepository}>
      <div class="actions"><button type="button" disabled={busy} onclick={() => chooseOfficialRepository('main')}>{$t('aether_plugins_main')}</button><button type="button" disabled={busy} onclick={() => chooseOfficialRepository('develop')}>{$t('aether_plugins_develop')}</button></div>
      <label>{$t('repository_name_optional')}<input maxlength="80" bind:value={repositoryName} disabled={busy} /></label>
      <label>{$t('repository_address')}<input type="url" required maxlength="2048" placeholder="https://example.com/manifest.json" bind:value={repositoryUrl} disabled={busy} /></label>
      <button disabled={busy}>{$t('add_repository')}</button>
    </form>
    {/if}
    {#each repositories as repository (repository.id)}
      <article class="card repository-card"><div class="repository-heading"><span class="extension-icon"><Icon name="server" size={22} /></span><div><h4>{repository.name}</h4><p>{repository.refreshedAt ? $t('refreshed_at', { date: new Date(repository.refreshedAt*1000).toLocaleString() }) : $t('catalogue_to_fetch')}</p></div>{#if repository.id==='community-default'}<span class="badge">{$t('preconfigured')}</span>{/if}</div><code class="repository-url">{repository.url}</code>
        <div class="actions"><button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}/refresh`,{method:'POST'}),$t('refresh_repository'))}>{$t('refresh_repository')}</button>
        <button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}`,{method:'DELETE'}),$t('remove_repository'))}>{$t('remove_repository')}</button></div><small>{$t('installed_extensions_kept')}</small>
      </article>
    {:else}<p>{$t('no_repository')}</p>{/each}
  {:else if tab==='catalogue'}
    {#if repositories.some(repository => !repository.refreshedAt)}<div class="catalogue-pending"><Icon name="info" size={20} /><div><strong>{$t('repository_first_refresh')}</strong><p>{$t('refresh_repositories_hint')}</p></div></div>{/if}
    {#if repositories.some(repository=>repository.id==='community-default')&&!catalogue.some(entry=>/intro.?skip/i.test(entry.manifest.id+' '+entry.manifest.name))}<p class="catalogue-pending">{$t('intro_skipper_is_not_in_the')}</p>{/if}
    <p class="section-help">{$t('enable_after_install')}</p>
    <div class="plugins-grid">
    {#each visibleCatalogue as group (group.key)}
      {@const entry = group.entries[0]}
      {@const existing = installed.find(item => item.manifest.id === entry.manifest.id)}
      {@const badges = [
        ...(existing ? [{ label: $t('installed'), tone: 'success' as const }] : []),
        ...(!entry.compatible ? [{ label: $t('incompatible') }] : []),
        ...(group.entries.length > 1 ? [{ label: $t('version_count', { count: group.entries.length }) }] : [])
      ]}
      {@render extensionCard(entry.manifest, entry.manifest.version, badges, $t('details_of', { value: displayName(entry.manifest.name) }), 'catalogue-card', () => openCatalogue(group.key))}
    {:else}<p>{$t('no_extension_results')}</p>{/each}
    </div>
  {:else}
  {#if updatableExtensions.length > 0}
    <div class="update-all-banner" role="region" aria-label={$t('updates_available')}>
      <div class="update-all-info">
        <span class="badge update">{$t('updates')}</span>
        <strong>{$t('extension_update_count', { count: updatableExtensions.length })}</strong>
      </div>
      <button class="primary btn-update-all" disabled={busy} onclick={updateAllExtensions}>
        <Icon name="download" size={16} /> {$t('update_all')} ({updatableExtensions.length})
      </button>
    </div>
  {/if}
  <div class="plugins-grid">{#each visibleExtensions as extension (extension.manifest.id)}
    {@const badges = [
      { label: extension.enabled ? $t('active_state') : $t('disabled_state'), tone: extension.enabled ? 'success' as const : undefined },
      ...(getUpdateEntry(extension) ? [{ label: $t('update'), tone: 'update' as const }] : [])
    ]}
    {@render extensionCard(extension.manifest, extension.manifest.version, badges, $t('open', { value: displayName(extension.manifest.name) }), 'installed-card', () => { managedId = extension.manifest.id; settingsOpen = false; })}
  {:else}<p>{$t('no_installed_extensions')}</p>{/each}</div>
  {/if}
</section>

<style>
  section { min-width: 0; color: var(--color-text); font-family: var(--font-sans); }

  /* Typographie */
  h2, h3, h4, strong { color: var(--color-text); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; }
  h3 { margin: 0; font-size: 16px; font-weight: 600; }
  h4 { margin: 0; font-size: 15px; font-weight: 600; }
  p { line-height: 1.5; color: var(--color-muted); }
  small { color: var(--color-muted); font-size: 12px; line-height: 1.5; }
  code { font-family: var(--font-mono); overflow-wrap: anywhere; white-space: normal; }

  /* Boutons : ghost par défaut, un seul primaire par zone */
  button, .button-link {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 40px;
    padding: 8px 16px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: 13px;
    text-decoration: none;
    cursor: pointer;
    transition: background-color var(--transition-fast), color var(--transition-fast);
  }
  button:hover:not(:disabled), .button-link:hover { background: var(--color-raised); }
  button:focus-visible, .button-link:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button.primary { border-color: transparent; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  button.primary:hover:not(:disabled) { background: var(--color-accent-hover); opacity: 0.9; }

  /* Champs */
  form { display: grid; gap: 14px; margin: 12px 0; }
  label { display: grid; gap: 6px; color: var(--color-muted); font-size: 13px; }
  input:not([type='checkbox']), select { box-sizing: border-box; width: 100%; min-width: 0; min-height: 40px; padding: 8px 12px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  input[type='checkbox'] { width: 18px; height: 18px; margin: 0; accent-color: var(--color-accent); }

  /* En-tête de page */
  .heading, .actions, .section-title { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .heading { justify-content: space-between; }
  .page-intro { margin: 4px 0 20px; font-size: 14px; }

  /* Onglets de sous-section */
  .extension-tabs { display: flex; gap: 4px; margin: 0 0 20px; border-bottom: 1px solid var(--color-line-subtle); overflow-x: auto; scrollbar-width: none; }
  .extension-tabs::-webkit-scrollbar { display: none; }
  .extension-tabs button { flex: none; min-height: 44px; margin-bottom: -1px; padding: 10px 16px; border: 0; border-bottom: 2px solid transparent; border-radius: 0; background: transparent; color: var(--color-muted); white-space: nowrap; }
  .extension-tabs button:hover:not(:disabled) { background: transparent; color: var(--color-text); }
  .extension-tabs button.active { border-bottom-color: var(--color-accent); color: var(--color-text); font-weight: 600; }
  .extension-tabs button span { color: var(--color-muted); font-variant-numeric: tabular-nums; }

  /* Pastilles de catégorie */
  .category-filters { display: flex; gap: 8px; margin-bottom: 16px; overflow-x: auto; scrollbar-width: none; }
  .category-filters::-webkit-scrollbar { display: none; }
  .category-filters button { flex: none; min-height: 34px; padding: 6px 14px; border-radius: var(--radius-pill); white-space: nowrap; }
  .category-filters .chosen, .category-filters .chosen:hover:not(:disabled) { border-color: transparent; background: var(--color-accent-soft); color: var(--color-accent); }

  /* Retours */
  [role='alert'], [role='status'] { margin: 0 0 16px; padding: 10px 14px; border-radius: var(--radius-sm); background: var(--color-surface); font-size: 13px; line-height: 1.5; }
  [role='alert'] { background: var(--color-danger-soft); color: var(--color-text); }
  [role='status'] { color: var(--color-muted); }
  .access-approval { margin: 0 0 16px; padding: 14px 18px; border-radius: var(--radius-md); border: 1px solid var(--color-warning); background: var(--color-warning-soft); color: var(--color-text); }
  .access-approval h4 { margin: 0 0 6px; font-size: 15px; }
  .access-approval p { margin: 0 0 8px; font-size: 13px; }
  .access-approval ul { margin: 0 0 12px; padding-left: 18px; font-size: 13px; overflow-wrap: anywhere; }
  .repository-warning { margin: 0 0 16px; padding: 10px 14px; border-radius: var(--radius-sm); background: var(--color-warning-soft); color: var(--color-text); font-size: 13px; }
  .catalogue-pending { display: flex; gap: 12px; margin: 0 0 16px; padding: 14px 18px; border-radius: var(--radius-md); background: var(--color-surface); font-size: 14px; }
  .catalogue-pending :global(svg) { flex: none; color: var(--color-info); }
  .catalogue-pending p { margin: 4px 0 0; font-size: 13px; }
  .section-help { margin: 8px 0 16px; font-size: 13px; }
  .update-all-banner { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; margin: 0 0 16px; padding: 12px 18px; border-radius: var(--radius-md); background: var(--color-accent-soft); }
  .update-all-info { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; font-size: 14px; }

  /* Badges */
  .badge { display: inline-flex; align-items: center; gap: 6px; padding: 3px 8px; border-radius: var(--radius-pill); background: var(--color-raised); color: var(--color-muted); font-size: 11px; white-space: nowrap; }
  .badge.success { background: var(--color-success-soft); color: var(--color-success); }
  .badge.update { background: var(--color-accent-soft); color: var(--color-accent); }

  /* Dépôts : liste en lignes */
  .section-title { justify-content: space-between; margin: 20px 0 12px; }
  .repository-form { grid-template-columns: minmax(0, 1fr) minmax(0, 2fr) auto; align-items: end; padding: 16px 20px; background: var(--color-surface); border-radius: var(--radius-md); }
  .card { min-width: 0; background: var(--color-surface); border-radius: var(--radius-md); }
  .repository-card { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px 24px; margin: 0; padding: 16px 20px; border-radius: 0; border-top: 1px solid var(--color-line-subtle); }
  .repository-card:first-of-type { border-top-left-radius: var(--radius-md); border-top-right-radius: var(--radius-md); }
  .repository-heading { grid-column: 1; display: flex; align-items: center; gap: 12px; margin: 0; }
  .repository-heading > div { min-width: 0; }
  .repository-heading p { margin: 2px 0 0; font-size: 12px; }
  .repository-heading .badge { margin-left: auto; }
  .extension-icon { display: inline-grid; place-items: center; flex: none; width: 40px; height: 40px; border-radius: var(--radius-md); background: var(--color-accent-soft); color: var(--color-accent); }
  .repository-url { grid-column: 1; display: block; padding-left: 52px; font-size: 12px; color: var(--color-muted); overflow-wrap: anywhere; }
  .repository-card .actions { grid-column: 2; grid-row: 1 / 3; margin: 0; justify-content: flex-end; }
  .repository-card > small { grid-column: 1 / -1; padding-left: 52px; }

  /* Filtres : identiques pour « Installées » et « Catalogue » */
  .list-filters { display: flex; flex-wrap: wrap; gap: 12px; margin: 0 0 16px; }
  .list-filters label:first-child { flex: 1; min-width: 200px; }
  .list-filters label:last-child { min-width: 180px; }
  .back-installed { min-height: 0; padding: 8px 0; border: 0; background: transparent; color: var(--color-muted); }
  .back-installed:hover:not(:disabled) { background: transparent; color: var(--color-text); }

  /* Grille d'extensions : une carte = un bouton, cliquable partout */
  .plugins-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 280px), 1fr)); gap: 14px; align-items: stretch; }
  .plugin-card { display: flex; flex-direction: column; align-items: stretch; justify-content: flex-start; gap: 0; width: 100%; min-height: 0; padding: 0; overflow: hidden; border: 1px solid transparent; text-align: left; transition: background-color var(--transition-fast), border-color var(--transition-fast); }
  .plugin-card:hover:not(:disabled), .plugin-card:focus-visible { border-color: var(--color-line); background: var(--color-surface-hover); }
  .plugin-card:focus-visible { outline-offset: 2px; }
  .plugin-body { display: flex; flex: 1; flex-direction: column; gap: 6px; padding: 12px 16px 14px; }
  .plugin-title-row { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
  .plugin-name { min-width: 0; color: var(--color-text); font-size: 15px; font-weight: 600; overflow-wrap: anywhere; }
  .plugin-version { flex: none; color: var(--color-muted); font-family: var(--font-mono); font-size: 12px; }
  .plugin-desc { display: -webkit-box; min-height: 2.9em; overflow: hidden; color: var(--color-muted); font-size: 13px; line-height: 1.45; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .plugin-badges { display: flex; flex-wrap: wrap; gap: 6px; margin-top: auto; padding-top: 4px; }

  /* Bannière : même cadre pour toutes les extensions, l'image occupe toute la largeur */
  .extension-artwork { box-sizing: border-box; position: relative; overflow: hidden; display: flex; align-items: center; justify-content: center; gap: 14px; width: 100%; aspect-ratio: 10 / 3; padding: 16px 20px; background: hsl(var(--art-hue) 30% 18%); color: var(--color-text-bright); }
  .extension-artwork.has-banner { padding: 0; background: var(--color-canvas); }
  .extension-artwork .banner-fill { position: absolute; inset: -12px; width: calc(100% + 24px); height: calc(100% + 24px); object-fit: cover; filter: blur(14px) saturate(1.1); opacity: 0.55; }
  .extension-artwork .banner-img { position: relative; display: block; width: 100%; height: 100%; object-fit: contain; }
  .art-symbol { display: grid; flex: none; color: var(--color-text-bright); opacity: 0.75; }
  .art-text { display: grid; gap: 2px; min-width: 0; text-align: left; }
  .art-text strong { color: var(--color-text-bright); font-size: 17px; line-height: 1.2; overflow-wrap: anywhere; }
  .art-category { color: var(--color-text-bright); opacity: 0.75; font-size: 11px; letter-spacing: 0.08em; text-transform: uppercase; }

  /* Fiche d'une extension : versions à gauche, l'extension à droite */
  .extension-detail { margin-top: 4px; }
  .detail-top-nav { margin-bottom: 12px; }
  .settings-screen { max-width: 880px; padding: 20px; background: var(--color-surface); border-radius: var(--radius-md); }
  .settings-screen h2 { margin-bottom: 16px; }
  .detail-layout { display: grid; grid-template-columns: minmax(240px, 300px) minmax(0, 1fr); gap: 32px; align-items: start; }
  .versions-panel { position: sticky; top: 16px; max-height: calc(100dvh - 32px); overflow-y: auto; padding: 12px; background: var(--color-surface); border-radius: var(--radius-md); }
  .revisions-heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 6px 10px; }
  .revisions-heading h3 { font-size: 14px; }
  .version-total { color: var(--color-muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .version-update { margin: 0 6px 10px; }
  .version-list { display: flex; flex-direction: column; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .version-row { border-radius: var(--radius-sm); }
  .version-row.current { background: var(--color-success-soft); }
  .version-row.open:not(.current) { background: var(--color-raised); }
  .version-head { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; grid-template-areas: 'number badges chevron' 'date date chevron'; align-items: center; gap: 2px 8px; width: 100%; min-height: 44px; padding: 8px 10px; border: 0; border-radius: var(--radius-sm); background: transparent; text-align: left; }
  .version-head:hover:not(:disabled) { background: var(--color-surface-hover); }
  .version-head:focus-visible { outline-offset: -2px; }
  .version-number { grid-area: number; font-family: var(--font-mono); font-size: 13px; font-weight: 600; }
  .version-head .revision-date { grid-area: date; }
  .version-badges { grid-area: badges; display: flex; flex-wrap: wrap; gap: 4px; min-width: 0; }
  .version-head .revision-chevron { grid-area: chevron; }
  .revision-date { font-size: 12px; color: var(--color-muted); }
  .revision-chevron { display: inline-flex; align-items: center; color: var(--color-muted); transition: transform var(--transition-normal); }
  .revision-chevron.rotated { transform: rotate(90deg); }
  .version-body { padding: 0 10px 12px; }
  .version-changelog { margin: 4px 0 10px; padding-left: 16px; color: var(--color-muted); font-size: 12px; line-height: 1.5; }
  .revision-fallback-desc { margin: 4px 0 10px; font-size: 12px; line-height: 1.5; color: var(--color-muted); }
  .revision-specs { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 10px; }
  .spec-tag { font-size: 11px; color: var(--color-muted); }
  .spec-tag code { padding: 1px 5px; border-radius: var(--radius-xs); background: var(--color-raised); font-size: 11px; }
  .revision-action-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .revision-action-row button { width: 100%; min-height: 36px; font-size: 12px; }
  .restore-action { display: grid; gap: 8px; font-size: 12px; }
  .restore-action p { margin: 0; font-size: 12px; }
  .restore-action label { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .installed-tag { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 500; color: var(--color-success); }
  .local-only-text, .empty-revisions { padding: 6px 10px; font-size: 12px; color: var(--color-muted); }

  .detail-main { display: grid; gap: 20px; min-width: 0; }
  .detail-main .extension-artwork { border-radius: var(--radius-md); }
  .detail-title h2 { margin: 0 0 8px; font-size: 30px; }
  .detail-description { max-width: 760px; margin: 0; font-size: 15px; line-height: 1.55; color: var(--color-muted); }
  .catalogue-overview { max-width: 760px; margin: 10px 0 0; font-size: 14px; white-space: pre-line; }
  .detail-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; }
  .detail-actions button, .detail-actions .button-link { min-height: 46px; padding: 10px 18px; font-size: 14px; }
  .detail-actions .install-button { min-width: min(100%, 280px); }
  .detail-actions .section-help { flex-basis: 100%; margin: 0; }
  .danger { color: var(--color-danger); }
  .danger:hover:not(:disabled) { background: var(--color-danger-soft); }

  /* Activer / désactiver : un bouton avec son interrupteur */
  .enable-toggle { gap: 10px; }
  .switch-track { position: relative; flex: 0 0 36px; width: 36px; height: 20px; border-radius: var(--radius-pill); background: var(--color-line-strong); transition: background-color var(--transition-fast); }
  .switch-track::after { content: ''; position: absolute; top: 3px; left: 3px; width: 14px; height: 14px; border-radius: 50%; background: var(--color-text-bright); transition: transform var(--transition-fast); }
  .enable-toggle.is-enabled .switch-track { background: var(--color-accent); }
  .enable-toggle.is-enabled .switch-track::after { transform: translateX(16px); }

  .detail-meta { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 220px), 1fr)); gap: 0 24px; margin: 0; padding: 4px 18px; background: var(--color-surface); border-radius: var(--radius-md); }
  .detail-meta > div { display: grid; grid-template-columns: 120px minmax(0, 1fr); gap: 12px; padding: 12px 0; border-top: 1px solid var(--color-line-subtle); font-size: 14px; }
  .detail-meta > div:nth-child(-n+2) { border-top: 0; }
  dt { color: var(--color-muted); }
  dd { margin: 0; color: var(--color-text); font-weight: 500; overflow-wrap: anywhere; }
  dd a { color: var(--color-accent); text-decoration: none; }
  dd a:hover { text-decoration: underline; }

  @media (max-width: 900px) {
    .detail-layout { grid-template-columns: minmax(0, 1fr); gap: 24px; }
    .detail-main { grid-row: 1; }
    .versions-panel { position: static; max-height: none; }
    .detail-meta > div:nth-child(2) { border-top: 1px solid var(--color-line-subtle); }
  }

  @media (max-width: 700px) {
    .repository-form { grid-template-columns: minmax(0, 1fr); }
    .repository-card { grid-template-columns: minmax(0, 1fr); padding: 16px; }
    .repository-card .actions { grid-column: 1; grid-row: auto; justify-content: flex-start; }
    .repository-card .actions button { flex: 1 1 160px; }
    .repository-heading { flex-wrap: wrap; }
    .repository-url, .repository-card > small { padding-left: 0; }
    .extension-tabs button { padding: 10px 12px; }
    .heading { align-items: flex-start; }
    .detail-title h2 { font-size: 24px; }
    .detail-actions > button, .detail-actions > .button-link { flex: 1 1 100%; }
    .detail-meta > div { grid-template-columns: 100px minmax(0, 1fr); }
    input:not([type='checkbox']), select { font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button, .plugin-card, .revision-chevron, .switch-track, .switch-track::after { transition: none; }
  }
</style>

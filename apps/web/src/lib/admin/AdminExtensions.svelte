<script lang="ts">
  import { displayManifest } from './plugin-text';
  import ExtensionSettings from './ExtensionSettings.svelte';
  import { settingsFields, settingsDefaults } from './extension-settings';
  import AvatarCollections from './AvatarCollections.svelte';
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import { extensionPins, isPinnableExtension, toggleExtensionPin } from './extension-pins';
  import { loadFeatures } from '../applications/api';
  import { t } from '../i18n';
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
  type CatalogueEntry = { repositoryId: string; repositoryName: string; compatible: boolean; manifest: Manifest; publisherId: string; publicKey: string; version?: string; timestamp?: string | null; changelog?: string | null; owner?: string | null; overview?: string | null; hasImage?: boolean };
  let tab = $state<'installed' | 'catalogue' | 'repositories'>('installed');
  let category = $state('all');
  let catalogueId = $state<string | null>(null);
  let addingRepository = $state(false);
  const categoryOf = (manifest: Manifest) => manifest.category || (manifest.permissions.includes('avatars') ? 'Avatars' : manifest.permissions.includes('metadata.read') ? 'Métadonnées' : 'Général');
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
    return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString();
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
  const visibleExtensions = $derived(installed.filter(extension =>
    (category === 'all' || categoryOf(extension.manifest) === category) &&
    (filter === 'all' || extension.enabled === (filter === 'enabled')) &&
    `${extension.manifest.name} ${extension.manifest.description}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())
  ));

  const permissionNames: Record<string,string> = {'metadata.read':'Consulter les métadonnées','metadata.write':'Modifier les métadonnées','images':'Importer des images','avatars':'Proposer des avatars','providers':'Fournisseur externe'};
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
  onMount(() => { void reload(true); });
  function chooseOfficialRepository(branch: 'main' | 'develop') {
    repositoryName = branch === 'develop' ? 'Aether Plugins (develop)' : 'Aether Plugins';
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
      const cmp = currentVersion ? compareVersions(v, currentVersion) : 0;

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

  async function switchVersion(entry: CatalogueEntry, showNotice = true) {
    const extId = managed?.manifest.id || entry.manifest.id;
    const currentInstalled = installed.find(e => e.manifest.id === extId);
    const action = async () => {
      await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/install`, {
        method: 'POST',
        body: JSON.stringify({ extensionId: entry.manifest.id, version: entry.manifest.version, upgrade: !!currentInstalled })
      });
      managedId = extId;
    };

    if (showNotice) {
      await perform(action, `Version ${entry.manifest.version} installée avec succès.`);
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
    }, `Version ${entry.manifest.version} installée et désactivée. Vérifiez ses paramètres avant de la réactiver.`);
  }

  async function updateAllExtensions() {
    if (updatableExtensions.length === 0) return;
    await perform(async () => {
      for (const { upd } of updatableExtensions) {
        await switchVersion(upd, false);
      }
    }, `${updatableExtensions.length} extension(s) mise(s) à jour avec succès.`);
  }

</script>

{#snippet artwork(manifest: Manifest)}
  {@const bannerSrc = toBannerSrc(manifest.banner) ?? imageFor(manifest.id) ?? null}
  {#if bannerSrc}
    <div class="extension-artwork has-banner">
      <img class="banner-img" src={bannerSrc} alt="" />
    </div>
  {:else}
    <div class="extension-artwork" style={`--art-hue:${hueOf(manifest.id)}`}>
      <span class="art-symbol"><Icon name={manifest.permissions.includes('avatars') ? 'profile' : manifest.permissions.includes('metadata.read') ? 'film' : 'globe'} size={48} /></span>
      <strong>{displayName(manifest.name)}</strong><span class="art-category">{categoryOf(manifest)}</span>
    </div>
  {/if}
{/snippet}

<section id="extensions-section" aria-labelledby="extensions-heading">
  {#if !managed || !settingsOpen || tab!=='installed'}
  <div class="heading page-heading"><h2 id="extensions-heading">{$t('extensions_title')}</h2>{#if tab==='catalogue'}<button aria-label={$t('refresh_repository')} disabled={busy || repositories.length===0} onclick={refreshRepositories}><Icon name="refresh" size={16} /> {$t('refresh')}</button>{:else}<button disabled={busy} onclick={() => void reload(true)}><Icon name="refresh" size={16} /> {$t('refresh')}</button>{/if}</div>
  <p class="page-intro">{$t('extensions_intro')}</p>
  <nav class="extension-tabs" aria-label={$t('extensions_management')}>
    <button aria-label={$t('installed_extensions')} class:active={tab==='installed'} aria-current={tab==='installed' ? 'page' : undefined} onclick={() => tab='installed'}>{$t('installed_extensions')} ({installed.length})</button>
    <button aria-label={$t('extension_catalogue')} class:active={tab==='catalogue'} aria-current={tab==='catalogue' ? 'page' : undefined} onclick={() => tab='catalogue'}>{$t('extension_catalogue')} <span>{catalogueGroups.length}</span></button>
    <button aria-label={$t('extension_repositories')} class:active={tab==='repositories'} aria-current={tab==='repositories' ? 'page' : undefined} onclick={() => tab='repositories'}>{$t('extension_repositories')} ({repositories.length})</button>
  </nav>
  {#if (tab === 'installed' && !managed) || (tab === 'catalogue' && !catalogueId)}
    <div class="category-filters" aria-label={$t('categories')}>
      <button class:chosen={category === 'all'} aria-pressed={category === 'all'} onclick={() => category='all'}>{$t('all')}</button>
      {#each categories as item (item)}<button class:chosen={category === item} aria-pressed={category === item} onclick={() => category=item}>{item}</button>{/each}
    </div>
  {/if}
  {/if}
  {#if busy}<p class="loading-notice" role="status">{$t('loading')}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if tab==='repositories'}
    <p class="repository-warning">{$t('third_party_warning')}</p>
    <div class="section-title"><h3>{$t('your_repositories')}</h3><button class="primary" aria-expanded={addingRepository} onclick={() => addingRepository=!addingRepository}>{addingRepository ? $t('close') : `+ ${$t('new_repository')}`}</button></div>
    {#if addingRepository}
    <form class="repository-form" onsubmit={addRepository}>
      <div class="actions"><button type="button" disabled={busy} onclick={() => chooseOfficialRepository('main')}>Aether Plugins · main</button><button type="button" disabled={busy} onclick={() => chooseOfficialRepository('develop')}>Aether Plugins · develop</button></div>
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
    {#if !catalogueId && repositories.some(repository => !repository.refreshedAt)}<div class="catalogue-pending"><Icon name="info" size={20} /><div><strong>{$t('repository_first_refresh')}</strong><p>{$t('refresh_repositories_hint')}</p></div></div>{/if}
    {#if !catalogueId}<label class="catalogue-search">{$t('catalogue_search')}<input type="search" bind:value={search} placeholder={$t('name_or_description')} /></label>
    <p class="section-help">{$t('enable_after_install')}</p>
    {#if repositories.some(repository=>repository.id==='community-default')&&!catalogue.some(entry=>/intro.?skip/i.test(entry.manifest.id+' '+entry.manifest.name))}<p class="catalogue-pending">Intro Skipper n’est pas présent dans le catalogue reçu. Sa version signée doit être publiée dans le dépôt, puis le catalogue actualisé.</p>{/if}{/if}
    {#if catalogueId}
      {@const group = catalogueGroups.find(item => item.key === catalogueId)}
      {#if group}
        {@const entry = group.entries[0]}
        {@const existing = installed.find(item => item.manifest.id === entry.manifest.id)}
        <button class="back-installed" onclick={() => catalogueId=null}><Icon name="chevron-left" size={16} /> {$t('extension_catalogue')}</button>
        <div class="catalogue-detail">
          <div><h3>{displayName(entry.manifest.name)}</h3><p>{entry.manifest.description}</p>
            {#if entry.overview}<p class="catalogue-overview">{entry.overview}</p>{/if}
            <h4>{$t('versions')}</h4>
            <p class="section-help">{$t('catalogue_versions_hint')}</p>
            <ol class="catalogue-versions" aria-label={$t('versions')}>
              {#each group.entries as version (version.manifest.version)}
                {@const order = existing ? compareVersions(version.manifest.version, existing.manifest.version) : 1}
                {@const label = `${displayName(version.manifest.name)} v${version.manifest.version}`}
                <li class="catalogue-version" class:current={order === 0}>
                  <div class="catalogue-version-heading">
                    <strong>v{version.manifest.version}</strong>
                    {#if formatDate(version.timestamp ?? version.manifest.releaseDate)}<span class="revision-date">{formatDate(version.timestamp ?? version.manifest.releaseDate)}</span>{/if}
                    {#if order === 0}<span class="badge success">{$t('extension_installed')}</span>{:else if version === group.entries[0]}<span class="badge update">{$t('latest_version')}</span>{/if}
                    {#if !version.compatible}<span class="badge">{$t('incompatible')}</span>{/if}
                  </div>
                  {#if changelogLines(version.changelog ?? version.manifest.changelog).length}
                    <ul class="catalogue-changelog">{#each changelogLines(version.changelog ?? version.manifest.changelog) as line, index (index)}<li>{line.replace(/^[•*-]\s*/, '')}</li>{/each}</ul>
                  {:else}<p class="revision-fallback-desc">{$t('published_without_changelog')}</p>{/if}
                  <div class="revision-action-row">
                    {#if order === 0}
                      <span class="installed-tag"><Icon name="check" size={16} /> {$t('current_version_installed')}</span>
                    {:else if !existing}
                      <button class:primary={version === group.entries[0]} aria-label={`${$t('install_this_version')} : ${label}`} disabled={busy || !version.compatible} onclick={() => installFromCatalogue(version)}><Icon name="download" size={16} /> {$t('install_this_version')}</button>
                    {:else if order > 0}
                      <button class="primary" aria-label={`${$t('update_to_version', { version: version.manifest.version })} : ${label}`} disabled={busy || !version.compatible} onclick={() => void switchVersion(version)}><Icon name="download" size={16} /> {$t('update_to_version', { version: version.manifest.version })}</button>
                    {:else}
                      <div class="restore-action"><p>{$t('downgrade_warning')}</p>
                        <label><input type="checkbox" checked={downgradeConfirmedFor === version.manifest.version} onchange={(event) => downgradeConfirmedFor = event.currentTarget.checked ? version.manifest.version : ''} /> {$t('confirm_downgrade')}</label>
                        {#if existing.rollbackVersion}<p>{$t('existing_backup', { version: existing.rollbackVersion })}</p>{/if}
                        <button aria-label={`${$t('install_this_version')} : ${label}`} disabled={busy || !version.compatible || downgradeConfirmedFor !== version.manifest.version} onclick={() => void downgrade(version, existing)}>{$t('install_this_version')}</button>
                      </div>
                    {/if}
                  </div>
                </li>
              {/each}
            </ol>
            <h4>{$t('permissions')}</h4><p>{accessLabels(entry.manifest.permissions) || $t('none')}</p><p>{$t('services_contacted')} : {entry.manifest.httpHosts?.join(', ') || $t('none').toLowerCase()}</p>
          </div>
          <aside>{@render artwork(entry.manifest)}<dl><div><dt>{$t('developer')}</dt><dd>{entry.owner || entry.manifest.author}</dd></div><div><dt>{$t('repository')}</dt><dd>{entry.repositoryName}</dd></div><div><dt>{$t('compatibility')}</dt><dd>{entry.compatible ? $t('compatible') : $t('incompatible')}</dd></div></dl>
            {#if existing}<button class="primary" onclick={() => {tab='installed';managedId=existing.manifest.id;catalogueId=null;}}>{$t('extension_settings_for', { name: displayName(existing.manifest.name) })}</button>{:else}<button class="primary" disabled={busy || !entry.compatible} onclick={() => installFromCatalogue(entry)}>{$t('install')} {displayName(entry.manifest.name)}</button>{/if}
            <p class="section-help">{existing ? $t('already_installed_hint') : $t('install_disabled_hint')}</p>
          </aside>
        </div>
      {/if}
    {:else}
    <div class="plugins-grid">
    {#each catalogueGroups.filter(group=>(category === 'all' || categoryOf(group.entries[0].manifest) === category) && `${group.entries[0].manifest.name} ${group.entries[0].manifest.description}`.toLowerCase().includes(search.toLowerCase())) as group (group.key)}
      {@const entry=group.entries[0]}
      {@const existing=installed.find(item => item.manifest.id===entry.manifest.id)}
      <article class="card plugin-card catalogue-card">
        <button class="artwork-button" aria-label={`Détails de ${displayName(entry.manifest.name)}`} onclick={() => catalogueId=group.key}>{@render artwork(entry.manifest)}</button>
        <div class="catalogue-identity"><span class="extension-icon"><Icon name={entry.manifest.permissions.includes('avatars') ? 'profile' : 'globe'} size={28} /></span>{#if existing || !entry.compatible}<span class="badge" class:success={!!existing}>{existing ? 'Installée' : 'Incompatible'}</span>{/if}</div>
        <div class="catalogue-content"><h4>{displayName(entry.manifest.name)}</h4><p class="plugin-desc">{shortDescription(entry.manifest.description)}</p>
        <button class="details-link" onclick={() => catalogueId=group.key}>{$t('extension_details')} <Icon name="chevron-right" size={14} /></button>
        </div><div class="catalogue-actions"><span class="version-label">v{entry.manifest.version}{#if group.entries.length>1}<button class="version-count" onclick={() => catalogueId=group.key}>{$t('version_count', { count: group.entries.length })}</button>{/if}</span>
        <button class="primary" aria-label={existing ? `${entry.manifest.name} ${$t('installed').toLowerCase()}` : `${$t('install')} ${entry.manifest.name}`} disabled={busy || !entry.compatible || !!existing} onclick={() => installFromCatalogue(entry)}>{existing ? $t('installed') : $t('install')}</button></div>
      </article>
    {:else}<p>{$t('no_extension_results')}</p>{/each}
    </div>
    {/if}
  {:else}
  {#if managed}
    {@const extension=managed}
    <div class="extension-detail">
      <div class="detail-top-nav">
        {#if settingsOpen}<button class="back-installed" onclick={()=>settingsOpen=false}><Icon name="chevron-left" size={16} /> {$t('back_to_extension')}</button>{:else}<button class="back-installed" onclick={()=>managedId=null}><Icon name="chevron-left" size={16} /> {$t('all_extensions')}</button>{/if}
      </div>
      {#if settingsOpen}
        <div class="settings-screen">
          <h2 id="extensions-heading">{$t('extension_settings_for', { name: displayName(extension.manifest.name) })}</h2>
          {#if extension.manifest.permissions.includes('metadata.read')}<p>{$t('metadata_language_note')}</p>{/if}
          {#if settingsFields(extension.manifest).length}
            <ExtensionSettings fields={settingsFields(extension.manifest)} bind:values={editingSettings[extension.manifest.id]} {busy} onsave={()=>void saveSettings(extension.manifest.id)} />
          {:else}<p>{$t('no_settings')}</p>{/if}
          {#if extension.manifest.permissions.includes('avatars')}<AvatarCollections extensionId={extension.manifest.id} />{/if}
        </div>
      {:else}
      <div class="detail-header-card">
        <div class="detail-header-main">
          <h2>{displayName(extension.manifest.name)}</h2>
          <p class="detail-description">{shortDescription(extension.manifest.description)}</p>

          <div class="revisions-section">
            <div class="revisions-heading">
              <h3>{$t('versions')}</h3>
              {#if getUpdateEntry(extension)}
                {@const upd = getUpdateEntry(extension)!}
                <span class="badge update-badge">
                  {$t('update_available', { version: upd.manifest.version })}
                </span>
              {/if}
            </div>

            <div class="revisions-list">
              {#each getRevisions(extension.manifest.id, extension.manifest, extension.rollbackVersion) as rev (rev.version)}
                {@const isOpen = openRevisions[rev.version] ?? (rev.isCurrent || rev.isNewer)}
                <div class="revision-item" class:current={rev.isCurrent} class:has-update={rev.isNewer}>
                  <button
                    type="button"
                    class="revision-header"
                    aria-expanded={isOpen}
                    onclick={() => openRevisions[rev.version] = !isOpen}
                  >
                    <div class="revision-title-row">
                      <span class="revision-version">{rev.version}</span>
                      {#if rev.releaseDate}
                        <span class="revision-date">— {rev.releaseDate}</span>
                      {/if}
                      {#if rev.isCurrent}
                        <span class="badge success">{$t('extension_installed')}</span>
                      {:else if rev.isRollback}
                        <span class="badge">{$t('rollback_available')}</span>
                      {:else if rev.isNewer}
                        <span class="badge update">{$t('update')}</span>
                      {/if}
                    </div>
                    <span class="revision-chevron" class:rotated={isOpen}>
                      <Icon name="chevron" size={16} />
                    </span>
                  </button>

                  {#if isOpen}
                    <div class="revision-content">
                      <h4 class="revision-version-tag">v{rev.version}</h4>

                      {#if rev.changelog}
                        <div class="revision-changelog">
                          {#each rev.changelog.split('\n').filter(line => line.trim()) as line, idx (idx)}
                            {#if line.trim().startsWith('•') || line.trim().startsWith('*') || line.trim().startsWith('-')}
                              <div class="changelog-item">
                                <span class="bullet">•</span>
                                <span>{line.replace(/^[\s•*-]+/, '').trim()}</span>
                              </div>
                            {:else}
                              <p class="changelog-p">{line}</p>
                            {/if}
                          {/each}
                        </div>
                      {:else}
                        <p class="revision-fallback-desc">{rev.manifest.description || $t('published_without_changelog')}</p>
                      {/if}

                      <div class="revision-specs">
                        {#if rev.manifest.targetAbi}
                          <span class="spec-tag">{$t('target_api')} : <code>{rev.manifest.targetAbi}</code></span>
                        {/if}
                        {#if rev.manifest.minServerVersion}
                          <span class="spec-tag">{$t('required_server')} : <code>{rev.manifest.minServerVersion}+</code></span>
                        {/if}
                      </div>

                      <div class="revision-action-row">
                        {#if rev.isCurrent}
                          <span class="installed-tag">
                            <Icon name="check" size={16} /> {$t('current_version_installed')}
                          </span>
                        {:else if rev.isRollback}
                          <div class="restore-action"><p>{$t('rollback_warning')}</p>
                            <label><input type="checkbox" checked={rollbackConfirmedFor === extension.manifest.id} onchange={(event) => rollbackConfirmedFor = event.currentTarget.checked ? extension.manifest.id : ''} /> {$t('confirm_restore')}</label>
                            <button type="button" disabled={busy || rollbackConfirmedFor !== extension.manifest.id} onclick={() => void rollback(extension)}>{$t('restore_version', { version: rev.version })}</button>
                          </div>
                        {:else if rev.isOlder && rev.entry}
                          <div class="restore-action"><p>{$t('downgrade_warning')}</p>
                            <label><input type="checkbox" checked={downgradeConfirmedFor === rev.version} onchange={(event) => downgradeConfirmedFor = event.currentTarget.checked ? rev.version : ''} /> {$t('confirm_downgrade')}</label>
                            {#if extension.rollbackVersion}<p>{$t('existing_backup', { version: extension.rollbackVersion })}</p>{/if}
                            <button disabled={busy || !rev.compatible || downgradeConfirmedFor !== rev.version} onclick={() => void downgrade(rev.entry!, extension)}>{$t('install_version', { version: rev.version })}</button>
                          </div>
                        {:else if rev.isOlder}
                          <span class="local-only-text">{$t('no_restorable_backup')}</span>
                        {:else if rev.entry}
                          <button
                            type="button"
                            class="btn-revision-install"
                            class:btn-revision-update={rev.isNewer}
                            disabled={busy || !rev.compatible}
                            onclick={() => void switchVersion(rev.entry!)}
                          >
                            <Icon name="download" size={16} />
                            {rev.isNewer ? $t('update_to_version', { version: rev.version }) : $t('install_version', { version: rev.version })}
                          </button>
                          {#if !rev.compatible}
                            <span class="incompatible-badge">{$t('incompatible_server')}</span>
                          {/if}
                        {:else}
                          <span class="local-only-text">{$t('local_version')}</span>
                        {/if}
                      </div>
                    </div>
                  {/if}
                </div>
              {:else}
                <p class="empty-revisions">{$t('no_extension_results')}</p>
              {/each}
            </div>
          </div>
        </div>
        <div class="detail-header-side">
          {@render artwork(extension.manifest)}
          <div class="detail-action-buttons">
            <label class="toggle-control">
              <input type="checkbox" role="switch" aria-label={`Activer ${displayName(extension.manifest.name)}`} checked={extension.enabled} disabled={busy} onchange={event=>void configure(extension,event.currentTarget.checked)} />
              <span>{extension.enabled ? $t('disable_plugin') : $t('enable_plugin')}</span>
            </label>
            <button class="btn-detail-settings" onclick={() => settingsOpen=true}>
              <Icon name="settings" size={18} />
              {$t('settings_title')}
            </button>
            {#if isPinnableExtension(extension.manifest.id)}
              <button aria-pressed={$extensionPins.includes(extension.manifest.id)} disabled={busy || (!extension.enabled && !$extensionPins.includes(extension.manifest.id))} onclick={() => toggleExtensionPin(extension.manifest.id)}><Icon name="star" size={16} />{$extensionPins.includes(extension.manifest.id) ? $t('unpin_menu') : $t('pin_menu')}</button>
              {#if extension.enabled}<a class="button-link" href={`#${extension.manifest.id === 'media-requests' ? 'requests' : 'reports'}-section`}>{$t('open_extension', { name: displayName(extension.manifest.name) })}</a>{/if}
            {/if}
            <button class="btn-detail-uninstall" disabled={busy} onclick={()=>void perform(async()=>{await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}`,{method:'DELETE',body:JSON.stringify({deleteData:false})});managedId=null;},'Extension désinstallée.')}>
              <Icon name="trash" size={18} />
              {$t('uninstall')}
            </button>
          </div>
          <div class="detail-meta-table">
            <div class="meta-row">
              <span class="meta-label">{$t('state')}</span>
              <span class="meta-value">{extension.enabled ? $t('active_state') : $t('disabled_state')}</span>
            </div>
            <div class="meta-row">
              <span class="meta-label">{$t('version')}</span>
              <span class="meta-value">{extension.manifest.version}</span>
            </div>
            <div class="meta-row">
              <span class="meta-label">{$t('developer')}</span>
              <span class="meta-value">{extension.manifest.author}</span>
            </div>
            {#if extension.manifest.repository}
              <div class="meta-row">
                <span class="meta-label">{$t('repository')}</span>
                <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- External link -->
                <span class="meta-value"><a href={extension.manifest.repository} target="_blank" rel="noreferrer">{displayName(extension.manifest.name)}</a></span>
              </div>
            {/if}
          </div>
        </div>
      </div>

      {/if}
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
  <div class="installed-filters"><label>{$t('filter_extensions')}<input type="search" bind:value={search} /></label><label>{$t('state')}<select bind:value={filter}><option value="all">{$t('all_extensions_filter')}</option><option value="enabled">{$t('enabled_extensions')}</option><option value="disabled">{$t('disabled_extensions')}</option></select></label></div>
  <div class="plugins-grid">{#each visibleExtensions as extension (extension.manifest.id)}
    <button type="button" class="card plugin-card installed-card" aria-label={`Ouvrir ${displayName(extension.manifest.name)}`} onclick={() => { managedId = extension.manifest.id; settingsOpen = false; }}>
      {@render artwork(extension.manifest)}
      <div class="plugin-header"><div class="plugin-title-row"><div class="installed-identity"><span class="extension-icon"><Icon name={extension.manifest.permissions.includes('avatars') ? 'profile' : 'globe'} size={22} /></span><div><h3>{displayName(extension.manifest.name)}</h3><small>v{extension.manifest.version}</small></div></div><span class="badge" class:success={extension.enabled}>{extension.enabled ? 'Activée' : 'Désactivée'}</span>{#if getUpdateEntry(extension)}<span class="badge update">Mise à jour</span>{/if}</div><p class="plugin-desc">{shortDescription(extension.manifest.description)}</p></div>
    </button>{:else}<p>{$t('no_installed_extensions')}</p>{/each}</div>
  {/if}
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
  .badge.update, .update-badge { background: var(--color-accent-soft); color: var(--color-accent); }

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

  /* Catalogue */
  .catalogue-search { max-width: 420px; margin: 0 0 8px; }
  .back-installed { min-height: 0; padding: 8px 0; border: 0; background: transparent; color: var(--color-muted); }
  .back-installed:hover:not(:disabled) { background: transparent; color: var(--color-text); }
  .catalogue-detail { display: grid; grid-template-columns: minmax(0, 1fr) minmax(280px, 36%); gap: 40px; margin-top: 8px; }
  .catalogue-detail h3 { font-size: 26px; font-weight: 700; letter-spacing: -0.02em; }
  .catalogue-detail h4 { margin: 24px 0 8px; }
  .catalogue-detail aside { min-width: 0; }
  .catalogue-detail aside > button { width: 100%; }
  .catalogue-detail .extension-artwork { border-radius: var(--radius-sm); }
  dl { margin: 16px 0; }
  dl > div { display: grid; grid-template-columns: 100px minmax(0, 1fr); gap: 12px; padding: 10px 0; border-top: 1px solid var(--color-line-subtle); font-size: 13px; }
  dt { color: var(--color-muted); }
  dd { margin: 0; color: var(--color-text); overflow-wrap: anywhere; }

  /* Grille d'extensions : surfaces plates */
  .plugins-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 260px), 1fr)); gap: 12px; align-items: stretch; }
  .plugin-card { display: flex; flex-direction: column; overflow: hidden; transition: background-color var(--transition-fast); }
  .plugin-card:hover, .plugin-card:focus-within { background: var(--color-surface-hover); }
  .artwork-button { display: block; width: 100%; min-height: 0; padding: 0; border: 0; border-radius: 0; background: transparent; text-align: center; }
  .artwork-button:hover:not(:disabled) { background: transparent; }
  .artwork-button:focus-visible { outline-offset: -3px; }

  /* Visuel de repli : couleur stable dérivée de l'identifiant */
  .extension-artwork { box-sizing: border-box; position: relative; overflow: hidden; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; width: 100%; aspect-ratio: 16 / 9; padding: 20px; background: hsl(var(--art-hue) 30% 18%); color: var(--color-text-bright); }
  .extension-artwork.has-banner { padding: 0; background: var(--color-canvas); border-radius: inherit; }
  .extension-artwork .banner-img { display: block; width: 100%; height: 100%; min-height: 0; object-fit: contain; }
  .extension-artwork strong { color: var(--color-text-bright); font-size: 18px; line-height: 1.2; text-wrap: balance; }
  .art-symbol { display: grid; color: var(--color-text-bright); opacity: 0.7; }
  .art-category { color: var(--color-text-bright); opacity: 0.75; font-size: 11px; letter-spacing: 0.08em; text-transform: uppercase; }

  .catalogue-card { height: 100%; box-sizing: border-box; }
  .catalogue-identity { display: flex; align-items: center; justify-content: space-between; min-height: 22px; padding: 12px 16px 0; }
  .catalogue-identity > .extension-icon, .installed-identity > .extension-icon { display: none; }
  .catalogue-content { flex: 1; padding: 8px 16px 0; }
  .plugin-desc { display: -webkit-box; margin: 6px 0; overflow: hidden; font-size: 13px; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .catalogue-card .plugin-desc { min-height: 3em; }
  .details-link { min-height: 0; padding: 4px 0; border: 0; background: transparent; color: var(--color-accent); }
  .details-link:hover:not(:disabled) { background: transparent; text-decoration: underline; }
  .catalogue-actions { display: flex; align-items: end; justify-content: space-between; gap: 12px; margin-top: auto; padding: 12px 16px 16px; border-top: 1px solid var(--color-line-subtle); }
  .catalogue-actions button { flex: 1; }
  .version-label { display: inline-flex; align-items: center; gap: 8px; align-self: center; font-family: var(--font-mono); font-size: 12px; color: var(--color-muted); white-space: nowrap; }
  .catalogue-actions .version-count { flex: none; min-height: 0; padding: 0; border: 0; background: transparent; color: var(--color-accent); font-family: var(--font-sans); font-size: 12px; }
  .catalogue-actions .version-count:hover:not(:disabled) { background: transparent; text-decoration: underline; }

  /* Versions publiées d'une extension du catalogue */
  .catalogue-overview { white-space: pre-line; }
  .catalogue-versions { display: flex; flex-direction: column; margin: 0; padding: 0; list-style: none; }
  .catalogue-version { padding: 14px 4px; border-top: 1px solid var(--color-line-subtle); }
  .catalogue-version:first-child { border-top: 0; }
  .catalogue-version.current { background: var(--color-surface); border-radius: var(--radius-sm); padding-inline: 12px; }
  .catalogue-version-heading { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .catalogue-version-heading strong { font-size: 14px; }
  .catalogue-changelog { margin: 8px 0; padding-left: 18px; color: var(--color-muted); font-size: 13px; line-height: 1.5; }

  /* Installées */
  .installed-filters { display: flex; flex-wrap: wrap; gap: 12px; margin: 0 0 16px; }
  .installed-filters label:first-child { flex: 1; min-width: 160px; }
  .installed-filters label:last-child { min-width: 160px; }
  .plugin-header { display: flex; flex-direction: column; flex: 1; padding: 12px 16px 16px; }
  .plugin-title-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
  .installed-identity { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .installed-identity small { font-family: var(--font-mono); }
  .installed-card { width: 100%; min-height: 0; padding: 0; border: 0; text-align: left; align-items: stretch; }
  .restore-action { display: grid; gap: 10px; }
  .restore-action label { display: flex; align-items: center; gap: 8px; }

  /* Fiche d'une extension */
  .extension-detail { margin-top: 4px; }
  .detail-top-nav { margin-bottom: 12px; }
  .settings-screen { max-width: 880px; padding: 20px; background: var(--color-surface); border-radius: var(--radius-md); }
  .settings-screen h2 { margin-bottom: 16px; }
  .detail-header-card { display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 340px); gap: 40px; align-items: start; margin-bottom: 32px; }
  .detail-header-main h2 { margin: 0 0 8px; font-size: 26px; }
  .detail-description { max-width: 680px; margin: 0; font-size: 15px; line-height: 1.5; color: var(--color-muted); }
  .detail-header-side { display: flex; flex-direction: column; gap: 16px; padding: 16px; background: var(--color-surface); border-radius: var(--radius-md); }
  .detail-header-side .extension-artwork { border-radius: var(--radius-sm); }
  .detail-action-buttons { display: flex; flex-direction: column; gap: 8px; }

  /* Interrupteur */
  .toggle-control { display: flex; align-items: center; gap: 12px; padding: 4px 0; color: var(--color-text); font-size: 14px; cursor: pointer; }
  .toggle-control input[type='checkbox'] { appearance: none; -webkit-appearance: none; position: relative; flex: 0 0 42px; width: 42px; height: 24px; margin: 0; padding: 0; min-height: 0; border: 0; border-radius: var(--radius-pill); background: var(--color-line-strong); cursor: pointer; transition: background-color var(--transition-fast); }
  .toggle-control input[type='checkbox']::after { content: ''; position: absolute; top: 3px; left: 3px; width: 18px; height: 18px; border-radius: 50%; background: var(--color-text-bright); transition: transform var(--transition-fast); }
  .toggle-control input[type='checkbox']:checked { background: var(--color-accent); }
  .toggle-control input[type='checkbox']:checked::after { transform: translateX(18px); }
  .toggle-control input[type='checkbox']:disabled { opacity: 0.45; cursor: not-allowed; }

  .btn-detail-settings, .btn-detail-uninstall { width: 100%; box-sizing: border-box; }
  .btn-detail-settings { border-color: transparent; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  .btn-detail-settings:hover:not(:disabled) { background: var(--color-accent-hover); opacity: 0.9; }
  .btn-detail-uninstall { color: var(--color-danger); }
  .btn-detail-uninstall:hover:not(:disabled) { background: var(--color-danger-soft); }

  .detail-meta-table { display: flex; flex-direction: column; }
  .meta-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 8px 0; border-top: 1px solid var(--color-line-subtle); font-size: 13px; }
  .meta-label { color: var(--color-muted); }
  .meta-value { min-width: 0; color: var(--color-text); font-weight: 500; text-align: right; overflow-wrap: anywhere; }
  .meta-value a { color: var(--color-accent); text-decoration: none; }
  .meta-value a:hover { text-decoration: underline; }
  .meta-value a:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-xs); }

  /* Révisions : lignes séparées */
  .revisions-section { margin-top: 24px; padding-top: 16px; border-top: 1px solid var(--color-line-subtle); }
  .revisions-heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; margin-bottom: 8px; }
  .revisions-list { display: flex; flex-direction: column; }
  .revision-item { border-top: 1px solid var(--color-line-subtle); }
  .revision-item:first-child { border-top: 0; }
  .revision-header { justify-content: space-between; width: 100%; min-height: 48px; padding: 12px 4px; border: 0; border-radius: 0; background: transparent; color: var(--color-text); text-align: left; }
  .revision-header:hover:not(:disabled) { background: var(--color-surface); }
  .revision-header:focus-visible { outline-offset: -2px; }
  .revision-title-row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .revision-version { font-size: 14px; font-weight: 600; }
  .revision-date { font-size: 12px; color: var(--color-muted); }
  .revision-chevron { display: inline-flex; align-items: center; color: var(--color-muted); transition: transform var(--transition-normal); }
  .revision-chevron.rotated { transform: rotate(180deg); }
  .revision-content { padding: 0 4px 16px; }
  .revision-version-tag { margin: 4px 0 8px; font-size: 14px; font-weight: 600; }
  .revision-changelog { display: flex; flex-direction: column; gap: 6px; margin: 8px 0 12px; }
  .changelog-item { display: flex; gap: 8px; font-size: 13px; line-height: 1.5; color: var(--color-muted); }
  .bullet { flex: none; color: var(--color-accent); }
  .changelog-p { margin: 4px 0; font-size: 13px; line-height: 1.5; color: var(--color-muted); }
  .revision-fallback-desc { margin: 8px 0 12px; font-size: 13px; line-height: 1.5; color: var(--color-muted); }
  .revision-specs { display: flex; flex-wrap: wrap; gap: 12px; margin-bottom: 12px; }
  .spec-tag { font-size: 12px; color: var(--color-muted); }
  .spec-tag code { padding: 2px 6px; border-radius: var(--radius-xs); background: var(--color-raised); font-size: 11px; }
  .revision-action-row { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-top: 8px; }
  .btn-revision-update { border-color: transparent; background: var(--color-accent-solid); color: var(--color-on-accent); }
  .btn-revision-update:hover:not(:disabled) { background: var(--color-accent-hover); opacity: 0.9; }
  .installed-tag { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 500; color: var(--color-success); }
  .incompatible-badge { font-size: 12px; color: var(--color-danger); }
  .local-only-text, .empty-revisions { font-size: 13px; color: var(--color-muted); }

  @media (max-width: 900px) {
    .detail-header-card { grid-template-columns: minmax(0, 1fr); gap: 24px; }
  }

  @media (max-width: 700px) {
    .catalogue-detail { grid-template-columns: minmax(0, 1fr); gap: 24px; }
    .catalogue-detail aside { grid-row: 1; }
    .repository-form { grid-template-columns: minmax(0, 1fr); }
    .repository-card { grid-template-columns: minmax(0, 1fr); padding: 16px; }
    .repository-card .actions { grid-column: 1; grid-row: auto; justify-content: flex-start; }
    .repository-card .actions button { flex: 1 1 160px; }
    .repository-heading { flex-wrap: wrap; }
    .repository-url, .repository-card > small { padding-left: 0; }
    .extension-tabs button { padding: 10px 12px; }
    .catalogue-search { max-width: none; }
    .heading { align-items: flex-start; }
    .catalogue-actions { padding: 12px 16px; }
    .plugin-header, .catalogue-content, .catalogue-identity { padding-inline: 16px; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button, .plugin-card, .revision-chevron, .toggle-control input[type='checkbox'], .toggle-control input[type='checkbox']::after { transition: none; }
  }
</style>

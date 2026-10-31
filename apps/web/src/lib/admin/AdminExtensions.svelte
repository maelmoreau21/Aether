<script lang="ts">
  import { displayManifest } from './plugin-text';
  import ExtensionSettings from './ExtensionSettings.svelte';
  import { settingsFields, settingsDefaults } from './extension-settings';
  import AvatarCollections from './AvatarCollections.svelte';
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import { SvelteMap } from 'svelte/reactivity';
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
  type Installed = { manifest: Manifest; publisherId: string; sha256: string; enabled: boolean; libraryIds: string[]; allLibraries?: boolean; settings?: ExtensionSettings };
  type Repository = { id: string; name: string; url: string; refreshedAt: number };
  type CatalogueEntry = { repositoryId: string; repositoryName: string; compatible: boolean; manifest: Manifest; publisherId: string; publicKey: string };
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
  let catalogueVersions = $state<Record<string,string>>({});
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
  $effect(() => {
    for (const group of catalogueGroups) {
      if (!group.entries.some(entry => entry.manifest.version===catalogueVersions[group.key])) catalogueVersions[group.key]=group.entries[0].manifest.version;
    }
  });
  let installed = $state<Installed[]>([]);
  const categories = $derived([...new Set([...catalogue.map(entry => categoryOf(entry.manifest)), ...installed.map(entry => categoryOf(entry.manifest))])].sort());
  let editingSettings = $state<Record<string, ExtensionSettings>>({});
  const managed=$derived(installed.find(extension=>extension.manifest.id===managedId));
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
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
  async function reload() {
    busy = true; error = '';
    try { await refresh(); }
    catch (e) { error = e instanceof Error ? e.message : 'Chargement des extensions impossible.'; }
    finally { busy = false; }
  }
  async function refreshRepositories() {
    busy = true; error = ''; notice = '';
    const failures: string[] = [];
    try {
      for (const repository of repositories) {
        try { await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}/refresh`, {method:'POST'}); }
        catch { failures.push(repository.name); }
      }
      await refresh();
      if (failures.length) error = `Dépôts injoignables : ${failures.join(', ')}. Le dernier catalogue disponible est conservé.`;
      else notice = 'Catalogue à jour. Choisissez les extensions à installer.';
    } catch (e) { error = e instanceof Error ? e.message : 'Actualisation impossible.'; }
    finally { busy = false; }
  }
  async function refresh() {
    const [extensions, sources, available] = await Promise.all([
      adminRequest<{ extensions: Installed[] }>('/api/v1/admin/extensions'),
      adminRequest<{ repositories: Repository[] }>('/api/v1/admin/extensions/repositories'),
      adminRequest<{ entries: CatalogueEntry[] }>('/api/v1/admin/extensions/catalogue')
    ]);
    installed = extensions.extensions.map(extension=>({...extension,manifest:displayManifest(extension.manifest)}));
    repositories = sources.repositories; catalogue = available.entries.map(entry=>({...entry,manifest:displayManifest(entry.manifest)}));

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
    catch (e) { error = e instanceof Error ? e.message : 'Opération impossible.'; }
    finally { busy = false; }
  }
  onMount(() => { void reload(); });
  function configure(extension: Installed, enabled: boolean) {
    return perform(() => adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}`, {
      method: 'PUT', body: JSON.stringify({ enabled, libraryIds: extension.libraryIds, allLibraries: extension.allLibraries ?? extension.libraryIds.length===0 })
    }), enabled ? 'Permissions enregistrées et extension activée.' : 'Extension désactivée.');
  }
  async function saveSettings(id: string) {
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(id)}/settings`, {
        method: 'PUT',
        body: JSON.stringify(editingSettings[id] ?? {})
      });

    }, 'Paramètres enregistrés.');
  }
  async function addRepository(event: SubmitEvent) {
    event.preventDefault();
    await perform(async () => {
      await adminRequest('/api/v1/admin/extensions/repositories',{method:'POST',body:JSON.stringify({name:repositoryName.trim() || new URL(repositoryUrl).hostname.slice(0,80),url:repositoryUrl})});
      repositoryName=''; repositoryUrl=''; addingRepository=false; tab='catalogue';
    },'Dépôt enregistré. Ses extensions sont disponibles dans le catalogue.');
  }
  async function installFromCatalogue(entry: CatalogueEntry) {
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/install`,{method:'POST',body:JSON.stringify({extensionId:entry.manifest.id,version:entry.manifest.version})});
      tab='installed'; catalogueId=null; managedId=entry.manifest.id;
    },'Extension installée. Configurez-la puis activez-la.');
  }

  function compareVersions(v1: string, v2: string): number {
    const p1 = (v1 || '0').split('.').map(n => parseInt(n, 10) || 0);
    const p2 = (v2 || '0').split('.').map(n => parseInt(n, 10) || 0);
    const len = Math.max(p1.length, p2.length);
    for (let i = 0; i < len; i++) {
      const a = p1[i] ?? 0;
      const b = p2[i] ?? 0;
      if (a > b) return 1;
      if (a < b) return -1;
    }
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

  function getRevisions(extensionId: string, currentManifest?: Manifest) {
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

    versions.sort((a, b) => compareVersions(b, a));

    return versions.map(v => {
      const entry = versionMap.get(v) ?? null;
      const manifest = entry ? entry.manifest : currentManifest!;
      const cmp = currentVersion ? compareVersions(v, currentVersion) : 0;

      let dateStr: string | null = null;
      if (manifest.releaseDate) {
        dateStr = manifest.releaseDate;
      } else if (entry) {
        const repo = repositories.find(r => r.id === entry.repositoryId);
        if (repo?.refreshedAt) {
          dateStr = new Date(repo.refreshedAt * 1000).toLocaleString();
        }
      }

      return {
        version: v,
        entry,
        manifest,
        releaseDate: dateStr,
        changelog: manifest.changelog || null,
        isCurrent: cmp === 0,
        isNewer: cmp > 0,
        isOlder: cmp < 0,
        compatible: entry ? entry.compatible : true
      };
    });
  }

  async function switchVersion(entry: CatalogueEntry) {
    const extId = managed?.manifest.id || entry.manifest.id;
    const currentInstalled = installed.find(e => e.manifest.id === extId);
    const wasEnabled = currentInstalled?.enabled ?? false;
    const libraryIds = currentInstalled?.libraryIds ?? [];
    const allLibraries = currentInstalled?.allLibraries ?? (libraryIds.length === 0);

    await perform(async () => {
      if (currentInstalled) {
        await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extId)}`, {
          method: 'DELETE',
          body: JSON.stringify({ deleteData: false })
        });
      }
      await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/install`, {
        method: 'POST',
        body: JSON.stringify({ extensionId: entry.manifest.id, version: entry.manifest.version })
      });
      if (currentInstalled && wasEnabled) {
        await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extId)}`, {
          method: 'PUT',
          body: JSON.stringify({ enabled: true, libraryIds, allLibraries })
        });
      }
      managedId = extId;
    }, `Version ${entry.manifest.version} installée avec succès.`);
  }

</script>

{#snippet artwork(manifest: Manifest)}
  {@const bannerSrc = toBannerSrc(manifest.banner)}
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
  <div class="heading page-heading"><h2 id="extensions-heading">Extensions</h2>{#if tab==='catalogue'}<button aria-label="Actualiser les dépôts" disabled={busy || repositories.length===0} onclick={refreshRepositories}><Icon name="refresh" size={16} /> Actualiser</button>{:else}<button disabled={busy} onclick={reload}><Icon name="refresh" size={16} /> Actualiser</button>{/if}</div>
  <p class="page-intro">Personnalisez votre médiathèque. Découvrez, installez et gérez vos extensions.</p>
  <nav class="extension-tabs" aria-label="Gestion des extensions">
    <button aria-label="Installées" class:active={tab==='installed'} aria-current={tab==='installed' ? 'page' : undefined} onclick={() => tab='installed'}>Installées ({installed.length})</button>
    <button aria-label="Catalogue" class:active={tab==='catalogue'} aria-current={tab==='catalogue' ? 'page' : undefined} onclick={() => tab='catalogue'}>Catalogue <span>{catalogueGroups.length}</span></button>
    <button aria-label="Dépôts" class:active={tab==='repositories'} aria-current={tab==='repositories' ? 'page' : undefined} onclick={() => tab='repositories'}>Dépôts ({repositories.length})</button>
  </nav>
  {#if (tab === 'installed' && !managed) || (tab === 'catalogue' && !catalogueId)}
    <div class="category-filters" aria-label="Catégories">
      <button class:chosen={category === 'all'} aria-pressed={category === 'all'} onclick={() => category='all'}>Tout</button>
      {#each categories as item (item)}<button class:chosen={category === item} aria-pressed={category === item} onclick={() => category=item}>{item}</button>{/each}
    </div>
  {/if}
  {/if}
  {#if busy}<p class="loading-notice" role="status">Chargement…</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if tab==='repositories'}
    <p class="repository-warning">Les plugins tiers peuvent présenter un danger. Ajoutez uniquement des dépôts dont vous connaissez la source.</p>
    <div class="section-title"><h3>Vos dépôts</h3><button class="primary" aria-expanded={addingRepository} onclick={() => addingRepository=!addingRepository}>{addingRepository ? "Fermer" : "+ Nouveau dépôt"}</button></div>
    {#if addingRepository}
    <form class="repository-form" onsubmit={addRepository}>
      <label>Nom du dépôt (facultatif)<input maxlength="80" bind:value={repositoryName} disabled={busy} /></label>
      <label>Adresse du dépôt<input type="url" required maxlength="2048" placeholder="https://exemple.fr/manifest.json" bind:value={repositoryUrl} disabled={busy} /></label>
      <button disabled={busy}>Ajouter le dépôt</button>
    </form>
    {/if}
    {#each repositories as repository (repository.id)}
      <article class="card repository-card"><div class="repository-heading"><span class="extension-icon"><Icon name="server" size={22} /></span><div><h4>{repository.name}</h4><p>{repository.refreshedAt ? `Actualisé le ${new Date(repository.refreshedAt*1000).toLocaleString()}` : 'Catalogue à récupérer'}</p></div>{#if repository.id==='community-default'}<span class="badge">Préconfiguré</span>{/if}</div><code class="repository-url">{repository.url}</code>
        <div class="actions"><button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}/refresh`,{method:'POST'}),'Catalogue actualisé.')}>Actualiser ce dépôt</button>
        <button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}`,{method:'DELETE'}),'Dépôt retiré. Les extensions installées sont conservées.')}>Retirer le dépôt</button></div><small>Les extensions installées sont conservées. Ce dépôt ne sera pas réajouté au redémarrage.</small>
      </article>
    {:else}<p>Aucun dépôt enregistré.</p>{/each}
  {:else if tab==='catalogue'}
    {#if !catalogueId && repositories.some(repository => !repository.refreshedAt)}<div class="catalogue-pending"><Icon name="info" size={20} /><div><strong>Un dépôt attend sa première actualisation</strong><p>Actualisez les dépôts pour récupérer leurs extensions.</p></div></div>{/if}
    {#if !catalogueId}<label class="catalogue-search">Rechercher dans le catalogue<input type="search" bind:value={search} placeholder="Nom ou description" /></label>
    <p class="section-help">Après installation, activez vos choix dans Installées.</p>
    {#if repositories.some(repository=>repository.id==='community-default')&&!catalogue.some(entry=>/intro.?skip/i.test(entry.manifest.id+' '+entry.manifest.name))}<p class="catalogue-pending">Intro Skipper n’est pas présent dans le catalogue reçu. Sa version signée doit être publiée dans le dépôt, puis le catalogue actualisé.</p>{/if}{/if}
    {#if catalogueId}
      {@const group = catalogueGroups.find(item => item.key === catalogueId)}
      {#if group}
        {@const entry = group.entries.find(item => item.manifest.version === catalogueVersions[group.key]) ?? group.entries[0]}
        {@const existing = installed.find(item => item.manifest.id === entry.manifest.id)}
        <button class="back-installed" onclick={() => catalogueId=null}><Icon name="chevron-left" size={16} /> Retour au catalogue</button>
        <div class="catalogue-detail">
          <div><h3>{displayName(entry.manifest.name)}</h3><p>{entry.manifest.description}</p>
            <h4>Versions disponibles</h4>
            <label>Version à installer<select bind:value={catalogueVersions[group.key]} disabled={busy}>{#each group.entries as version (version.manifest.version)}<option value={version.manifest.version}>{version.manifest.version}{version.compatible ? '' : ' · incompatible'}</option>{/each}</select></label>
            <h4>Accès demandés</h4><p>{accessLabels(entry.manifest.permissions) || 'Aucun accès supplémentaire'}</p><p>Services contactés : {entry.manifest.httpHosts?.join(', ') || 'aucun'}</p>
          </div>
          <aside>{@render artwork(entry.manifest)}<dl><div><dt>Développeur</dt><dd>{entry.manifest.author}</dd></div><div><dt>Dépôt</dt><dd>{entry.repositoryName}</dd></div><div><dt>Compatibilité</dt><dd>{entry.compatible ? 'Compatible' : 'Incompatible'}</dd></div></dl>
            {#if existing}<button class="primary" onclick={() => {tab='installed';managedId=existing.manifest.id;catalogueId=null;}}>Paramètres de {displayName(existing.manifest.name)}</button>{:else}<button class="primary" disabled={busy || !entry.compatible} onclick={() => installFromCatalogue(entry)}>Installer {displayName(entry.manifest.name)}</button>{/if}
            <p class="section-help">{existing ? 'Cette extension est déjà installée. Retrouvez sa configuration dans les paramètres.' : 'L’installation conserve l’extension désactivée. Vous pourrez ensuite la configurer et l’activer.'}</p>
          </aside>
        </div>
      {/if}
    {:else}
    <div class="plugins-grid">
    {#each catalogueGroups.filter(group=>(category === 'all' || categoryOf(group.entries[0].manifest) === category) && `${group.entries[0].manifest.name} ${group.entries[0].manifest.description}`.toLowerCase().includes(search.toLowerCase())) as group (group.key)}
      {@const entry=group.entries.find(item => item.manifest.version===catalogueVersions[group.key]) ?? group.entries[0]}
      {@const existing=installed.find(item => item.manifest.id===entry.manifest.id)}
      <article class="card plugin-card catalogue-card">
        <button class="artwork-button" aria-label={`Détails de ${displayName(entry.manifest.name)}`} onclick={() => catalogueId=group.key}>{@render artwork(entry.manifest)}</button>
        <div class="catalogue-identity"><span class="extension-icon"><Icon name={entry.manifest.permissions.includes('avatars') ? 'profile' : 'globe'} size={28} /></span>{#if existing || !entry.compatible}<span class="badge" class:success={!!existing}>{existing ? 'Installée' : 'Incompatible'}</span>{/if}</div>
        <div class="catalogue-content"><h4>{displayName(entry.manifest.name)}</h4><p class="plugin-desc">{shortDescription(entry.manifest.description)}</p>
        <button class="details-link" onclick={() => catalogueId=group.key}>Voir les détails <Icon name="chevron-right" size={14} /></button>
        </div><div class="catalogue-actions">{#if group.entries.length>1}<label for={`extension-version-${group.key}`}>Version<select aria-label={`Version de ${entry.manifest.name}`} id={`extension-version-${group.key}`} bind:value={catalogueVersions[group.key]} disabled={busy}>{#each group.entries as option (option.manifest.version)}<option value={option.manifest.version}>{option.manifest.version}{option.compatible ? '' : ' · incompatible'}</option>{/each}</select></label>{:else}<span class="version-label">v{entry.manifest.version}</span>{/if}
        <button class="primary" aria-label={existing ? `${entry.manifest.name} déjà installée` : `Installer ${entry.manifest.name}`} disabled={busy || !entry.compatible || !!existing} onclick={() => installFromCatalogue(entry)}>{existing ? 'Déjà installée' : 'Installer'}</button></div>
      </article>
    {:else}<p>Aucune extension ne correspond. Ajoutez un dépôt ou modifiez votre recherche.</p>{/each}
    </div>
    {/if}
  {:else}
  {#if managed}
    {@const extension=managed}
    <div class="extension-detail">
      <div class="detail-top-nav">
        {#if settingsOpen}<button class="back-installed" onclick={()=>settingsOpen=false}><Icon name="chevron-left" size={16} /> Retour à la fiche</button>{:else}<button class="back-installed" onclick={()=>managedId=null}><Icon name="chevron-left" size={16} /> Toutes les extensions</button>{/if}
      </div>
      {#if settingsOpen}
        <div class="settings-screen">
          <h2 id="extensions-heading">Paramètres de {displayName(extension.manifest.name)}</h2>
          {#if extension.manifest.permissions.includes('metadata.read')}<p>La langue des métadonnées est définie dans les paramètres de chaque bibliothèque.</p>{/if}
          {#if settingsFields(extension.manifest).length}
            <ExtensionSettings fields={settingsFields(extension.manifest)} bind:values={editingSettings[extension.manifest.id]} {busy} onsave={()=>void saveSettings(extension.manifest.id)} />
          {:else}<p>Aucun paramètre à configurer.</p>{/if}
          {#if extension.manifest.permissions.includes('avatars')}<AvatarCollections extensionId={extension.manifest.id} />{/if}
        </div>
      {:else}
      <div class="detail-header-card">
        <div class="detail-header-main">
          <h2>{displayName(extension.manifest.name)}</h2>
          <p class="detail-description">{shortDescription(extension.manifest.description)}</p>

          <div class="revisions-section">
            <div class="revisions-heading">
              <h3>Historique des révisions</h3>
              {#if getUpdateEntry(extension)}
                {@const upd = getUpdateEntry(extension)!}
                <span class="badge update-badge">
                  Mise à jour v{upd.manifest.version} disponible
                </span>
              {/if}
            </div>

            <div class="revisions-list">
              {#each getRevisions(extension.manifest.id, extension.manifest) as rev (rev.version)}
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
                        <span class="badge success">Installée</span>
                      {:else if rev.isNewer}
                        <span class="badge update">Mise à jour</span>
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
                        <p class="revision-fallback-desc">{rev.manifest.description || 'Version publiée sans journal de modifications.'}</p>
                      {/if}

                      <div class="revision-specs">
                        {#if rev.manifest.targetAbi}
                          <span class="spec-tag">API Cible : <code>{rev.manifest.targetAbi}</code></span>
                        {/if}
                        {#if rev.manifest.minServerVersion}
                          <span class="spec-tag">Serveur requis : <code>{rev.manifest.minServerVersion}+</code></span>
                        {/if}
                      </div>

                      <div class="revision-action-row">
                        {#if rev.isCurrent}
                          <span class="installed-tag">
                            <Icon name="check" size={16} /> Version actuelle installée
                          </span>
                        {:else if rev.entry}
                          <button
                            type="button"
                            class="btn-revision-install"
                            class:btn-revision-update={rev.isNewer}
                            disabled={busy || !rev.compatible}
                            onclick={() => void switchVersion(rev.entry!)}
                          >
                            <Icon name="download" size={16} />
                            {rev.isNewer ? `Mettre à jour vers v${rev.version}` : `Installer v${rev.version}`}
                          </button>
                          {#if !rev.compatible}
                            <span class="incompatible-badge">Incompatible avec ce serveur</span>
                          {/if}
                        {:else}
                          <span class="local-only-text">Version locale (non disponible dans les dépôts)</span>
                        {/if}
                      </div>
                    </div>
                  {/if}
                </div>
              {:else}
                <p class="empty-revisions">Aucune révision disponible.</p>
              {/each}
            </div>
          </div>
        </div>
        <div class="detail-header-side">
          {@render artwork(extension.manifest)}
          <div class="detail-action-buttons">
            <label class="toggle-control">
              <input type="checkbox" role="switch" aria-label={`Activer ${displayName(extension.manifest.name)}`} checked={extension.enabled} disabled={busy} onchange={event=>void configure(extension,event.currentTarget.checked)} />
              <span>{extension.enabled ? 'Désactiver' : 'Activer'} le plugin</span>
            </label>
            <button class="btn-detail-settings" onclick={() => settingsOpen=true}>
              <Icon name="settings" size={18} />
              Paramètres
            </button>
            <button class="btn-detail-uninstall" disabled={busy} onclick={()=>void perform(async()=>{await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}`,{method:'DELETE',body:JSON.stringify({deleteData:false})});managedId=null;},'Extension désinstallée.')}>
              <Icon name="trash" size={18} />
              Désinstaller
            </button>
          </div>
          <div class="detail-meta-table">
            <div class="meta-row">
              <span class="meta-label">État</span>
              <span class="meta-value">{extension.enabled ? "Active" : "Désactivée"}</span>
            </div>
            <div class="meta-row">
              <span class="meta-label">Version</span>
              <span class="meta-value">{extension.manifest.version}</span>
            </div>
            <div class="meta-row">
              <span class="meta-label">Développeur</span>
              <span class="meta-value">{extension.manifest.author}</span>
            </div>
            {#if extension.manifest.repository}
              <div class="meta-row">
                <span class="meta-label">Dépôt</span>
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
  <div class="installed-filters"><label>Rechercher<input type="search" bind:value={search} /></label><label>État<select bind:value={filter}><option value="all">Toutes</option><option value="enabled">Activées</option><option value="disabled">Désactivées</option></select></label></div>
  <div class="plugins-grid">{#each visibleExtensions as extension (extension.manifest.id)}<article class="card plugin-card installed-card"><button class="artwork-button" aria-label={`Ouvrir ${displayName(extension.manifest.name)}`} onclick={()=>managedId=extension.manifest.id}>{@render artwork(extension.manifest)}</button><div class="plugin-header"><div class="plugin-title-row"><div class="installed-identity"><span class="extension-icon"><Icon name={extension.manifest.permissions.includes('avatars')?'profile':'globe'} size={22} /></span><div><h3>{displayName(extension.manifest.name)}</h3><small>v{extension.manifest.version}</small></div></div><span class="badge" class:success={extension.enabled}>{extension.enabled?'Activée':'Désactivée'}</span>{#if getUpdateEntry(extension)}<span class="badge update">Mise à jour</span>{/if}</div><p class="plugin-desc">{shortDescription(extension.manifest.description)}</p><button disabled={busy} onclick={()=>{managedId=extension.manifest.id;settingsOpen=true;}}>Paramètres de {displayName(extension.manifest.name)}</button></div></article>{:else}<p>Aucune extension installée.</p>{/each}</div>
  {/if}
  {/if}
</section>

<style>
  section{ min-width:0; }
.settings-screen{max-width:880px;padding:24px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:var(--radius-md);}
.settings-screen h2{margin-bottom:24px;}
h2,h3,h4,strong{ color:var(--text-primary); }
h2{ font-size:1.7rem;letter-spacing:-.03em;margin:0; }
h3{font-size:1.05rem;margin:0;}
h4{font-size:1.1rem;margin:0;}
p{line-height:1.55;color:var(--text-secondary);}
small{color:var(--text-secondary);line-height:1.5;}
.heading,.actions,.section-title{display:flex;align-items:center;gap:12px;flex-wrap:wrap;}
.heading{justify-content:space-between;}
.section-title{margin-top:24px;}
.extension-tabs{display:flex;gap:4px;border-bottom:1px solid var(--border-subtle);margin:12px 0 24px;}
.extension-tabs button{border:0;border-bottom:2px solid transparent;border-radius:0;background:transparent;padding:12px 16px;}
.extension-tabs button.active{border-color:var(--accent-primary);color:var(--accent-primary);}
button{display:inline-flex;align-items:center;justify-content:center;gap:7px;padding:9px 13px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-primary);cursor:pointer;font:inherit;font-size:13px;min-height:38px;}
button:hover:not(:disabled){border-color:var(--accent-primary);}
button:focus-visible{outline:2px solid var(--accent-primary);outline-offset:3px;}
button:disabled{opacity:.45;cursor:default;}
button.primary{background:var(--color-accent-solid);color:white;border-color:transparent;}
form{display:grid;gap:14px;margin:16px 0;}
label{display:grid;gap:6px;color:var(--text-secondary);font-size:13px;}
input:not([type=checkbox]),select{width:100%;min-width:0;box-sizing:border-box;padding:10px 12px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-primary);font:inherit;}
input[type=checkbox]{accent-color:var(--accent-primary);}
.catalogue-search{max-width:420px;margin:20px 0 8px;}
.section-help{font-size:13px;margin:12px 0 22px;}
.plugins-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,240px),1fr));gap:16px;align-items:start;}
.card{border:1px solid var(--border-subtle);border-radius:var(--radius-md);background:var(--bg-surface);min-width:0;}
.plugin-card{overflow:hidden;}
.catalogue-identity{display:flex;align-items:center;justify-content:space-between;padding:20px 20px 0;}
.extension-icon{width:46px;height:46px;display:inline-flex;align-items:center;justify-content:center;background:var(--bg-elevated);border:1px solid var(--border-subtle);border-radius:var(--radius-md);color:var(--accent-primary);flex-shrink:0;}
.badge{font-size:11px;background:var(--bg-elevated);padding:4px 8px;border-radius:var(--radius-sm);color:var(--text-secondary);white-space:nowrap;}
.badge.success{color:var(--status-success);}
.catalogue-content{padding:18px 20px 0;flex:1;}
.catalogue-card{display:flex;flex-direction:column;height:100%;box-sizing:border-box;}
.plugin-desc{font-size:13px;margin:10px 0;}
.catalogue-card .plugin-desc{min-height:4.6em;}
.catalogue-actions{display:flex;align-items:end;gap:12px;padding:16px 20px 20px;justify-content:space-between;border-top:1px solid var(--border-subtle);margin-top:16px;}
.catalogue-actions label{min-width:75px;max-width:110px;}
.catalogue-actions button{flex:1;}
.catalogue-pending{display:flex;gap:12px;padding:18px;background:var(--bg-elevated);border:1px solid var(--border-subtle);border-radius:var(--radius-md);margin:20px 0;}
.catalogue-pending p{font-size:13px;margin:5px 0 0;}
.repository-form{max-width:760px;grid-template-columns:1fr 2fr auto;align-items:end;padding:20px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:var(--radius-md);}
.repository-card{padding:20px;margin:16px 0;}
.repository-heading{display:flex;align-items:center;gap:12px;margin-bottom:14px;}
.repository-heading p{font-size:12px;margin:4px 0;}
.repository-heading .badge{margin-left:auto;}
.repository-url{display:block;overflow-wrap:anywhere;white-space:normal;font-size:12px;color:var(--text-secondary);}
.repository-card .actions{margin:16px 0;}
.installed-identity{display:flex;align-items:center;gap:12px;min-width:0;}
.plugin-header{padding:20px 20px 0;}
.plugin-title-row{display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;}
code{overflow-wrap:anywhere;white-space:normal;}
[role=alert],[role=status]{padding:12px 16px;border-radius:var(--radius-sm);background:var(--bg-elevated);font-size:13px;}
[role=alert]{border-left:3px solid var(--status-error);}
[role=status]{color:var(--text-secondary);}

  @media(max-width:650px){.repository-form{grid-template-columns:1fr;}
.heading{align-items:start;}
.extension-tabs button{padding:12px 10px;font-size:12px;}
.catalogue-actions{padding:14px 16px;}
.plugin-header,.catalogue-content{padding-inline:16px;}
.repository-heading{flex-wrap:wrap;}
}
.installed-filters{display:flex;gap:12px;flex-wrap:wrap;margin:20px 0;}
.installed-filters label:first-child{flex:1;min-width:160px;}
.version-label{font-size:12px;color:var(--text-secondary);white-space:nowrap;}
.catalogue-card .plugin-desc{min-height:3em;}
.extension-detail{margin-top:12px;}
.detail-top-nav{margin-bottom:16px;}
.back-installed{display:inline-flex;align-items:center;gap:8px;background:transparent;border:0;padding:8px 0;color:var(--text-secondary);cursor:pointer;}
.detail-header-card{display:grid;grid-template-columns:minmax(0,1fr) minmax(320px,360px);gap:40px;align-items:start;margin-bottom:32px;}
.detail-header-main h2{font-size:32px;letter-spacing:-.02em;margin:0 0 16px;}
.detail-description{font-size:15px;line-height:1.6;color:var(--text-secondary);margin:0;max-width:680px;}
.detail-header-side{display:flex;flex-direction:column;gap:16px;}
.detail-header-side .extension-artwork{border-radius:var(--radius-md);box-shadow:0 4px 20px rgba(0,0,0,0.25);}
.detail-action-buttons{display:flex;flex-direction:column;gap:10px;margin-top:4px;}
.toggle-control{display:flex;align-items:center;gap:12px;font-size:14px;font-weight:500;padding:4px 0;cursor:pointer;}
.toggle-control input{appearance:none;width:42px;height:24px;flex:0 0 42px;border-radius:20px;position:relative;background:var(--text-muted,#555);cursor:pointer;margin:0;}
.toggle-control input::after{content:'';position:absolute;left:3px;top:3px;width:18px;height:18px;border-radius:50%;background:white;transition:transform .15s;}
.toggle-control input:checked{background:var(--accent-primary,#00a4dc);}
.toggle-control input:checked::after{transform:translateX(18px);}
.btn-detail-settings{display:flex;align-items:center;justify-content:center;gap:8px;width:100%;min-height:42px;padding:10px 16px;border:0;border-radius:var(--radius-sm);background:#00a4dc;color:#ffffff;font-weight:600;font-size:14px;cursor:pointer;box-sizing:border-box;}
.btn-detail-settings:hover{background:#0093c7;}
.btn-detail-uninstall{display:flex;align-items:center;justify-content:center;gap:8px;width:100%;min-height:42px;padding:10px 16px;border:0;border-radius:var(--radius-sm);background:#c33;color:#ffffff;font-weight:600;font-size:14px;cursor:pointer;box-sizing:border-box;}
.btn-detail-uninstall:hover{background:#b22;}
.detail-meta-table{background:rgba(255,255,255,0.03);border:1px solid var(--border-subtle);border-radius:var(--radius-md);padding:12px 16px;display:flex;flex-direction:column;gap:8px;}
.meta-row{display:flex;justify-content:space-between;align-items:center;font-size:13px;padding:6px 0;border-bottom:1px solid rgba(255,255,255,0.06);}
.meta-row:last-child{border-bottom:0;}
.meta-label{color:var(--text-secondary);}
.meta-value{color:var(--text-primary);font-weight:500;}
.meta-value a{color:#00a4dc;text-decoration:none;}
.meta-value a:hover{text-decoration:underline;}
.repository-warning{font-size:13px;color:var(--text-secondary);padding:14px 0;}

@media(max-width:900px){.detail-header-card{grid-template-columns:1fr;gap:24px;}

}
.details-link{background:transparent;border:0;padding:6px 0;color:var(--accent-primary);}
.page-intro{margin:8px 0 26px;font-size:14px;}
.extension-tabs{gap:8px;align-items:center;flex-wrap:wrap;padding-bottom:18px;margin-bottom:18px;}
.extension-tabs button{border:1px solid transparent;border-radius:24px;background:var(--bg-elevated);padding:10px 20px;}
.extension-tabs button.active,.category-filters .chosen{background:var(--color-accent-solid);color:white;border-color:transparent;}
.category-filters{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:20px;}
.category-filters button{border-radius:24px;padding:8px 16px;}
.plugins-grid{grid-template-columns:repeat(auto-fill,minmax(min(100%,270px),1fr));gap:24px;align-items:stretch;}
.plugin-card{border:1px solid var(--border-subtle);border-radius:8px;transition:transform .18s,border-color .18s;}
.plugin-card:focus-within,.plugin-card:hover{border-color:var(--accent-primary);transform:translateY(-3px);}
.artwork-button{display:block;width:100%;padding:0;border:0;border-radius:0;background:transparent;text-align:center;height:auto;}
.extension-artwork{box-sizing:border-box;position:relative;isolation:isolate;overflow:hidden;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:14px;width:100%;aspect-ratio:16/9;padding:24px;background:linear-gradient(135deg,hsl(var(--art-hue) 35% 23%),hsl(var(--art-hue) 42% 9%));color:#fff;}
.extension-artwork::before{content:'';position:absolute;z-index:-1;width:220px;height:220px;border:1px solid #ffffff15;border-radius:50%;right:-65px;top:-100px;box-shadow:0 0 0 35px #ffffff04,0 0 0 70px #ffffff03;}
.extension-artwork.has-banner{padding:0;background:#141414;border-radius:inherit;}
.extension-artwork .banner-img{width:100%;height:100%;object-fit:contain;display:block;min-height:0;}
.extension-artwork strong{color:white;font-size:22px;line-height:1.2;text-wrap:balance;}
.art-symbol{color:hsl(var(--art-hue) 75% 78%);}
.art-category{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#ffffffa8;}
.catalogue-identity{padding:14px 20px 0;min-height:22px;}
.catalogue-identity>.extension-icon{display:none;}
.catalogue-content{padding-top:10px;}
.catalogue-actions{margin-top:auto;}
.plugin-header{padding-bottom:20px;}
.installed-identity>.extension-icon{display:none;}
.installed-card .plugin-header button{width:100%;margin-top:10px;}
.plugin-desc{display:-webkit-box;-webkit-line-clamp:2;line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;}
.repository-card{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:8px 24px;border:0;border-radius:0;border-bottom:1px solid var(--border-subtle);margin:0;padding:24px;}
.repository-heading{grid-column:1;margin:0;}
.repository-url{grid-column:1;padding-left:58px;}
.repository-card .actions{grid-column:2;grid-row:1 / 3;margin:0;}
.repository-card>small{grid-column:1 / -1;padding-left:58px;}
.section-title{justify-content:space-between;margin-bottom:20px;}
.repository-form{max-width:none;}
.catalogue-detail{display:grid;grid-template-columns:minmax(0,1fr) minmax(280px,36%);gap:48px;margin-top:16px;}
.catalogue-detail h3{font-size:30px;}
.catalogue-detail h4{margin:32px 0 16px;}
.catalogue-detail select{max-width:320px;}
.catalogue-detail aside>button{width:100%;}
.catalogue-detail aside{min-width:0;}
.catalogue-detail .extension-artwork{border-radius:6px;}
dl{margin:20px 0;}
dl>div{display:grid;grid-template-columns:100px minmax(0,1fr);gap:12px;padding:16px 0;border-bottom:1px solid var(--border-subtle);font-size:13px;}
dt{color:var(--text-secondary);}
dd{margin:0;overflow-wrap:anywhere;color:var(--text-primary);}

  @media(prefers-reduced-motion:reduce){.plugin-card{transition:none;}
.plugin-card:hover,.plugin-card:focus-within{transform:none;}
}

  @media(max-width:700px){.plugins-grid{gap:16px;}
.catalogue-detail{grid-template-columns:1fr;gap:24px;}
.catalogue-detail aside{grid-row:1;}
.repository-card{grid-template-columns:1fr;padding:18px;}
.repository-card .actions{grid-column:1;grid-row:auto;justify-content:flex-start;}
.repository-url,.repository-card>small{padding-left:0;}
.extension-tabs button{padding:10px 14px;}
.catalogue-search{max-width:none;}
}


.revisions-section{margin-top:28px;border-top:1px solid var(--border-subtle);padding-top:20px;}
.revisions-heading{display:flex;align-items:center;justify-content:space-between;margin-bottom:14px;flex-wrap:wrap;gap:8px;}
.revisions-heading h3{font-size:1.15rem;font-weight:600;color:var(--text-primary);margin:0;}
.update-badge{background:rgba(0,164,220,0.15);color:#00a4dc;border:1px solid rgba(0,164,220,0.35);font-size:12px;padding:3px 10px;border-radius:12px;}
.revisions-list{display:flex;flex-direction:column;gap:8px;}
.revision-item{background:rgba(255,255,255,0.02);border:1px solid var(--border-subtle);border-radius:var(--radius-md);overflow:hidden;transition:border-color .15s;}
.revision-item.current{border-color:rgba(46,204,113,0.35);}
.revision-item.has-update{border-color:rgba(0,164,220,0.45);}
.revision-header{display:flex;justify-content:space-between;align-items:center;width:100%;padding:12px 16px;background:transparent;border:0;cursor:pointer;color:var(--text-primary);text-align:left;}
.revision-header:hover{background:rgba(255,255,255,0.03);}
.revision-title-row{display:flex;align-items:center;gap:10px;flex-wrap:wrap;}
.revision-version{font-weight:600;font-size:14px;}
.revision-date{font-size:12px;color:var(--text-secondary);}
.revision-chevron{display:inline-flex;align-items:center;transition:transform .2s ease;color:var(--text-secondary);}
.revision-chevron.rotated{transform:rotate(180deg);}
.revision-content{padding:0 16px 16px;border-top:1px solid rgba(255,255,255,0.04);}
.revision-version-tag{font-size:14px;font-weight:700;color:var(--text-primary);margin:12px 0 8px;}
.revision-changelog{display:flex;flex-direction:column;gap:6px;margin:8px 0 12px;}
.changelog-item{display:flex;gap:8px;font-size:13px;line-height:1.5;color:var(--text-secondary);}
.bullet{color:#00a4dc;flex-shrink:0;}
.changelog-p{font-size:13px;line-height:1.5;color:var(--text-secondary);margin:4px 0;}
.revision-fallback-desc{font-size:13px;color:var(--text-secondary);line-height:1.5;margin:8px 0 12px;}
.revision-specs{display:flex;gap:12px;margin-bottom:12px;flex-wrap:wrap;}
.spec-tag{font-size:12px;color:var(--text-secondary);}
.spec-tag code{background:rgba(255,255,255,0.06);padding:2px 6px;border-radius:4px;font-size:11px;}
.revision-action-row{display:flex;align-items:center;gap:12px;margin-top:8px;}
.btn-revision-install{display:inline-flex;align-items:center;gap:8px;padding:8px 16px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-primary);font-size:13px;font-weight:600;cursor:pointer;}
.btn-revision-install:hover:not(:disabled){border-color:#00a4dc;}
.btn-revision-update{background:#00a4dc;color:#fff;border-color:#00a4dc;}
.btn-revision-update:hover:not(:disabled){background:#0093c7;}
.installed-tag{display:inline-flex;align-items:center;gap:6px;font-size:13px;font-weight:500;color:#2ecc71;}
.badge.update{background:rgba(0,164,220,0.15);color:#00a4dc;border:1px solid rgba(0,164,220,0.35);font-size:11px;padding:2px 8px;border-radius:12px;}
.incompatible-badge{font-size:12px;color:#e74c3c;}
.local-only-text,.empty-revisions{font-size:13px;color:var(--text-secondary);}

</style>

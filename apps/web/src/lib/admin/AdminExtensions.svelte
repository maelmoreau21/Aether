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
  type Installed = { manifest: Manifest; publisherId: string; sha256: string; enabled: boolean; libraryIds: string[]; allLibraries?: boolean; settings?: ExtensionSettings; rollbackVersion?: string | null };
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
  let rollbackConfirmedFor = $state('');
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
  async function rollback(extension: Installed) {
    if (rollbackConfirmedFor !== extension.manifest.id) return;
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}/rollback`, {
        method: 'POST', body: JSON.stringify({ expectedSha256: extension.sha256, expectedVersion: extension.manifest.version, confirmDataRestore: true })
      });
      delete editingSettings[extension.manifest.id];
      rollbackConfirmedFor = '';
    }, 'Version et données précédentes restaurées. Vérifiez les paramètres puis réactivez l’extension.');
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
              {#if extension.rollbackVersion}
                <div class="revision-item">
                  <p>Revenir à v{extension.rollbackVersion} restaure les paramètres et les données privées sauvegardés avant la dernière mise à jour. Les changements ultérieurs seront perdus et l’extension sera désactivée.</p>
                  <label><input type="checkbox" checked={rollbackConfirmedFor === extension.manifest.id} onchange={(event) => rollbackConfirmedFor = event.currentTarget.checked ? extension.manifest.id : ''} /> Je confirme la restauration des données précédentes.</label>
                  <button type="button" disabled={busy || rollbackConfirmedFor !== extension.manifest.id} onclick={() => void rollback(extension)}>Restaurer v{extension.rollbackVersion}</button>
                </div>
              {/if}
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
  {#if updatableExtensions.length > 0}
    <div class="update-all-banner" role="region" aria-label="Mises à jour disponibles">
      <div class="update-all-info">
        <span class="badge update">Mises à jour</span>
        <strong>{updatableExtensions.length} extension{updatableExtensions.length > 1 ? 's ont' : ' a'} une nouvelle version disponible.</strong>
      </div>
      <button class="primary btn-update-all" disabled={busy} onclick={updateAllExtensions}>
        <Icon name="download" size={16} /> Tout mettre à jour ({updatableExtensions.length})
      </button>
    </div>
  {/if}
  <div class="installed-filters"><label>Rechercher<input type="search" bind:value={search} /></label><label>État<select bind:value={filter}><option value="all">Toutes</option><option value="enabled">Activées</option><option value="disabled">Désactivées</option></select></label></div>
  <div class="plugins-grid">{#each visibleExtensions as extension (extension.manifest.id)}<article class="card plugin-card installed-card"><button class="artwork-button" aria-label={`Ouvrir ${displayName(extension.manifest.name)}`} onclick={()=>managedId=extension.manifest.id}>{@render artwork(extension.manifest)}</button><div class="plugin-header"><div class="plugin-title-row"><div class="installed-identity"><span class="extension-icon"><Icon name={extension.manifest.permissions.includes('avatars')?'profile':'globe'} size={22} /></span><div><h3>{displayName(extension.manifest.name)}</h3><small>v{extension.manifest.version}</small></div></div><span class="badge" class:success={extension.enabled}>{extension.enabled?'Activée':'Désactivée'}</span>{#if getUpdateEntry(extension)}<span class="badge update">Mise à jour</span>{/if}</div><p class="plugin-desc">{shortDescription(extension.manifest.description)}</p>
        <div class="installed-card-actions">
          {#if getUpdateEntry(extension)}
            <button class="primary btn-card-update" disabled={busy} onclick={() => switchVersion(getUpdateEntry(extension)!)}>
              <Icon name="download" size={14} /> Mettre à jour v{getUpdateEntry(extension)!.manifest.version}
            </button>
          {/if}
          <button disabled={busy} onclick={()=>{managedId=extension.manifest.id;settingsOpen=true;}}>Paramètres de {displayName(extension.manifest.name)}</button>
        </div></div></article>{:else}<p>Aucune extension installée.</p>{/each}</div>
  {/if}
  {/if}
</section>

<style>
  section { min-width: 0; color: var(--tx); font-family: var(--font); }

  /* Typographie */
  h2, h3, h4, strong { color: var(--tx); }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; }
  h4 { margin: 0; font-size: 17px; font-weight: 700; }
  p { line-height: 1.55; color: var(--mu); }
  small { color: var(--mu); font-size: 13px; line-height: 1.5; }
  code { font-family: var(--font-mono); overflow-wrap: anywhere; white-space: normal; }

  /* Boutons : secondaire par défaut, principal = encre inversée */
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font-family: var(--font);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button.primary { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); font-size: 15px; box-shadow: none; }
  button.primary:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }

  /* Champs */
  form { display: grid; gap: 16px; margin: 16px 0; }
  label { display: grid; gap: 8px; color: var(--tx); font-size: 14px; font-weight: 700; }
  input:not([type='checkbox']), select {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    height: 46px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background-color: var(--pn);
    color: var(--tx);
    font-family: var(--font);
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }
  select { padding-right: 40px; }
  input:not([type='checkbox']):hover, select:hover { background-color: var(--pn-2); }
  input:not([type='checkbox']):focus, select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  input[type='checkbox'] { width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }

  /* En-tête de page */
  .heading, .actions, .section-title { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .heading { justify-content: space-between; }
  .page-intro { margin: 6px 0 20px; font-size: 15px; }

  /* Onglets de sous-section */
  .extension-tabs { display: flex; gap: 24px; margin: 0 0 20px; border-bottom: 1px solid var(--line); overflow-x: auto; scrollbar-width: none; }
  .extension-tabs::-webkit-scrollbar { display: none; }
  .extension-tabs button { flex: none; height: 46px; min-height: 46px; margin-bottom: -1px; padding: 0; border-bottom: 2px solid transparent; border-radius: 0; background: transparent; color: var(--mu); white-space: nowrap; }
  .extension-tabs button:hover:not(:disabled) { background: transparent; color: var(--tx); }
  .extension-tabs button:active:not(:disabled) { transform: none; }
  .extension-tabs button.active { border-bottom-color: var(--ac); color: var(--tx); }
  .extension-tabs button span { display: inline-grid; place-items: center; min-width: 22px; height: 22px; padding: 0 6px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--mu); font-size: 12px; font-variant-numeric: tabular-nums; }

  /* Pastilles de catégorie */
  .category-filters { display: flex; gap: 8px; margin-bottom: 20px; overflow-x: auto; scrollbar-width: none; padding-bottom: 2px; }
  .category-filters::-webkit-scrollbar { display: none; }
  .category-filters button { flex: none; height: 38px; min-height: 38px; padding: 0 14px; border-radius: var(--r-pill); background: var(--pn); font-weight: 600; white-space: nowrap; }
  .category-filters button:hover:not(:disabled) { background: var(--pn-2); }
  .category-filters .chosen, .category-filters .chosen:hover:not(:disabled) { background: var(--ac-soft); color: var(--ac); }

  /* Retours */
  [role='alert'], [role='status'] { margin: 0 0 16px; padding: 14px 16px; border-radius: var(--r-md); font-size: 14px; line-height: 1.5; }
  [role='alert'] { background: var(--err-soft); color: var(--tx); }
  [role='status'] { background: var(--pn); color: var(--mu); }
  .repository-warning { margin: 0 0 20px; padding: 14px 16px; border-radius: var(--r-md); background: var(--warn-soft); color: var(--tx); font-size: 14px; }
  .catalogue-pending { display: flex; gap: 12px; margin: 0 0 20px; padding: 14px 16px; border-radius: var(--r-md); background: var(--info-soft); color: var(--tx); font-size: 14px; }
  .catalogue-pending :global(svg) { flex: none; color: var(--info); }
  .catalogue-pending p { margin: 4px 0 0; font-size: 13px; }
  .section-help { margin: 8px 0 20px; font-size: 13px; }
  .update-all-banner { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; margin: 0 0 20px; padding: 14px 16px; border-radius: var(--r-md); background: var(--info-soft); }
  .update-all-info { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; font-size: 14px; }

  /* Badges */
  .badge { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 9px; border: 0; border-radius: var(--r-xs); background: var(--pn-2); color: var(--mu); font-size: 12px; font-weight: 700; white-space: nowrap; }
  .badge.success { background: var(--ok-soft); color: var(--ok); }
  .badge.update, .update-badge { background: var(--info-soft); color: var(--info); }

  /* Dépôts */
  .section-title { justify-content: space-between; margin: 0 0 16px; }
  .repository-form { grid-template-columns: minmax(0, 1fr) minmax(0, 2fr) auto; align-items: end; padding: 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .card { min-width: 0; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .repository-card { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px 24px; margin: 0 0 12px; padding: 20px 24px; }
  .repository-heading { grid-column: 1; display: flex; align-items: center; gap: 12px; margin: 0; }
  .repository-heading > div { min-width: 0; }
  .repository-heading p { margin: 4px 0 0; font-size: 13px; }
  .repository-heading .badge { margin-left: auto; }
  .extension-icon { display: inline-grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: 14px; background: var(--pn); color: var(--tx); }
  .repository-url { grid-column: 1; display: block; padding-left: 56px; font-size: 12px; color: var(--mu); overflow-wrap: anywhere; }
  .repository-card .actions { grid-column: 2; grid-row: 1 / 3; margin: 0; justify-content: flex-end; }
  .repository-card > small { grid-column: 1 / -1; padding-left: 56px; }

  /* Catalogue */
  .catalogue-search { max-width: 420px; margin: 0 0 8px; }
  .back-installed { min-height: 44px; padding: 0 12px 0 6px; background: transparent; color: var(--mu); }
  .back-installed:hover:not(:disabled) { background: var(--pn); color: var(--tx); }
  .catalogue-detail { display: grid; grid-template-columns: minmax(0, 1fr) minmax(280px, 36%); gap: 40px; margin-top: 16px; }
  .catalogue-detail h3 { font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  .catalogue-detail h4 { margin: 28px 0 12px; }
  .catalogue-detail select { max-width: 320px; }
  .catalogue-detail aside { min-width: 0; }
  .catalogue-detail aside > button { width: 100%; }
  .catalogue-detail .extension-artwork { border-radius: var(--r-lg); }
  dl { margin: 20px 0; }
  dl > div { display: grid; grid-template-columns: 110px minmax(0, 1fr); gap: 12px; padding: 14px 0; border-bottom: 1px solid var(--line); font-size: 14px; }
  dt { color: var(--mu); }
  dd { margin: 0; color: var(--tx); font-weight: 600; overflow-wrap: anywhere; }

  /* Grille d'extensions : l'image est la carte */
  .plugins-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 270px), 1fr)); gap: 20px; align-items: stretch; }
  .plugin-card { display: flex; flex-direction: column; overflow: hidden; transition: border-color var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  .plugin-card:hover, .plugin-card:focus-within { border-color: var(--line-2); }
  .artwork-button { display: block; width: 100%; height: auto; min-height: 0; padding: 0; border-radius: 0; background: transparent; text-align: center; }
  .artwork-button:hover:not(:disabled) { background: transparent; }
  .artwork-button:active:not(:disabled) { transform: none; }
  .artwork-button:focus-visible { outline-offset: -3px; }

  /* Visuel de repli : couleur stable dérivée de l'identifiant, texte toujours clair */
  .extension-artwork { box-sizing: border-box; position: relative; overflow: hidden; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; width: 100%; aspect-ratio: 16 / 9; padding: 24px; background: hsl(var(--art-hue) 30% 18%); color: var(--on-media); }
  .extension-artwork.has-banner { padding: 0; background: var(--pn); border-radius: inherit; }
  .extension-artwork .banner-img { display: block; width: 100%; height: 100%; min-height: 0; object-fit: contain; }
  .extension-artwork strong { color: var(--on-media); font-size: 20px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.2; text-wrap: balance; }
  .art-symbol { display: grid; color: var(--on-media); opacity: 0.7; }
  .art-category { color: var(--on-media); opacity: 0.75; font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; }

  .catalogue-card { height: 100%; box-sizing: border-box; }
  .catalogue-identity { display: flex; align-items: center; justify-content: space-between; min-height: 24px; padding: 16px 20px 0; }
  .catalogue-identity > .extension-icon, .installed-identity > .extension-icon { display: none; }
  .catalogue-content { flex: 1; padding: 8px 20px 0; }
  .plugin-desc { display: -webkit-box; margin: 8px 0; overflow: hidden; font-size: 14px; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .catalogue-card .plugin-desc { min-height: 3em; }
  .details-link { min-height: 44px; padding: 0; background: transparent; color: var(--tx); }
  .details-link:hover:not(:disabled) { background: transparent; color: var(--mu); }
  .catalogue-actions { display: flex; align-items: end; justify-content: space-between; gap: 12px; margin-top: auto; padding: 16px 20px 20px; border-top: 1px solid var(--line); }
  .catalogue-actions label { min-width: 80px; max-width: 120px; font-size: 13px; }
  .catalogue-actions button { flex: 1; }
  .version-label { align-self: center; font-family: var(--font-mono); font-size: 12px; color: var(--mu); white-space: nowrap; }

  /* Installées */
  .installed-filters { display: flex; flex-wrap: wrap; gap: 12px; margin: 0 0 20px; }
  .installed-filters label:first-child { flex: 1; min-width: 180px; }
  .installed-filters label:last-child { min-width: 180px; }
  .plugin-header { display: flex; flex-direction: column; flex: 1; padding: 16px 20px 20px; }
  .plugin-title-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .installed-identity { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .installed-identity small { font-family: var(--font-mono); font-size: 12px; }
  .installed-card-actions { display: flex; flex-direction: column; gap: 8px; margin-top: auto; padding-top: 8px; }
  .installed-card .plugin-header button { width: 100%; }
  .btn-card-update { width: 100%; }

  /* Fiche d'une extension */
  .extension-detail { margin-top: 4px; }
  .detail-top-nav { margin-bottom: 16px; }
  .settings-screen { max-width: 880px; padding: 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .settings-screen h2 { margin-bottom: 20px; }
  .detail-header-card { display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 360px); gap: 40px; align-items: start; margin-bottom: 32px; }
  .detail-header-main h2 { margin: 0 0 12px; font-size: clamp(26px, 3vw, 32px); letter-spacing: -0.03em; }
  .detail-description { max-width: 680px; margin: 0; font-size: 15px; line-height: 1.6; color: var(--mu); }
  .detail-header-side { display: flex; flex-direction: column; gap: 16px; }
  .detail-header-side .extension-artwork { border-radius: var(--r-lg); }
  .detail-action-buttons { display: flex; flex-direction: column; gap: 8px; }

  /* Interrupteur */
  .toggle-control { display: flex; align-items: center; gap: 12px; min-height: 44px; font-size: 14px; font-weight: 600; cursor: pointer; }
  .toggle-control input[type='checkbox'] { appearance: none; -webkit-appearance: none; position: relative; flex: 0 0 46px; width: 46px; height: 28px; margin: 0; border-radius: var(--r-pill); background: var(--pn-3); cursor: pointer; transition: background-color var(--t-med) ease; }
  .toggle-control input[type='checkbox']::after { content: ''; position: absolute; top: 3px; left: 3px; width: 22px; height: 22px; border-radius: 50%; background: var(--tx); transition: transform var(--t-med) var(--ease), background-color var(--t-med) ease; }
  .toggle-control input[type='checkbox']:checked { background: var(--ac); }
  .toggle-control input[type='checkbox']:checked::after { transform: translateX(18px); background: var(--on-media); }
  .toggle-control input[type='checkbox']:disabled { opacity: 0.45; cursor: not-allowed; }

  .btn-detail-settings, .btn-detail-uninstall { width: 100%; height: 48px; box-sizing: border-box; }
  .btn-detail-settings { background: var(--btn); color: var(--btn-tx); }
  .btn-detail-settings:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  .btn-detail-uninstall { background: var(--err-soft); color: var(--err); }
  .btn-detail-uninstall:hover:not(:disabled) { background: var(--err-soft); box-shadow: inset 0 0 0 1px currentColor; }

  .detail-meta-table { display: flex; flex-direction: column; padding: 8px 20px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .meta-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--line); font-size: 14px; }
  .meta-row:last-child { border-bottom: 0; }
  .meta-label { color: var(--mu); }
  .meta-value { min-width: 0; color: var(--tx); font-weight: 600; text-align: right; overflow-wrap: anywhere; }
  .meta-value a { color: var(--tx); text-decoration: underline; text-decoration-color: var(--line-2); text-underline-offset: 3px; }
  .meta-value a:hover { text-decoration-color: currentColor; }
  .meta-value a:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; border-radius: 4px; }

  /* Révisions */
  .revisions-section { margin-top: 28px; padding-top: 24px; border-top: 1px solid var(--line); }
  .revisions-heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; margin-bottom: 14px; }
  .revisions-list { display: flex; flex-direction: column; gap: 8px; }
  .revision-item { overflow: hidden; border: 1px solid var(--line); border-radius: var(--r-md); background: var(--pn); transition: border-color var(--t-fast) ease; }
  .revision-item.current, .revision-item.has-update { border-color: var(--line-2); }
  .revision-item > p { margin: 0; padding: 14px 16px 4px; font-size: 14px; }
  .revision-item > label { display: flex; align-items: center; gap: 10px; min-height: 44px; padding: 0 16px; font-weight: 600; }
  .revision-item > label + button { margin: 4px 16px 16px; }
  .revision-header { justify-content: space-between; width: 100%; min-height: 52px; padding: 0 16px; border-radius: 0; background: transparent; color: var(--tx); text-align: left; }
  .revision-header:hover:not(:disabled) { background: var(--pn); }
  .revision-header:active:not(:disabled) { transform: none; }
  .revision-header:focus-visible { outline-offset: -2px; }
  .revision-title-row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .revision-version { font-family: var(--font-mono); font-size: 14px; font-weight: 700; }
  .revision-date { font-size: 13px; font-weight: 500; color: var(--mu); }
  .revision-chevron { display: inline-flex; align-items: center; color: var(--mu); transition: transform var(--t-med) var(--ease); }
  .revision-chevron.rotated { transform: rotate(180deg); }
  .revision-content { padding: 0 16px 16px; border-top: 1px solid var(--line); }
  .revision-version-tag { margin: 14px 0 8px; font-family: var(--font-mono); font-size: 14px; }
  .revision-changelog { display: flex; flex-direction: column; gap: 6px; margin: 8px 0 12px; }
  .changelog-item { display: flex; gap: 8px; font-size: 14px; line-height: 1.5; color: var(--mu); }
  .bullet { flex: none; color: var(--mu-2); }
  .changelog-p, .revision-fallback-desc { margin: 4px 0 12px; font-size: 14px; line-height: 1.5; color: var(--mu); }
  .revision-specs { display: flex; flex-wrap: wrap; gap: 12px; margin-bottom: 12px; }
  .spec-tag { font-size: 13px; color: var(--mu); }
  .spec-tag code { padding: 2px 6px; border-radius: 6px; background: var(--pn-2); color: var(--tx); font-size: 12px; }
  .revision-action-row { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-top: 8px; }
  .btn-revision-update { background: var(--btn); color: var(--btn-tx); }
  .btn-revision-update:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  .installed-tag { display: inline-flex; align-items: center; gap: 6px; font-size: 14px; font-weight: 600; color: var(--ok); }
  .incompatible-badge { display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--err-soft); color: var(--err); font-size: 12px; font-weight: 700; }
  .local-only-text, .empty-revisions { font-size: 14px; color: var(--mu); }

  @media (max-width: 900px) {
    .detail-header-card { grid-template-columns: minmax(0, 1fr); gap: 24px; }
  }

  @media (max-width: 700px) {
    .plugins-grid { grid-template-columns: minmax(0, 1fr); gap: 16px; }
    .catalogue-detail { grid-template-columns: minmax(0, 1fr); gap: 24px; }
    .catalogue-detail aside { grid-row: 1; }
    .repository-form { grid-template-columns: minmax(0, 1fr); padding: 16px; }
    .repository-card { grid-template-columns: minmax(0, 1fr); padding: 16px; }
    .repository-card .actions { grid-column: 1; grid-row: auto; justify-content: flex-start; }
    .repository-card .actions button { flex: 1 1 160px; }
    .repository-heading { flex-wrap: wrap; }
    .repository-url, .repository-card > small { padding-left: 0; }
    .catalogue-search { max-width: none; }
    .heading { align-items: flex-start; }
    .settings-screen { padding: 16px; }
    .catalogue-actions { padding: 14px 16px 16px; }
    .plugin-header, .catalogue-content { padding-inline: 16px; }
    .catalogue-identity { padding-inline: 16px; }
    .detail-meta-table { padding: 4px 16px; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button, .plugin-card, .revision-chevron, .toggle-control input[type='checkbox'], .toggle-control input[type='checkbox']::after { transition: none; }
  }
</style>

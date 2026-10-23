<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import { SvelteMap } from 'svelte/reactivity';
  type ConfigOption = { label: string; value: string };
  type ConfigField = {
    key: string;
    label: string;
    description?: string;
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
    configSchema?: ConfigField[];
  };
  type SettingValue = string | number | boolean;
  type ExtensionSettings = Record<string, SettingValue>;
  type Installed = { manifest: Manifest; publisherId: string; sha256: string; enabled: boolean; libraryIds: string[]; settings?: ExtensionSettings };
  type Repository = { id: string; name: string; url: string; refreshedAt: number };
  type CatalogueEntry = { repositoryId: string; repositoryName: string; compatible: boolean; manifest: Manifest; publisherId: string; publicKey: string };
  let tab = $state<'installed' | 'catalogue' | 'repositories'>('catalogue');
  let managedId = $state<string | null>(null);
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
  type Library = { id: string; name: string; enabled: boolean | number };
  type Run = { id: string; extensionId: string; workId: string; status: string; durationMs: number; result: { title?: string; summary?: string; error?: string; pluginError?: string; operation?: string; output?: Record<string, unknown> } };
  type Candidate = { id: string; title: string; year?: number; overview?: string };
  type ProviderImage = { url: string; imageType: string; language?: string; width?: number; height?: number };
  type Proposal = { workId: string; value: Record<string, unknown> };
  let proposals = $state<Record<string, Proposal>>({});
  let candidates = $state<Record<string, Candidate[]>>({});
  let providerImages = $state<Record<string, ProviderImage[]>>({});
  let candidateIds = $state<Record<string, string>>({});
  async function invoke(id: string, operation: string, candidate?: string) {
    const workId = workIds[id];
    if (operation === 'metadata.search') resetWork(id);
    else if (operation !== 'metadata.images') { delete proposals[id]; delete providerImages[id]; delete candidateIds[id]; }
    await perform(async () => {
      const response = await adminRequest<{ proposal: unknown }>(`/api/v1/admin/extensions/${encodeURIComponent(id)}/run`, {
        method: 'POST', body: JSON.stringify({ workId, operation, input: { id: candidate } })
      });
      if (workIds[id] !== workId) return;
      if (operation === 'metadata.search') {
        candidates[id] = response.proposal as Candidate[]; delete proposals[id]; delete providerImages[id]; delete candidateIds[id];
      } else if (operation === 'metadata.images') providerImages[id] = response.proposal as ProviderImage[];
      else { proposals[id] = { workId, value: response.proposal as Record<string, unknown> }; if (candidate) candidateIds[id] = candidate; }
    }, operation === 'metadata.search' ? 'Choisissez le résultat correspondant à votre œuvre.' : operation === 'metadata.propose' ? 'Proposition générée. Consultez les résultats ci-dessous.' : 'Résultat reçu.');
  }
  function resetWork(id: string) { delete proposals[id]; delete candidates[id]; delete providerImages[id]; delete candidateIds[id]; }
  function imageKind(type: string): string | undefined {
    return ({ poster: 'poster', backdrop: 'backdrop', logo: 'logo', banner: 'banner', thumb: 'thumb', clear_art: 'illustration', disc_art: 'disc' } as Record<string,string>)[type];
  }
  let installed = $state<Installed[]>([]);
  let libraries = $state<Library[]>([]);
  let runs = $state<Run[]>([]);
  let publishers = $state<{ id: string; publicKey: string; active: boolean }[]>([]);
  let audit = $state<{ actorId: string; extensionId: string; action: string; createdAt: number }[]>([]);
  let publisherId = $state('');
  let publicKey = $state('');
  let selected = $state<Record<string, string[]>>({});
  let workIds = $state<Record<string, string>>({});
  let queries = $state<Record<string, string>>({});
  let workOptions = $state<Record<string, { id: string; title: string }[]>>({});
  let purge = $state<Record<string, boolean>>({});
  let editingSettings = $state<Record<string, ExtensionSettings>>({});
  let activeSettingsId = $state<string | null>(null);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let filter = $state('all');
  let search = $state('');
  const visibleExtensions = $derived(installed.filter(extension =>
    (filter === 'all' || extension.enabled === (filter === 'enabled')) &&
    `${extension.manifest.name} ${extension.manifest.description}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())
  ));

  const permissionNames: Record<string,string> = {'metadata.read':'Consulter les métadonnées','metadata.write':'Modifier les métadonnées','images':'Importer des images','avatars':'Proposer des avatars','providers':'Fournisseur externe'};
  const displayName = (name: string) => name.replace(/\s+\([^)]*\)$/, '');
  const shortDescription = (description: string) => description.split(' (')[0];
  const accessLabels = (permissions: string[]) => permissions.map(permission => permissionNames[permission] ?? permission).join(' · ');
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
    const [extensions, libs, history, keys, journal, sources, available] = await Promise.all([
      adminRequest<{ extensions: Installed[] }>('/api/v1/admin/extensions'),
      adminRequest<{ libraries: Library[] }>('/api/v1/admin/libraries'),
      adminRequest<{ runs: Run[] }>('/api/v1/admin/extensions/runs'),
      adminRequest<{ publishers: typeof publishers }>('/api/v1/admin/extensions/publishers'),
      adminRequest<{ audit: typeof audit }>('/api/v1/admin/extensions/audit'),
      adminRequest<{ repositories: Repository[] }>('/api/v1/admin/extensions/repositories'),
      adminRequest<{ entries: CatalogueEntry[] }>('/api/v1/admin/extensions/catalogue')
    ]);
    installed = extensions.extensions; libraries = libs.libraries; runs = history.runs; publishers = keys.publishers;
    audit = journal.audit;
    repositories = sources.repositories; catalogue = available.entries;
    selected = Object.fromEntries(installed.map(e => [e.manifest.id, [...e.libraryIds]]));

    for (const ext of installed) {
      if (!editingSettings[ext.manifest.id]) {
        const defaults: ExtensionSettings = {};
        if (ext.manifest.configSchema) {
          for (const field of ext.manifest.configSchema) {
            if (field.fieldType === 'boolean') {
              defaults[field.key] = field.defaultValue === 'true';
            } else {
              defaults[field.key] = field.defaultValue ?? '';
            }
          }
        }
        editingSettings[ext.manifest.id] = { ...defaults, ...(ext.settings ?? {}) };
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
      method: 'PUT', body: JSON.stringify({ enabled, libraryIds: selected[extension.manifest.id] ?? [] })
    }), enabled ? 'Permissions enregistrées et extension activée.' : 'Extension désactivée.');
  }
  async function saveSettings(id: string) {
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(id)}/settings`, {
        method: 'PUT',
        body: JSON.stringify(editingSettings[id] ?? {})
      });
      activeSettingsId = null;
    }, 'Paramètres enregistrés.');
  }
  async function searchWorks(id: string) {
    busy = true; error = '';
    try {
      const response = await adminRequest<{ items: { id: string; title: string }[] }>(`/api/v1/admin/extensions/${encodeURIComponent(id)}/works?q=${encodeURIComponent(queries[id] ?? '')}`);
      workOptions[id] = response.items; workIds[id] = response.items[0]?.id ?? '';
      if (!response.items.length) notice = 'Aucune œuvre trouvée dans les bibliothèques autorisées.';
    } catch (e) { error = e instanceof Error ? e.message : 'Recherche impossible.'; }
    finally { busy = false; }
  }
  async function addRepository(event: SubmitEvent) {
    event.preventDefault();
    await perform(async () => {
      await adminRequest('/api/v1/admin/extensions/repositories',{method:'POST',body:JSON.stringify({name:repositoryName.trim() || new URL(repositoryUrl).hostname.slice(0,80),url:repositoryUrl})});
      repositoryName=''; repositoryUrl=''; tab='catalogue';
    },'Dépôt enregistré. Ses extensions sont disponibles dans le catalogue.');
  }
  async function installFromCatalogue(entry: CatalogueEntry) {
    await perform(async () => {
      await adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(entry.repositoryId)}/install`,{method:'POST',body:JSON.stringify({extensionId:entry.manifest.id,version:entry.manifest.version})});
      tab='installed';
    },'Extension installée. Configurez ses paramètres et ses bibliothèques puis activez-la. Aucun redémarrage nécessaire.');
  }
</script>

<section id="extensions-section" aria-labelledby="extensions-heading">
  <div class="heading page-heading"><h2 id="extensions-heading">Extensions</h2>{#if tab==='catalogue'}<button aria-label="Actualiser les dépôts" disabled={busy || repositories.length===0} onclick={refreshRepositories}><Icon name="refresh" size={16} /> Actualiser</button>{:else}<button disabled={busy} onclick={reload}><Icon name="refresh" size={16} /> Actualiser</button>{/if}</div>
  <nav class="extension-tabs" aria-label="Gestion des extensions">
    <button aria-label="Installées" class:active={tab==='installed'} aria-current={tab==='installed' ? 'page' : undefined} onclick={() => tab='installed'}>Installées ({installed.length})</button>
    <button aria-label="Catalogue" class:active={tab==='catalogue'} aria-current={tab==='catalogue' ? 'page' : undefined} onclick={() => tab='catalogue'}>Catalogue <span>{catalogueGroups.length}</span></button>
    <button aria-label="Dépôts" class:active={tab==='repositories'} aria-current={tab==='repositories' ? 'page' : undefined} onclick={() => tab='repositories'}>Dépôts ({repositories.length})</button>
  </nav>
  {#if busy}<p class="loading-notice" role="status">Chargement…</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if tab==='repositories'}
    <div class="section-title"><Icon name="server" size={20} /><h3>Vos sources d’extensions</h3></div>
    <form class="repository-form" onsubmit={addRepository}>
      <label>Nom du dépôt (facultatif)<input maxlength="80" bind:value={repositoryName} disabled={busy} /></label>
      <label>Adresse du dépôt<input type="url" required maxlength="2048" placeholder="https://exemple.fr/manifest.json" bind:value={repositoryUrl} disabled={busy} /></label>
      <button disabled={busy}>Ajouter le dépôt</button>
    </form>
    {#each repositories as repository (repository.id)}
      <article class="card repository-card"><div class="repository-heading"><span class="extension-icon"><Icon name="server" size={22} /></span><div><h4>{repository.name}</h4><p>{repository.refreshedAt ? `Actualisé le ${new Date(repository.refreshedAt*1000).toLocaleString()}` : 'Catalogue à récupérer'}</p></div>{#if repository.id==='community-default'}<span class="badge">Préconfiguré</span>{/if}</div><code class="repository-url">{repository.url}</code>
        <div class="actions"><button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}/refresh`,{method:'POST'}),'Catalogue actualisé.')}>Actualiser ce dépôt</button>
        <button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}`,{method:'DELETE'}),'Dépôt retiré. Les extensions installées sont conservées.')}>Retirer le dépôt</button></div><small>Les extensions installées sont conservées. Ce dépôt ne sera pas réajouté au redémarrage.</small>
      </article>
    {:else}<p>Aucun dépôt enregistré.</p>{/each}
  {:else if tab==='catalogue'}
    {#if repositories.some(repository => !repository.refreshedAt)}<div class="catalogue-pending"><Icon name="info" size={20} /><div><strong>Catalogue à charger</strong><p>Actualisez pour afficher les extensions.</p></div></div>{/if}
    <label class="catalogue-search">Rechercher dans le catalogue<input type="search" bind:value={search} placeholder="Nom ou description" /></label>
    <p class="section-help">Après installation, activez vos choix dans Installées.</p>
    <div class="plugins-grid">
    {#each catalogueGroups.filter(group=>`${group.entries[0].manifest.name} ${group.entries[0].manifest.description}`.toLowerCase().includes(search.toLowerCase())) as group (group.key)}
      {@const entry=group.entries.find(item => item.manifest.version===catalogueVersions[group.key]) ?? group.entries[0]}
      {@const publisher=publishers.find(item => item.id===entry.publisherId)}
      {@const trusted=publisher?.active && publisher.publicKey===entry.publicKey}
      {@const existing=installed.find(item => item.manifest.id===entry.manifest.id)}
      <article class="card plugin-card catalogue-card">
        <div class="catalogue-identity"><span class="extension-icon"><Icon name={entry.manifest.permissions.includes('avatars') ? 'profile' : 'globe'} size={28} /></span>{#if existing || !entry.compatible}<span class="badge" class:success={!!existing}>{existing ? 'Installée' : 'Incompatible'}</span>{/if}</div>
        <div class="catalogue-content"><h4>{displayName(entry.manifest.name)}</h4><p class="plugin-desc">{shortDescription(entry.manifest.description)}</p>
        {#if !entry.compatible}<p>Cette version n’est pas compatible avec votre serveur.</p>{/if}
        {#if existing}<p>Version {existing.manifest.version} installée.</p>{/if}
        <details class="access-details"><summary>Détails et accès</summary><p>{entry.manifest.description}</p><p>{entry.manifest.author} · {entry.repositoryName}</p><p>{accessLabels(entry.manifest.permissions)}</p><p>Services contactés : {entry.manifest.httpHosts?.join(', ') || 'aucun'}</p></details>
        {#if !trusted}
          <details><summary>Vérifier l’éditeur {entry.publisherId}</summary><p>Comparez cette clé publique avec celle publiée par l’auteur sur un canal de confiance avant de l’approuver.</p><code class="repository-url">{entry.publicKey}</code>
          {#if publisher}<p>Identifiant déjà approuvé avec une autre clé ou révoqué. Demandez à l’auteur un nouvel identifiant ; la clé existante ne sera pas remplacée.</p>
          {:else}<button disabled={busy} onclick={() => perform(() => adminRequest('/api/v1/admin/extensions/publishers',{method:'POST',body:JSON.stringify({id:entry.publisherId,publicKey:entry.publicKey})}),'Éditeur approuvé.')}>J’ai vérifié cette clé : approuver</button>{/if}</details>
        {/if}
        </div><div class="catalogue-actions">{#if group.entries.length>1}<label for={`extension-version-${group.key}`}>Version<select aria-label={`Version de ${entry.manifest.name}`} id={`extension-version-${group.key}`} bind:value={catalogueVersions[group.key]} disabled={busy}>{#each group.entries as option (option.manifest.version)}<option value={option.manifest.version}>{option.manifest.version}{option.compatible ? '' : ' · incompatible'}</option>{/each}</select></label>{:else}<span class="version-label">v{entry.manifest.version}</span>{/if}
        <button class="primary" aria-label={existing ? `${entry.manifest.name} déjà installée` : `Installer ${entry.manifest.name}`} disabled={busy || !trusted || !entry.compatible || !!existing} onclick={() => installFromCatalogue(entry)}>{existing ? 'Déjà installée' : 'Installer'}</button></div>
      </article>
    {:else}<p>Aucune extension ne correspond. Ajoutez un dépôt ou modifiez votre recherche.</p>{/each}
    </div>
  {:else}
  <div class="installed-filters"><label>Rechercher<input type="search" bind:value={search} /></label><label>État<select bind:value={filter}><option value="all">Toutes</option><option value="enabled">Activées</option><option value="disabled">Désactivées</option></select></label></div>
  <div class="plugins-grid">
  {#each visibleExtensions as extension (extension.manifest.id)}
    <article class="card plugin-card installed-card" class:expanded={managedId===extension.manifest.id}>
      <div class="plugin-header">
        <div class="plugin-title-row">
          <div class="installed-identity"><span class="extension-icon"><Icon name={extension.manifest.permissions.includes('avatars') ? 'profile' : 'globe'} size={22} /></span><div><h3>{displayName(extension.manifest.name)}</h3><small>v{extension.manifest.version}</small></div></div>
          <span class="badge" class:success={extension.enabled}>{extension.enabled ? 'Activée' : 'Désactivée'}</span>
        </div>
        <p class="plugin-desc">{shortDescription(extension.manifest.description)}</p>
      </div>

      <details class="manage-extension" open={managedId===extension.manifest.id} ontoggle={event => { if(event.currentTarget.open) managedId=extension.manifest.id; else if(managedId===extension.manifest.id) managedId=null; }}><summary>Configurer et gérer</summary>
      <div class="extension-access"><p>{extension.manifest.description}</p><p>{accessLabels(extension.manifest.permissions)}</p><p>Services contactés : {extension.manifest.httpHosts?.join(', ') || 'aucun'}</p><details><summary>Identité du paquet</summary><p>Éditeur : {extension.publisherId}</p><code>{extension.sha256}</code></details></div>
      {#if extension.manifest.configSchema && extension.manifest.configSchema.length > 0}
        <div class="settings-toggle">
          <button
            type="button"
            class="btn-settings"
            onclick={() => activeSettingsId = activeSettingsId === extension.manifest.id ? null : extension.manifest.id}
          >
            ⚙ {activeSettingsId === extension.manifest.id ? 'Masquer la configuration' : 'Paramètres du plugin'}
          </button>
        </div>

        {#if activeSettingsId === extension.manifest.id}
          <div class="settings-panel">
            <h4>Paramètres de l'extension</h4>
            <form onsubmit={(e) => { e.preventDefault(); void saveSettings(extension.manifest.id); }}>
              {#each extension.manifest.configSchema as field (field.key)}
                <div class="setting-row">
                  {#if field.fieldType === 'boolean'}
                    <label class="check">
                      <input
                        type="checkbox"
                        checked={Boolean(editingSettings[extension.manifest.id]?.[field.key])}
                        onchange={(e) => { editingSettings[extension.manifest.id][field.key] = (e.currentTarget as HTMLInputElement).checked; }}
                      />
                      <span><strong>{field.label}</strong>{#if field.description} — {field.description}{/if}</span>
                    </label>
                  {:else if field.fieldType === 'password'}
                    <label>
                      <span><strong>{field.label}</strong></span>
                      <input
                        type="password"
                        placeholder={field.defaultValue ?? ''}
                        bind:value={editingSettings[extension.manifest.id][field.key]}
                      />
                      {#if field.description}<small>{field.description}</small>{/if}
                    </label>
                  {:else if field.fieldType === 'select'}
                    <label>
                      <span><strong>{field.label}</strong></span>
                      <select bind:value={editingSettings[extension.manifest.id][field.key]}>
                        {#each field.options ?? [] as opt (opt.value)}
                          <option value={opt.value}>{opt.label}</option>
                        {/each}
                      </select>
                      {#if field.description}<small>{field.description}</small>{/if}
                    </label>
                  {:else}
                    <label>
                      <span><strong>{field.label}</strong></span>
                      <input
                        type="text"
                        placeholder={field.defaultValue ?? ''}
                        bind:value={editingSettings[extension.manifest.id][field.key]}
                      />
                      {#if field.description}<small>{field.description}</small>{/if}
                    </label>
                  {/if}
                </div>
              {/each}
              <div class="settings-actions">
                <button type="submit" disabled={busy}>Enregistrer les paramètres</button>
                <button type="button" onclick={() => activeSettingsId = null}>Fermer</button>
              </div>
            </form>
          </div>
        {/if}
      {/if}

      <fieldset disabled={busy}><legend>Bibliothèques autorisées</legend>
        {#each libraries.filter(l => l.enabled) as library (library.id)}
          <label class="check"><input type="checkbox" value={library.id} bind:group={selected[extension.manifest.id]} />{library.name}</label>
        {/each}
      </fieldset>
      <div class="actions">
        <button disabled={busy} onclick={() => configure(extension, true)}>Enregistrer et activer</button>
        <button disabled={busy || !extension.enabled} onclick={() => configure(extension, false)}>Désactiver</button>
      </div>
      {#if extension.manifest.permissions.includes('metadata.read')}
      <form onsubmit={event => { event.preventDefault(); void invoke(extension.manifest.id, 'metadata.search'); }}>
        <label>Rechercher une œuvre<input maxlength="120" bind:value={queries[extension.manifest.id]} /></label>
        <button type="button" disabled={busy || !extension.enabled} onclick={() => searchWorks(extension.manifest.id)}>Rechercher dans les bibliothèques autorisées</button>
        <label for={`extension-work-${extension.manifest.id}`}>Œuvre à analyser</label>
        <select id={`extension-work-${extension.manifest.id}`} required disabled={busy} bind:value={workIds[extension.manifest.id]} onchange={() => resetWork(extension.manifest.id)}><option value="">Choisir une œuvre</option>
          {#each workOptions[extension.manifest.id] ?? [] as work (work.id)}<option value={work.id}>{work.title}</option>{/each}
        </select>
        <button disabled={busy || !extension.enabled || !workIds[extension.manifest.id]}>Rechercher chez ce fournisseur</button>
        <button type="button" disabled={busy || !extension.enabled || !workIds[extension.manifest.id]} onclick={() => invoke(extension.manifest.id, 'metadata.propose')}>Exécuter sur cette œuvre</button>
        <small>Le second bouton utilise la proposition simple des anciens paquets.</small>
      </form>
      {#each candidates[extension.manifest.id] ?? [] as match (match.id)}
        <div class="card"><strong>{match.title}{match.year ? ` (${match.year})` : ''}</strong>
          {#if match.overview}<p>{match.overview}</p>{/if}
          <button disabled={busy || !extension.enabled} onclick={() => invoke(extension.manifest.id, 'metadata.details', match.id)}>Choisir ce résultat</button>
        </div>
      {/each}
      {#if proposals[extension.manifest.id] && proposals[extension.manifest.id].workId === workIds[extension.manifest.id]}
        {@const proposal = proposals[extension.manifest.id]}
        <div class="card"><strong>{String(proposal.value.title ?? 'Métadonnées')}</strong>
          <p>{String(proposal.value.synopsis ?? proposal.value.summary ?? '')}</p>
          <details><summary>Tous les champs proposés</summary><pre>{JSON.stringify(proposal.value, null, 2)}</pre></details>
          <button disabled={busy || !extension.enabled || !extension.manifest.permissions.includes('metadata.write')} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}/apply`, {method:'POST',body:JSON.stringify({workId:proposal.workId,metadata:proposal.value})}), 'Métadonnées appliquées.')}>Appliquer ces métadonnées</button>
          {#if extension.manifest.permissions.includes('images') && candidateIds[extension.manifest.id]}
            <button disabled={busy || !extension.enabled} onclick={() => invoke(extension.manifest.id, 'metadata.images', candidateIds[extension.manifest.id])}>Charger les images proposees</button>
          {/if}
        </div>
      {/if}
      {#each providerImages[extension.manifest.id] ?? [] as image, index (index)}
        {#if imageKind(image.imageType)}
          <div class="card"><p>{image.imageType} - {image.language ?? 'Toutes langues'} - {image.width ?? '?'} x {image.height ?? '?'}</p>
            <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
            <a href={image.url} target="_blank" rel="noreferrer">Voir cette image</a>
            <button disabled={busy || !extension.enabled || !extension.manifest.permissions.includes('metadata.write')} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}/image`, {method:'POST',body:JSON.stringify({workId:workIds[extension.manifest.id],url:image.url,kind:imageKind(image.imageType)})}), 'Image importee dans la mediatheque.')}>Utiliser cette image</button>
          </div>
        {/if}
      {/each}
      {/if}
      {#if extension.manifest.permissions.includes('avatars')}<p>Les packs de cette extension sont disponibles dans les parametres de votre image de profil apres activation.</p>{/if}
      <details><summary>Désinstaller</summary>
        <label class="check"><input type="checkbox" bind:checked={purge[extension.manifest.id]} />Supprimer aussi les résultats conservés</label>
        <button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}`, { method: 'DELETE', body: JSON.stringify({ deleteData: purge[extension.manifest.id] ?? false }) }), 'Extension désinstallée.')}>Désinstaller cette extension</button>
      </details>
      </details>
    </article>
  {:else}<p>{installed.length ? 'Aucune extension ne correspond à la recherche.' : 'Aucune extension installée.'}</p>{/each}
  </div>
    <details>
    <summary>Éditeurs approuvés</summary>
    <p>Vérifiez la clé publique auprès de l’éditeur par un canal de confiance avant de l’approuver.</p>
    <form onsubmit={event => { event.preventDefault(); void perform(() => adminRequest('/api/v1/admin/extensions/publishers', { method: 'POST', body: JSON.stringify({ id: publisherId, publicKey }) }), 'Éditeur approuvé.'); }}>
      <label>Identifiant de l’éditeur<input required maxlength="80" bind:value={publisherId} /></label>
      <label>Clé publique Ed25519 (Base64)<input required maxlength="64" bind:value={publicKey} /></label>
      <button disabled={busy}>Approuver cet éditeur</button>
    </form>
    {#each publishers as publisher (publisher.id)}
      <div class="row"><span>{publisher.id} — {publisher.active ? 'Approuvé' : 'Révoqué'}</span><code>{publisher.publicKey}</code>
        <button disabled={busy || !publisher.active} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/publishers/${encodeURIComponent(publisher.id)}`, { method: 'DELETE' }), 'Éditeur révoqué.')}>Révoquer</button></div>
    {/each}
  </details>

  <details class="execution-history"><summary>Dernières exécutions ({runs.length})</summary>
  {#each runs as run (run.id)}
    <div class="card"><p>{run.extensionId} · {run.status} · {run.durationMs} ms</p>
      {#if run.result.operation}<p>{run.result.operation}</p>{/if}
      {#if run.result.output}<details><summary>Résultat</summary><pre>{JSON.stringify(run.result.output, null, 2)}</pre></details>{/if}
      {#if run.result.pluginError}<p>{run.result.pluginError}</p>{/if}
      {#if run.result.title}<strong>{run.result.title}</strong>{/if}
      {#if run.result.summary}<p>{run.result.summary}</p>{/if}
      {#if run.result.error}<p>{run.result.error}</p>{/if}
    </div>
  {/each}
  </details>
  <details><summary>Journal des opérations</summary>
    {#each audit as entry, index (index)}
      <p>{new Date(entry.createdAt * 1000).toLocaleString()} · {entry.extensionId} · {entry.action}</p>
    {/each}
  </details>
  {/if}
</section>

<style>
  section { min-width:0; }
  h2,h3,h4,strong,legend,summary { color:var(--text-primary); }
  h2 { font-size:1.7rem;letter-spacing:-.03em;margin:0; } h3{font-size:1.05rem;margin:0;} h4{font-size:1.1rem;margin:0;} p{line-height:1.55;color:var(--text-secondary);} small{color:var(--text-secondary);line-height:1.5;}

  .heading,.row,.actions,.settings-actions,.section-title{display:flex;align-items:center;gap:12px;flex-wrap:wrap;}.heading{justify-content:space-between;}.section-title{margin-top:24px;}

  .extension-tabs{display:flex;gap:4px;border-bottom:1px solid var(--border-subtle);margin:12px 0 24px;}.extension-tabs button{border:0;border-bottom:2px solid transparent;border-radius:0;background:transparent;padding:12px 16px;}.extension-tabs button.active{border-color:var(--accent-primary);color:var(--accent-primary);}
  button{display:inline-flex;align-items:center;justify-content:center;gap:7px;padding:9px 13px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-primary);cursor:pointer;font:inherit;font-size:13px;min-height:38px;}button:hover:not(:disabled){border-color:var(--accent-primary);}button:focus-visible,summary:focus-visible{outline:2px solid var(--accent-primary);outline-offset:3px;}button:disabled{opacity:.45;cursor:default;}button.primary{background:var(--color-accent-solid);color:white;border-color:transparent;}
  form{display:grid;gap:14px;margin:16px 0;}label{display:grid;gap:6px;color:var(--text-secondary);font-size:13px;}input:not([type=checkbox]),select{width:100%;min-width:0;box-sizing:border-box;padding:10px 12px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-primary);font:inherit;}input[type=checkbox]{accent-color:var(--accent-primary);}.check{display:flex;align-items:center;gap:9px;}
  .catalogue-search{max-width:420px;margin:20px 0 8px;}.section-help{font-size:13px;margin:12px 0 22px;}.plugins-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,240px),1fr));gap:16px;align-items:start;}.card{border:1px solid var(--border-subtle);border-radius:var(--radius-md);background:var(--bg-surface);min-width:0;}.plugin-card{overflow:hidden;}.catalogue-identity{display:flex;align-items:center;justify-content:space-between;padding:20px 20px 0;}.extension-icon{width:46px;height:46px;display:inline-flex;align-items:center;justify-content:center;background:var(--bg-elevated);border:1px solid var(--border-subtle);border-radius:var(--radius-md);color:var(--accent-primary);flex-shrink:0;}.badge{font-size:11px;background:var(--bg-elevated);padding:4px 8px;border-radius:var(--radius-sm);color:var(--text-secondary);white-space:nowrap;}.badge.success{color:var(--status-success);}.catalogue-content{padding:18px 20px 0;flex:1;}.catalogue-card{display:flex;flex-direction:column;height:100%;box-sizing:border-box;}.plugin-desc{font-size:13px;margin:10px 0;}.catalogue-card .plugin-desc{min-height:4.6em;}.catalogue-actions{display:flex;align-items:end;gap:12px;padding:16px 20px 20px;justify-content:space-between;border-top:1px solid var(--border-subtle);margin-top:16px;}.catalogue-actions label{min-width:75px;max-width:110px;}.catalogue-actions button{flex:1;}
  details{border:1px solid var(--border-subtle);border-radius:var(--radius-sm);padding:12px;margin:14px 0;min-width:0;}summary{cursor:pointer;font-size:13px;font-weight:600;}.access-details{border:0;padding:0;margin:12px 0;font-size:12px;}.access-details p{font-size:12px;}.catalogue-pending{display:flex;gap:12px;padding:18px;background:var(--bg-elevated);border:1px solid var(--border-subtle);border-radius:var(--radius-md);margin:20px 0;}.catalogue-pending p{font-size:13px;margin:5px 0 0;}
  .repository-form{max-width:760px;grid-template-columns:1fr 2fr auto;align-items:end;padding:20px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:var(--radius-md);}.repository-card{padding:20px;margin:16px 0;}.repository-heading{display:flex;align-items:center;gap:12px;margin-bottom:14px;}.repository-heading p{font-size:12px;margin:4px 0;}.repository-heading .badge{margin-left:auto;}.repository-url{display:block;overflow-wrap:anywhere;white-space:normal;font-size:12px;color:var(--text-secondary);}.repository-card .actions{margin:16px 0;}
  .installed-identity{display:flex;align-items:center;gap:12px;min-width:0;}.plugin-header{padding:20px 20px 0;}.plugin-title-row{display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;}.manage-extension{margin:18px 20px 20px;}.installed-card.expanded{grid-column:1/-1;}.extension-access{font-size:13px;}.extension-access code{font-size:11px;}.settings-panel{padding:18px;background:var(--bg-elevated);border-radius:var(--radius-sm);margin:16px 0;}.settings-toggle{margin:16px 0;}.setting-row{min-width:0;}.setting-row small{font-size:12px;}fieldset{border:1px solid var(--border-subtle);border-radius:var(--radius-sm);display:grid;gap:10px;margin:18px 0;padding:16px;}legend{font-size:13px;}code{overflow-wrap:anywhere;white-space:normal;}pre{white-space:pre-wrap;overflow-wrap:anywhere;max-height:24rem;overflow:auto;font-size:12px;}.execution-history{margin-top:24px;}.execution-history .card{padding:16px;margin-top:12px;}
  [role=alert],[role=status]{padding:12px 16px;border-radius:var(--radius-sm);background:var(--bg-elevated);font-size:13px;}[role=alert]{border-left:3px solid var(--status-error);} [role=status]{color:var(--text-secondary);}
  @media(max-width:650px){.repository-form{grid-template-columns:1fr;}.heading{align-items:start;}.extension-tabs button{padding:12px 10px;font-size:12px;}.catalogue-actions{padding:14px 16px;}.plugin-header,.catalogue-content{padding-inline:16px;}.manage-extension{margin-inline:16px;}.repository-heading{flex-wrap:wrap;}}
.installed-filters{display:flex;gap:12px;flex-wrap:wrap;margin:20px 0;}.installed-filters label:first-child{flex:1;min-width:160px;}.version-label{font-size:12px;color:var(--text-secondary);white-space:nowrap;}.catalogue-card .plugin-desc{min-height:3em;}
</style>

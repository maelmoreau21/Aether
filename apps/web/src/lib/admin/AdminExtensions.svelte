<script lang="ts">
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
    category?: string;
    targetAbi?: string;
    minServerVersion?: string;
    maxServerVersion?: string;
    checksum?: string;
    banner?: string;
    configSchema?: ConfigField[];
  };
  type Package = { manifest: Manifest; publisherId: string; sha256: string; signature: string; wasm: string };
  type SettingValue = string | number | boolean;
  type ExtensionSettings = Record<string, SettingValue>;
  type Installed = { manifest: Manifest; publisherId: string; sha256: string; enabled: boolean; libraryIds: string[]; settings?: ExtensionSettings };
  type Repository = { id: string; name: string; url: string; refreshedAt: number };
  type CatalogueEntry = { repositoryId: string; repositoryName: string; compatible: boolean; manifest: Manifest; publisherId: string; publicKey: string };
  let tab = $state<'installed' | 'catalogue' | 'repositories'>('installed');
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
      if (!catalogueVersions[group.key]) catalogueVersions[group.key]=group.entries[0].manifest.version;
    }
  });
  type Library = { id: string; name: string; enabled: boolean | number };
  type Run = { id: string; extensionId: string; status: string; durationMs: number; result: { title?: string; summary?: string; error?: string } };
  let installed = $state<Installed[]>([]);
  let libraries = $state<Library[]>([]);
  let runs = $state<Run[]>([]);
  let publishers = $state<{ id: string; publicKey: string; active: boolean }[]>([]);
  let audit = $state<{ actorId: string; extensionId: string; action: string; createdAt: number }[]>([]);
  let candidate = $state<Package | null>(null);
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
  $effect(() => { void perform(refresh, ''); });
  async function readPackage(event: Event) {
    candidate = null; error = '';
    const file = (event.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    if (file.size > 400000) { error = 'Paquet limité à 400 Ko.'; return; }
    try {
      const parsed = JSON.parse(await file.text()) as Package;
      if (!parsed?.manifest || typeof parsed.manifest.id !== 'string' || typeof parsed.manifest.name !== 'string' ||
          !Array.isArray(parsed.manifest.permissions) || !parsed.manifest.permissions.every(p => typeof p === 'string') ||
          typeof parsed.publisherId !== 'string' || typeof parsed.sha256 !== 'string') throw new Error('Paquet invalide.');
      candidate = parsed;
    } catch { error = 'Paquet JSON invalide.'; }
  }
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
      await adminRequest('/api/v1/admin/extensions/repositories',{method:'POST',body:JSON.stringify({name:repositoryName,url:repositoryUrl})});
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
  <div class="heading"><h2 id="extensions-heading">Extensions</h2><button disabled={busy} onclick={() => perform(refresh, '')}>Actualiser</button></div>
  <nav class="extension-tabs" aria-label="Gestion des extensions">
    <button aria-current={tab==='installed' ? 'page' : undefined} onclick={() => tab='installed'}>Installées</button>
    <button aria-current={tab==='catalogue' ? 'page' : undefined} onclick={() => tab='catalogue'}>Catalogue</button>
    <button aria-current={tab==='repositories' ? 'page' : undefined} onclick={() => tab='repositories'}>Dépôts</button>
  </nav>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if tab==='repositories'}
    <h3>Dépôts d’extensions</h3>
    <p>Ajoutez l’adresse HTTPS du manifeste fourni par l’auteur. Les dépôts et modules Jellyfin ne sont pas compatibles.</p>
    <form class="repository-form" onsubmit={addRepository}>
      <label>Nom du dépôt<input required maxlength="80" bind:value={repositoryName} disabled={busy} /></label>
      <label>URL du manifeste<input type="url" required maxlength="2048" placeholder="https://exemple.fr/manifest.json" bind:value={repositoryUrl} disabled={busy} /></label>
      <button disabled={busy}>Ajouter le dépôt</button>
    </form>
    {#each repositories as repository (repository.id)}
      <article class="card"><h4>{repository.name}</h4><p class="repository-url">{repository.url}</p><p>Actualisé le {new Date(repository.refreshedAt*1000).toLocaleString()}</p>
        <div class="actions"><button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}/refresh`,{method:'POST'}),'Catalogue actualisé.')}>Actualiser ce dépôt</button>
        <button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/repositories/${encodeURIComponent(repository.id)}`,{method:'DELETE'}),'Dépôt retiré. Les extensions installées sont conservées.')}>Retirer le dépôt</button></div>
      </article>
    {:else}<p>Aucun dépôt enregistré.</p>{/each}
  {:else if tab==='catalogue'}
    <div class="heading"><h3>Catalogue</h3><button onclick={() => tab='repositories'}>+ Ajouter un dépôt</button></div>
    <p>Choisissez une version puis installez-la. L’activation se fait dans Installées après configuration des accès.</p>
    <div class="plugins-grid">
    {#each catalogueGroups as group (group.key)}
      {@const entry=group.entries.find(item => item.manifest.version===catalogueVersions[group.key]) ?? group.entries[0]}
      {@const publisher=publishers.find(item => item.id===entry.publisherId)}
      {@const trusted=publisher?.active && publisher.publicKey===entry.publicKey}
      {@const existing=installed.find(item => item.manifest.id===entry.manifest.id)}
      <article class="card plugin-card"><h4>{entry.manifest.name}</h4><p>{entry.manifest.description}</p><p>{entry.repositoryName} · {entry.manifest.author}</p>
        <label for={`extension-version-${group.key}`}>Version de {entry.manifest.name}</label>
        <select id={`extension-version-${group.key}`} bind:value={catalogueVersions[group.key]} disabled={busy}>{#each group.entries as option (option.manifest.version)}<option value={option.manifest.version}>{option.manifest.version}{option===group.entries[0] ? ' · la plus récente' : ''}{option.compatible ? '' : ' · incompatible'}</option>{/each}</select>
        <p>Permissions : {entry.manifest.permissions.join(', ')}</p>
        {#if !entry.compatible}<p>Cette version n’est pas compatible avec le serveur actuel.</p>{/if}
        {#if existing}<p>Version {existing.manifest.version} déjà installée. Gérez-la dans Installées.</p>{/if}
        {#if !trusted}
          <details><summary>Vérifier l’éditeur {entry.publisherId}</summary><p>Comparez cette clé publique avec celle publiée par l’auteur sur un canal de confiance avant de l’approuver.</p><code class="repository-url">{entry.publicKey}</code>
          {#if publisher}<p>Identifiant déjà approuvé avec une autre clé ou révoqué. Demandez à l’auteur un nouvel identifiant ; la clé existante ne sera pas remplacée.</p>
          {:else}<button disabled={busy} onclick={() => perform(() => adminRequest('/api/v1/admin/extensions/publishers',{method:'POST',body:JSON.stringify({id:entry.publisherId,publicKey:entry.publicKey})}),'Éditeur approuvé.')}>J’ai vérifié cette clé : approuver</button>{/if}</details>
        {/if}
        <button disabled={busy || !trusted || !entry.compatible || !!existing} onclick={() => installFromCatalogue(entry)}>Installer {entry.manifest.name}</button>
      </article>
    {:else}<p>Aucune extension disponible. Ajoutez un dépôt pour remplir le catalogue. Aucun dépôt officiel distant n’est configuré sur ce serveur.</p>{/each}
    </div>
  {:else}
  <p>Installez un paquet signé, configurez ses paramètres, choisissez ses bibliothèques puis activez-le. Les originaux restent en lecture seule.</p>
  <details class="availability">
    <summary>Comment installer une extension en local ?</summary>
    <ol>
      <li>Préparez le fichier <code>*.package.json</code> signé avec le script <code>Aether-Plugins/scripts/Build-LocalPackage.ps1</code>.</li>
      <li>Dans « Éditeurs approuvés », ajoutez l’identifiant et la clé publique affichée lors de la création de votre clé de signature.</li>
      <li>Importez le paquet ci-dessous. Le serveur vérifie sa signature.</li>
      <li>Ouvrez « Gérer cette extension », renseignez les paramètres du fournisseur, choisissez ses bibliothèques et activez-la.</li>
    </ol>
    <p>Une clé API Aether sert à lire le catalogue depuis une autre application. Une clé TMDb ou fanart.tv se renseigne dans les paramètres de l’extension. La clé privée de signature reste sur votre ordinateur.</p>
    <p>L’onglet Dépôts permet aussi d’ajouter un manifeste HTTPS et de choisir une extension dans Catalogue. Les paquets Jellyfin et leurs fichiers DLL ne sont pas compatibles.</p>
  </details>
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
  <div class="card">
    <h3>Installer un paquet local</h3>
    <label>Paquet JSON signé<input type="file" accept="application/json,.json" onchange={readPackage} disabled={busy} /></label>
    {#if candidate}
      <div class="candidate-preview">
        {#if candidate.manifest.banner}
          {@const bannerSrc = toBannerSrc(candidate.manifest.banner)}
          {#if bannerSrc}
            <div class="plugin-banner">
              <img src={bannerSrc} alt="" />
            </div>
          {/if}
        {/if}
        <p><strong>{candidate.manifest.name}</strong> · v{candidate.manifest.version} · {candidate.publisherId}</p>
        <p>{candidate.manifest.description}</p>
        <div class="plugin-badges">
          {#if candidate.manifest.category}<span class="badge category">{candidate.manifest.category}</span>{/if}
          {#if candidate.manifest.targetAbi}<span class="badge abi">ABI {candidate.manifest.targetAbi}</span>{/if}
          {#if candidate.manifest.minServerVersion}<span class="badge server">Aether &ge; {candidate.manifest.minServerVersion}</span>{/if}
        </div>
        <p>Droits demandés : {candidate.manifest.permissions.join(', ')}</p>
        <code>SHA-256 : {candidate.sha256}</code>
        <p>L’installation vérifie la signature et conserve l’extension désactivée.</p>
        {#if !publishers.some(publisher => publisher.id === candidate?.publisherId && publisher.active)}
          <p>Approuvez d’abord l’éditeur « {candidate.publisherId} » avec sa clé publique dans « Éditeurs approuvés ».</p>
        {/if}
        <button disabled={busy || !publishers.some(publisher => publisher.id === candidate?.publisherId && publisher.active)} onclick={() => perform(async () => { await adminRequest('/api/v1/admin/extensions', { method: 'POST', body: JSON.stringify(candidate) }); candidate = null; }, 'Paquet installé, encore désactivé.')}>Vérifier et installer</button>
      </div>
    {/if}
  </div>
  <div class="heading"><h3>Installées ({installed.length})</h3>
    <div class="row">
      <label>Rechercher une extension<input type="search" bind:value={search} /></label>
      <label>État<select bind:value={filter}><option value="all">Toutes</option><option value="enabled">Activées</option><option value="disabled">Désactivées</option></select></label>
    </div>
  </div>
  <div class="plugins-grid">
  {#each visibleExtensions as extension (extension.manifest.id)}
    <article class="card plugin-card">
      {#if extension.manifest.banner}
        {@const bannerSrc = toBannerSrc(extension.manifest.banner)}
        {#if bannerSrc}
          <div class="plugin-banner">
            <img src={bannerSrc} alt="" />
          </div>
        {/if}
      {/if}
      <div class="plugin-header">
        <div class="plugin-title-row">
          <h3>{extension.manifest.name}</h3>
          <div class="plugin-badges">
            <span class="badge version">v{extension.manifest.version}</span>
            {#if extension.manifest.category}<span class="badge category">{extension.manifest.category}</span>{/if}
            {#if extension.manifest.targetAbi}<span class="badge abi">ABI {extension.manifest.targetAbi}</span>{/if}
            {#if extension.manifest.minServerVersion}<span class="badge server">Aether &ge; {extension.manifest.minServerVersion}</span>{/if}
          </div>
        </div>
        <p class="plugin-desc">{extension.manifest.description}</p>
        <p class="plugin-meta">{extension.enabled ? 'Activée' : 'Désactivée'} · Éditeur: {extension.publisherId} · Droits: {extension.manifest.permissions.join(', ')}</p>
        {#if extension.manifest.checksum}
          <p class="checksum-info"><code>Checksum: {extension.manifest.checksum}</code></p>
        {/if}
      </div>

      <details class="manage-extension"><summary>Gérer cette extension</summary>
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
      <form onsubmit={event => { event.preventDefault(); void perform(() => adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}/run`, { method: 'POST', body: JSON.stringify({ workId: workIds[extension.manifest.id] }) }), 'Proposition générée. Consultez les résultats ci-dessous.'); }}>
        <label>Rechercher une œuvre<input maxlength="120" bind:value={queries[extension.manifest.id]} /></label>
        <button type="button" disabled={busy || !extension.enabled} onclick={() => searchWorks(extension.manifest.id)}>Rechercher dans les bibliothèques autorisées</button>
        <label for={`extension-work-${extension.manifest.id}`}>Œuvre à analyser</label>
        <select id={`extension-work-${extension.manifest.id}`} required bind:value={workIds[extension.manifest.id]}><option value="">Choisir une œuvre</option>
          {#each workOptions[extension.manifest.id] ?? [] as work (work.id)}<option value={work.id}>{work.title}</option>{/each}
        </select>
        <button disabled={busy || !extension.enabled || !workIds[extension.manifest.id]}>Exécuter sur cette œuvre</button>
      </form>
      <details><summary>Désinstaller</summary>
        <label class="check"><input type="checkbox" bind:checked={purge[extension.manifest.id]} />Supprimer aussi les résultats conservés</label>
        <button disabled={busy} onclick={() => perform(() => adminRequest(`/api/v1/admin/extensions/${encodeURIComponent(extension.manifest.id)}`, { method: 'DELETE', body: JSON.stringify({ deleteData: purge[extension.manifest.id] ?? false }) }), 'Extension désinstallée.')}>Désinstaller cette extension</button>
      </details>
      </details>
    </article>
  {:else}<p>{installed.length ? 'Aucune extension ne correspond à la recherche.' : 'Aucune extension installée.'}</p>{/each}
  </div>
  <h3>Dernières exécutions</h3>
  {#each runs as run (run.id)}
    <div class="card"><p>{run.extensionId} · {run.status} · {run.durationMs} ms</p>
      {#if run.result.title}<strong>{run.result.title}</strong>{/if}
      {#if run.result.summary}<p>{run.result.summary}</p>{/if}
      {#if run.result.error}<p>{run.result.error}</p>{/if}
    </div>
  {/each}
  <details><summary>Journal des opérations</summary>
    {#each audit as entry, index (index)}
      <p>{new Date(entry.createdAt * 1000).toLocaleString()} · {entry.extensionId} · {entry.action}</p>
    {/each}
  </details>
  {/if}
</section>

<style>
  .extension-tabs { display:flex;gap:.5rem;flex-wrap:wrap;margin-block:1rem; }
  .extension-tabs button[aria-current="page"] { border-color:var(--color-accent);color:var(--color-accent); }
  .repository-form { display:grid;gap:.8rem;max-width:800px; }
  .repository-url { overflow-wrap:anywhere;white-space:normal; }
  section { padding: 1.5rem; border: 1px solid var(--border-subtle); background: var(--bg-surface); border-radius: 16px; }
  h2,h3,h4,strong,legend,summary { color: var(--text-primary); }
  h2 { margin: 0; font-size: 1.3rem; } h3 { font-size: 1.1rem; margin: 0; } h4 { margin: 0 0 .6rem 0; font-size: 1rem; }
  p,label { color: var(--text-secondary); line-height: 1.6; }
  .heading,.row,.actions,.settings-actions { display: flex; align-items: center; gap: .7rem; flex-wrap: wrap; }
  .heading { justify-content: space-between; }
  .card,.availability,details { margin-block: 1rem; padding: 1rem; border: 1px solid var(--border-subtle); border-radius: 10px; }
  .availability { background: var(--bg-elevated); }
  form { display: grid; gap: .7rem; margin-block: 1rem; }
  label { display: grid; gap: .3rem; } .check { display: flex; align-items: center; gap: .5rem; }
  input:not([type=checkbox]),select { min-width: 0; width: 100%; padding: .6rem; border: 1px solid var(--border-subtle); border-radius: 6px; background: var(--bg-elevated); color: var(--text-primary); box-sizing: border-box; }
  button { padding: .6rem .8rem; border: 1px solid var(--border-subtle); border-radius: 6px; background: var(--bg-elevated); color: var(--text-primary); cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; } code { color: var(--text-secondary); overflow-wrap: anywhere; }
  fieldset { border: 1px solid var(--border-subtle); display: grid; gap: .4rem; }
  summary { cursor: pointer; } [role=alert] { border-inline-start: 3px solid var(--text-primary); padding-inline-start: .8rem; }
  
  /* Jellyfin-style plugin presentation */
  .plugin-card { overflow: hidden; padding: 0; }
  .plugins-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 320px), 1fr)); gap: 1rem; align-items: start; }
  .plugins-grid .plugin-card { margin-block: 1rem 0; min-width: 0; }
  .manage-extension { margin: 1rem; }
  .plugin-banner { width: 100%; max-height: 140px; overflow: hidden; background: var(--bg-elevated); border-bottom: 1px solid var(--border-subtle); display: flex; align-items: center; justify-content: center; }
  .plugin-banner img { width: 100%; height: auto; max-height: 140px; object-fit: contain; display: block; }
  .plugin-header { padding: 1rem 1rem 0 1rem; }
  .plugin-title-row { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: .5rem; }
  .plugin-badges { display: flex; gap: .4rem; flex-wrap: wrap; }
  .badge { font-size: .75rem; padding: .2rem .5rem; border-radius: 4px; font-weight: 600; text-transform: uppercase; letter-spacing: .5px; }
  .badge { background: var(--bg-elevated); color: var(--text-secondary); }
  .plugin-desc { margin: .5rem 0 .2rem 0; }
  .plugin-meta { font-size: .85rem; margin: 0 0 .5rem 0; opacity: .8; }
  .checksum-info { font-size: .8rem; margin: .2rem 0; }
  .manage-extension > fieldset, .manage-extension > .actions, .manage-extension > form, .settings-toggle { padding-inline: 1rem; }
  .settings-toggle { margin-top: .5rem; }
  .btn-settings { background: var(--bg-elevated); color: var(--accent-primary); border-color: var(--accent-primary); font-weight: 500; }
  .settings-panel { margin: 1rem; padding: 1rem; background: var(--bg-elevated); border: 1px solid var(--border-subtle); border-radius: 8px; }
  .setting-row { margin-bottom: .8rem; }
  .setting-row small { color: var(--text-secondary); opacity: .8; display: block; margin-top: .2rem; }
  @media(max-width:600px) { section { padding: 1rem; } }
</style>

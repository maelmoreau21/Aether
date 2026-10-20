<script lang="ts">
  import { adminRequest } from './request';
  type Manifest = { id: string; name: string; version: string; description: string; author: string; permissions: string[] };
  type Package = { manifest: Manifest; publisherId: string; sha256: string; signature: string; wasm: string };
  type Installed = { manifest: Manifest; publisherId: string; sha256: string; enabled: boolean; libraryIds: string[] };
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
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  async function refresh() {
    const [extensions, libs, history, keys, journal] = await Promise.all([
      adminRequest<{ extensions: Installed[] }>('/api/v1/admin/extensions'),
      adminRequest<{ libraries: Library[] }>('/api/v1/admin/libraries'),
      adminRequest<{ runs: Run[] }>('/api/v1/admin/extensions/runs'),
      adminRequest<{ publishers: typeof publishers }>('/api/v1/admin/extensions/publishers'),
      adminRequest<{ audit: typeof audit }>('/api/v1/admin/extensions/audit')
    ]);
    installed = extensions.extensions; libraries = libs.libraries; runs = history.runs; publishers = keys.publishers;
    audit = journal.audit;
    selected = Object.fromEntries(installed.map(e => [e.manifest.id, [...e.libraryIds]]));
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
  async function searchWorks(id: string) {
    busy = true; error = '';
    try {
      const response = await adminRequest<{ items: { id: string; title: string }[] }>(`/api/v1/admin/extensions/${encodeURIComponent(id)}/works?q=${encodeURIComponent(queries[id] ?? '')}`);
      workOptions[id] = response.items; workIds[id] = response.items[0]?.id ?? '';
      if (!response.items.length) notice = 'Aucune œuvre trouvée dans les bibliothèques autorisées.';
    } catch (e) { error = e instanceof Error ? e.message : 'Recherche impossible.'; }
    finally { busy = false; }
  }
</script>

<section id="extensions-section" aria-labelledby="extensions-heading">
  <div class="heading"><h2 id="extensions-heading">Extensions</h2><button disabled={busy} onclick={() => perform(refresh, '')}>Actualiser</button></div>
  <p>Installez un paquet signé, choisissez ses bibliothèques puis activez-le. Les originaux restent en lecture seule.</p>
  <p class="availability">Première API : propositions de métadonnées, sans accès réseau. Le catalogue distant, Trickplay et Intro Skipper ne sont pas encore disponibles.</p>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
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
      <p><strong>{candidate.manifest.name}</strong> · {candidate.manifest.version} · {candidate.publisherId}</p>
      <p>{candidate.manifest.description}</p>
      <p>Droits demandés : {candidate.manifest.permissions.join(', ')}</p>
      <code>SHA-256 : {candidate.sha256}</code>
      <p>L’installation vérifie la signature et conserve l’extension désactivée.</p>
      <button disabled={busy} onclick={() => perform(async () => { await adminRequest('/api/v1/admin/extensions', { method: 'POST', body: JSON.stringify(candidate) }); candidate = null; }, 'Paquet installé, encore désactivé.')}>Vérifier et installer</button>
    {/if}
  </div>
  {#each installed as extension (extension.manifest.id)}
    <article class="card">
      <h3>{extension.manifest.name} · {extension.manifest.version}</h3>
      <p>{extension.manifest.description}</p>
      <p>{extension.enabled ? 'Activée' : 'Désactivée'} · {extension.publisherId} · {extension.manifest.permissions.join(', ')}</p>
      <fieldset disabled={busy}><legend>Bibliothèques autorisées</legend>
        {#each libraries.filter(l => l.enabled) as library (library.id)}
          <label class="check"><input type="checkbox" value={library.id} bind:group={selected[extension.manifest.id]} />{library.name}</label>
        {/each}
      </fieldset>
      <div class="actions"><button disabled={busy} onclick={() => configure(extension, true)}>Enregistrer et activer</button>
        <button disabled={busy || !extension.enabled} onclick={() => configure(extension, false)}>Désactiver</button></div>
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
    </article>
  {:else}<p>Aucune extension installée.</p>{/each}
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
</section>

<style>
  section { padding: 1.5rem; border: 1px solid var(--border-subtle); background: var(--bg-surface); border-radius: 16px; }
  h2,h3,strong,legend,summary { color: var(--text-primary); }
  h2 { margin: 0; font-size: 1.3rem; } h3 { font-size: 1rem; }
  p,label { color: var(--text-secondary); line-height: 1.6; }
  .heading,.row,.actions { display: flex; align-items: center; gap: .7rem; flex-wrap: wrap; }
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
  @media(max-width:600px) { section { padding: 1rem; } }
</style>

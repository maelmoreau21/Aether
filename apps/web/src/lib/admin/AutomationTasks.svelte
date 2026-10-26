<script lang="ts">
  import { adminRequest } from './request';
  import type { Library } from './types';
  let { active = false, libraries }: { active?: boolean; libraries: Library[] } = $props();
  type Policy = { extensionId: string; libraryId: string; mode: string; priority: number; operation: string; configRevision: string; algorithmVersion: string; intervalSeconds: number; nextRunAt: number; maxConcurrent: number };
  type Extension = { manifest: { id: string; name: string; permissions: string[]; httpHosts?: string[] }; enabled: boolean };
  type History = { id: string; extensionId: string; libraryId: string; status: string; operation: string; priority: number; attempt: number; durationSeconds: number | null; error: string };
  type Quota = { provider: string; nextAllowedAt: number; minIntervalSeconds: number; maxConcurrent: number };
  type Source = { workId: string; field: string; extensionId: string; providerId: string; priority: number; updatedAt: number };
  let extensions = $state<Extension[]>([]);
  let policies = $state<Policy[]>([]);
  let history = $state<History[]>([]);
  let quotas = $state<Quota[]>([]);
  let sources = $state<Source[]>([]);
  let extensionId = $state('');
  let libraryId = $state('');
  let mode = $state('automatic');
  let hours = $state(24);
  let priority = $state(100);
  let busy = $state(false);
  let loaded = $state(false);
  let attempted = $state(false);
  let error = $state('');
  let notice = $state('');
  const usable = $derived(libraries.filter(l => !l.id.startsWith('fixture-')));
  async function refresh() {
    busy = true; error = '';
    try {
      const [installed, configured, recent] = await Promise.all([
        adminRequest<{ extensions: Extension[] }>('/api/v1/admin/extensions'),
        adminRequest<{ policies: Policy[] }>('/api/v1/admin/tasks/automation/policies'),
        adminRequest<{ tasks: History[]; quotas: Quota[]; sources: Source[] }>('/api/v1/admin/tasks/automation/history')
      ]);
      extensions = installed.extensions.filter(e => e.enabled && e.manifest.permissions.includes('metadata.read'));
      policies = configured.policies;
      history = recent.tasks; quotas = recent.quotas; sources = recent.sources;
      loaded = true;
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Chargement impossible.'; }
    finally { busy = false; }
  }
  $effect(() => { if (active && !attempted) { attempted = true; void refresh(); } });
  function selectPolicy() {
    const existing = policies.find(p => p.extensionId === extensionId && p.libraryId === libraryId);
    mode = existing?.mode ?? 'automatic'; hours = (existing?.intervalSeconds ?? 86400) / 3600; priority = existing?.priority ?? 100;
  }
  async function save() {
    busy = true; error = ''; notice = '';
    const previous = policies.find(p => p.extensionId === extensionId && p.libraryId === libraryId);
    const network = extensions.find(e => e.manifest.id === extensionId)?.manifest.httpHosts?.length;
    const policy: Policy = { extensionId, libraryId, mode, priority,
      operation: previous?.operation ?? (network ? 'metadata.search' : 'metadata.propose'),
      configRevision: previous?.configRevision ?? '', algorithmVersion: previous?.algorithmVersion ?? '1',
      intervalSeconds: Math.round(hours * 3600), nextRunAt: 0, maxConcurrent: 1 };
    try {
      await adminRequest('/api/v1/admin/tasks/automation/policies', { method: 'PUT', body: JSON.stringify(policy) });
      policies = [...policies.filter(p => p.extensionId !== extensionId || p.libraryId !== libraryId), policy];
      notice = 'Déclencheur enregistré.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Enregistrement impossible.'; }
    finally { busy = false; }
  }
  async function run(missingOnly = false) {
    busy = true; error = ''; notice = '';
    try {
      await adminRequest('/api/v1/admin/tasks/automation/run', { method: 'POST', body: JSON.stringify({ extensionId, libraryId, idempotencyKey: crypto.randomUUID(), missingOnly }) });
      notice = missingOnly ? 'Traitements manquants planifiés.' : 'Demande enregistrée dans la file. Les résultats inchangés sont réutilisés.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Admission impossible.'; }
    finally { busy = false; }
  }
</script>
<section aria-labelledby="automation-heading">
  <h3 id="automation-heading">Automatisation des métadonnées</h3>
  <p>Les extensions actives et autorisées traitent les nouveaux médias. Les corrections manuelles et la langue de la bibliothèque restent prioritaires.</p>
  {#if !loaded && busy}<p role="status">Chargement…</p>{/if}
  {#if loaded && !extensions.length}<p>Aucune extension de métadonnées active.</p>{/if}
  <form onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <label>Extension<select bind:value={extensionId} onchange={selectPolicy} disabled={busy} required><option value="">Choisir</option>{#each extensions as e (e.manifest.id)}<option value={e.manifest.id}>{e.manifest.name}</option>{/each}</select></label>
    <label>Bibliothèque<select bind:value={libraryId} onchange={selectPolicy} disabled={busy} required><option value="">Choisir</option>{#each usable as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
    <label>Déclenchement<select bind:value={mode} disabled={busy}><option value="automatic">Automatique après changement</option><option value="manual">Manuel</option><option value="scheduled">Programmé</option><option value="disabled">Désactivé</option></select></label>
    <label>Priorité<input type="number" min="0" max="1000" step="1" bind:value={priority} required disabled={busy} /></label>
    {#if mode === 'scheduled'}<label>Intervalle en heures<input type="number" min="1" max="8760" step="1" bind:value={hours} required disabled={busy} /></label>{/if}
    <button type="submit" disabled={busy || !extensionId || !libraryId}>Enregistrer</button>
    <button type="button" disabled={busy || !extensionId || !libraryId || mode === 'disabled'} onclick={() => void run()}>Exécuter maintenant</button>
    <button type="button" disabled={busy || !extensionId || !libraryId || mode === 'disabled'} onclick={() => void run(true)}>Relancer les manquants</button>
    <button type="button" disabled={busy} onclick={() => void refresh()}>Actualiser</button>
  </form>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if history.length}
    <h4>Derniers traitements</h4>
    <table><thead><tr><th>Extension</th><th>Bibliothèque</th><th>État</th><th>Tentatives</th><th>Durée</th><th>Échec</th></tr></thead><tbody>
      {#each history.slice(0, 25) as task (task.id)}<tr><td>{task.extensionId}</td><td>{task.libraryId}</td><td>{task.status}</td><td>{task.attempt}</td><td>{task.durationSeconds == null ? '—' : `${task.durationSeconds} s`}</td><td>{task.error || '—'}</td></tr>{/each}
    </tbody></table>
  {/if}
  {#if quotas.length}<h4>Quotas fournisseurs</h4><ul>{#each quotas as quota (quota.provider)}<li>{quota.provider} : intervalle {quota.minIntervalSeconds} s, {quota.maxConcurrent} en parallèle ; prochain appel après {new Date(quota.nextAllowedAt * 1000).toLocaleString()}</li>{/each}</ul>{/if}
  {#if sources.length}<h4>Provenance récente</h4><ul>{#each sources.slice(0, 15) as source (`${source.workId}:${source.field}`)}<li>{source.workId} · {source.field} : {source.extensionId}{source.providerId ? ` (${source.providerId})` : ''}, priorité {source.priority}</li>{/each}</ul>{/if}
</section>
<style>
  section { margin-top: 2rem; padding-top: 1rem; border-top: 1px solid var(--border); }
  form { display: flex; flex-wrap: wrap; align-items: end; gap: .8rem; }
  label { display: grid; gap: .35rem; }
  select, input, button { padding: .6rem; color: inherit; background: var(--surface); border: 1px solid var(--border); border-radius: .4rem; }
  p { max-width: 75ch; }
  table { width: 100%; border-collapse: collapse; margin-top: .8rem; }
  th, td { padding: .45rem; border-bottom: 1px solid var(--border); text-align: left; }
</style>

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
  const statuses: Record<string, string> = { queued:'En attente', running:'En cours', completed:'Terminée', succeeded:'Réussie', error:'Échec', failed:'Échec', cancelled:'Annulée', paused:'Suspendue', interrupted:'Interrompue' };
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
    <div class="history-list" role="region" aria-label="Derniers traitements des métadonnées">
      {#each history.slice(0, 25) as task (task.id)}<article><header><strong>{extensions.find(extension => extension.manifest.id === task.extensionId)?.manifest.name ?? 'Traitement de métadonnées'}</strong><span class="history-status" data-status={task.status}>{statuses[task.status] ?? task.status}</span></header><p>{libraries.find(library => library.id === task.libraryId)?.name ?? task.libraryId}</p><p class="history-meta">{task.attempt} tentative(s) · Durée : {task.durationSeconds == null ? 'indisponible' : `${task.durationSeconds} s`}</p>{#if task.error}<p class="history-error">{task.error}</p>{/if}<details><summary>Détails du traitement</summary><p>Extension : {task.extensionId}</p><p>Identifiant : {task.id} · Opération : {task.operation} · Priorité : {task.priority}</p></details></article>{/each}
    </div>
  {/if}
  {#if quotas.length}<h4>Quotas fournisseurs</h4><ul>{#each quotas as quota (quota.provider)}<li>{quota.provider} : intervalle {quota.minIntervalSeconds} s, {quota.maxConcurrent} en parallèle ; prochain appel après {new Date(quota.nextAllowedAt * 1000).toLocaleString()}</li>{/each}</ul>{/if}
  {#if sources.length}<h4>Provenance récente</h4><ul>{#each sources.slice(0, 15) as source (`${source.workId}:${source.field}`)}<li>{source.workId} · {source.field} : {source.extensionId}{source.providerId ? ` (${source.providerId})` : ''}, priorité {source.priority}</li>{/each}</ul>{/if}
</section>
<style>
  section { min-width: 0; padding-bottom: 16px; color: var(--tx); font-family: var(--font); }
  h3 { margin: 0 0 6px; font-size: 17px; font-weight: 700; }
  h4 { margin: 24px 0 8px; font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--mu); }
  p, li { max-width: 75ch; margin: 0 0 12px; font-size: 14px; line-height: 1.6; color: var(--mu); overflow-wrap: anywhere; }
  ul { margin: 0; padding-left: 20px; }
  li { margin-bottom: 6px; }

  /* Formulaire */
  form { display: flex; flex-wrap: wrap; align-items: end; gap: 12px; margin: 16px 0; }
  label { display: grid; gap: 8px; flex: 1 1 200px; min-width: 0; font-size: 14px; font-weight: 700; color: var(--tx); }
  select, input {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    max-width: 100%;
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
  select:hover, input:hover { background-color: var(--pn-2); }
  select:focus, input:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  select:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  select:disabled, input:disabled { opacity: 0.45; cursor: not-allowed; }

  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 44px; max-width: 100%; padding: 0 16px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font-family: var(--font); font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button[type='submit'] { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); box-shadow: none; }
  button[type='submit']:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }

  /* Retours */
  [role='alert'], [role='status'] { max-width: none; padding: 14px 16px; border-radius: var(--r-md); color: var(--tx); }
  [role='alert'] { background: var(--err-soft); }
  [role='status'] { background: var(--pn); }

  /* Historique : lignes séparées */
  .history-list { min-width: 0; border-top: 1px solid var(--line); }
  article { padding: 16px 0; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; }
  article header { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  article header strong { font-size: 15px; font-weight: 700; color: var(--tx); }
  article p { margin: 6px 0; }
  .history-status { display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 700; white-space: nowrap; }
  .history-status[data-status='completed'], .history-status[data-status='succeeded'] { background: var(--ok-soft); color: var(--ok); }
  .history-status[data-status='error'], .history-status[data-status='failed'] { background: var(--err-soft); color: var(--err); }
  .history-status[data-status='paused'], .history-status[data-status='interrupted'] { background: var(--warn-soft); color: var(--warn); }
  .history-status[data-status='running'], .history-status[data-status='queued'] { background: var(--ac-soft); color: var(--ac); }
  .history-meta, details { font-size: 13px; color: var(--mu); }
  .history-error { padding: 12px 14px; border-radius: var(--r-md); background: var(--err-soft); color: var(--tx); }
  details p { font-family: var(--font-mono); font-size: 12px; }
  summary { display: flex; align-items: center; min-height: 44px; cursor: pointer; font-weight: 700; color: var(--tx); }
  summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; border-radius: var(--r-xs); }

  @media (max-width: 640px) {
    select, input { font-size: 16px; }
    form > button { flex: 1 1 200px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button, select, input { transition: none; }
  }
</style>

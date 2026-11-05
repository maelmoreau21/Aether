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
  section { display: grid; gap: 16px; min-width: 0; padding-bottom: 8px; color: var(--color-text); font-family: var(--font-sans); }
  h3 { margin: 0; font-size: 18px; font-weight: 600; }
  h4 { margin: 8px 0 0; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  section > p { margin: 0; max-width: 75ch; font-size: 14px; line-height: 1.5; color: var(--color-muted); }
  p, li { max-width: 75ch; line-height: 1.5; overflow-wrap: anywhere; color: var(--color-muted); font-size: 14px; }

  /* Formulaire */
  form { display: flex; flex-wrap: wrap; align-items: end; gap: 12px; padding: 16px 20px; background: var(--color-raised); border-radius: var(--radius-md); }
  label { display: grid; gap: 6px; flex: 1 1 200px; min-width: 0; font-size: 13px; color: var(--color-muted); }
  select, input { box-sizing: border-box; width: 100%; min-width: 0; max-width: 100%; min-height: 40px; padding: 8px 12px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: 14px; }
  select:focus-visible, input:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  select:disabled, input:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Boutons : ghost, un seul primaire */
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 40px; max-width: 100%; padding: 8px 16px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 13px; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-surface-hover); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  button[type='submit'] { border-color: transparent; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  button[type='submit']:hover:not(:disabled) { background: var(--color-accent-hover); opacity: 0.9; }

  /* Retours */
  [role='alert'], [role='status'] { max-width: none; margin: 0; padding: 10px 14px; border-radius: var(--radius-sm); color: var(--color-text); font-size: 14px; }
  [role='alert'] { background: var(--color-danger-soft); }
  [role='status'] { background: transparent; color: var(--color-muted); padding-inline: 0; }

  /* Listes en lignes */
  ul { margin: 0; padding: 0; list-style: none; background: var(--color-surface); border-radius: var(--radius-md); }
  li { max-width: none; margin: 0; padding: 10px 20px; border-top: 1px solid var(--color-line-subtle); font-size: 13px; }
  li:first-child { border-top: 0; }
  .history-list { min-width: 0; padding: 4px 20px; background: var(--color-surface); border-radius: var(--radius-md); }
  article { padding: 14px 0; border-top: 1px solid var(--color-line-subtle); overflow-wrap: anywhere; }
  article:first-child { border-top: 0; }
  article header { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; flex-wrap: wrap; font-size: 14px; }
  article header strong { font-size: 15px; font-weight: 600; color: var(--color-text); }
  article p { margin: 4px 0 0; }
  .history-status { flex-shrink: 0; color: var(--color-muted); font-size: 13px; font-weight: 600; white-space: nowrap; }
  .history-status[data-status='completed'], .history-status[data-status='succeeded'] { color: var(--color-success); }
  .history-status[data-status='error'], .history-status[data-status='failed'] { color: var(--color-danger); }
  .history-status[data-status='paused'], .history-status[data-status='interrupted'] { color: var(--color-warning); }
  .history-status[data-status='running'], .history-status[data-status='queued'] { color: var(--color-accent); }
  .history-meta, details { font-size: 13px; color: var(--color-muted); }
  .history-error { padding: 10px 12px; border-radius: var(--radius-sm); background: var(--color-danger-soft); color: var(--color-text); }
  details p { font-family: var(--font-mono); font-size: 12px; }
  summary { min-height: 36px; padding-block: 8px; box-sizing: border-box; cursor: pointer; color: var(--color-accent); }
  summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-xs); }

  @media (max-width: 640px) {
    select, input { font-size: 16px; }
    form > button { flex: 1 1 200px; }
    li { padding-inline: 14px; }
    .history-list { padding-inline: 14px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button { transition: none; }
  }
</style>

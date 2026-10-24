<script lang="ts">
  import { untrack } from 'svelte';
  import { adminRequest } from './request';
  import type { AdminNode, Library } from './types';

  let { active, libraries, nodes }: { active: boolean; libraries: Library[]; nodes: AdminNode[] } = $props();
  type Job = {
    id: string; libraryId: string; owner: string; nodeId: string; status: string;
    category: string; priority: number; attempt: number; maxAttempts: number; progress: number; processedItems?: number;
    targetId?: string; resultJson?: string;
    createdAt: number; updatedAt: number; startedAt: number | null; finishedAt: number | null;
    leaseUntil: number | null; error: string;
  };
  type Page = { jobs: Job[]; limit: number; offset: number };
  const limit = 50;
  const labels: Record<string, string> = { queued: 'En attente', running: 'En cours', paused: 'Suspendue', completed: 'Terminée', cancelled: 'Annulée', error: 'Échec' };
  const categories: Record<string, string> = { scan: 'Scan incrémental', analyze: 'Analyse technique', prepare: 'Préparation multimédia', image: 'Traitement d’image' };
  let jobs = $state<Job[]>([]);
  let offset = $state(0);
  let libraryId = $state('');
  let category = $state('scan');
  let targetId = $state('');
  let component = $state('video');
  let width = $state(640);
  let loading = $state(false);
  let loaded = $state(false);
  let mutating = $state(false);
  let error = $state('');
  let notice = $state('');
  let observedAt = $state(0);
  let results = $state<Record<string, string>>({});
  let resultErrors = $state<Record<string, string>>({});
  let resultLoading = $state<Record<string, boolean>>({});
  let submission: { signature: string; key: string } | null = null;
  const availableLibraries = $derived(libraries.filter(library => library.enabled && !library.id.startsWith('fixture-')));

  async function refresh(nextOffset = offset) {
    if (loading) return;
    loading = true;
    try {
      const page = await adminRequest<Page>(`/api/v1/admin/tasks/jobs?limit=${limit}&offset=${nextOffset}`);
      jobs = page.jobs; offset = nextOffset; observedAt = Math.floor(Date.now() / 1000); loaded = true; error = '';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Le suivi des tâches est indisponible.'; }
    finally { loading = false; }
  }
  $effect(() => {
    if (!active) return;
    untrack(() => void refresh());
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void refresh();
    }, 5000);
    return () => clearInterval(timer);
  });
  async function enqueue(event: SubmitEvent) {
    event.preventDefault();
    if (!libraryId || mutating) return;
    if (category !== 'scan' && !targetId.trim()) return;
    mutating = true; error = ''; notice = '';
    try {
      const input = { libraryId, category, priority: 100, ...(category === 'scan' ? {} : { targetId: targetId.trim() }), ...(category === 'prepare' ? { component: component.trim() } : {}), ...(category === 'image' ? { width } : {}) };
      const signature = JSON.stringify(input);
      if (submission?.signature !== signature) submission = { signature, key: Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join('') };
      await adminRequest<Job>('/api/v1/admin/tasks/jobs', { method: 'POST', body: JSON.stringify({ ...input, idempotencyKey: submission.key }) });
      submission = null;
      notice = category === 'scan' ? 'Scan ajouté à la file persistante.' : 'Tâche ajoutée à la file persistante.';
      await refresh(0);
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Impossible de planifier la tâche.'; }
    finally { mutating = false; }
  }
  async function control(job: Job, action: 'pause' | 'resume' | 'cancel') {
    if (mutating) return;
    mutating = true; error = ''; notice = '';
    try {
      await adminRequest(`/api/v1/admin/tasks/jobs/${encodeURIComponent(job.id)}/${action}`, { method: 'POST' });
      notice = action === 'pause' ? 'Suspension demandée.' : action === 'resume' ? 'Tâche remise en attente.' : 'Annulation demandée.';
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Action impossible.'; }
    finally { mutating = false; }
  }
  async function loadResult(job: Job) {
    if (resultLoading[job.id]) return;
    resultLoading[job.id] = true; resultErrors[job.id] = '';
    try {
      const detail = await adminRequest<Job>(`/api/v1/admin/tasks/jobs/${encodeURIComponent(job.id)}`);
      results[job.id] = detail.resultJson ?? '{}';
    } catch (cause) { resultErrors[job.id] = cause instanceof Error ? cause.message : 'Résultat indisponible.'; }
    finally { resultLoading[job.id] = false; }
  }
  function date(value: number) { return new Date(value * 1000).toLocaleString(); }
  function duration(job: Job) {
    if (job.startedAt === null) return 'Pas encore démarrée';
    const seconds = Math.max(0, (job.finishedAt ?? observedAt) - job.startedAt);
    return seconds < 60 ? `${seconds} s` : `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
  }
</script>

<section aria-labelledby="distributed-tasks-heading">
  <header><h3 id="distributed-tasks-heading">File de tâches distribuées</h3><button disabled={loading} onclick={() => void refresh()}>Actualiser la file</button></header>
  <p class="explanation">Chaque tâche attend dans une file persistante et s’exécute sur le nœud qui possède les médias. La suspension et l’annulation prennent effet aux points d’arrêt du traitement.</p>
  <form onsubmit={enqueue}>
    <label>Bibliothèque<select aria-label="Bibliothèque cible" bind:value={libraryId} disabled={mutating}><option value="">Choisir une bibliothèque</option>{#each availableLibraries as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
    <label>Type de tâche<select aria-label="Type de tâche" bind:value={category} disabled={mutating}>{#each Object.entries(categories) as [value, title] (value)}<option {value}>{title}</option>{/each}</select></label>
    {#if category !== 'scan'}<label>{category === 'image' ? 'Identifiant de l’image inventoriée' : 'Identifiant de l’asset inventorié'}<input aria-label="Identifiant cible" required maxlength="128" bind:value={targetId} disabled={mutating} placeholder={category === 'image' ? 'ID de l’image' : 'ID de l’asset'} /></label>{/if}
    {#if category === 'prepare'}<label>Composant à préparer<input aria-label="Composant à préparer" required maxlength="64" pattern="video|audio-[0-9]+|subtitle-[0-9]+" bind:value={component} disabled={mutating} placeholder="video" /></label><p class="form-help">Formats : video, audio-1 ou subtitle-2. Utilisez l’index réel de la piste dans l’inventaire. La préparation ne réencode pas la vidéo.</p>{/if}
    {#if category === 'image'}<label>Largeur<select aria-label="Largeur de l’image" bind:value={width} disabled={mutating}><option value={320}>320 px</option><option value={640}>640 px</option><option value={1280}>1280 px</option></select></label>{/if}
    <button type="submit" disabled={!libraryId || mutating || (category !== 'scan' && !targetId.trim())}>{category === 'scan' ? 'Planifier un scan' : 'Planifier la tâche'}</button>
  </form>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if notice}<p class="notice" role="status">{notice}</p>{/if}
  {#if loaded}
    <div class="jobs" aria-busy={loading}>
      {#each jobs as job (job.id)}
        <article aria-label={`Tâche ${job.id}`}>
          <div class="job-heading"><strong>{libraries.find(library => library.id === job.libraryId)?.name ?? job.libraryId}</strong><span class="status">{labels[job.status] ?? job.status}</span></div>
          <dl>
            <div><dt>Propriétaire</dt><dd>{job.owner}</dd></div>
            <div><dt>Nœud exécutant</dt><dd>{nodes.find(node => node.id === job.nodeId)?.name ?? (job.nodeId === 'local' ? 'Nœud local' : job.nodeId || 'En attente d’attribution')}</dd></div>
            <div><dt>Durée</dt><dd>{duration(job)}</dd></div>
            <div><dt>Tentative</dt><dd>{job.attempt} / {job.maxAttempts}</dd></div>
          </dl>
          {#if job.processedItems === undefined || job.progress === 100}<div class="progress"><progress max="100" value={job.progress} aria-label="Progression de la tâche"></progress><span>{job.progress} %</span></div>{/if}
          {#if job.processedItems !== undefined}<p class="processed">{job.processedItems} éléments traités{job.progress < 100 ? ' · Total encore inconnu' : ''}</p>{/if}
          <details><summary>Détails de la tâche</summary><p>Identifiant : {job.id}</p><p>Créée le {date(job.createdAt)} · Mise à jour le {date(job.updatedAt)}</p><p>Catégorie : {categories[job.category] ?? job.category} · Priorité : {job.priority}</p>{#if job.targetId}<p>Cible : {job.targetId}</p>{/if}{#if job.leaseUntil}<p>Attribution valable jusqu’au {date(job.leaseUntil)}</p>{/if}</details>
          {#if job.category !== 'scan'}<details ontoggle={(event) => { if (event.currentTarget.open) void loadResult(job); }}><summary>Résultat du traitement</summary>{#if resultLoading[job.id]}<p role="status">Chargement du résultat…</p>{:else if resultErrors[job.id]}<p class="job-error" role="alert">{resultErrors[job.id]}</p><button onclick={() => void loadResult(job)}>Réessayer</button>{:else if results[job.id]}<pre>{results[job.id]}</pre>{/if}</details>{/if}
          {#if job.error}<p class="job-error">{job.error}</p>{/if}
          {#if ['queued', 'running', 'paused'].includes(job.status)}
            <div class="actions">
              {#if job.status === 'paused'}<button disabled={mutating || job.leaseUntil !== null} onclick={() => void control(job, 'resume')}>{job.leaseUntil !== null ? 'Arrêt en cours…' : 'Reprendre'}</button>{:else}<button disabled={mutating} onclick={() => void control(job, 'pause')}>Suspendre</button>{/if}
              <button disabled={mutating} onclick={() => void control(job, 'cancel')}>Annuler</button>
            </div>
          {/if}
        </article>
      {:else}<p class="empty">Aucune tâche sur cette page.</p>{/each}
    </div>
    <nav aria-label="Pages des tâches"><button disabled={loading || offset === 0} onclick={() => void refresh(Math.max(0, offset - limit))}>Précédent</button><span>Page {Math.floor(offset / limit) + 1}</span><button disabled={loading || jobs.length < limit} onclick={() => void refresh(offset + limit)}>Suivant</button></nav>
  {:else if loading}<p role="status">Chargement des tâches…</p>{/if}
</section>

<style>
  section { margin-top: 36px; min-width: 0; }
  header, .job-heading, .actions, nav, .progress { display: flex; align-items: center; gap: 12px; }
  header, .job-heading { justify-content: space-between; }
  h3 { margin: 0; font-size: 24px; font-weight: 400; }
  .explanation, dt, details, .empty { color: var(--text-secondary); line-height: 1.6; }
  form { display: flex; align-items: end; flex-wrap: wrap; gap: 12px; margin: 20px 0; }
  label { display: grid; gap: 8px; flex: 1; min-width: min(240px, 100%); }
  button, select, input { font: inherit; color: var(--text-primary); min-height: 44px; padding: 10px 14px; border: 1px solid var(--border-subtle); border-radius: var(--radius-md); background: var(--bg-surface); }
  select, input { width: 100%; min-width: 0; } .form-help { flex-basis: 100%; margin: 0; color: var(--text-secondary); font-size: 13px; line-height: 1.5; }
  button { flex-shrink: 0; } button:hover:not(:disabled) { background: var(--bg-elevated); }
  .jobs { display: grid; gap: 12px; } article { padding: 20px; background: var(--bg-surface); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); min-width: 0; overflow-wrap: anywhere; }
  .job-heading { align-items: start; } .status { color: var(--text-secondary); flex-shrink: 0; }
  dl { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 16px; font-size: 14px; } dd { margin: 4px 0 0; } dt { font-size: 12px; }
  .progress { margin: 16px 0; } progress { flex: 1; min-width: 0; accent-color: var(--accent-primary); } .progress span { font-variant-numeric: tabular-nums; font-size: 13px; }
  .processed { color: var(--text-secondary); font-size: 13px; }
  details { font-size: 13px; } details p { margin: 8px 0; } summary { cursor: pointer; min-height: 32px; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 320px; overflow: auto; font-size: 12px; }
  .actions { margin-top: 12px; flex-wrap: wrap; } nav { justify-content: end; margin-top: 16px; flex-wrap: wrap; } nav span { color: var(--text-secondary); font-size: 14px; }
  .error, .job-error, .notice { padding: 12px; line-height: 1.5; overflow-wrap: anywhere; } .error, .job-error { background: var(--color-danger-soft); border-left: 3px solid var(--status-error); } .notice { background: var(--color-success-soft); }
  @media (max-width: 640px) { header { flex-wrap: wrap; } dl { grid-template-columns: repeat(2, minmax(0, 1fr)); } article { padding: 14px; } .job-heading { flex-wrap: wrap; } }
</style>

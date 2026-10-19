<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import type { AdminNode, Library, Scan } from './types';
  import { scanLabels } from './types';

  interface Props {
    libraries: Library[];
    scans: Record<string, Scan>;
    pending: string[];
    nodes?: AdminNode[];
    onscan: (library: Library) => void;
  }

  let { libraries, scans, pending, nodes = [], onscan }: Props = $props();
  const scannable = $derived(libraries.filter(library => !library.id.startsWith('fixture-')));
  const activeCount = $derived(scannable.filter(library => busy(library.id)).length);

  type Schedule = { libraryId: string; intervalMinutes: number; enabled: number; nextRunAt: number; lastRunAt: number | null; lastOutcome: string };
  let schedules = $state<Record<string, Schedule>>({});
  let intervals = $state<Record<string, number>>({});
  let enabledPlans = $state<Record<string, boolean>>({});
  let scheduleError = $state('');
  let scheduleNotice = $state('');
  let savingSchedule = $state<string | null>(null);
  let schedulesLoaded = $state(false);
  async function loadSchedules() {
    try {
      const response = await fetch('/api/v1/admin/tasks/schedules', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
      if (!response.ok) throw new Error('Impossible de charger les planifications.');
      const data: Schedule[] = await response.json();
      schedules = Object.fromEntries(data.map(item => [item.libraryId, item]));
      intervals = Object.fromEntries(data.map(item => [item.libraryId, item.intervalMinutes]));
      enabledPlans = Object.fromEntries(data.map(item => [item.libraryId, item.enabled === 1]));
      schedulesLoaded = true;
      scheduleError = '';
    } catch (error) { scheduleError = error instanceof Error ? error.message : 'Chargement impossible.'; }
  }
  onMount(() => { void loadSchedules(); });
  async function saveSchedule(id: string) {
    if (!$session) return;
    savingSchedule = id; scheduleError = ''; scheduleNotice = '';
    try {
      const response = await fetch(`/api/v1/admin/tasks/schedules/${encodeURIComponent(id)}`, {
        method: 'PUT', credentials: 'same-origin', signal: AbortSignal.timeout(10000),
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
        body: JSON.stringify({ intervalMinutes: intervals[id] ?? 1440, enabled: enabledPlans[id] ?? false })
      });
      if (!response.ok) throw new Error(response.status === 403 ? 'Session ou droits insuffisants.' : 'Planification refusée (5 à 10 080 minutes).');
      await loadSchedules(); scheduleNotice = 'Planification enregistrée.';
    } catch (error) { scheduleError = error instanceof Error ? error.message : 'Enregistrement impossible.'; }
    finally { savingSchedule = null; }
  }
  function dateTime(value: number): string { return new Date(value * 1000).toLocaleString(); }
  const outcomeLabels: Record<string, string> = { never: 'Jamais exécuté', queued: 'En attente', running: 'En cours', completed: 'Terminé', error: 'Échec', interrupted: 'Interrompu', dispatching: 'Démarrage', already_running: 'Déjà en cours', unavailable: 'Bibliothèque indisponible', stopped: 'Serveur en arrêt' };

  function busy(id: string): boolean {
    return pending.includes(id) || scans[id]?.status === 'queued' || scans[id]?.status === 'running';
  }

  function unavailable(library: Library): string | null {
    if (!library.enabled) return 'Bibliothèque désactivée';
    const node = nodes.find(item => item.id === library.nodeId);
    if (library.nodeId !== 'local' && (!node || node.status !== 'connected')) return 'Media Node indisponible';
    return null;
  }
</script>

<section id="tasks-section" class="dashboard-section" aria-labelledby="tasks-heading">
  <header>
    <div>
      <h2 id="tasks-heading">Tâches</h2>
      <p>Déclenchez l'indexation de vos bibliothèques et consultez leur dernier état connu.</p>
    </div>
    <span class="badge">{activeCount} en attente ou en cours</span>
  </header>

  <aside aria-labelledby="scheduling-heading">
    <h3 id="scheduling-heading">Scans automatiques</h3>
    <p>Choisissez un intervalle par bibliothèque. Les horaires sont conservés au redémarrage. Une exécution manquée est reprise sans multiplier les scans ; les scans actifs sont prioritaires.</p>
    <button type="button" onclick={() => void loadSchedules()}>Actualiser les planifications</button>
    {#if scheduleError}<p role="alert">{scheduleError}</p>{/if}
    {#if scheduleNotice}<p role="status">{scheduleNotice}</p>{/if}
  </aside>

  {#if scannable.length === 0}
    <div class="empty">
      <h3>Aucune bibliothèque à scanner</h3>
      <p>Ajoutez une bibliothèque dans la section Bibliothèques pour lancer une indexation.</p>
    </div>
  {:else}
    <div class="task-list">
      {#each scannable as library (library.id)}
        {@const scan = scans[library.id]}
        {@const reason = unavailable(library)}
        <article aria-labelledby={`task-${library.id}`}>
          <div class="task-top">
            <div>
              <h3 id={`task-${library.id}`}>Indexer {library.name}</h3>
              <p>{library.kind === 'series' ? 'Séries' : 'Films'} · {library.nodeId === 'local' ? 'Node local' : nodes.find(node => node.id === library.nodeId)?.name ?? 'Node distant'}</p>
            </div>
            <span class="badge" data-status={scan?.status ?? 'unknown'} role="status">
              {pending.includes(library.id) ? 'Envoi de la demande…' : scan ? scanLabels[scan.status] : 'État indisponible'}
            </span>
          </div>
          {#if scan}
            <dl>
              <div><dt>Fichiers découverts</dt><dd>{scan.discovered}</dd></div>
              <div><dt>Analysés</dt><dd>{scan.probed}</dd></div>
              <div><dt>Analyses réutilisées</dt><dd>{scan.reused}</dd></div>
              <div><dt>Erreurs</dt><dd>{scan.errorsCount ?? scan.errors.length}</dd></div>
              <div><dt>Avertissements</dt><dd>{scan.warningsCount ?? 'Indisponible'}</dd></div>
            </dl>
            {#if scan.errors.length > 0}
              <details>
                <summary>Consulter les anomalies ({scan.errors.length})</summary>
                <ul>{#each scan.errors as issue, index (index)}<li>{issue}</li>{/each}</ul>
              </details>
            {/if}
          {/if}
          <form class="schedule" onsubmit={(event) => { event.preventDefault(); void saveSchedule(library.id); }}>
            <label><input type="checkbox" bind:checked={enabledPlans[library.id]} disabled={!schedulesLoaded} /> Activer le scan périodique</label>
            <label>Intervalle en minutes (5 à 10 080)
              <input type="number" min="5" max="10080" step="1" value={intervals[library.id] ?? 1440} oninput={(event) => intervals[library.id] = Number(event.currentTarget.value)} required disabled={!schedulesLoaded} />
            </label>
            <button type="submit" disabled={!schedulesLoaded || savingSchedule === library.id}>{savingSchedule === library.id ? 'Enregistrement…' : 'Enregistrer la planification'}</button>
            {#if schedules[library.id]}
              {@const plan = schedules[library.id]}
              <p>Prochaine échéance : {plan.enabled === 1 ? dateTime(plan.nextRunAt) : 'Désactivée'} · Dernier lancement : {plan.lastRunAt ? dateTime(plan.lastRunAt) : 'Jamais'} · {outcomeLabels[plan.lastOutcome] ?? plan.lastOutcome}</p>
            {/if}
          </form>
          <div class="task-action">
            <p>{reason ?? (busy(library.id) ? 'Le scan continue sur le Media Node.' : 'Lecture seule des médias. Les fichiers inchangés réutilisent leur analyse.')}</p>
            <button type="button" disabled={!!reason || busy(library.id)} onclick={() => onscan(library)}>
              {busy(library.id) ? 'Scan en cours…' : `Scanner ${library.name}`}
            </button>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .schedule { display: flex; flex-wrap: wrap; align-items: center; gap: 1rem; margin-top: 1rem; padding-top: 1rem; border-top: 1px solid var(--color-line); }
  .schedule label { display: flex; align-items: center; gap: .5rem; font-size: .85rem; }
  .schedule input[type="number"] { width: 7rem; padding: .5rem; background: var(--color-raised); color: var(--color-text); border: 1px solid var(--color-line-strong); border-radius: .4rem; }
  .schedule p { width: 100%; }
  section { color: var(--color-text); }
  header, .task-top, .task-action { display: flex; align-items: center; justify-content: space-between; gap: 1rem; }
  header { margin-bottom: 1.5rem; }
  h2 { margin: 0; font-size: 1.4rem; }
  h3 { margin: 0; font-size: 1rem; }
  p { margin: .5rem 0 0; color: var(--color-muted); font-size: .9rem; line-height: 1.6; }
  aside, article, .empty { padding: 1.4rem; background: var(--color-surface); border: 1px solid var(--color-line); border-radius: 1rem; }
  aside { margin-bottom: 1.25rem; border-left: 3px solid var(--color-warning); }
  .task-list { display: grid; gap: 1rem; }
  .badge { flex-shrink: 0; padding: .4rem .65rem; border-radius: .5rem; background: var(--color-raised); font-size: .8rem; }
  .badge[data-status='running'], .badge[data-status='queued'] { color: var(--color-text); border: 1px solid var(--color-warning); }
  .badge[data-status='error'] { border: 1px solid var(--color-danger); }
  .badge[data-status='completed'] { border: 1px solid var(--color-success); }
  dl { display: flex; flex-wrap: wrap; gap: 1rem 2rem; margin: 1.25rem 0; }
  dt { font-size: .8rem; color: var(--color-muted); }
  dd { margin: .3rem 0 0; font-size: 1.15rem; font-weight: 600; }
  details { margin: 1rem 0; font-size: .9rem; }
  summary { cursor: pointer; }
  li { overflow-wrap: anywhere; margin: .5rem 0; }
  .task-action { padding-top: 1rem; margin-top: 1rem; border-top: 1px solid var(--color-line); }
  .task-action p { margin: 0; }
  button { flex-shrink: 0; padding: .65rem .9rem; border: 1px solid var(--color-line-strong); border-radius: .5rem; color: var(--color-text); background: var(--color-raised); cursor: pointer; font: inherit; font-size: .85rem; }
  button:hover:not(:disabled) { background: var(--color-raised-hover); }
  button:disabled { opacity: .55; cursor: not-allowed; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 3px; }
  @media (max-width: 640px) {
    header, .task-top, .task-action { align-items: flex-start; flex-direction: column; }
    .task-action button { width: 100%; }
    aside, article, .empty { padding: 1rem; }
  }
</style>

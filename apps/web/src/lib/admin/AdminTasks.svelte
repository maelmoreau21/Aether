<script lang="ts">
  import { untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import AutomationTasks from './AutomationTasks.svelte';
  import DistributedTasks from './DistributedTasks.svelte';
  import { adminRequest } from './request';
  import type { AdminNode, Library, Scan } from './types';
  import { scanLabels } from './types';

  let { libraries, scans, pending, nodes = [], onscan, selectedId = null, active = false }: {
    libraries: Library[]; scans: Record<string, Scan>; pending: string[]; nodes?: AdminNode[];
    onscan: (library: Library) => void; selectedId?: string | null; active?: boolean;
  } = $props();

  type Schedule = { libraryId: string; intervalMinutes: number; enabled: number; nextRunAt: number; lastRunAt: number | null; lastOutcome: string };
  type Backup = { available: boolean; reason?: string | null; running: boolean; schedule: { enabled: boolean; intervalMinutes: number; nextDueAt: number; lastStartedAt: number | null; lastCompletedAt: number | null; lastOutcome: string } };
  type SystemTask = { id: string; name: string; description: string; category: string; intervalMinutes: number; enabled: number; nextDueAt: number; lastStartedAt: number | null; lastCompletedAt: number | null; lastOutcome: string; lastErrorKind?: string | null; lastErrorScope?: string | null; lastMessage?: string | null };
  type Task = {
    id: string;
    systemId?: string;
    name: string;
    description: string;
    library?: Library;
    category: string;
    running: boolean;
    reason: string | null;
    plan: { enabled: boolean; interval: number; next: number; last: number | null; outcome: string } | null;
  };

  let schedules = $state<Record<string, Schedule>>({});
  let backup = $state<Backup | null>(null);
  let systemTasks = $state<SystemTask[]>([]);
  let runningSystemTasks = $state<Record<string, boolean>>({});

  let scanError = $state('');
  let backupError = $state('');
  let systemTasksError = $state('');
  let error = $state('');
  let notice = $state('');
  let loaded = $state(false);
  let refreshing = $state(false);
  let saving = $state(false);
  let creatingBackup = $state(false);
  let editor = $state<HTMLDialogElement>();
  let frequency = $state(24);
  let unit = $state(60);
  let triggerEnabled = $state(true);
  let queueOpen = $state(false);
  let refreshPromise: Promise<void> | undefined;

  const tasks = $derived<Task[]>([
    ...libraries.filter(l => !l.id.startsWith('fixture-')).map(library => {
      const plan = schedules[library.id];
      return {
        id: `scan-${library.id}`,
        name: `Indexer ${library.name}`,
        category: 'Bibliothèques',
        library,
        description: `Analyse les fichiers ${library.kind === 'series' ? 'de séries' : library.kind === 'music' ? 'de musique' : 'de films'} et actualise le catalogue. Les médias restent en lecture seule.`,
        running: pending.includes(library.id) || ['queued', 'running'].includes(scans[library.id]?.status),
        reason: !library.enabled ? 'Bibliothèque désactivée.' : library.nodeId !== 'local' && !nodes.some(n => n.id === library.nodeId && n.status === 'connected') ? 'Media Node indisponible.' : null,
        plan: plan ? { enabled: plan.enabled === 1, interval: plan.intervalMinutes, next: plan.nextRunAt, last: plan.lastRunAt, outcome: plan.lastOutcome } : null
      };
    }),
    ...systemTasks.filter(st => st.category === 'Extensions').map(st => ({
      id: `system-${st.id}`,
      systemId: st.id,
      name: st.name,
      category: 'Extensions',
      description: st.description,
      running: !!runningSystemTasks[st.id] || ['queued', 'running'].includes(st.lastOutcome),
      reason: null,
      plan: { enabled: st.enabled === 1, interval: st.intervalMinutes, next: st.nextDueAt, last: st.lastStartedAt, outcome: st.lastOutcome }
    })),
    {
      id: 'backup',
      name: 'Sauvegarder le serveur',
      category: 'Maintenance',
      description: 'Crée une archive non chiffrée de la base et de la configuration du serveur. Réservez son accès à l’administrateur. Les fichiers médias ne sont pas inclus.',
      running: creatingBackup || backup?.running === true,
      reason: backupError ? 'État des sauvegardes indisponible.' : !backup ? 'Chargement des sauvegardes…' : !backup.available ? (backup.reason ?? 'Sauvegardes indisponibles.') : null,
      plan: backup ? { enabled: backup.schedule.enabled, interval: backup.schedule.intervalMinutes, next: backup.schedule.nextDueAt, last: backup.schedule.lastStartedAt, outcome: backup.schedule.lastOutcome } : null
    },
    ...systemTasks.filter(st => st.category !== 'Extensions').map(st => ({
      id: `system-${st.id}`,
      systemId: st.id,
      name: st.name,
      category: st.category,
      description: st.description,
      running: !!runningSystemTasks[st.id] || ['queued', 'running'].includes(st.lastOutcome),
      reason: null,
      plan: { enabled: st.enabled === 1, interval: st.intervalMinutes, next: st.nextDueAt, last: st.lastStartedAt, outcome: st.lastOutcome }
    }))
  ]);

  const selected = $derived(tasks.find(task => task.id === selectedId));
  const selectedScan = $derived(selected?.library ? scans[selected.library.id] : undefined);
  const selectedSystemTask = $derived(selected?.systemId ? systemTasks.find(st => st.id === selected.systemId) : undefined);

  const outcomes: Record<string, string> = {
    never: 'Jamais exécutée',
    queued: 'En attente',
    running: 'En cours',
    completed: 'Terminée',
    error: 'Échec',
    interrupted: 'Interrompue',
    dispatching: 'Démarrage',
    already_running: 'Déjà en cours',
    unavailable: 'Bibliothèque indisponible',
    stopped: 'Serveur en arrêt',
    succeeded: 'Réussie',
    failed: 'Échec', paused: 'Suspendue', cancelled: 'Annulée'
  };

  const categories = $derived([...new Set(tasks.map(t => t.category))]);

  function taskHref(id?: string) { return resolve(`admin/libraries/#tasks-section${id ? `?task=${encodeURIComponent(id)}` : ''}`); }
  function date(value: number) { return new Date(value * 1000).toLocaleString(); }
  function intervalLabel(minutes: number) {
    // These plans repeat after a duration; they do not run at a fixed calendar hour.
    if (minutes % 1440 === 0) return `Toutes les ${minutes / 60} heures`;
    const count = minutes % 60 === 0 ? minutes / 60 : minutes;
    const suffix = minutes % 60 === 0 ? 'heure' : 'minute';
    return count === 1 ? `Chaque ${suffix}` : `Toutes les ${count} ${suffix}s`;
  }

  function lastRun(task: Task) {
    if (!loaded) return 'Chargement du dernier lancement…';
    if (task.library && scanError) return 'Dernier lancement indisponible';
    if (task.id === 'backup' && backupError) return 'Dernier lancement indisponible';
    if (task.systemId && systemTasksError) return 'Dernier lancement indisponible';
    return task.plan?.last ? `${task.library ? 'Dernier lancement planifié' : 'Dernier lancement'} : ${date(task.plan.last)}` : 'Aucun lancement enregistré';
  }
  function executionLabel(task: Task) {
    if (task.plan && ['queued', 'running', 'paused'].includes(task.plan.outcome)) return outcomes[task.plan.outcome];
    return task.running ? 'Démarrage…' : task.plan ? outcomes[task.plan.outcome] ?? task.plan.outcome : 'Aucune exécution planifiée';
  }

  function refresh(): Promise<void> {
    if (!refreshPromise) refreshPromise = loadSchedules().finally(() => { refreshPromise = undefined; });
    return refreshPromise;
  }
  async function loadSchedules() {
    refreshing = true;
    const [plans, archives, sys] = await Promise.allSettled([
      adminRequest<Schedule[]>('/api/v1/admin/tasks/schedules'),
      adminRequest<Backup>('/api/v1/admin/backups'),
      adminRequest<SystemTask[]>('/api/v1/admin/tasks/system')
    ]);
    if (plans.status === 'fulfilled') {
      schedules = Object.fromEntries(plans.value.map(p => [p.libraryId, p]));
      scanError = '';
    } else {
      scanError = 'Les planifications des bibliothèques sont indisponibles. Actualisez pour réessayer.';
    }

    if (archives.status === 'fulfilled') {
      backup = archives.value;
      backupError = '';
    } else {
      backupError = 'Les sauvegardes sont indisponibles. Actualisez pour réessayer.';
    }

    if (sys.status === 'fulfilled') {
      systemTasks = sys.value;
      systemTasksError = '';
    } else {
      systemTasksError = 'Les tâches système planifiées sont indisponibles.';
    }

    loaded = true;
    refreshing = false;
  }

  $effect(() => {
    if (!active) return;
    untrack(() => void refresh());
    const update = () => { if (document.visibilityState === 'visible' && !saving && !creatingBackup) void refresh(); };
    const timer = setInterval(update, 10000);
    document.addEventListener('visibilitychange', update);
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', update); };
  });
  $effect(() => { if (selectedId !== undefined) { error = ''; notice = ''; editor?.close(); } });

  function editTrigger() {
    if (!selected) return;
    const interval = selected.plan?.interval ?? 1440;
    unit = interval % 1440 === 0 ? 1440 : interval % 60 === 0 ? 60 : 1;
    frequency = interval / unit;
    triggerEnabled = selected.plan?.enabled ?? true;
    error = '';
    editor?.showModal();
  }

  async function saveTrigger(event: SubmitEvent) {
    event.preventDefault();
    const task = selected;
    if (!task || saving) return;
    const minutes = frequency * unit;
    const minAllowed = task.library || task.systemId ? 5 : 60;
    if (!Number.isInteger(minutes) || minutes < minAllowed || minutes > 10080) {
      error = 'Intervalle invalide. Vérifiez les limites indiquées.';
      return;
    }
    saving = true;
    error = '';
    notice = '';
    try {
      await refreshPromise;
      if (task.library) {
        await adminRequest(`/api/v1/admin/tasks/schedules/${encodeURIComponent(task.library.id)}`, {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: minutes, enabled: triggerEnabled })
        });
      } else if (task.systemId) {
        await adminRequest(`/api/v1/admin/tasks/system/${encodeURIComponent(task.systemId)}/schedule`, {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: minutes, enabled: triggerEnabled })
        });
      } else {
        await adminRequest('/api/v1/admin/backups/schedule', {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: minutes, enabled: triggerEnabled })
        });
      }
      await refresh();
      editor?.close();
      notice = triggerEnabled ? 'Intervalle activé et enregistré. La prochaine échéance part de cet enregistrement.' : 'Intervalle enregistré et désactivé. Le lancement manuel reste disponible.';
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Enregistrement impossible.';
    } finally {
      saving = false;
    }
  }

  async function removeTrigger() {
    const task = selected;
    if (!task || saving) return;
    saving = true;
    error = '';
    notice = '';
    try {
      await refreshPromise;
      if (task.library) {
        await adminRequest(`/api/v1/admin/tasks/schedules/${encodeURIComponent(task.library.id)}`, { method: 'PUT', body: JSON.stringify({ intervalMinutes: task.plan?.interval ?? 1440, enabled: false }) });
      } else if (task.systemId) {
        await adminRequest(`/api/v1/admin/tasks/system/${encodeURIComponent(task.systemId)}/schedule`, {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: task.plan?.interval ?? 1440, enabled: false })
        });
      } else {
        await adminRequest('/api/v1/admin/backups/schedule', {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: task.plan?.interval ?? 1440, enabled: false })
        });
      }
      await refresh();
      notice = 'Intervalle désactivé. Les résultats sont conservés et le lancement manuel reste disponible.';
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Suppression impossible.';
    } finally {
      saving = false;
    }
  }

  async function run(task: Task) {
    if (task.running || task.reason) return;
    if (task.library) {
      onscan(task.library);
      return;
    }
    if (task.systemId) {
      runningSystemTasks[task.systemId] = true;
      error = '';
      notice = '';
      try {
        await refreshPromise;
        await adminRequest(`/api/v1/admin/tasks/system/${encodeURIComponent(task.systemId)}/run`, { method: 'POST' });
        notice = `Tâche ${task.name} ajoutée à la file d’exécution.`;
        await refresh();
      } catch (cause) {
        error = cause instanceof Error ? cause.message : 'Démarrage de la tâche impossible.';
      } finally {
        runningSystemTasks[task.systemId] = false;
      }
      return;
    }
    creatingBackup = true;
    error = '';
    notice = '';
    try {
      await refreshPromise;
      await adminRequest('/api/v1/admin/backups', { method: 'POST' });
      notice = 'Sauvegarde créée.';
      await refresh();
    } catch (cause) {
      error = `${cause instanceof Error ? cause.message : 'Sauvegarde impossible.'} Actualisez son état avant de relancer : elle peut continuer côté serveur.`;
    } finally {
      creatingBackup = false;
    }
  }
</script>

<section id="tasks-section" aria-labelledby="tasks-heading">
  {#if selectedId}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- taskHref resolves the base path and task hash. -->
    <a class="back" href={taskHref()}><Icon name="back" size={24} /><span>Retour aux tâches</span></a>
    {#if selected}
      <header class="detail-header">
        <h2 id="tasks-heading">{selected.name}</h2>
        <button class="run" aria-label={`Exécuter ${selected.name}`} disabled={selected.running || !!selected.reason || !loaded} onclick={() => void run(selected)}>
          <Icon name="play" size={22} />
        </button>
      </header>
      <p class="description">{selected.description}</p>
      <button class="refresh" disabled={refreshing} onclick={() => void refresh()}><Icon name="refresh" size={18} /> Actualiser l’état</button>
      {#if selected.reason}<p class="reason">{selected.reason} {#if !selected.library && !selected.systemId}<a href={resolve('admin/libraries/#backups-section')}>Ouvrir les sauvegardes</a>{/if}</p>{/if}
      <button class="primary" disabled={!loaded || saving || refreshing || !!(selected.library ? scanError : selected.systemId ? systemTasksError : backupError) || (selected.id === 'backup' && !backup?.available)} onclick={editTrigger}>
        <span aria-hidden="true">+</span> {selected.plan ? 'Configurer l’intervalle' : 'Ajouter un intervalle'}
      </button>
      <div class="trigger-list" aria-label="Déclencheurs">
        <h3>Intervalle</h3>
        {#if selected.plan?.enabled}
          <div class="trigger-row">
            <div>
              <p>{intervalLabel(selected.plan.interval)}</p>
              <small>Prochaine échéance : {date(selected.plan.next)}</small>
            </div>
            <button class="remove" disabled={saving || refreshing} aria-label="Désactiver l’intervalle" title="Désactiver l’intervalle" onclick={() => void removeTrigger()}>−</button>
          </div>
        {:else}
          <p class="no-trigger">Aucun déclencheur actif. Cette tâche peut être lancée manuellement.</p>
          {#if selected.plan}<p>Intervalle conservé : {intervalLabel(selected.plan.interval)}.</p>{/if}
        {/if}
      </div>
      <div class="results">
        <h3>Dernier résultat</h3>
        <p>{lastRun(selected)}</p>
        <p>{executionLabel(selected)}</p>
        {#if selectedSystemTask?.lastMessage}
          <p class:error={selectedSystemTask.lastOutcome === 'failed'}>{selectedSystemTask.lastMessage}</p>
          {#if selectedSystemTask.lastErrorKind}<p>Type : {selectedSystemTask.lastErrorKind}{selectedSystemTask.lastErrorScope ? ` · ${selectedSystemTask.lastErrorScope}` : ''}</p>{/if}
        {/if}
        {#if selected.id === 'backup' && backup?.schedule.lastStartedAt && backup.schedule.lastCompletedAt && backup.schedule.lastCompletedAt >= backup.schedule.lastStartedAt}
          <p>Durée : {backup.schedule.lastCompletedAt - backup.schedule.lastStartedAt} secondes</p>
        {/if}
        {#if selectedSystemTask && selectedSystemTask.lastStartedAt && selectedSystemTask.lastCompletedAt && selectedSystemTask.lastCompletedAt >= selectedSystemTask.lastStartedAt}
          <p>Durée : {selectedSystemTask.lastCompletedAt - selectedSystemTask.lastStartedAt} secondes</p>
        {/if}
        {#if selectedScan}
          <p>Dernier scan connu : {scanLabels[selectedScan.status]}</p>
          <dl>
            <div><dt>Découverts</dt><dd>{selectedScan.discovered}</dd></div>
            <div><dt>Analysés</dt><dd>{selectedScan.probed}</dd></div>
            <div><dt>Réutilisés</dt><dd>{selectedScan.reused}</dd></div>
            <div><dt>Erreurs</dt><dd>{selectedScan.errorsCount ?? selectedScan.errors.length}</dd></div>
          </dl>
          {#if selectedScan.errors.length}
            <details>
              <summary>Consulter les anomalies</summary>
              <ul>
                {#each selectedScan.errors as issue, i (i)}
                  <li>{issue}</li>
                {/each}
              </ul>
            </details>
          {/if}
        {/if}
      </div>
    {:else if !loaded}
      <h2 id="tasks-heading">Chargement de la tâche…</h2>
      <p role="status">Lecture des planifications du serveur.</p>
    {:else if selectedId.startsWith('system-') && systemTasksError}
      <h2 id="tasks-heading">État de la tâche indisponible</h2>
      <button class="refresh" disabled={refreshing} onclick={() => void refresh()}>Réessayer</button>
    {:else}
      <h2 id="tasks-heading">Tâche introuvable</h2>
      <p>Cette tâche a peut-être été supprimée ou renommée.</p>
    {/if}
  {:else}
    <header class="list-header">
      <h2 id="tasks-heading">Tâches et indexations</h2>
    </header>
    <details class="plans-section" open><summary>Planifications et lancement manuel</summary><p class="section-intro">Choisissez une opération pour régler son intervalle ou la lancer maintenant. L’état s’actualise toutes les 10 secondes tant que cette page est visible.</p><button class="refresh" disabled={refreshing} onclick={() => void refresh()}><Icon name="refresh" size={18} /> Actualiser les planifications</button>
    {#each categories as category (category)}
      <div class="category">
        <h3>{category}</h3>
        <div class="task-list">
          {#each tasks.filter(task => task.category === category) as task (task.id)}
            <div class="task-row">
              <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- taskHref resolves the base path and task hash. -->
              <a class="task-link" href={taskHref(task.id)} aria-label={`Configurer ${task.name}`}>
                <span class="clock"><Icon name="clock" size={26} /></span>
                <span class="task-copy">
                  <span class="task-title">{task.name}</span>
                  <span class="task-subtitle">{task.plan?.enabled ? `${intervalLabel(task.plan.interval)} · prochaine échéance ${date(task.plan.next)}` : 'Lancement manuel · aucun intervalle actif'}</span>
                  <span class="task-subtitle task-status" data-outcome={task.running ? 'running' : task.plan?.outcome}>{executionLabel(task)}</span>
                  <span class="task-subtitle">{lastRun(task)}</span>
                  {#if task.reason}<span class="task-subtitle">{task.reason}</span>{/if}
                </span>
              </a>
              <button class="run" aria-label={`Exécuter ${task.name}`} title="Exécuter maintenant" disabled={task.running || !!task.reason || !loaded} onclick={() => void run(task)}>
                <Icon name={task.running ? 'refresh' : 'play'} size={22} />
              </button>
            </div>
          {:else}
            <p class="no-trigger">Aucune tâche disponible dans cette catégorie.</p>
          {/each}
        </div>
      </div>
    {/each}
    </details>
    <details class="queue-section" bind:open={queueOpen}><summary>File d’exécution et opérations en cours</summary><DistributedTasks active={active && queueOpen} {libraries} {nodes} /></details>
    <details class="automation-section"><summary>Automatisation des métadonnées</summary><AutomationTasks {active} {libraries} /></details>
  {/if}
  {#if scanError}<p class="feedback error" role="alert">{scanError}</p>{/if}
  {#if backupError}<p class="feedback error" role="alert">{backupError}</p>{/if}
  {#if systemTasksError}<p class="feedback error" role="alert">{systemTasksError}</p>{/if}
  {#if error && !editor?.open}<p class="feedback error" role="alert">{error}</p>{/if}
  {#if notice}<p class="feedback" role="status">{notice}</p>{/if}
</section>

<dialog bind:this={editor} aria-labelledby="trigger-title" oncancel={(event) => { if (saving) event.preventDefault(); }}>
  <form onsubmit={saveTrigger}>
    <h2 id="trigger-title">Déclencheur périodique</h2>
    <p>{selected?.name}</p>
    <label class="enabled"><input type="checkbox" bind:checked={triggerEnabled} disabled={saving} /> Activer cet intervalle</label>
    <label>Toutes les<input type="number" min={Math.max(1, Math.ceil((selected?.library || selected?.systemId ? 5 : 60) / unit))} max={Math.floor(10080 / unit)} step="1" required bind:value={frequency} disabled={saving} /></label>
    <label>Unité
      <select aria-label="Unité" bind:value={unit} disabled={saving}>
        <option value={1}>Minutes</option>
        <option value={60}>Heures</option>
        <option value={1440}>Jours</option>
      </select>
    </label>
    <p>De {selected?.library || selected?.systemId ? '5 minutes' : '1 heure'} à 7 jours. Un déclencheur périodique par tâche.</p>
    <p>Après enregistrement, la prochaine échéance part de maintenant. Un intervalle de 24 heures ne fixe pas une heure quotidienne.</p>
    {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
    <div class="dialog-actions">
      <button type="button" disabled={saving} onclick={() => editor?.close()}>Annuler</button>
      <button class="primary" type="submit" disabled={saving}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button>
    </div>
  </form>
</dialog>

<style>
  section { min-width: 0; color: var(--tx); font-family: var(--font); }

  /* Titres */
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; }
  .list-header, .detail-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; }

  /* Sections repliables */
  .plans-section, .automation-section, .queue-section { margin-top: 20px; padding: 0 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .plans-section { padding-bottom: 24px; }
  .plans-section:not([open]), .automation-section:not([open]), .queue-section:not([open]) { padding-bottom: 0; }
  .automation-section[open], .queue-section[open] { padding-bottom: 8px; }
  .plans-section > summary, .automation-section > summary, .queue-section > summary { display: flex; align-items: center; gap: 10px; min-height: 64px; cursor: pointer; font-size: 17px; font-weight: 700; color: var(--tx); list-style: none; }
  .plans-section > summary::-webkit-details-marker, .automation-section > summary::-webkit-details-marker, .queue-section > summary::-webkit-details-marker { display: none; }
  .plans-section > summary::after, .automation-section > summary::after, .queue-section > summary::after { content: ''; width: 8px; height: 8px; margin-left: auto; border-right: 2px solid var(--mu); border-bottom: 2px solid var(--mu); transform: rotate(45deg); transition: transform var(--t-med) var(--ease); }
  .plans-section[open] > summary::after, .automation-section[open] > summary::after, .queue-section[open] > summary::after { transform: rotate(-135deg); }
  summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; border-radius: var(--r-xs); }
  .section-intro { margin: 0 0 16px; max-width: 75ch; color: var(--mu); font-size: 14px; line-height: 1.6; }

  /* Catégories et lignes de tâches */
  .category { margin-top: 28px; }
  .category > h3 { margin: 0 0 8px; font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--mu); }
  .task-list { border-top: 1px solid var(--line); }
  .task-row { display: flex; align-items: center; gap: 12px; min-height: 88px; padding-right: 8px; border-bottom: 1px solid var(--line); border-radius: 0; transition: background-color var(--t-fast) ease; }
  .task-row:hover { background: var(--pn); }
  .task-link { display: flex; align-items: center; gap: 16px; flex: 1; min-width: 0; padding: 16px 8px; color: var(--tx); border-radius: var(--r-sm); }
  .task-link:focus-visible { outline: 2px solid var(--ac); outline-offset: -2px; }
  .clock { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: 14px; background: var(--pn); color: var(--mu); }
  .task-copy { display: grid; justify-items: start; gap: 4px; min-width: 0; overflow-wrap: anywhere; }
  .task-title { font-size: 15px; font-weight: 700; color: var(--tx); }
  .task-subtitle { font-size: 13px; line-height: 1.45; color: var(--mu); }
  .task-status { display: inline-flex; align-items: center; height: 24px; margin: 2px 0; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 700; line-height: 1; white-space: nowrap; }
  .task-status[data-outcome='completed'], .task-status[data-outcome='succeeded'] { background: var(--ok-soft); color: var(--ok); }
  .task-status[data-outcome='error'], .task-status[data-outcome='failed'] { background: var(--err-soft); color: var(--err); }
  .task-status[data-outcome='interrupted'], .task-status[data-outcome='unavailable'], .task-status[data-outcome='stopped'], .task-status[data-outcome='paused'] { background: var(--warn-soft); color: var(--warn); }
  .task-status[data-outcome='running'], .task-status[data-outcome='queued'], .task-status[data-outcome='dispatching'], .task-status[data-outcome='already_running'] { background: var(--ac-soft); color: var(--ac); }

  /* Boutons */
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 44px; padding: 0 16px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font-family: var(--font); font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .run { display: grid; place-items: center; flex: none; width: 44px; height: 44px; min-height: 44px; padding: 0; border-radius: 14px; background: var(--pn); }
  .run:hover:not(:disabled) { background: var(--pn-2); }
  .refresh { margin-bottom: 4px; }
  .primary { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); font-size: 15px; box-shadow: none; }
  .primary:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  .primary > span { font-size: 20px; line-height: 1; }

  /* Détail d'une tâche */
  .back { display: inline-flex; align-items: center; gap: 8px; min-height: 44px; margin-bottom: 16px; padding: 0 14px 0 8px; border-radius: var(--r-sm); color: var(--mu); font-size: 14px; font-weight: 600; transition: background-color var(--t-fast) ease, color var(--t-fast) ease; }
  .back:hover { background: var(--pn); color: var(--tx); }
  .back:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .description { max-width: 75ch; margin: 12px 0 20px; font-size: 15px; line-height: 1.6; color: var(--mu); }
  .description + .refresh { margin-bottom: 16px; }
  .reason { margin: 0 0 16px; padding: 14px 16px; border-radius: var(--r-md); background: var(--warn-soft); color: var(--tx); font-size: 14px; line-height: 1.6; }
  .reason a { color: var(--tx); font-weight: 700; text-decoration: underline; text-underline-offset: 3px; }
  .reason a:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  .trigger-list, .results { margin: 20px 0; padding: 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .trigger-list h3, .results h3 { margin: 0 0 12px; }
  .trigger-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 0; border-top: 1px solid var(--line); }
  .trigger-row p { margin: 0 0 4px; font-size: 15px; font-weight: 700; color: var(--tx); }
  small { font-size: 13px; color: var(--mu); }
  .trigger-list > p, .results p { margin: 6px 0; font-size: 14px; line-height: 1.6; color: var(--mu); }
  .remove { display: grid; place-items: center; flex: none; width: 44px; height: 44px; padding: 0; border-radius: 14px; background: var(--err-soft); color: var(--err); font-size: 22px; }
  .remove:hover:not(:disabled) { background: var(--err-soft); box-shadow: inset 0 0 0 1px currentColor; }
  .no-trigger { margin: 0; padding: 16px 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .results p.error { padding: 12px 14px; border-radius: var(--r-md); background: var(--err-soft); color: var(--tx); }
  dl { display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr)); gap: 16px; margin: 20px 0; }
  dl > div { padding: 14px 16px; border-radius: var(--r-md); background: var(--pn); }
  dt { font-size: 12px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; color: var(--mu); }
  dd { margin: 6px 0 0; font-size: 22px; font-weight: 800; font-variant-numeric: tabular-nums; color: var(--tx); }
  details summary { min-height: 44px; cursor: pointer; }
  .results details summary { display: flex; align-items: center; font-size: 14px; font-weight: 700; }
  li { color: var(--mu); font-size: 14px; line-height: 1.6; overflow-wrap: anywhere; }

  /* Retours */
  .feedback { margin: 16px 0 0; padding: 14px 16px; border-radius: var(--r-md); background: var(--ok-soft); color: var(--tx); font-size: 14px; line-height: 1.5; }
  .feedback.error { background: var(--err-soft); }
  [role='status'] { color: var(--mu); }

  /* Dialogue */
  dialog { width: min(480px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; padding: 28px; border: 1px solid var(--line); border-radius: var(--r-xl); background: var(--bg-3); color: var(--tx); font-family: var(--font); box-shadow: var(--shadow); }
  dialog::backdrop { background: rgb(6 8 14 / 0.55); backdrop-filter: blur(4px); -webkit-backdrop-filter: blur(4px); }
  dialog form { display: grid; gap: 18px; }
  dialog h2 { font-size: 22px; }
  dialog p { margin: 0; font-size: 14px; line-height: 1.5; color: var(--mu); }
  dialog .feedback { margin: 0; color: var(--tx); }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input:not([type='checkbox']), select { box-sizing: border-box; width: 100%; min-width: 0; height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font-family: var(--font); font-size: 14px; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  input:not([type='checkbox']):hover, select:hover { background-color: var(--pn-2); }
  input:not([type='checkbox']):focus, select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .enabled { display: flex; align-items: center; gap: 12px; min-height: 44px; font-weight: 600; }
  .enabled input { width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  .dialog-actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 8px; margin-top: 4px; }
  .dialog-actions button { min-height: 48px; }

  @media (max-width: 640px) {
    .list-header { flex-wrap: wrap; }
    .plans-section, .automation-section, .queue-section { padding: 0 16px; }
    .plans-section[open] { padding-bottom: 16px; }
    .task-link { gap: 12px; padding: 14px 4px; }
    .task-row { padding-right: 4px; }
    .trigger-list, .results { padding: 16px; }
    dialog { width: 100vw; max-width: 100vw; margin: auto 0 0; padding: 20px 16px calc(20px + env(safe-area-inset-bottom, 0px)); border-radius: var(--r-xl) var(--r-xl) 0 0; }
    .dialog-actions button { flex: 1 1 auto; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button, .task-row, .back, input, select, summary::after { transition: none; }
  }
</style>

<script lang="ts">
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
  type SystemTask = { id: string; name: string; description: string; category: string; intervalMinutes: number; enabled: number; nextDueAt: number; lastStartedAt: number | null; lastCompletedAt: number | null; lastOutcome: string };
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
      description: 'Crée une archive chiffrée de la base et de la configuration du serveur. Les fichiers médias ne sont pas inclus.',
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
    failed: 'Échec'
  };

  const categories = $derived([...new Set(tasks.map(t => t.category))]);

  function taskHref(id?: string) { return resolve(`admin/libraries/#tasks-section${id ? `?task=${encodeURIComponent(id)}` : ''}`); }
  function date(value: number) { return new Date(value * 1000).toLocaleString(); }
  function intervalLabel(minutes: number) {
    const count = minutes % 1440 === 0 ? minutes / 1440 : minutes % 60 === 0 ? minutes / 60 : minutes;
    const suffix = minutes % 1440 === 0 ? 'jour' : minutes % 60 === 0 ? 'heure' : 'minute';
    return `${count === 1 ? 'Chaque' : 'Toutes les'} ${count === 1 ? '' : `${count} `}${suffix}${count > 1 ? 's' : ''}`;
  }

  function lastRun(task: Task) {
    if (!loaded) return 'Chargement du dernier lancement…';
    if (task.library && scanError) return 'Dernier lancement indisponible';
    if (task.id === 'backup' && backupError) return 'Dernier lancement indisponible';
    if (task.systemId && systemTasksError) return 'Dernier lancement indisponible';
    return task.plan?.last ? `Dernier lancement planifié : ${date(task.plan.last)}` : 'Aucun lancement planifié enregistré';
  }

  async function refresh() {
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

  $effect(() => { if (active) void refresh(); });
  $effect(() => { if (selectedId !== undefined) { error = ''; notice = ''; editor?.close(); } });

  function editTrigger() {
    if (!selected) return;
    const interval = selected.plan?.interval ?? 1440;
    unit = interval % 1440 === 0 ? 1440 : interval % 60 === 0 ? 60 : 1;
    frequency = interval / unit;
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
      if (task.library) {
        await adminRequest(`/api/v1/admin/tasks/schedules/${encodeURIComponent(task.library.id)}`, {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: minutes, enabled: true })
        });
      } else if (task.systemId) {
        await adminRequest(`/api/v1/admin/tasks/system/${encodeURIComponent(task.systemId)}/schedule`, {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: minutes, enabled: true })
        });
      } else {
        await adminRequest('/api/v1/admin/backups/schedule', {
          method: 'PUT',
          body: JSON.stringify({ intervalMinutes: minutes, enabled: true })
        });
      }
      await refresh();
      editor?.close();
      notice = 'Déclencheur enregistré.';
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
      if (task.library) {
        await adminRequest(`/api/v1/admin/tasks/schedules/${encodeURIComponent(task.library.id)}`, { method: 'DELETE' });
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
      notice = 'Déclencheur supprimé. Le lancement manuel reste disponible.';
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
        await adminRequest(`/api/v1/admin/tasks/system/${encodeURIComponent(task.systemId)}/run`, { method: 'POST' });
        notice = `Tâche ${task.name} démarrée.`;
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
      {#if selected.reason}<p class="reason">{selected.reason} {#if !selected.library && !selected.systemId}<a href={resolve('admin/libraries/#backups-section')}>Ouvrir les sauvegardes</a>{/if}</p>{/if}
      <button class="primary" disabled={!loaded || saving || refreshing || !!(selected.library ? scanError : selected.systemId ? systemTasksError : backupError) || (selected.id === 'backup' && !backup?.available)} onclick={editTrigger}>
        <span aria-hidden="true">+</span> {selected.plan?.enabled ? 'Modifier le déclencheur' : 'Ajouter un déclencheur'}
      </button>
      <div class="trigger-list" aria-label="Déclencheurs">
        <h3>Heure</h3>
        {#if selected.plan?.enabled}
          <div class="trigger-row">
            <div>
              <p>{intervalLabel(selected.plan.interval)}</p>
              <small>Prochaine échéance : {date(selected.plan.next)}</small>
            </div>
            <button class="remove" disabled={saving} aria-label="Supprimer le déclencheur" title="Supprimer le déclencheur" onclick={() => void removeTrigger()}>−</button>
          </div>
        {:else}
          <p class="no-trigger">Aucun déclencheur actif. Cette tâche peut être lancée manuellement.</p>
        {/if}
      </div>
      <div class="results">
        <h3>Dernier résultat</h3>
        <p>{lastRun(selected)}</p>
        <p>{selected.running ? 'En cours…' : selected.plan ? outcomes[selected.plan.outcome] ?? selected.plan.outcome : 'Aucune exécution planifiée'}</p>
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
    {:else}
      <h2 id="tasks-heading">Tâche introuvable</h2>
      <p>Cette tâche a peut-être été supprimée ou renommée.</p>
    {/if}
  {:else}
    <header class="list-header">
      <h2 id="tasks-heading">Tâches planifiées</h2>
      <button class="refresh" disabled={refreshing} onclick={() => void refresh()}><Icon name="refresh" size={18} /> Actualiser</button>
    </header>
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
                  <span class="task-subtitle">{task.running ? 'En cours…' : lastRun(task)}</span>
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
    <DistributedTasks {active} {libraries} {nodes} />
    <AutomationTasks {active} {libraries} />
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
    <label>Toutes les<input type="number" min="1" max="10080" step="1" required bind:value={frequency} disabled={saving} /></label>
    <label>Unité
      <select aria-label="Unité" bind:value={unit} disabled={saving}>
        <option value={1}>Minutes</option>
        <option value={60}>Heures</option>
        <option value={1440}>Jours</option>
      </select>
    </label>
    <p>De {selected?.library || selected?.systemId ? '5 minutes' : '1 heure'} à 7 jours. Un déclencheur périodique par tâche.</p>
    {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
    <div class="dialog-actions">
      <button type="button" disabled={saving} onclick={() => editor?.close()}>Annuler</button>
      <button class="primary" type="submit" disabled={saving}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button>
    </div>
  </form>
</dialog>

<style>
  section { min-width: 0; color: var(--text-primary); }
  h2 { font-size: clamp(24px, 2.2vw, 32px); font-weight: 400; margin: 0; overflow-wrap: anywhere; }
  .list-header, .detail-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .category { margin-top: 32px; } .category > h3 { font-size: 24px; font-weight: 400; margin: 0 0 18px; }
  .task-list, .trigger-list { background: var(--bg-surface); }
  .task-row { display: flex; align-items: center; min-height: 96px; padding-right: 20px; }
  .task-row:hover { background: var(--bg-elevated); }
  .task-link { display: flex; align-items: center; gap: 20px; flex: 1; min-width: 0; padding: 20px 24px; }
  .clock { display: grid; place-items: center; flex-shrink: 0; width: 52px; height: 52px; border-radius: 50%; background: var(--accent-primary); color: var(--color-on-accent); }
  .task-copy { display: grid; gap: 4px; min-width: 0; overflow-wrap: anywhere; }
  .task-title { font-size: 18px; } .task-subtitle { font-size: 14px; color: var(--text-secondary); line-height: 1.45; }
  button { font: inherit; color: var(--text-primary); min-height: 44px; padding: 10px 16px; border: 1px solid var(--border-subtle); border-radius: var(--radius-md); background: var(--bg-surface); }
  button:hover:not(:disabled) { background: var(--bg-elevated); }
  .run { display: grid; place-items: center; width: 48px; padding: 8px; flex-shrink: 0; border: 0; background: transparent; }
  .refresh { display: inline-flex; align-items: center; gap: 8px; }
  .primary { display: inline-flex; align-items: center; gap: 12px; border: 0; background: var(--color-play); color: var(--color-on-play); }
  .primary:hover:not(:disabled) { background: var(--color-play); box-shadow: var(--shadow-sm); } .primary > span { font-size: 24px; line-height: 1; }
  .back { display: inline-flex; align-items: center; gap: 12px; min-height: 44px; margin-bottom: 24px; color: var(--text-secondary); }
  .description { margin: 24px 0; font-size: 16px; color: var(--text-secondary); line-height: 1.6; }
  .reason { color: var(--text-secondary); line-height: 1.6; } .reason a { color: var(--accent-primary); text-decoration: underline; }
  .trigger-list { margin: 24px 0 32px; } .trigger-list h3 { margin: 0; padding: 18px 24px; font-size: 16px; border-bottom: 1px solid var(--border-subtle); }
  .trigger-row { display: flex; justify-content: space-between; align-items: center; gap: 16px; padding: 20px 24px; border-bottom: 1px solid var(--border-subtle); }
  .trigger-row p { margin: 0 0 6px; font-size: 18px; } small, .results p { color: var(--text-secondary); }
  .remove { border: 0; background: var(--status-error); color: white; font-size: 24px; border-radius: 50%; width: 44px; padding: 0; flex-shrink: 0; }
  .remove:hover:not(:disabled) { background: var(--status-error); opacity: .85; }
  .no-trigger { padding: 24px; color: var(--text-secondary); line-height: 1.6; } .results h3 { font-weight: 500; }
  dl { display: flex; flex-wrap: wrap; gap: 24px; margin: 24px 0; } dt { font-size: 13px; color: var(--text-secondary); } dd { margin: 6px 0 0; font-size: 18px; }
  li { overflow-wrap: anywhere; line-height: 1.6; } .feedback { padding: 16px; background: var(--color-success-soft); margin-top: 20px; }
  .error { background: var(--color-danger-soft); border-left: 3px solid var(--status-error); }
  dialog { width: min(480px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; border: 1px solid var(--border-subtle); border-radius: var(--radius-xl); background: var(--bg-elevated); color: var(--text-primary); padding: 24px; }
  dialog::backdrop { background: rgb(0 0 0 / .65); } dialog form, label { display: grid; gap: 10px; }
  dialog form { gap: 20px; } dialog p { margin: 0; line-height: 1.5; color: var(--text-secondary); }
  input { min-width: 0; width: 100%; padding: 12px; background: var(--bg-surface); color: var(--text-primary); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); }
  .dialog-actions { display: flex; justify-content: flex-end; gap: 12px; }
  @media (max-width: 640px) {
    .list-header { flex-wrap: wrap; } .task-link { padding: 16px 12px; gap: 12px; } .task-row { padding-right: 8px; }
    .clock { width: 40px; height: 40px; } .task-title { font-size: 16px; } .task-subtitle { font-size: 13px; } .trigger-list h3, .trigger-row { padding: 16px; }
  }
</style>

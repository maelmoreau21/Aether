<script lang="ts">
  import { untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import type { AdminNode, Library, Scan } from './types';
  import { scanLabels } from './types';
  import { t } from '../i18n';

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
      scanError = $t('loading_impossible');
    }

    if (archives.status === 'fulfilled') {
      backup = archives.value;
      backupError = '';
    } else {
      backupError = $t('backup_unavailable');
    }

    if (sys.status === 'fulfilled') {
      systemTasks = sys.value;
      systemTasksError = '';
    } else {
      systemTasksError = $t('loading_impossible');
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
      error = $t('invalid_interval');
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
      notice = triggerEnabled ? $t('interval_enabled') : $t('interval_disabled');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('operation_failed');
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
      notice = $t('interval_removed');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('operation_failed');
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
        notice = $t('task_queued', { name: task.name });
        await refresh();
      } catch (cause) {
        error = cause instanceof Error ? cause.message : $t('task_start_failed');
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
      notice = $t('backup_created_short');
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
    <a class="back" href={taskHref()}><Icon name="back" size={24} /><span>{$t('tasks_back')}</span></a>
    {#if selected}
      <header class="detail-header">
        <h2 id="tasks-heading">{selected.name}</h2>
        <button class="run" aria-label={$t('run_task', { name: selected.name })} disabled={selected.running || !!selected.reason || !loaded} onclick={() => void run(selected)}>
          <Icon name="play" size={22} />
        </button>
      </header>
      <p class="description">{selected.description}</p>
      <button class="refresh" disabled={refreshing} onclick={() => void refresh()}><Icon name="refresh" size={18} /> {$t('refresh_state')}</button>
      {#if selected.reason}<p class="reason">{selected.reason} {#if !selected.library && !selected.systemId}<a href={resolve('admin/libraries/#backups-section')}>{$t('open_backups')}</a>{/if}</p>{/if}
      <button class="primary" disabled={!loaded || saving || refreshing || !!(selected.library ? scanError : selected.systemId ? systemTasksError : backupError) || (selected.id === 'backup' && !backup?.available)} onclick={editTrigger}>
        <span aria-hidden="true">+</span> {selected.plan ? $t('configure_interval') : $t('add_interval')}
      </button>
      <div class="trigger-list" aria-label={$t('triggers')}>
        <h3>{$t('interval')}</h3>
        {#if selected.plan?.enabled}
          <div class="trigger-row">
            <div>
              <p>{intervalLabel(selected.plan.interval)}</p>
              <small>{$t('next_due', { date: date(selected.plan.next) })}</small>
            </div>
            <button class="remove" disabled={saving || refreshing} aria-label={$t('disable_interval')} title={$t('disable_interval')} onclick={() => void removeTrigger()}>−</button>
          </div>
        {:else}
          <p class="no-trigger">{$t('no_active_trigger')}</p>
          {#if selected.plan}<p>{$t('preserved_interval', { value: intervalLabel(selected.plan.interval) })}</p>{/if}
        {/if}
      </div>
      <div class="results">
        <h3>{$t('last_result')}</h3>
        <p>{lastRun(selected)}</p>
        <p>{executionLabel(selected)}</p>
        {#if selectedSystemTask?.lastMessage}
          <p class:error={selectedSystemTask.lastOutcome === 'failed'}>{selectedSystemTask.lastMessage}</p>
          {#if selectedSystemTask.lastErrorKind}<p>Type : {selectedSystemTask.lastErrorKind}{selectedSystemTask.lastErrorScope ? ` · ${selectedSystemTask.lastErrorScope}` : ''}</p>{/if}
        {/if}
        {#if selected.id === 'backup' && backup?.schedule.lastStartedAt && backup.schedule.lastCompletedAt && backup.schedule.lastCompletedAt >= backup.schedule.lastStartedAt}
          <p>{$t('duration_seconds', { count: backup.schedule.lastCompletedAt - backup.schedule.lastStartedAt })}</p>
        {/if}
        {#if selectedSystemTask && selectedSystemTask.lastStartedAt && selectedSystemTask.lastCompletedAt && selectedSystemTask.lastCompletedAt >= selectedSystemTask.lastStartedAt}
          <p>{$t('duration_seconds', { count: selectedSystemTask.lastCompletedAt - selectedSystemTask.lastStartedAt })}</p>
        {/if}
        {#if selectedScan}
          <p>{$t('last_known_scan')} : {scanLabels[selectedScan.status]}</p>
          <dl>
            <div><dt>{$t('discovered')}</dt><dd>{selectedScan.discovered}</dd></div>
            <div><dt>{$t('analyzed')}</dt><dd>{selectedScan.probed}</dd></div>
            <div><dt>{$t('reused')}</dt><dd>{selectedScan.reused}</dd></div>
            <div><dt>{$t('errors')}</dt><dd>{selectedScan.errorsCount ?? selectedScan.errors.length}</dd></div>
          </dl>
          {#if selectedScan.errors.length}
            <details>
              <summary>{$t('inspect_issues')}</summary>
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
      <h2 id="tasks-heading">{$t('loading_task')}</h2>
      <p role="status">{$t('reading_schedules')}</p>
    {:else if selectedId.startsWith('system-') && systemTasksError}
      <h2 id="tasks-heading">{$t('task_state_unavailable')}</h2>
      <button class="refresh" disabled={refreshing} onclick={() => void refresh()}>{$t('retry_action')}</button>
    {:else}
      <h2 id="tasks-heading">{$t('task_not_found')}</h2>
      <p>{$t('task_removed_renamed')}</p>
    {/if}
  {:else}
    <header class="list-header">
      <h2 id="tasks-heading">{$t('tasks_title')}</h2>
    </header>
    <details class="plans-section" open><summary>{$t('schedules_manual')}</summary><p class="section-intro">{$t('schedules_intro')}</p><button class="refresh" disabled={refreshing} onclick={() => void refresh()}><Icon name="refresh" size={18} /> {$t('refresh_schedules')}</button>
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
                  <span class="task-subtitle">{task.plan?.enabled ? `${intervalLabel(task.plan.interval)} · ${$t('next_due', { date: date(task.plan.next) })}` : $t('manual_no_interval')}</span>
                  <span class="task-subtitle task-status" data-outcome={task.running ? 'running' : task.plan?.outcome}>{executionLabel(task)}</span>
                  <span class="task-subtitle">{lastRun(task)}</span>
                  {#if task.reason}<span class="task-subtitle">{task.reason}</span>{/if}
                </span>
              </a>
              <button class="run" aria-label={$t('run_task', { name: task.name })} title={$t('run_now')} disabled={task.running || !!task.reason || !loaded} onclick={() => void run(task)}>
                <Icon name={task.running ? 'refresh' : 'play'} size={22} />
              </button>
            </div>
          {:else}
            <p class="no-trigger">{$t('no_tasks_category')}</p>
          {/each}
        </div>
      </div>
    {/each}
    </details>
  {/if}
  {#if scanError}<p class="feedback error" role="alert">{scanError}</p>{/if}
  {#if backupError}<p class="feedback error" role="alert">{backupError}</p>{/if}
  {#if systemTasksError}<p class="feedback error" role="alert">{systemTasksError}</p>{/if}
  {#if error && !editor?.open}<p class="feedback error" role="alert">{error}</p>{/if}
  {#if notice}<p class="feedback" role="status">{notice}</p>{/if}
</section>

<dialog bind:this={editor} aria-labelledby="trigger-title" oncancel={(event) => { if (saving) event.preventDefault(); }}>
  <form onsubmit={saveTrigger}>
    <h2 id="trigger-title">{$t('periodic_trigger')}</h2>
    <p>{selected?.name}</p>
    <label class="enabled"><input type="checkbox" bind:checked={triggerEnabled} disabled={saving} /> {$t('enable_interval')}</label>
    <label>{$t('every')}<input type="number" min={Math.max(1, Math.ceil((selected?.library || selected?.systemId ? 5 : 60) / unit))} max={Math.floor(10080 / unit)} step="1" required bind:value={frequency} disabled={saving} /></label>
    <label>{$t('unit')}
      <select aria-label={$t('unit')} bind:value={unit} disabled={saving}>
        <option value={1}>{$t('minutes')}</option>
        <option value={60}>{$t('hours')}</option>
        <option value={1440}>{$t('days')}</option>
      </select>
    </label>
    <p>{$t('periodic_range', { minimum: selected?.library || selected?.systemId ? '5 minutes' : '1 heure' })}</p>
    <p>{$t('schedule_from_now')}</p>
    {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
    <div class="dialog-actions">
      <button type="button" disabled={saving} onclick={() => editor?.close()}>{$t('cancel_action')}</button>
      <button class="primary" type="submit" disabled={saving}>{saving ? $t('loading') : $t('save')}</button>
    </div>
  </form>
</dialog>

<style>
  section { min-width: 0; color: var(--color-text); font-family: var(--font-sans); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  h3 { margin: 0; font-size: 16px; font-weight: 600; }
  .list-header, .detail-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 8px; }

  /* Groupes repliables : surface plate */
  .plans-section { margin-top: 24px; padding: 0 20px; background: var(--color-surface); border-radius: var(--radius-md); }
  .plans-section[open] { padding-bottom: 20px; }
  .plans-section > summary { display: flex; align-items: center; gap: 10px; min-height: 56px; box-sizing: border-box; cursor: pointer; font-size: 16px; font-weight: 600; color: var(--color-text); list-style: none; }
  .plans-section > summary::-webkit-details-marker { display: none; }
  .plans-section > summary::after { content: ''; width: 7px; height: 7px; margin-left: auto; border-right: 2px solid var(--color-muted); border-bottom: 2px solid var(--color-muted); transform: rotate(45deg); transition: transform var(--transition-normal); }
  .plans-section[open] > summary::after { transform: rotate(-135deg); }
  summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-xs); }
  .section-intro { margin: 0 0 12px; max-width: 75ch; color: var(--color-muted); font-size: 14px; line-height: 1.5; }

  /* Catégories et lignes de tâches */
  .category { margin-top: 20px; }
  .category > h3 { margin: 0 0 4px; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  .task-list { display: grid; }
  .task-row { display: flex; align-items: center; gap: 8px; padding-right: 4px; border-top: 1px solid var(--color-line-subtle); transition: background-color var(--transition-fast); }
  .task-row:first-child { border-top: 0; }
  .task-row:hover { background: var(--color-surface-hover); }
  .task-link { display: flex; align-items: center; gap: 14px; flex: 1; min-width: 0; padding: 12px 8px; color: inherit; text-decoration: none; }
  .task-link:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; border-radius: var(--radius-sm); }
  .clock { display: grid; place-items: center; flex: none; width: 36px; height: 36px; color: var(--color-muted); }
  .task-copy { display: grid; justify-items: start; gap: 2px; min-width: 0; overflow-wrap: anywhere; }
  .task-title { font-size: 15px; font-weight: 600; color: var(--color-text); }
  .task-subtitle { font-size: 13px; line-height: 1.4; color: var(--color-muted); }
  .task-status { font-weight: 600; color: var(--color-text); }
  .task-status[data-outcome='completed'], .task-status[data-outcome='succeeded'] { color: var(--color-success); }
  .task-status[data-outcome='error'], .task-status[data-outcome='failed'] { color: var(--color-danger); }
  .task-status[data-outcome='interrupted'], .task-status[data-outcome='unavailable'], .task-status[data-outcome='stopped'], .task-status[data-outcome='paused'] { color: var(--color-warning); }
  .task-status[data-outcome='running'], .task-status[data-outcome='queued'], .task-status[data-outcome='dispatching'], .task-status[data-outcome='already_running'] { color: var(--color-accent); }

  /* Boutons : ghost par défaut, un seul primaire */
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 40px; padding: 8px 16px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 13px; cursor: pointer; transition: background-color var(--transition-fast), color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  .run { display: grid; place-items: center; flex: none; width: 44px; min-height: 44px; padding: 0; border: 0; color: var(--color-muted); }
  .run:hover:not(:disabled) { background: var(--color-accent-soft); color: var(--color-accent); }
  .refresh { margin-bottom: 4px; }
  .primary { border: 0; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-accent-hover); opacity: 0.9; }
  .primary > span { font-size: 20px; line-height: 1; }

  /* Détail d'une tâche */
  .back { display: inline-flex; align-items: center; gap: 10px; min-height: 44px; margin-bottom: 12px; color: var(--color-muted); font-size: 14px; text-decoration: none; transition: color var(--transition-fast); }
  .back:hover { color: var(--color-text); }
  .back:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-sm); }
  .description { max-width: 75ch; margin: 0 0 20px; font-size: 15px; line-height: 1.5; color: var(--color-muted); }
  .description + .refresh { margin-bottom: 16px; }
  .reason { margin: 0 0 16px; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-warning-soft); color: var(--color-text); font-size: 14px; line-height: 1.5; }
  .reason a { color: var(--color-accent); text-decoration: underline; text-underline-offset: 3px; }
  .reason a:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }

  .trigger-list, .results { margin: 20px 0 0; padding: 4px 20px 12px; background: var(--color-surface); border-radius: var(--radius-md); }
  .trigger-list h3, .results h3 { margin: 0; padding: 14px 0 8px; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  .trigger-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 8px 0; border-top: 1px solid var(--color-line-subtle); }
  .trigger-row p { margin: 0 0 2px; font-size: 15px; font-weight: 600; color: var(--color-text); }
  small { font-size: 13px; color: var(--color-muted); }
  .trigger-list > p, .results p { margin: 0 0 6px; font-size: 14px; line-height: 1.5; color: var(--color-muted); }
  .remove { display: grid; place-items: center; flex: none; width: 40px; min-height: 40px; padding: 0; border: 0; border-radius: var(--radius-full); color: var(--color-danger); font-size: 20px; line-height: 1; }
  .remove:hover:not(:disabled) { background: var(--color-danger-soft); }
  .no-trigger { margin: 0; padding: 8px 0; color: var(--color-muted); font-size: 14px; line-height: 1.5; }
  .results p.error { padding: 10px 14px; border-radius: var(--radius-sm); background: var(--color-danger-soft); color: var(--color-text); }
  dl { display: flex; flex-wrap: wrap; gap: 16px 32px; margin: 16px 0; }
  dt { font-size: 12px; color: var(--color-muted); }
  dd { margin: 4px 0 0; font-size: 18px; font-weight: 600; font-variant-numeric: tabular-nums; color: var(--color-text); }
  details summary { min-height: 40px; cursor: pointer; }
  .results details summary { display: flex; align-items: center; color: var(--color-accent); font-size: 13px; }
  li { color: var(--color-muted); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }

  /* Retours */
  .feedback { margin: 16px 0 0; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-success-soft); color: var(--color-text); font-size: 14px; line-height: 1.5; }
  .feedback.error { background: var(--color-danger-soft); }
  [role='status'] { color: var(--color-muted); }

  /* Dialogue */
  dialog { width: min(480px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; padding: 24px; border: 0; border-radius: var(--radius-xl); background: var(--color-surface-elevated); color: var(--color-text); font-family: var(--font-sans); box-shadow: var(--shadow-lg); }
  dialog::backdrop { background: var(--color-overlay); }
  dialog form { display: grid; gap: 16px; }
  dialog h2 { font-size: 20px; }
  dialog p { margin: 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }
  dialog .feedback { margin: 0; color: var(--color-text); }
  label { display: grid; gap: 8px; font-size: 13px; color: var(--color-muted); }
  input:not([type='checkbox']), select { box-sizing: border-box; width: 100%; min-width: 0; min-height: 40px; padding: 8px 12px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .enabled { display: flex; align-items: center; gap: 12px; min-height: 44px; color: var(--color-text); font-size: 14px; }
  .enabled input { width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }
  .dialog-actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 12px; }

  @media (max-width: 640px) {
    .list-header { flex-wrap: wrap; }
    .plans-section { padding: 0 14px; }
    .plans-section[open] { padding-bottom: 14px; }
    .task-link { gap: 10px; padding: 10px 4px; }
    .task-title { font-size: 14px; }
    dialog { width: 100vw; max-width: 100vw; margin: auto 0 0; padding: 20px 16px calc(20px + env(safe-area-inset-bottom, 0px)); border-radius: var(--radius-xl) var(--radius-xl) 0 0; }
    .dialog-actions button { flex: 1 1 auto; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button, .task-row, .back, summary::after { transition: none; }
  }
</style>

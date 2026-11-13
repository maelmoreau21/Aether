<script lang="ts">
  import { untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import ArtworkJobs from './ArtworkJobs.svelte';
  import type { AdminNode, Library, Scan } from './types';
  import { t, fmt } from '../i18n';
  import {
    type ScheduledTask, type TaskTrigger, type TriggerDraft, type TriggerType,
    agoLabel, categoryLabel, categoryOrder, clockLabel, dayLabel, durationLabel, intervalLabel, intervalOptions, isMediaTask, isPausable,
    lastRunSummary, nestTasks, scheduleSummary, statsLabel, taskName, taskStatus, timeSlots, triggerLabel, triggerPayload, weekdays
  } from './task-schedule';

  // `scans`, `pending` and `nodes` stay accepted for the host page; the unified task API is the source of truth.
  let { libraries, pending = [], onscan, selectedId = null, active = false }: {
    libraries: Library[]; scans?: Record<string, Scan>; pending?: string[]; nodes?: AdminNode[];
    onscan: (library: Library) => void; selectedId?: string | null; active?: boolean;
  } = $props();

  type Filter = 'all' | 'active' | 'failed';
  let tasks = $state<ScheduledTask[]>([]);
  let loaded = $state(false);
  let loadError = $state('');
  let error = $state('');
  let notice = $state('');
  let busy = $state<Record<string, boolean>>({});
  let saving = $state(false);
  let filter = $state<Filter>('all');
  let now = $state(Math.floor(Date.now() / 1000));
  let editor = $state<HTMLDialogElement>();
  let draft = $state<TriggerDraft>({ type: 'daily', dayOfWeek: 1, timeOfDayMinutes: 180, intervalMinutes: 1440, maxRuntimeHours: null });
  let refreshPromise: Promise<void> | undefined;

  const libraryNames = $derived(Object.fromEntries(libraries.map(library => [library.id, library.name])));
  const selected = $derived(selectedId ? tasks.find(task => task.id === selectedId || `system-${task.id}` === selectedId) : undefined);
  const counts = $derived({
    all: tasks.length,
    active: tasks.filter(task => running(task)).length,
    failed: tasks.filter(task => taskStatus(task) === 'failed').length
  });
  const visible = $derived(tasks.filter(task => filter === 'all' || (filter === 'active' ? running(task) : taskStatus(task) === 'failed')));
  const groups = $derived(
    [...new Set(visible.map(task => task.category))]
      .sort((a, b) => categoryOrder(a) - categoryOrder(b) || a.localeCompare(b))
      .map(category => ({ category, rows: nestTasks(visible.filter(task => task.category === category)) }))
  );
  const anyActive = $derived(tasks.some(task => task.state !== 'idle'));
  const filters: { id: Filter; key: 'sched_filter_all' | 'sched_filter_active' | 'sched_filter_failed' }[] = [
    { id: 'all', key: 'sched_filter_all' }, { id: 'active', key: 'sched_filter_active' }, { id: 'failed', key: 'sched_filter_failed' }
  ];
  const triggerTypes: TriggerType[] = ['daily', 'weekly', 'interval', 'startup'];
  const typeKeys = { daily: 'sched_type_daily', weekly: 'sched_type_weekly', interval: 'sched_type_interval', startup: 'sched_type_startup' } as const;
  const outcomeKeys: Record<string, 'sched_outcome_succeeded' | 'sched_outcome_failed' | 'sched_outcome_interrupted' | 'sched_outcome_cancelled'> = {
    succeeded: 'sched_outcome_succeeded', failed: 'sched_outcome_failed', interrupted: 'sched_outcome_interrupted', cancelled: 'sched_outcome_cancelled'
  };
  const statusIcons = { running: 'refresh', paused: 'pause', failed: 'alert', warning: 'warning', ok: 'check', never: 'clock' } as const;

  function nameOf(task: ScheduledTask) { return taskName(task, task.libraryId ? libraryNames[task.libraryId] : undefined, $t); }
  function taskHref(id?: string) { return resolve(`admin/libraries/#tasks-section${id ? `?task=${encodeURIComponent(id)}` : ''}`); }
  function date(value: number) { return $fmt.dateTime(value); }
  function running(task: ScheduledTask) { return task.state !== 'idle' || (task.kind === 'scan' && task.libraryId !== null && pending.includes(task.libraryId)); }
  function rowStatus(task: ScheduledTask) { return task.state === 'paused' ? 'paused' : running(task) ? 'running' : taskStatus(task); }
  function message(cause: unknown) { return cause instanceof Error ? cause.message : $t('sched_action_failed'); }
  /** Short, single-line state for the list; the full message stays in the detail view. */
  function rowState(task: ScheduledTask): string {
    if (running(task)) return lastRunSummary(task, now, $t).text;
    const run = task.lastRun;
    const when = run ? run.end ?? run.start : null;
    if (!run || !when) return $t('sched_never');
    const ago = agoLabel(when, now, $t);
    if (run.outcome === 'failed') return $t('sched_failed_ago', { ago });
    if (run.outcome === 'interrupted') return $t('sched_interrupted', { ago });
    if (run.outcome === 'cancelled') return $t('sched_cancelled', { ago });
    return $t('sched_succeeded_ago', { ago });
  }

  function refresh(): Promise<void> {
    if (!refreshPromise) refreshPromise = load().finally(() => { refreshPromise = undefined; });
    return refreshPromise;
  }
  async function load() {
    try {
      tasks = await adminRequest<ScheduledTask[]>('/api/v1/admin/tasks');
      loadError = '';
    } catch {
      loadError = $t('sched_load_failed');
    } finally {
      now = Math.floor(Date.now() / 1000);
      loaded = true;
    }
  }

  $effect(() => {
    if (!active) return;
    untrack(() => void refresh());
    // Faster refresh while a task runs, so progress bars move.
    const period = anyActive || pending.length ? 3000 : 15000;
    const update = () => { if (document.visibilityState === 'visible' && !saving) void refresh(); };
    const timer = setInterval(update, period);
    document.addEventListener('visibilitychange', update);
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', update); };
  });
  $effect(() => { if (selectedId !== undefined) { error = ''; notice = ''; editor?.close(); } });

  async function start(task: ScheduledTask) {
    if (running(task) || task.reason || busy[task.id]) return;
    error = ''; notice = '';
    const library = task.kind === 'scan' ? libraries.find(candidate => candidate.id === task.libraryId) : undefined;
    if (library) {
      // Library scans keep the page's scan flow (refresh mode, pending state).
      onscan(library);
      setTimeout(() => void refresh(), 1000);
      return;
    }
    busy[task.id] = true;
    try {
      await adminRequest(`/api/v1/admin/tasks/${encodeURIComponent(task.id)}/run`, { method: 'POST' });
      notice = $t('sched_task_queued', { name: nameOf(task) });
      await refresh();
    } catch (cause) {
      error = message(cause);
    } finally {
      busy[task.id] = false;
    }
  }

  const controlNotices = { cancel: 'sched_task_stopped', pause: 'sched_task_paused', resume: 'sched_task_resumed' } as const;
  /** Stop, pause or resume every active run of a task. */
  async function control(task: ScheduledTask, action: keyof typeof controlNotices) {
    if (busy[task.id]) return;
    error = ''; notice = '';
    busy[task.id] = true;
    try {
      await adminRequest(`/api/v1/admin/tasks/${encodeURIComponent(task.id)}/${action}`, { method: 'POST' });
      notice = $t(controlNotices[action], { name: nameOf(task) });
      await refresh();
    } catch (cause) {
      error = message(cause);
    } finally {
      busy[task.id] = false;
    }
  }

  function openEditor() {
    draft = { type: 'daily', dayOfWeek: 1, timeOfDayMinutes: 180, intervalMinutes: selected?.kind === 'backup' ? 1440 : 720, maxRuntimeHours: null };
    error = '';
    editor?.showModal();
  }

  async function addTrigger(event: SubmitEvent) {
    event.preventDefault();
    const task = selected;
    if (!task || saving) return;
    if (draft.maxRuntimeHours !== null && !(draft.maxRuntimeHours >= 0 && draft.maxRuntimeHours <= 168)) { error = $t('sched_action_failed'); return; }
    saving = true; error = ''; notice = '';
    try {
      await adminRequest(`/api/v1/admin/tasks/${encodeURIComponent(task.id)}/triggers`, { method: 'POST', body: JSON.stringify(triggerPayload(draft)) });
      editor?.close();
      notice = $t('sched_trigger_added');
      await refresh();
    } catch (cause) {
      error = message(cause);
    } finally {
      saving = false;
    }
  }

  async function removeTrigger(task: ScheduledTask, trigger: TaskTrigger) {
    if (saving) return;
    saving = true; error = ''; notice = '';
    try {
      await adminRequest(`/api/v1/admin/tasks/${encodeURIComponent(task.id)}/triggers/${encodeURIComponent(trigger.id)}`, { method: 'DELETE' });
      notice = $t('sched_trigger_removed');
      await refresh();
    } catch (cause) {
      error = message(cause);
    } finally {
      saving = false;
    }
  }
</script>

{#snippet action(task: ScheduledTask, labelled: boolean)}
  <span class="actions">
  {#if running(task)}
    {#if isPausable(task) && task.state === 'paused'}
      <button class="action" class:labelled aria-label={$t('sched_resume', { name: nameOf(task) })} title={$t('sched_resume', { name: nameOf(task) })} disabled={busy[task.id]} onclick={() => void control(task, 'resume')}>
        <Icon name="play" size={18} /><span>{$t('sched_resume_short')}</span>
      </button>
    {:else if isPausable(task)}
      <button class="action" class:labelled aria-label={$t('sched_pause', { name: nameOf(task) })} title={$t('sched_pause', { name: nameOf(task) })} disabled={busy[task.id] || task.state === 'idle'} onclick={() => void control(task, 'pause')}>
        <Icon name="pause" size={18} /><span>{$t('sched_pause_short')}</span>
      </button>
    {/if}
    <button class="action" class:labelled class:secondary={labelled && isPausable(task)} aria-label={$t('sched_stop', { name: nameOf(task) })} title={$t('sched_stop', { name: nameOf(task) })} disabled={busy[task.id] || task.state === 'idle'} onclick={() => void control(task, 'cancel')}>
      <Icon name="close" size={18} /><span>{$t('sched_stop_short')}</span>
    </button>
  {:else}
    <button class="action" class:labelled aria-label={$t('sched_run', { name: nameOf(task) })} title={$t('sched_run', { name: nameOf(task) })} disabled={!!task.reason || busy[task.id] || !loaded} onclick={() => void start(task)}>
      <Icon name="play" size={18} /><span>{$t('sched_run_short')}</span>
    </button>
  {/if}
  </span>
{/snippet}

{#snippet taskRow(task: ScheduledTask)}
  {@const status = rowStatus(task)}
  {@const sub = isMediaTask(task)}
  {@const stats = statsLabel(task, $t)}
  <div class="task-row" class:subtask={sub}>
    <span class="status-icon" data-status={status} aria-hidden="true"><Icon name={statusIcons[status]} size={16} /></span>
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- taskHref resolves the base path and task hash. -->
    <a class="task-link" href={taskHref(task.id)}>
      <span class="task-title">{nameOf(task)}</span>
      <span class="task-meta">
        <span class="state" data-status={status}>{rowState(task)}</span>
        {#if stats}<span class="schedule">{stats}</span>{/if}
        <span class="schedule">{sub ? $t(task.afterScan ? 'sched_after_scan' : 'sched_manual_only') : scheduleSummary(task.triggers, $t)}</span>
      </span>
      {#if (task.state === 'running' || task.state === 'paused') && task.progress !== null}
        <span class="progress" role="progressbar" aria-label={rowState(task)} aria-valuemin="0" aria-valuemax="100" aria-valuenow={task.progress}><span style:width={`${Math.max(task.progress, 2)}%`}></span></span>
      {/if}
    </a>
    {#if task.nextRunAt && task.state === 'idle' && !task.reason}
      <span class="next" title={$t('sched_next', { date: date(task.nextRunAt) })}><small>{$t('sched_next_label')}</small>{date(task.nextRunAt)}</span>
    {:else if task.reason}
      <span class="next unavailable" title={task.reason}><Icon name="info" size={16} />{$t('sched_unavailable')}</span>
    {/if}
    {@render action(task, false)}
  </div>
{/snippet}

<section id="tasks-section" aria-labelledby="tasks-heading">
  {#if selectedId}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- taskHref resolves the base path and task hash. -->
    <a class="back" href={taskHref()}><Icon name="back" size={20} /><span>{$t('sched_back')}</span></a>
    {#if selected}
      {@const task = selected}
      {@const status = rowStatus(task)}
      {@const summary = lastRunSummary(task, now, $t)}
      <header class="detail-header">
        <div class="detail-title">
          <span class="status-icon" data-status={status} aria-hidden="true"><Icon name={statusIcons[status]} size={18} /></span>
          <div>
            <h2 id="tasks-heading">{nameOf(task)}</h2>
            <p class="detail-state" data-status={status}>{rowState(task)}</p>
          </div>
        </div>
        {@render action(task, true)}
      </header>
      <p class="description">{task.description}</p>
      {#if task.reason}<p class="reason" role="note">{task.reason}</p>{/if}

      <div class="detail-grid">
        <div class="card" role="region" aria-labelledby="schedule-heading">
          <header class="card-header">
            <h3 id="schedule-heading">{$t('sched_schedule')}</h3>
            {#if !isMediaTask(task)}<button class="ghost" disabled={saving || !!loadError} onclick={openEditor}><span aria-hidden="true">+</span> {$t('sched_add_trigger')}</button>{/if}
          </header>
          {#if isMediaTask(task)}
            <div class="trigger-row">
              <Icon name="refresh" size={18} />
              <div class="trigger-text">
                <p>{$t(task.afterScan ? 'sched_after_scan' : 'sched_manual_only')}</p>
                <small>{$t(task.afterScan ? 'sched_after_scan_note' : 'sched_manual_note')}</small>
              </div>
            </div>
            <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- taskHref resolves the base path and task hash. -->
            {#if task.parentId}<p class="card-foot"><a href={taskHref(task.parentId)}>{$t('sched_open_parent')}</a></p>{/if}
          {/if}
          {#each task.triggers as trigger (trigger.id)}
            <div class="trigger-row" class:disabled={!trigger.enabled}>
              <Icon name="clock" size={18} />
              <div class="trigger-text">
                <p>{triggerLabel(trigger, $t)}</p>
                {#if trigger.nextFireAt || trigger.maxRuntimeSeconds}
                  <small>
                    {#if trigger.nextFireAt}{$t('sched_next', { date: date(trigger.nextFireAt) })}{/if}
                    {#if trigger.maxRuntimeSeconds}{trigger.nextFireAt ? ' · ' : ''}{$t('sched_time_limit', { duration: durationLabel(trigger.maxRuntimeSeconds) })}{/if}
                  </small>
                {/if}
              </div>
              <button class="icon danger" disabled={saving} aria-label={$t('sched_delete_trigger')} title={$t('sched_delete_trigger')} onclick={() => void removeTrigger(task, trigger)}><Icon name="trash" size={18} /></button>
            </div>
          {:else}
            {#if !isMediaTask(task)}<p class="empty">{$t('sched_no_trigger')}</p>{/if}
          {/each}
          {#if task.kind === 'backup'}<p class="card-foot"><a href={resolve('admin/libraries/#backups-section')}>{$t('sched_backups_link')}</a></p>{/if}
        </div>

        <div class="card" role="region" aria-labelledby="last-run-heading">
          <header class="card-header"><h3 id="last-run-heading">{$t('sched_last_execution')}</h3></header>
          {#if running(task)}
            <p class="status" data-tone={summary.tone}>{summary.text}</p>
            {#if task.progress !== null && (task.state === 'running' || task.state === 'paused')}<div class="progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={task.progress}><span style:width={`${task.progress}%`}></span></div>{/if}
          {:else if task.lastRun}
            {@const run = task.lastRun}
            <dl>
              <div><dt>{$t('sched_outcome')}</dt><dd data-tone={summary.tone}>{outcomeKeys[run.outcome] ? $t(outcomeKeys[run.outcome]) : run.outcome}</dd></div>
              {#if run.start}<div><dt>{$t('sched_started')}</dt><dd>{date(run.start)}</dd></div>{/if}
              {#if run.start && run.end && run.end >= run.start}<div><dt>{$t('sched_duration')}</dt><dd>{durationLabel(run.end - run.start)}</dd></div>{/if}
            </dl>
            {#if run.message}<p class="message" class:error={run.outcome === 'failed'}>{run.message}</p>{/if}
            {#if run.errorKind && run.outcome === 'failed'}<p class="meta">{$t('sched_error_type')} : {run.errorKind}{run.errorScope ? ` · ${run.errorScope}` : ''}</p>{/if}
          {:else}
            <p class="empty">{$t('sched_never')}</p>
          {/if}
          {#if task.stats}<p class="meta">{statsLabel(task, $t)}</p>{/if}
          {#if task.issues.length}
            <details>
              <summary>{isMediaTask(task) ? $t('sched_errors', { count: task.issues.length }) : $t('sched_issues', { count: task.warningsCount || task.issues.length })}</summary>
              <ul>{#each task.issues as issue, index (index)}<li>{issue}</li>{/each}</ul>
            </details>
          {/if}
        </div>
      </div>
    {:else if !loaded}
      <h2 id="tasks-heading">{$t('sched_loading')}</h2>
    {:else}
      <h2 id="tasks-heading">{$t('sched_not_found')}</h2>
      <p class="description">{$t('sched_not_found_hint')}</p>
    {/if}
  {:else}
    <!-- The page title and active tab already name the list: the heading stays for assistive technologies. -->
    <header class="list-header">
      <h2 id="tasks-heading" class="sr-only">{$t('sched_title')}</h2>
      <p class="description">{$t('sched_intro')}</p>
    </header>
    {#if !loaded}<p role="status">{$t('sched_loading')}</p>{/if}
    {#if tasks.length}
      <div class="filters" role="group" aria-label={$t('sched_filter_label')}>
        {#each filters as option (option.id)}
          <button class="chip" aria-pressed={filter === option.id} data-filter={option.id} disabled={option.id !== 'all' && !counts[option.id] && filter !== option.id} onclick={() => { filter = option.id; }}>
            {$t(option.key)}<span class="count">{counts[option.id]}</span>
          </button>
        {/each}
      </div>
    {/if}
    {#each groups as group (group.category)}
      <div class="category" role="region" aria-labelledby={`category-${group.category}`}>
        <h3 id={`category-${group.category}`}>{categoryLabel(group.category, $t)}</h3>
        <ul class="task-list">
          {#each group.rows as row (row.task.id)}
            <li class="task-item">
              {@render taskRow(row.task)}
              {#if row.children.length}
                <ul class="subtask-list" aria-label={$t('sched_subtasks_label', { name: row.task.libraryId ? libraryNames[row.task.libraryId] ?? nameOf(row.task) : nameOf(row.task) })}>
                  {#each row.children as child (child.id)}
                    <li>{@render taskRow(child)}</li>
                  {/each}
                </ul>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {:else}
      {#if loaded && tasks.length}<p class="empty-filter">{$t('sched_filter_empty')}</p>{/if}
    {/each}
    <ArtworkJobs {libraryNames} />
  {/if}
  {#if loadError}<p class="feedback error" role="alert">{loadError} <button class="ghost" onclick={() => void refresh()}><Icon name="refresh" size={16} /></button></p>{/if}
  {#if error && !editor?.open}<p class="feedback error" role="alert">{error}</p>{/if}
  {#if notice}<p class="feedback" role="status">{notice}</p>{/if}
</section>

<dialog bind:this={editor} aria-labelledby="trigger-title" oncancel={(event) => { if (saving) event.preventDefault(); }}>
  <form onsubmit={addTrigger}>
    <h2 id="trigger-title">{$t('sched_add_trigger')}</h2>
    {#if selected}<p>{nameOf(selected)}</p>{/if}
    <label>{$t('sched_trigger_type')}
      <select bind:value={draft.type} disabled={saving}>
        {#each triggerTypes as type (type)}<option value={type}>{$t(typeKeys[type])}</option>{/each}
      </select>
    </label>
    {#if draft.type === 'weekly'}
      <label>{$t('sched_day')}
        <select bind:value={draft.dayOfWeek} disabled={saving}>
          {#each weekdays as day (day)}<option value={day}>{dayLabel(day, $t)}</option>{/each}
        </select>
      </label>
    {/if}
    {#if draft.type === 'daily' || draft.type === 'weekly'}
      <label>{$t('sched_time')}
        <select bind:value={draft.timeOfDayMinutes} disabled={saving}>
          {#each timeSlots as slot (slot)}<option value={slot}>{clockLabel(slot)}</option>{/each}
        </select>
        <small>{$t('sched_server_time')}</small>
      </label>
    {/if}
    {#if draft.type === 'interval'}
      <label>{$t('sched_interval')}
        <select bind:value={draft.intervalMinutes} disabled={saving}>
          {#each intervalOptions.filter(minutes => selected?.kind !== 'backup' || minutes >= 60) as minutes (minutes)}<option value={minutes}>{intervalLabel(minutes, $t)}</option>{/each}
        </select>
      </label>
    {/if}
    <label>{$t('sched_time_limit_hours')}
      <input type="number" min="0" max="168" step="0.5" bind:value={draft.maxRuntimeHours} disabled={saving} />
      <small>{$t('sched_time_limit_hint')}</small>
    </label>
    {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
    <div class="dialog-actions">
      <button type="button" class="ghost" disabled={saving} onclick={() => editor?.close()}>{$t('sched_cancel')}</button>
      <button class="primary" type="submit" disabled={saving}>{$t('sched_add')}</button>
    </div>
  </form>
</dialog>

<style>
  section { min-width: 0; color: var(--color-text); font-family: var(--font-sans); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  h3 { margin: 0; font-size: 16px; font-weight: 600; }
  .description { max-width: 70ch; margin: 6px 0 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }

  /* Liste */
  .list-header { margin-bottom: 20px; }
  .filters { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 8px; }
  .chip { min-height: 36px; padding: 6px 14px; border: 1px solid var(--color-line); border-radius: var(--radius-full); background: transparent; color: var(--color-muted); font-size: 13px; font-weight: 600; }
  .chip .count { min-width: 20px; padding: 1px 7px; border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-text); font-size: 12px; font-variant-numeric: tabular-nums; }
  .chip[aria-pressed='true'] { border-color: transparent; background: var(--color-accent-soft); color: var(--color-accent); }
  .chip[data-filter='failed'] .count { background: var(--color-danger-soft); color: var(--color-danger); }
  .chip[data-filter='active'] .count { background: var(--color-accent-soft); color: var(--color-accent); }

  .category { margin-top: 20px; }
  .category > h3 { margin: 0 0 6px; padding: 0 4px; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  .task-list { display: grid; margin: 0; padding: 0; list-style: none; background: var(--color-surface); border-radius: var(--radius-md); overflow: hidden; }
  .task-item { border-top: 1px solid var(--color-line-subtle); }
  .task-item:first-child { border-top: 0; }
  .task-row { display: flex; align-items: center; gap: 12px; padding: 0 12px 0 16px; transition: background-color var(--transition-fast); }
  .subtask-list { margin: 0; padding: 0 0 4px; list-style: none; }
  .task-row.subtask { padding-left: 40px; }
  .task-row.subtask .task-link { padding: 8px 0; }
  .task-row.subtask .task-title { font-size: 14px; font-weight: 500; }
  .task-row.subtask .status-icon { width: 24px; height: 24px; }
  .task-row:hover { background: var(--color-surface-hover); }
  .task-link { display: grid; gap: 3px; flex: 1; min-width: 0; padding: 12px 0; color: inherit; text-decoration: none; }
  .task-link:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-sm); }
  .task-title { font-size: 15px; font-weight: 600; color: var(--color-text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .task-meta { display: flex; flex-wrap: wrap; gap: 2px 12px; min-width: 0; font-size: 13px; line-height: 1.4; color: var(--color-muted); }
  .task-meta .schedule::before { content: '·'; margin-right: 12px; color: var(--color-subtle); }
  .state, .detail-state { color: var(--color-muted); }
  .state[data-status='failed'], .detail-state[data-status='failed'] { color: var(--color-danger); }
  .state[data-status='warning'], .detail-state[data-status='warning'],
  .state[data-status='paused'], .detail-state[data-status='paused'] { color: var(--color-warning); }
  .state[data-status='running'], .detail-state[data-status='running'] { color: var(--color-accent); }
  .next { display: grid; flex: none; justify-items: end; gap: 1px; font-size: 13px; color: var(--color-text); font-variant-numeric: tabular-nums; text-align: right; }
  .next small { font-size: 11px; letter-spacing: 0.04em; text-transform: uppercase; color: var(--color-muted); }
  .next.unavailable { display: inline-flex; align-items: center; gap: 6px; color: var(--color-warning); }
  .empty-filter { margin: 16px 0 0; padding: 16px; border-radius: var(--radius-md); background: var(--color-surface); font-size: 14px; color: var(--color-muted); }

  .status-icon { display: grid; place-items: center; flex: none; width: 30px; height: 30px; border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-muted); }
  .status-icon[data-status='ok'] { background: var(--color-success-soft); color: var(--color-success); }
  .status-icon[data-status='failed'] { background: var(--color-danger-soft); color: var(--color-danger); }
  .status-icon[data-status='warning'], .status-icon[data-status='paused'] { background: var(--color-warning-soft); color: var(--color-warning); }
  .status-icon[data-status='running'] { background: var(--color-accent-soft); color: var(--color-accent); }
  .status-icon[data-status='running'] :global(svg) { animation: spin 1.6s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  [data-tone='error'] { color: var(--color-danger); }
  [data-tone='warning'] { color: var(--color-warning); }
  [data-tone='active'] { color: var(--color-accent); }
  [data-tone='ok'] { color: var(--color-success); }

  .progress { display: block; width: min(320px, 100%); height: 6px; margin: 4px 0; overflow: hidden; border-radius: var(--radius-full); background: var(--color-line-subtle); }
  .progress > span { display: block; height: 100%; border-radius: inherit; background: var(--color-accent-solid); transition: width var(--transition-normal); }

  /* Boutons */
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 40px; padding: 8px 16px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 13px; cursor: pointer; transition: background-color var(--transition-fast), color var(--transition-fast), border-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .ghost { border-color: transparent; color: var(--color-accent); font-weight: 600; }
  .ghost:hover:not(:disabled) { background: var(--color-accent-soft); }
  .ghost > span { font-size: 18px; line-height: 1; }
  .primary { border: 0; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .actions { display: inline-flex; flex: none; align-items: center; gap: 8px; }
  .action { flex: none; width: 40px; padding: 0; border: 0; border-radius: var(--radius-full); color: var(--color-muted); }
  .action > span { display: none; }
  .action:hover:not(:disabled) { background: var(--color-accent-soft); color: var(--color-accent); }
  .action.labelled { width: auto; padding: 8px 18px 8px 14px; border-radius: var(--radius-full); background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  .action.labelled > span { display: inline; }
  .action.labelled:hover:not(:disabled) { background: var(--color-accent-hover); color: var(--color-on-accent); }
  .action.labelled.secondary { background: transparent; color: var(--color-text); box-shadow: inset 0 0 0 1px var(--color-line-strong); }
  .action.labelled.secondary:hover:not(:disabled) { background: var(--color-surface-hover); color: var(--color-text); }
  .icon { width: 40px; padding: 0; border: 0; border-radius: var(--radius-full); }
  .icon.danger { color: var(--color-danger); }
  .icon.danger:hover:not(:disabled) { background: var(--color-danger-soft); }

  /* Détail */
  .back { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; margin: 0 0 12px -8px; padding: 0 12px 0 8px; border-radius: var(--radius-full); color: var(--color-muted); font-size: 14px; text-decoration: none; transition: color var(--transition-fast), background-color var(--transition-fast); }
  .back:hover { color: var(--color-text); background: var(--color-raised); }
  .back:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .detail-header { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 16px; }
  .detail-title { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .detail-title .status-icon { width: 40px; height: 40px; }
  .detail-state { margin: 2px 0 0; font-size: 14px; }
  .detail-header + .description { margin-top: 12px; }
  .reason { max-width: 70ch; margin: 16px 0 0; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-warning-soft); font-size: 14px; line-height: 1.5; }
  .detail-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 340px), 1fr)); align-items: start; gap: 16px; margin-top: 24px; }
  .card { min-width: 0; padding: 4px 20px 16px; background: var(--color-surface); border-radius: var(--radius-md); }
  .card-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 52px; }
  .card-header h3 { font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  .card-header .ghost { margin-right: -12px; }
  .trigger-row { display: flex; align-items: center; gap: 12px; padding: 10px 0; border-top: 1px solid var(--color-line-subtle); color: var(--color-muted); }
  .trigger-row.disabled { opacity: 0.6; }
  .trigger-text { flex: 1; min-width: 0; }
  .trigger-text p { margin: 0 0 2px; font-size: 15px; font-weight: 600; color: var(--color-text); }
  .card-foot { margin: 8px 0 0; font-size: 14px; }
  .card-foot a { color: var(--color-accent); text-underline-offset: 3px; }
  small { font-size: 13px; color: var(--color-muted); }
  .empty, .meta, .status { margin: 0; padding: 8px 0; color: var(--color-muted); font-size: 14px; line-height: 1.5; }
  .message { margin: 4px 0 8px; font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .message.error { padding: 10px 14px; border-radius: var(--radius-sm); background: var(--color-danger-soft); }
  dl { display: flex; flex-wrap: wrap; gap: 12px 32px; margin: 0 0 8px; }
  dt { font-size: 12px; color: var(--color-muted); }
  dd { margin: 4px 0 0; font-size: 15px; font-weight: 600; font-variant-numeric: tabular-nums; }
  details summary { display: flex; align-items: center; min-height: 40px; cursor: pointer; color: var(--color-accent); font-size: 13px; }
  details ul { margin: 0; padding-left: 18px; }
  li { overflow-wrap: anywhere; }
  details li { color: var(--color-muted); font-size: 13px; line-height: 1.5; }

  .feedback { display: flex; align-items: center; gap: 12px; margin: 16px 0 0; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-success-soft); font-size: 14px; line-height: 1.5; }
  .feedback.error { background: var(--color-danger-soft); }
  [role='status'] { color: var(--color-muted); }

  dialog { width: min(440px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; padding: 24px; border: 0; border-radius: var(--radius-xl); background: var(--color-surface-elevated); color: var(--color-text); font-family: var(--font-sans); box-shadow: var(--shadow-lg); }
  dialog::backdrop { background: var(--color-overlay); }
  dialog form { display: grid; gap: 16px; }
  dialog h2 { font-size: 20px; }
  dialog p { margin: 0; font-size: 14px; color: var(--color-muted); }
  dialog .feedback { margin: 0; color: var(--color-text); }
  label { display: grid; gap: 8px; font-size: 13px; color: var(--color-muted); }
  input, select { box-sizing: border-box; width: 100%; min-width: 0; min-height: 40px; padding: 8px 12px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .dialog-actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 12px; }

  @media (max-width: 640px) {
    h2 { font-size: 22px; }
    .task-row { gap: 10px; padding: 0 6px 0 12px; }
    .task-row.subtask { padding-left: 24px; }
    .task-title { font-size: 14px; white-space: normal; }
    .task-meta .schedule { display: none; }
    .next:not(.unavailable) { display: none; }
    .next.unavailable { font-size: 0; gap: 0; }
    .card { padding-inline: 16px; }
    dialog { width: 100vw; max-width: 100vw; margin: auto 0 0; padding: 20px 16px calc(20px + env(safe-area-inset-bottom, 0px)); border-radius: var(--radius-xl) var(--radius-xl) 0 0; }
    .dialog-actions button { flex: 1 1 auto; }
    input, select { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) {
    button, .task-row, .back, .progress > span { transition: none; }
    .status-icon[data-status='running'] :global(svg) { animation: none; }
  }
</style>

<script lang="ts">
  import { untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import type { AdminNode, Library, Scan } from './types';
  import { t } from '../i18n';
  import {
    type ScheduledTask, type TaskTrigger, type TriggerDraft, type TriggerType,
    categoryLabel, categoryOrder, clockLabel, dayLabel, durationLabel, intervalLabel, intervalOptions,
    lastRunSummary, taskName, timeSlots, triggerLabel, triggerPayload, weekdays
  } from './task-schedule';

  // `scans`, `pending` and `nodes` stay accepted for the host page; the unified task API is the source of truth.
  let { libraries, pending = [], onscan, selectedId = null, active = false }: {
    libraries: Library[]; scans?: Record<string, Scan>; pending?: string[]; nodes?: AdminNode[];
    onscan: (library: Library) => void; selectedId?: string | null; active?: boolean;
  } = $props();

  let tasks = $state<ScheduledTask[]>([]);
  let loaded = $state(false);
  let loadError = $state('');
  let error = $state('');
  let notice = $state('');
  let busy = $state<Record<string, boolean>>({});
  let saving = $state(false);
  let now = $state(Math.floor(Date.now() / 1000));
  let editor = $state<HTMLDialogElement>();
  let draft = $state<TriggerDraft>({ type: 'daily', dayOfWeek: 1, timeOfDayMinutes: 180, intervalMinutes: 1440, maxRuntimeHours: null });
  let refreshPromise: Promise<void> | undefined;

  const libraryNames = $derived(Object.fromEntries(libraries.map(library => [library.id, library.name])));
  const selected = $derived(selectedId ? tasks.find(task => task.id === selectedId || `system-${task.id}` === selectedId) : undefined);
  const groups = $derived(
    [...new Set(tasks.map(task => task.category))]
      .sort((a, b) => categoryOrder(a) - categoryOrder(b) || a.localeCompare(b))
      .map(category => ({ category, tasks: tasks.filter(task => task.category === category) }))
  );
  const anyActive = $derived(tasks.some(task => task.state !== 'idle'));
  const triggerTypes: TriggerType[] = ['daily', 'weekly', 'interval', 'startup'];
  const typeKeys = { daily: 'sched_type_daily', weekly: 'sched_type_weekly', interval: 'sched_type_interval', startup: 'sched_type_startup' } as const;
  const outcomeKeys: Record<string, 'sched_outcome_succeeded' | 'sched_outcome_failed' | 'sched_outcome_interrupted' | 'sched_outcome_cancelled'> = {
    succeeded: 'sched_outcome_succeeded', failed: 'sched_outcome_failed', interrupted: 'sched_outcome_interrupted', cancelled: 'sched_outcome_cancelled'
  };

  function nameOf(task: ScheduledTask) { return taskName(task, task.libraryId ? libraryNames[task.libraryId] : undefined, $t); }
  function taskHref(id?: string) { return resolve(`admin/libraries/#tasks-section${id ? `?task=${encodeURIComponent(id)}` : ''}`); }
  function date(value: number) { return new Date(value * 1000).toLocaleString(); }
  function running(task: ScheduledTask) { return task.state !== 'idle' || (task.libraryId !== null && pending.includes(task.libraryId)); }
  function message(cause: unknown) { return cause instanceof Error ? cause.message : $t('sched_action_failed'); }

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

  async function stop(task: ScheduledTask) {
    if (busy[task.id]) return;
    error = ''; notice = '';
    busy[task.id] = true;
    try {
      await adminRequest(`/api/v1/admin/tasks/${encodeURIComponent(task.id)}/cancel`, { method: 'POST' });
      notice = $t('sched_task_stopped', { name: nameOf(task) });
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
      const text = message(cause);
      error = text.startsWith('Ce nom existe déjà') ? $t('sched_trigger_conflict') : text;
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

<section id="tasks-section" aria-labelledby="tasks-heading">
  {#if selectedId}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- taskHref resolves the base path and task hash. -->
    <a class="back" href={taskHref()} aria-label={$t('sched_back')}><Icon name="back" size={24} /></a>
    {#if selected}
      {@const task = selected}
      {@const summary = lastRunSummary(task, now, $t)}
      <header class="detail-header">
        <h2 id="tasks-heading">{nameOf(task)}</h2>
        {#if running(task)}
          <button class="run" aria-label={$t('sched_stop', { name: nameOf(task) })} title={$t('sched_stop', { name: nameOf(task) })} disabled={busy[task.id] || task.state === 'idle'} onclick={() => void stop(task)}><Icon name="close" size={22} /></button>
        {:else}
          <button class="run" aria-label={$t('sched_run', { name: nameOf(task) })} title={$t('sched_run', { name: nameOf(task) })} disabled={!!task.reason || busy[task.id]} onclick={() => void start(task)}><Icon name="play" size={22} /></button>
        {/if}
      </header>
      <p class="description">{task.description}</p>
      {#if task.reason}<p class="reason">{task.reason}</p>{/if}
      {#if task.kind === 'backup'}<p class="hint"><a href={resolve('admin/libraries/#backups-section')}>{$t('sched_backups_link')}</a></p>{/if}

      <button class="primary" disabled={saving || !!loadError} onclick={openEditor}><span aria-hidden="true">+</span> {$t('sched_add_trigger')}</button>

      <div class="panel" role="region" aria-label={$t('sched_triggers')}>
        <h3>{$t('sched_triggers')}</h3>
        {#each task.triggers as trigger (trigger.id)}
          <div class="trigger-row" class:disabled={!trigger.enabled}>
            <div>
              <p>{triggerLabel(trigger, $t)}</p>
              <small>
                {#if trigger.nextFireAt}{$t('sched_next', { date: date(trigger.nextFireAt) })}{/if}
                {#if trigger.maxRuntimeSeconds}{trigger.nextFireAt ? ' · ' : ''}{$t('sched_time_limit', { duration: durationLabel(trigger.maxRuntimeSeconds) })}{/if}
              </small>
            </div>
            <button class="remove" disabled={saving} aria-label={$t('sched_delete_trigger')} title={$t('sched_delete_trigger')} onclick={() => void removeTrigger(task, trigger)}><Icon name="trash" size={18} /></button>
          </div>
        {:else}
          <p class="empty">{$t('sched_no_trigger')}</p>
        {/each}
      </div>

      <div class="panel">
        <h3>{$t('sched_last_execution')}</h3>
        {#if running(task)}
          <p class="status" data-tone={summary.tone}>{summary.text}</p>
          {#if task.progress !== null && task.state === 'running'}<div class="progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={task.progress}><span style:width={`${task.progress}%`}></span></div>{/if}
        {:else if task.lastRun}
          {@const run = task.lastRun}
          <dl>
            {#if run.start}<div><dt>{$t('sched_started')}</dt><dd>{date(run.start)}</dd></div>{/if}
            {#if run.start && run.end && run.end >= run.start}<div><dt>{$t('sched_duration')}</dt><dd>{durationLabel(run.end - run.start)}</dd></div>{/if}
            <div><dt>{$t('sched_outcome')}</dt><dd data-tone={summary.tone}>{outcomeKeys[run.outcome] ? $t(outcomeKeys[run.outcome]) : run.outcome}</dd></div>
          </dl>
          {#if run.message}<p class="message" class:error={run.outcome === 'failed'}>{run.message}</p>{/if}
          {#if run.errorKind}<p class="meta">{$t('sched_error_type')} : {run.errorKind}{run.errorScope ? ` · ${run.errorScope}` : ''}</p>{/if}
        {:else}
          <p class="empty">{$t('sched_never')}</p>
        {/if}
        {#if task.issues.length}
          <details>
            <summary>{$t('sched_issues', { count: task.warningsCount || task.issues.length })}</summary>
            <ul>{#each task.issues as issue, index (index)}<li>{issue}</li>{/each}</ul>
          </details>
        {/if}
      </div>
    {:else if !loaded}
      <h2 id="tasks-heading">{$t('sched_loading')}</h2>
    {:else}
      <h2 id="tasks-heading">{$t('sched_not_found')}</h2>
      <p class="description">{$t('sched_not_found_hint')}</p>
    {/if}
  {:else}
    <header class="list-header">
      <h2 id="tasks-heading">{$t('sched_title')}</h2>
    </header>
    <p class="description">{$t('sched_intro')}</p>
    {#if !loaded}<p role="status">{$t('sched_loading')}</p>{/if}
    {#each groups as group (group.category)}
      <div class="category">
        <h3>{categoryLabel(group.category, $t)}</h3>
        <div class="task-list">
          {#each group.tasks as task (task.id)}
            {@const summary = lastRunSummary(task, now, $t)}
            <div class="task-row">
              <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- taskHref resolves the base path and task hash. -->
              <a class="task-link" href={taskHref(task.id)}>
                <span class="task-title">{nameOf(task)}</span>
                {#if task.state === 'running' && task.progress !== null}
                  <span class="progress" role="progressbar" aria-label={summary.text} aria-valuemin="0" aria-valuemax="100" aria-valuenow={task.progress}><span style:width={`${Math.max(task.progress, 2)}%`}></span></span>
                {:else}
                  <span class="task-subtitle" data-tone={summary.tone}>{summary.text}</span>
                {/if}
                {#if task.reason}<span class="task-subtitle">{task.reason}</span>{/if}
              </a>
              {#if running(task)}
                <button class="run" aria-label={$t('sched_stop', { name: nameOf(task) })} title={$t('sched_stop', { name: nameOf(task) })} disabled={busy[task.id] || task.state === 'idle'} onclick={() => void stop(task)}><Icon name="close" size={22} /></button>
              {:else}
                <button class="run" aria-label={$t('sched_run', { name: nameOf(task) })} title={$t('sched_run', { name: nameOf(task) })} disabled={!!task.reason || busy[task.id] || !loaded} onclick={() => void start(task)}><Icon name="play" size={22} /></button>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/each}
  {/if}
  {#if loadError}<p class="feedback error" role="alert">{loadError} <button onclick={() => void refresh()}><Icon name="refresh" size={16} /></button></p>{/if}
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
      <button type="button" disabled={saving} onclick={() => editor?.close()}>{$t('sched_cancel')}</button>
      <button class="primary" type="submit" disabled={saving}>{$t('sched_add')}</button>
    </div>
  </form>
</dialog>

<style>
  section { min-width: 0; color: var(--color-text); font-family: var(--font-sans); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  h3 { margin: 0; font-size: 16px; font-weight: 600; }
  .list-header, .detail-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 8px; }
  .description { max-width: 75ch; margin: 0 0 20px; font-size: 15px; line-height: 1.5; color: var(--color-muted); }

  .category { margin-top: 24px; }
  .category > h3 { margin: 0 0 4px; padding: 0 8px; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  .task-list { display: grid; background: var(--color-surface); border-radius: var(--radius-md); }
  .task-row { display: flex; align-items: center; gap: 8px; padding-right: 8px; border-top: 1px solid var(--color-line-subtle); transition: background-color var(--transition-fast); }
  .task-row:first-child { border-top: 0; }
  .task-row:hover { background: var(--color-surface-hover); }
  .task-link { display: grid; gap: 4px; flex: 1; min-width: 0; padding: 12px 16px; color: inherit; text-decoration: none; overflow-wrap: anywhere; }
  .task-link:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; border-radius: var(--radius-sm); }
  .task-title { font-size: 15px; font-weight: 600; color: var(--color-text); }
  .task-subtitle { font-size: 13px; line-height: 1.4; color: var(--color-muted); }
  [data-tone='error'] { color: var(--color-danger); }
  [data-tone='warning'] { color: var(--color-warning); }
  [data-tone='active'] { color: var(--color-accent); }

  .progress { display: block; width: min(320px, 100%); height: 6px; margin: 4px 0; overflow: hidden; border-radius: var(--radius-full); background: var(--color-line-subtle); }
  .progress > span { display: block; height: 100%; border-radius: inherit; background: var(--color-accent-solid); transition: width var(--transition-normal); }

  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 40px; padding: 8px 16px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 13px; cursor: pointer; transition: background-color var(--transition-fast), color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  .run { display: grid; place-items: center; flex: none; width: 44px; min-height: 44px; padding: 0; border: 0; border-radius: var(--radius-full); color: var(--color-muted); }
  .run:hover:not(:disabled) { background: var(--color-accent-soft); color: var(--color-accent); }
  .primary { border: 0; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .primary > span { font-size: 20px; line-height: 1; }

  .back { display: inline-grid; place-items: center; width: 44px; height: 44px; margin: 0 0 8px -10px; border-radius: var(--radius-full); color: var(--color-muted); transition: color var(--transition-fast), background-color var(--transition-fast); }
  .back:hover { color: var(--color-text); background: var(--color-raised); }
  .back:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .reason { margin: 0 0 16px; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-warning-soft); font-size: 14px; line-height: 1.5; }
  .hint { margin: -8px 0 16px; font-size: 14px; }
  .hint a { color: var(--color-accent); text-underline-offset: 3px; }

  .panel { margin: 20px 0 0; padding: 4px 20px 12px; background: var(--color-surface); border-radius: var(--radius-md); }
  .panel h3 { padding: 14px 0 8px; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  .trigger-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 8px 0; border-top: 1px solid var(--color-line-subtle); }
  .trigger-row.disabled { opacity: 0.6; }
  .trigger-row p { margin: 0 0 2px; font-size: 15px; font-weight: 600; }
  small { font-size: 13px; color: var(--color-muted); }
  .remove { display: grid; place-items: center; flex: none; width: 40px; min-height: 40px; padding: 0; border: 0; border-radius: var(--radius-full); color: var(--color-danger); }
  .remove:hover:not(:disabled) { background: var(--color-danger-soft); }
  .empty, .meta, .status { margin: 0; padding: 8px 0; color: var(--color-muted); font-size: 14px; line-height: 1.5; }
  .message { margin: 4px 0 8px; font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .message.error { padding: 10px 14px; border-radius: var(--radius-sm); background: var(--color-danger-soft); }
  dl { display: flex; flex-wrap: wrap; gap: 12px 32px; margin: 8px 0; }
  dt { font-size: 12px; color: var(--color-muted); }
  dd { margin: 4px 0 0; font-size: 15px; font-weight: 600; font-variant-numeric: tabular-nums; }
  details summary { display: flex; align-items: center; min-height: 40px; cursor: pointer; color: var(--color-accent); font-size: 13px; }
  li { color: var(--color-muted); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }

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
    .task-link { padding: 10px 12px; }
    .task-title { font-size: 14px; }
    dialog { width: 100vw; max-width: 100vw; margin: auto 0 0; padding: 20px 16px calc(20px + env(safe-area-inset-bottom, 0px)); border-radius: var(--radius-xl) var(--radius-xl) 0 0; }
    .dialog-actions button { flex: 1 1 auto; }
    input, select { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) {
    button, .task-row, .back, .progress > span { transition: none; }
  }
</style>

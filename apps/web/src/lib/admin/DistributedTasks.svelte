<script lang="ts">
  import { untrack } from 'svelte';
  import { adminRequest } from './request';
  import type { AdminNode, Library } from './types';
  import { t, fmt, translatedLabels } from '../i18n';

  let { active, libraries, nodes }: { active: boolean; libraries: Library[]; nodes: AdminNode[] } = $props();
  type Job = {
    id: string; libraryId: string; owner: string; nodeId: string; status: string;
    category: string; priority: number; attempt: number; failureCount?: number; maxAttempts: number; progress: number; processedItems?: number;
    targetId?: string; resultJson?: string;
    createdAt: number; updatedAt: number; startedAt: number | null; finishedAt: number | null;
    leaseUntil: number | null; error: string;
  };
  type Page = { jobs: Job[]; limit: number; offset: number };
  const limit = 50;
  const labels: Readonly<Record<string, string>> = translatedLabels({ queued: 'waiting', running: 'stats_kpi_active', paused: 'suspended', completed: 'stats_status_completed', cancelled: 'sched_outcome_cancelled', error: 'sched_outcome_failed' });
  const categories: Readonly<Record<string, string>> = translatedLabels({ scan: 'incremental_scan', analyze: 'technical_analysis', prepare: 'media_preparation', image: 'image_processing' });
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
  let loadError = $state('');
  let statusFilter = $state('all');
  let notice = $state('');
  let observedAt = $state(0);
  let results = $state<Record<string, string>>({});
  let resultErrors = $state<Record<string, string>>({});
  let resultLoading = $state<Record<string, boolean>>({});
  let submission: { signature: string; key: string } | null = null;
  const availableLibraries = $derived(libraries.filter(library => library.enabled && !library.id.startsWith('fixture-')));
  const visibleJobs = $derived(jobs.filter(job => statusFilter === 'all' || job.status === statusFilter));

  async function refresh(nextOffset = offset) {
    if (loading) return;
    loading = true;
    try {
      const page = await adminRequest<Page>(`/api/v1/admin/tasks/jobs?limit=${limit}&offset=${nextOffset}`);
      jobs = page.jobs;
      const visible = new Set(jobs.map(job => job.id));
      results = Object.fromEntries(Object.entries(results).filter(([id]) => visible.has(id)));
      resultErrors = Object.fromEntries(Object.entries(resultErrors).filter(([id]) => visible.has(id)));
      resultLoading = Object.fromEntries(Object.entries(resultLoading).filter(([id]) => visible.has(id)));
      offset = nextOffset; observedAt = Math.floor(Date.now() / 1000); loaded = true; loadError = '';
    } catch (cause) { loadError = cause instanceof Error ? cause.message : $t('task_tracking_is_unavailable'); }
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
      notice = category === 'scan' ? $t('scan_added_to_the_persistent_queue') : $t('task_added_to_the_persistent_queue');
      await refresh(0);
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('unable_to_schedule_the_task'); }
    finally { mutating = false; }
  }
  async function control(job: Job, action: 'pause' | 'resume' | 'cancel') {
    if (mutating) return;
    mutating = true; error = ''; notice = '';
    try {
      await adminRequest(`/api/v1/admin/tasks/jobs/${encodeURIComponent(job.id)}/${action}`, { method: 'POST' });
      notice = action === 'pause' ? $t('suspension_requested') : action === 'resume' ? $t('task_put_back_on_hold') : $t('cancellation_requested');
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('action_impossible'); }
    finally { mutating = false; }
  }
  async function loadResult(job: Job) {
    if (resultLoading[job.id]) return;
    resultLoading[job.id] = true; resultErrors[job.id] = '';
    try {
      const detail = await adminRequest<Job>(`/api/v1/admin/tasks/jobs/${encodeURIComponent(job.id)}`);
      if (jobs.some(item => item.id === job.id)) results[job.id] = detail.resultJson ?? '{}';
    } catch (cause) { resultErrors[job.id] = cause instanceof Error ? cause.message : $t('result_unavailable'); }
    finally { resultLoading[job.id] = false; }
  }
  function date(value: number) { return $fmt.dateTime(value); }
  function duration(job: Job) {
    if (job.startedAt === null) return $t('not_started_yet');
    const seconds = Math.max(0, (job.finishedAt ?? observedAt) - job.startedAt);
    return seconds < 60 ? `${seconds} s` : `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
  }
</script>

<section aria-labelledby="distributed-tasks-heading">
  <header><h3 id="distributed-tasks-heading">{$t('admin_operations_tab')}</h3><button disabled={loading} onclick={() => void refresh()}>{$t('refresh_the_queue')}</button></header>
  <p class="explanation">{$t('indexing_and_processing_on_your_servers')}</p>
  <details class="enqueue"><summary>{$t('add_a_task')}</summary><p class="form-help">{$t('choose_a_library_advanced_tasks_use')}</p>
  <form onsubmit={enqueue}>
    <label>{$t('library')}<select aria-label={$t('target_library')} bind:value={libraryId} disabled={mutating}><option value="">{$t('choose_a_library')}</option>{#each availableLibraries as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
    <label>{$t('task_type')}<select aria-label={$t('task_type')} bind:value={category} disabled={mutating}>{#each Object.entries(categories) as [value, title] (value)}<option {value}>{title}</option>{/each}</select></label>
    {#if category !== 'scan'}<label>{category === 'image' ? $t('inventoried_image_identifier') : $t('inventoried_asset_identifier')}<input aria-label={$t('target_identifier')} required maxlength="128" bind:value={targetId} disabled={mutating} placeholder={category === 'image' ? $t('image_id') : $t('asset_id')} /></label>{/if}
    {#if category === 'prepare'}<label>{$t('component_to_prepare')}<input aria-label={$t('component_to_prepare')} required maxlength="64" pattern="video|audio-[0-9]+|subtitle-[0-9]+" bind:value={component} disabled={mutating} placeholder={$t('video')} /></label><p class="form-help">{$t('formats_video_audio_1_or_subtitle')}</p>{/if}
    {#if category === 'image'}<label>{$t('width')}<select aria-label={$t('image_width')} bind:value={width} disabled={mutating}><option value={320}>{$t('n_320_px')}</option><option value={640}>{$t('n_640_px')}</option><option value={1280}>{$t('n_1280_px')}</option></select></label>{/if}
    <button type="submit" disabled={!libraryId || mutating || (category !== 'scan' && !targetId.trim())}>{category === 'scan' ? $t('schedule_a_scan') : $t('schedule_the_task')}</button>
  </form>
  </details>
  {#if error}<p class="error queue-action-error" role="alert">{error}</p>{/if}
  {#if loadError}<div class="error queue-load-error" role="alert"><p>{loadError}</p>{#if loaded}<p>{$t('the_data_shown_comes_from_the')}</p>{/if}<button disabled={loading} onclick={() => void refresh()}>{$t('retry_loading')}</button></div>{/if}
  {#if notice}<p class="notice" role="status">{notice}</p>{/if}
  {#if loaded}
    <div class="queue-tools"><label>{$t('task_status')}<select bind:value={statusFilter}><option value="all">{$t('all_statuses')}</option>{#each Object.entries(labels) as [value, title] (value)}<option {value}>{title}</option>{/each}</select></label><p>{$t('of_tasks_on_this_page', { count: visibleJobs.length, count2: jobs.length })}{#if observedAt}<span>{$t('admin_updated_at', { value: new Date(observedAt * 1000).toLocaleTimeString() })}</span>{/if}</p></div>
    <div class="jobs" aria-busy={loading}>
      {#each visibleJobs as job (job.id)}
        <article aria-label={$t('task', { id: job.id })}>
          <div class="job-heading"><div><h4>{categories[job.category] ?? job.category}</h4><p>{libraries.find(library => library.id === job.libraryId)?.name ?? job.libraryId}</p></div><span class="status" data-status={job.status} class:failed={job.status === 'error'}>{labels[job.status] ?? job.status}</span></div>
          <dl>
            <div><dt>{$t('executing_node')}</dt><dd>{nodes.find(node => node.id === job.nodeId)?.name ?? (job.nodeId === 'local' ? $t('admin_local_node') : job.nodeId || $t('awaiting_assignment'))}</dd></div>
            <div><dt>{$t('sched_duration')}</dt><dd>{duration(job)}</dd></div>
          </dl>
          {#if !['error', 'cancelled'].includes(job.status) && (job.processedItems === undefined || job.progress === 100)}<div class="progress"><progress max="100" value={job.progress} aria-label={$t('task_progress')}></progress><span>{job.progress} %</span></div>{/if}
          {#if job.processedItems !== undefined}<p class="processed">{$t('processed_items', { count: job.processedItems, unknown: job.progress < 100 ? $t('total_unknown') : '' })}</p>{/if}
          {#if job.error}<p class="job-error">{job.error}</p>{/if}
          {#if ['queued', 'running', 'paused'].includes(job.status)}
            <div class="actions">
              {#if job.status === 'paused'}<button disabled={mutating || job.leaseUntil !== null} onclick={() => void control(job, 'resume')}>{job.leaseUntil !== null ? $t('stopping') : 'Reprendre'}</button>{:else}<button disabled={mutating} onclick={() => void control(job, 'pause')}>{$t('suspend')}</button>{/if}
              <button disabled={mutating} onclick={() => void control(job, 'cancel')}>{$t('sched_cancel')}</button>
            </div>
          {/if}
          <details><summary>{$t('task_details')}</summary><p>{$t('admin_identifier', { id: job.id })}</p><p>{$t('job_owner_line', { owner: job.owner, attempts: job.attempt, failures: job.failureCount ?? '—', max: job.maxAttempts })}</p><p>{$t('created_on_updated_on', { value: date(job.createdAt), value2: date(job.updatedAt) })}</p><p>{$t('job_category_line', { category: categories[job.category] ?? job.category, priority: job.priority })}</p>{#if job.targetId}<p>{$t('target', { targetId: job.targetId })}</p>{/if}{#if job.leaseUntil}<p>{$t('assignment_valid_until', { value: date(job.leaseUntil) })}</p>{/if}</details>
          {#if job.category !== 'scan'}<details ontoggle={(event) => { if (event.currentTarget.open) void loadResult(job); }}><summary>{$t('run_result')}</summary>{#if resultLoading[job.id]}<p role="status">{$t('loading_result')}</p>{:else if resultErrors[job.id]}<p class="job-error" role="alert">{resultErrors[job.id]}</p><button onclick={() => void loadResult(job)}>{$t('request_retry')}</button>{:else if results[job.id]}<pre>{results[job.id]}</pre>{/if}</details>{/if}
        </article>
      {:else}<div class="empty"><p>{jobs.length ? $t('no_task_on_this_page_matches') : $t('no_tasks_on_this_page')}</p>{#if statusFilter !== 'all'}<button onclick={() => statusFilter = 'all'}>{$t('show_all_statuses')}</button>{/if}</div>{/each}
    </div>
    <nav aria-label={$t('task_pages')}><button disabled={loading || offset === 0} onclick={() => void refresh(Math.max(0, offset - limit))}>{$t('previous')}</button><span>{$t('admin_page', { value: Math.floor(offset / limit) + 1 })}</span><button disabled={loading || jobs.length < limit} onclick={() => void refresh(offset + limit)}>{$t('next')}</button></nav>
  {:else if loading}<p role="status">{$t('sched_loading')}</p>{/if}
</section>

<style>
  section { display: grid; gap: 16px; min-width: 0; margin-top: 8px; color: var(--color-text); font-family: var(--font-sans); }
  header, .job-heading, .actions, nav, .progress { display: flex; align-items: center; gap: 12px; }
  header, .job-heading { justify-content: space-between; }
  h3 { margin: 0; font-size: 18px; font-weight: 600; }
  h4 { margin: 0; font-size: 15px; font-weight: 600; }
  .job-heading { align-items: flex-start; }
  .job-heading > div { min-width: 0; }
  .job-heading p { margin: 2px 0 0; font-size: 13px; color: var(--color-muted); }
  .explanation, dt, details, .empty { color: var(--color-muted); line-height: 1.5; }
  .explanation { max-width: 75ch; margin: 0; font-size: 14px; }

  /* Ajout d'une tâche */
  .enqueue { padding: 0 20px; background: var(--color-raised); border-radius: var(--radius-md); }
  .enqueue > summary { font-size: 14px; color: var(--color-text); }
  .form-help { flex-basis: 100%; margin: 0; font-size: 13px; line-height: 1.5; color: var(--color-muted); }
  form { display: flex; flex-wrap: wrap; align-items: end; gap: 12px; margin: 8px 0 16px; }
  label { display: grid; gap: 6px; flex: 1; min-width: min(240px, 100%); font-size: 13px; color: var(--color-muted); }
  select, input { box-sizing: border-box; width: 100%; min-width: 0; min-height: 40px; padding: 8px 14px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; }
  .enqueue select, .enqueue input { background: var(--color-surface); }
  select:focus-visible, input:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  select:disabled, input:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Boutons : ghost, un seul primaire */
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; flex-shrink: 0; min-height: 40px; padding: 8px 16px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 13px; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  form button[type='submit'] { border-color: transparent; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  form button[type='submit']:hover:not(:disabled) { background: var(--color-accent-hover); opacity: 0.9; }

  /* Outils de la file */
  .queue-tools { display: flex; align-items: end; justify-content: space-between; gap: 16px; }
  .queue-tools label { flex: 0 1 240px; min-width: 0; }
  .queue-tools p { margin: 0; font-size: 12px; line-height: 1.6; color: var(--color-muted); text-align: right; font-variant-numeric: tabular-nums; }
  .queue-tools span { display: block; }

  /* Tâches : lignes dans une surface plate */
  .jobs { display: grid; padding: 4px 20px; background: var(--color-surface); border-radius: var(--radius-md); }
  article { min-width: 0; padding: 14px 0; border-top: 1px solid var(--color-line-subtle); overflow-wrap: anywhere; }
  article:first-child { border-top: 0; }
  .status { flex-shrink: 0; color: var(--color-muted); font-size: 13px; font-weight: 600; white-space: nowrap; }
  .status[data-status='completed'] { color: var(--color-success); }
  .status[data-status='error'], .failed { color: var(--color-danger); }
  .status[data-status='paused'] { color: var(--color-warning); }
  .status[data-status='running'], .status[data-status='queued'] { color: var(--color-accent); }
  dl { display: flex; flex-wrap: wrap; gap: 4px 24px; margin: 8px 0 0; font-size: 13px; }
  dl div { display: flex; gap: 6px; }
  dt { font-size: 13px; }
  dt::after { content: ' :'; }
  dd { margin: 0; color: var(--color-text); }

  /* Progression */
  .progress { margin: 10px 0 0; }
  progress { flex: 1; min-width: 0; height: 4px; border: 0; border-radius: var(--radius-pill); background: var(--color-line-strong); overflow: hidden; appearance: none; -webkit-appearance: none; accent-color: var(--color-accent); }
  progress::-webkit-progress-bar { background: var(--color-line-strong); border-radius: var(--radius-pill); }
  progress::-webkit-progress-value { background: var(--color-accent); border-radius: var(--radius-pill); }
  progress::-moz-progress-bar { background: var(--color-accent); border-radius: var(--radius-pill); }
  .progress span { min-width: 40px; font-size: 12px; font-variant-numeric: tabular-nums; color: var(--color-muted); text-align: right; }
  .processed { margin: 6px 0 0; font-size: 13px; color: var(--color-muted); }

  .actions { flex-wrap: wrap; margin: 8px 0 0; }
  .actions button { min-height: 34px; padding: 4px 12px; }
  details { font-size: 13px; }
  details p { margin: 6px 0; }
  summary { min-height: 36px; padding-block: 8px; box-sizing: border-box; cursor: pointer; color: var(--color-accent); }
  .enqueue > summary { min-height: 48px; padding-block: 0; display: flex; align-items: center; color: var(--color-text); }
  summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-xs); }
  pre { max-height: 320px; margin: 8px 0; padding: 10px 12px; overflow: auto; border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font-family: var(--font-mono); font-size: 12px; line-height: 1.6; white-space: pre-wrap; overflow-wrap: anywhere; }
  .empty { padding: 20px 0; font-size: 14px; }
  .empty p { margin: 0 0 8px; }

  /* Pagination */
  nav { justify-content: flex-end; flex-wrap: wrap; }
  nav span { font-size: 13px; color: var(--color-muted); font-variant-numeric: tabular-nums; }

  /* Retours */
  .error, .job-error, .notice { margin: 0; padding: 10px 14px; border-radius: var(--radius-sm); color: var(--color-text); font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .error, .job-error { background: var(--color-danger-soft); }
  .job-error { margin-top: 8px; }
  .error p { margin: 0 0 8px; }
  .notice { background: var(--color-success-soft); }
  [role='status']:not(.notice) { color: var(--color-muted); }

  @media (max-width: 640px) {
    header { flex-wrap: wrap; }
    .jobs { padding: 4px 14px; }
    .job-heading { flex-wrap: wrap; }
    .queue-tools { flex-direction: column; align-items: stretch; gap: 8px; }
    .queue-tools label { flex: auto; }
    .queue-tools p { text-align: left; }
    form > button { flex: 1 1 100%; }
    input, select { font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button { transition: none; }
  }
</style>

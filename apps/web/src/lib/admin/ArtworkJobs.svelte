<script lang="ts">
  import { onMount } from 'svelte';
  import { adminRequest } from './request';
  import { t } from '../i18n';

  /** One durable artwork collection pass (`/api/v1/admin/artwork/jobs`). */
  interface ArtworkJob {
    id: string; libraryId: string; owner: string; status: string;
    processed: number; downloaded: number; duplicates: number; skipped: number; errors: number;
    lastError: string;
  }

  let { libraryNames }: { libraryNames: Record<string, string> } = $props();
  let jobs = $state<ArtworkJob[]>([]);
  let error = $state('');
  let busy = $state('');
  const ACTIVE = new Set(['queued', 'running', 'paused']);
  const active = $derived(jobs.some((job) => job.status === 'queued' || job.status === 'running'));

  async function refresh() {
    try { jobs = await adminRequest<ArtworkJob[]>('/api/v1/admin/artwork/jobs'); error = ''; }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('artwork_jobs_unavailable'); }
  }

  // Polls only while a pass is moving, so an idle admin page stays quiet.
  onMount(() => {
    void refresh();
    const timer = setInterval(() => { if (active && document.visibilityState === 'visible') void refresh(); }, 5000);
    return () => clearInterval(timer);
  });

  async function control(job: ArtworkJob, action: 'pause' | 'resume' | 'cancel') {
    if (busy) return;
    busy = job.id;
    try {
      await adminRequest(`/api/v1/admin/artwork/jobs/${encodeURIComponent(job.id)}`, { method: 'PUT', body: JSON.stringify({ action }) });
      await refresh();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('artwork_jobs_unavailable');
    } finally {
      busy = '';
    }
  }

  const STATUS = {
    queued: 'artwork_job_queued', running: 'artwork_job_running', paused: 'artwork_job_paused',
    completed: 'artwork_job_completed', cancelled: 'artwork_job_cancelled', failed: 'artwork_job_failed'
  } as const;
  const statusKey = (status: string) => STATUS[status as keyof typeof STATUS] ?? STATUS.failed;
  const shown = $derived(jobs.filter((job, index) => ACTIVE.has(job.status) || index < 8));
</script>

<div class="category" role="region" aria-labelledby="artwork-jobs-heading">
  <h3 id="artwork-jobs-heading">{$t('artwork_jobs_title')}</h3>
  <p class="intro">{$t('artwork_jobs_intro')}</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if shown.length}
    <ul class="jobs">
      {#each shown as job (job.id)}
        <li class="job">
          <div class="head">
            <strong>{libraryNames[job.libraryId] ?? job.libraryId}</strong>
            <span class="status" data-status={job.status}>{$t(statusKey(job.status))}</span>
          </div>
          <p class="counts">
            {$t('artwork_jobs_counts', { processed: job.processed, downloaded: job.downloaded, duplicates: job.duplicates, skipped: job.skipped, errors: job.errors })}
          </p>
          {#if job.lastError}<p class="last-error">{job.lastError}</p>{/if}
          {#if ACTIVE.has(job.status)}
            <div class="actions">
              {#if job.status === 'paused'}
                <button disabled={busy !== ''} onclick={() => control(job, 'resume')}>{$t('artwork_jobs_resume')}</button>
              {:else}
                <button disabled={busy !== ''} onclick={() => control(job, 'pause')}>{$t('artwork_jobs_pause')}</button>
              {/if}
              <button class="danger" disabled={busy !== ''} onclick={() => control(job, 'cancel')}>{$t('artwork_jobs_cancel')}</button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if !error}
    <p class="intro">{$t('artwork_jobs_empty')}</p>
  {/if}
</div>

<style>
  .category { margin-top: 24px; }
  h3 { margin: 0 0 4px; font-size: 16px; font-weight: 600; }
  .intro { margin: 0 0 12px; color: var(--color-muted); font-size: 14px; line-height: 1.5; }
  .jobs { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .job { display: grid; gap: 6px; padding: 12px 16px; border-radius: var(--radius-md); background: var(--color-surface); min-width: 0; }
  .head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; }
  .status { padding: 2px 10px; border-radius: 999px; background: var(--color-canvas); color: var(--color-muted); font-size: 12px; font-weight: 600; }
  .status[data-status='running'] { color: var(--color-accent); }
  .status[data-status='failed'] { color: var(--color-danger, #e5484d); }
  .counts, .last-error { margin: 0; font-size: 13px; color: var(--color-muted); overflow-wrap: anywhere; }
  .last-error { color: var(--color-danger, #e5484d); }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; }
  button { min-height: 36px; padding: 0 14px; border: 1px solid var(--color-line); border-radius: var(--radius-sm, 8px); background: var(--color-canvas); color: var(--color-text); font: inherit; font-size: 13px; font-weight: 600; cursor: pointer; }
  button.danger { color: var(--color-danger, #e5484d); }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .error { margin: 0 0 8px; color: var(--color-danger, #e5484d); font-weight: 600; }
</style>

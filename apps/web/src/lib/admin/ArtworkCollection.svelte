<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { session } from '../auth/session';
  import { invalidateCatalog } from '../catalog/page-cache';
  import type { LibraryOptions } from './types';
  import { t } from '../i18n';
  let { libraryId, options }: { libraryId: string; options?: LibraryOptions } = $props();
  type Job = { id: string; status: string; processed: number; downloaded: number; duplicates: number; skipped: number; errors: number; lastError: string; cursorWork: string };
  let job = $state<Job | null>(null), error = $state(''), busy = $state(false);
  let maxWorks = $state(1000);
  const active = $derived(job && ['queued', 'running', 'paused'].includes(job.status));
  let alive = false, generation = 0;
  async function refresh() {
    const requestedLibrary = libraryId, request = generation;
    if (!requestedLibrary) { job = null; return; }
    try {
      const response = await fetch(`/api/v1/admin/artwork/jobs?libraryId=${encodeURIComponent(requestedLibrary)}`, { cache: 'no-store', signal: AbortSignal.timeout(8000) });
      if (!response.ok) throw new Error($t('progress_unavailable'));
      const jobs: Job[] = await response.json();
      if (alive && requestedLibrary === libraryId && request === generation) {
        const previous = job?.downloaded;
        job = jobs.find(value => ['queued', 'running', 'paused'].includes(value.status)) ?? jobs[0] ?? null;
        if (job?.downloaded !== previous) invalidateCatalog(requestedLibrary);
      }
    } catch (cause) { if (alive && request === generation) error = cause instanceof Error ? cause.message : 'Erreur'; }
  }
  $effect(() => { void libraryId; untrack(() => { generation++; job = null; if (alive) void refresh(); }); });
  // Languages, neutral images and per-work limit come from the library settings (Images).
  const languages = $derived(options?.artworkLanguages ?? ['fr', 'en']);
  const includeNeutral = $derived(options?.artworkIncludeNeutral ?? true);
  const maxImages = $derived(options?.artworkMaxImages ?? 12);
  onMount(() => { alive = true; void refresh(); const timer = setInterval(() => void refresh(), 5000); return () => { alive = false; clearInterval(timer); }; });
  async function submit(action?: 'pause' | 'resume' | 'cancel') {
    if (busy || !libraryId) return;
    busy = true; error = '';
    try {
      const response = await fetch(action ? `/api/v1/admin/artwork/jobs/${encodeURIComponent(job?.id ?? '')}` : '/api/v1/admin/artwork/jobs', {
        method: action ? 'PUT' : 'POST', headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify(action ? { action } : { libraryId, after: job?.status === 'completed' ? job.cursorWork : undefined, languages, includeNeutral, maxImages, maxWorks }),
        signal: AbortSignal.timeout(10000)
      });
      if (!response.ok) throw new Error($t('collection_not_saved_http', { status: response.status }));
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }
</script>
<section aria-label={$t('durable_image_collection')}>
  <h3>{$t('collect_images')}</h3>
  <p>{$t('collection_continues_after_this_page_is')}</p>
  <form onsubmit={event => { event.preventDefault(); void submit(); }}>
    <fieldset disabled={busy || !!active}>
      <label>{$t('works_budget_per_collection')}<input type="number" min="1" max="100000" bind:value={maxWorks} required /></label>
    </fieldset>
    {#if !active}<button type="submit" disabled={busy || !libraryId}>{job?.status === 'completed' ? $t('continue_the_collection') : $t('start_the_collection')}</button>{/if}
  </form>
  {#if job}
    <p role="status">{$t('collection_progress', { status: ({ queued: $t('waiting'), running: $t('stats_kpi_active'), paused: $t('paused'), completed: $t('collection_complete'), cancelled: $t('sched_outcome_cancelled') })[job.status] ?? job.status, processed: job.processed, downloaded: job.downloaded, duplicates: job.duplicates, skipped: job.skipped, errors: job.errors })}</p>
    {#if active}<div class="actions">
      <button type="button" disabled={busy} onclick={() => void submit(job?.status === 'paused' ? 'resume' : 'pause')}>{job.status === 'paused' ? $t('resume_the_collection') : $t('stats_w_pause')}</button>
      <button type="button" disabled={busy} onclick={() => void submit('cancel')}>{$t('cancel_collection')}</button>
    </div>{/if}
    {#if job.lastError}<p>{$t('admin_last_result', { lastError: job.lastError })}</p>{/if}
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  h3 { margin-top: 0; font-size: 17px; }
  p { color: var(--mu); font-size: 14px; line-height: 1.6; }
  fieldset { display: grid; gap: 12px; border: 0; padding: 0; margin: 16px 0; }
  label { display: grid; gap: 6px; }
  .actions { display: flex; gap: 12px; flex-wrap: wrap; }
</style>

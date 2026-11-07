<script lang="ts">
  import { onMount } from 'svelte';
  import { applicationRequest, dateLabel, features, type RemoteServer } from './api';
  import { t, translatedLabels } from '../i18n';
  interface ImportJob { id: string; sourceUrl: string; status: string; phase: string; processed: number; errors: number; updatedAt: number; lastError?: string; failures?: { phase: string; sourceId: string; error: string }[] }
  interface Preview { id: string; sourceUrl: string; users: { id: string; name: string }[]; servers: { id: string; name: string; kind: string }[]; counts: { requests: number; reports: number }; warnings: string[] }
  let { servers }: { servers: RemoteServer[] } = $props();
  let baseUrl = $state('');
  let apiKey = $state('');
  let preview = $state<Preview | null>(null);
  let userMappings = $state<Record<string, string>>({});
  let serverMappings = $state<Record<string, string>>({});
  let users = $state<{ id: string; login: string; displayName?: string }[]>([]);
  let jobs = $state<ImportJob[]>([]);
  let busy = $state(false);
  let error = $state('');
  let acknowledged = $state(false);
  const labels: Readonly<Record<string, string>> = translatedLabels({ preview: 'player_preview', ready: 'ready', pending: 'to_start', running: 'stats_kpi_active', completed: 'done', completed_with_errors: 'completed_with_errors', failed: 'failed_3', paused: 'paused', requests: 'settings_requests', reports: 'reports_title', comments: 'comments', done: 'done' });
  async function refresh() { jobs = (await applicationRequest<{ imports: ImportJob[] }>('/api/v1/admin/imports')).imports; }
  onMount(() => {
    void refresh().catch(cause => error = cause instanceof Error ? cause.message : $t('imports_unavailable'));
    const interval = setInterval(() => { if (jobs.some(job => job.status === 'running') && !busy) void refresh().catch(() => {}); }, 5000);
    return () => clearInterval(interval);
  });
  async function analyze(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; preview = null; acknowledged = false;
    try {
      const result = await applicationRequest<Preview>('/api/v1/admin/imports/preview', { method: 'POST', body: JSON.stringify({ baseUrl, apiKey }) });
      apiKey = ''; preview = result; userMappings = {}; serverMappings = {};
      users = (await applicationRequest<{ users: { id: string; login: string; displayName?: string }[] }>('/api/v1/admin/users')).users;
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('analysis_failed'); }
    finally { busy = false; }
  }
  async function start() {
    if (!preview || !acknowledged) return; busy = true; error = '';
    try {
      await applicationRequest(`/api/v1/admin/imports/${encodeURIComponent(preview.id)}/start`, { method: 'POST', body: JSON.stringify({ userMappings: Object.fromEntries(Object.entries(userMappings).filter(([, id]) => id)), serverMappings: Object.fromEntries(Object.entries(serverMappings).filter(([, id]) => id)) }) });
      preview = null; await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('import_failed'); }
    finally { busy = false; }
  }
  async function progress(id: string, action: 'run' | 'pause') {
    busy = true; error = '';
    try { await applicationRequest(`/api/v1/admin/imports/${encodeURIComponent(id)}/${action}`, { method: 'POST' }); await refresh(); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('action_impossible'); }
    finally { busy = false; }
  }
</script>
<section class="panel stack" aria-labelledby="import-title"><h2 id="import-title">{$t('import_existing_folders')}</h2><p class="muted">{$t('read_only_import_of_requests_reports')}</p>
  <form class="stack" onsubmit={analyze}><div class="fields"><label>{$t('previous_application_address')}<input type="url" required bind:value={baseUrl} maxlength="2048" placeholder="http://serveur:5055" /></label><label>{$t('source_administrator_api_key')}<input type="password" required autocomplete="new-password" bind:value={apiKey} maxlength="512" /></label></div><div><button disabled={busy}>{busy ? 'Analyse…' : $t('analyse_without_importing')}</button></div></form>
  {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
  {#if preview}<div class="detail stack"><h3>{$t('preview_of', { sourceUrl: preview.sourceUrl })}</h3><p>{$t('applications_text', { requests: preview.counts.requests, reports: preview.counts.reports })}</p>{#each preview.warnings as warning, itemIndex (itemIndex)}<p class="feedback">{warning}</p>{/each}
    {#if !$features.reports && preview.counts.reports}<p class="feedback">{$t('also_enable_reports_to_bring_over')}</p>{/if}
    <h3>{$t('match_users')}</h3><p class="muted">{$t('authors_with_no_match_will_be')}</p><div class="fields">{#each preview.users as source (source.id)}<label>{source.name}<select bind:value={userMappings[source.id]}><option value="">{$t('historical_author')}</option>{#each users as user (user.id)}<option value={user.id}>{user.displayName || user.login}</option>{/each}</select></label>{/each}</div>
    {#if preview.servers.length}<h3>{$t('match_servers')}</h3><div class="fields">{#each preview.servers as source (source.id)}<label>{source.name} · {source.kind}<select bind:value={serverMappings[source.id]}><option value="">{$t('no_match')}</option>{#each servers.filter(server => server.kind === source.kind) as target (target.id)}<option value={target.id}>{target.name}</option>{/each}</select></label>{/each}</div>{/if}
    <label class="check"><input type="checkbox" bind:checked={acknowledged} />{$t('i_have_checked_the_matches_imported')}</label><div><button class="primary" disabled={busy || !acknowledged || (!$features.requests && preview.counts.requests > 0) || (!$features.reports && preview.counts.reports > 0)} onclick={start}>{$t('import_folders')}</button></div>
  </div>{/if}
  <div class="heading"><h3>{$t('import_history')}</h3><button disabled={busy} onclick={() => void refresh().catch(cause => error = cause instanceof Error ? cause.message : $t('refresh_failed'))}>{$t('refresh')}</button></div><ul class="records">{#each jobs as job (job.id)}<li class="record"><div class="record-header"><div><h3>{job.sourceUrl}</h3><small>{dateLabel(job.updatedAt)} · {labels[job.phase] ?? job.phase}</small></div><span class="badge">{labels[job.status] ?? job.status}</span></div><p>{$t('processed', { processed: job.processed, errors: job.errors })}</p>{#if job.lastError}<p class="error">{job.lastError}</p>{/if}{#if job.failures?.length}<details><summary>{$t('items_to_check')}</summary>{#each job.failures as failure, index (index)}<p class="text">{labels[failure.phase] ?? failure.phase} · {failure.sourceId} : {failure.error}</p>{/each}</details>{/if}<div class="row">{#if job.status === 'running'}<button disabled={busy} onclick={() => progress(job.id, 'pause')}>{$t('stats_w_pause')}</button>{/if}{#if ['running','paused','failed'].includes(job.status)}<button disabled={busy} onclick={() => progress(job.id, 'run')}>{$t('process_the_next_batch')}</button>{/if}</div></li>{:else}<li class="muted">{$t('no_imports_recorded')}</li>{/each}</ul>
</section>

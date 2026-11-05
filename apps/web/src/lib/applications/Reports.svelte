<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { applicationRequest, categoryLabels, dateLabel, features, loadFeatures, statusLabels, type MediaReport } from './api';
  import './applications.css';
  import { t } from '../i18n';
  let { embedded = false }: { embedded?: boolean } = $props();
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let reports = $state<MediaReport[]>([]);
  let nextOffset = $state<number | null>(null);
  let selected = $state<MediaReport | null>(null);
  let message = $state('');
  let filter = $state('all');
  const isAdmin = $derived($session?.user.role === 'administrator');
  const visible = $derived(reports.filter(item => filter === 'all' || item.status === filter));
  async function refresh(append = false) {
    const result = await applicationRequest<{ reports: MediaReport[]; nextOffset?: number | null }>(`/api/v1/reports${append && nextOffset !== null ? `?offset=${nextOffset}` : ''}`);
    reports = append ? [...reports, ...result.reports.filter(item => !reports.some(existing => existing.id === item.id))] : result.reports;
    nextOffset = result.nextOffset ?? null;
  }
  onMount(() => { void (async () => { try { const available = await loadFeatures(true); if (available.reports) await refresh(); } catch (cause) { error = cause instanceof Error ? cause.message : $t('reports_unavailable_short'); } finally { loading = false; } })(); });
  async function show(id: string) {
    busy = true; error = ''; message = '';
    try { selected = selected?.id === id ? null : await applicationRequest<MediaReport>(`/api/v1/reports/${encodeURIComponent(id)}`); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('detail_unavailable'); }
    finally { busy = false; }
  }
  async function update(status: string) {
    if (!selected) return; busy = true; error = '';
    try { await applicationRequest(`/api/v1/reports/${encodeURIComponent(selected.id)}/status`, { method: 'POST', body: JSON.stringify({ status }) }); selected = await applicationRequest<MediaReport>(`/api/v1/reports/${encodeURIComponent(selected.id)}`); await refresh(); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('modification_impossible'); }
    finally { busy = false; }
  }
  async function comment(event: SubmitEvent) {
    event.preventDefault(); if (!selected) return; busy = true; error = '';
    try { await applicationRequest(`/api/v1/reports/${encodeURIComponent(selected.id)}/comments`, { method: 'POST', body: JSON.stringify({ message: message.trim() }) }); message = ''; selected = await applicationRequest<MediaReport>(`/api/v1/reports/${encodeURIComponent(selected.id)}`); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('comment_impossible'); }
    finally { busy = false; }
  }
</script>
<div class="application-page">
  <header class="heading"><div><p class="muted">{$t('reports_subtitle')}</p>{#if !embedded}<h1>{$t('reports_title')}</h1>{/if}<p class="muted">{$t('reports_hint')}</p></div><button disabled={busy || loading || !$features.reports} onclick={() => { busy = true; void refresh().catch(cause => error = String(cause instanceof Error ? cause.message : cause)).finally(() => busy = false); }}>{$t('refresh')}</button></header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}
  {#if loading}<p class="empty" role="status">{$t('loading')}</p>{:else if !$features.reports}<p class="panel">{$t('reports_unavailable')}</p>{:else}
    <div class="heading"><h2>{isAdmin ? $t('all_reports') : $t('my_reports')}</h2><label>{$t('status')}<select bind:value={filter}><option value="all">{$t('all_statuses')}</option>{#each ['open','in_progress','resolved','closed'] as status (status)}<option value={status}>{statusLabels[status]}</option>{/each}</select></label></div>
    <ul class="records">{#each visible as report (report.id)}<li class="record"><div class="record-header"><div><h3>{report.title ?? categoryLabels[report.category] ?? $t('report')}</h3><small>{categoryLabels[report.category] ?? report.category} · {dateLabel(report.createdAt)}</small></div><span class="badge">{statusLabels[report.status] ?? report.status}</span></div><p class="text">{report.description}</p><button disabled={busy} onclick={() => show(report.id)}>{selected?.id === report.id ? $t('hide_tracking') : $t('open_tracking')}</button>
      {#if selected?.id === report.id}<div class="detail stack">
        {#if selected.positionSeconds != null}<p>Position : {selected.positionSeconds} s{selected.endSeconds != null ? ` → ${selected.endSeconds} s` : ''}</p>{/if}
        {#if selected.assetId}<p class="muted">{$t('affected_copy', { id: selected.assetId })}</p>{/if}
        {#if selected.context}<details><summary>{$t('technical_context')}</summary>{#each Object.entries(selected.context).filter(([, value]) => value) as [key, value] (key)}<p class="text">{key} : {value}</p>{/each}</details>{/if}
        <div class="row">{#if isAdmin}{#each ['open','in_progress','resolved','closed'].filter(status => status !== selected?.status) as status (status)}<button disabled={busy} onclick={() => update(status)}>{status === 'open' ? $t('reopen') : status === 'in_progress' ? $t('take_over') : status === 'resolved' ? $t('mark_resolved') : $t('close_without_fix')}</button>{/each}{:else if ['resolved','closed'].includes(selected.status)}<button disabled={busy} onclick={() => update('open')}>{$t('reopen')}</button>{/if}</div>
        <section aria-label={$t('comments')}><h3>{$t('exchanges')}</h3>{#each selected.comments ?? [] as entry, itemIndex (itemIndex)}<article><small>{entry.authorLabel ?? entry.author ?? entry.userId ?? $t('historical_author')} · {dateLabel(entry.createdAt)}</small><p class="text">{entry.message}</p></article>{:else}<p class="muted">{$t('no_comments')}</p>{/each}<form class="stack" onsubmit={comment}><label>{$t('add_comment')}<textarea maxlength="8000" required bind:value={message}></textarea></label><div><button disabled={busy || !message.trim()}>{$t('send')}</button></div></form></section>
        {#if selected.history?.length}<details><summary>{$t('history')}</summary>{#each selected.history as entry, itemIndex (itemIndex)}<p>{dateLabel(entry.createdAt)} · {statusLabels[entry.status ?? ''] ?? entry.action ?? entry.status}</p>{/each}</details>{/if}
      </div>{/if}
    </li>{:else}<li class="empty">{$t('no_reports_selection')}</li>{/each}</ul>
    {#if nextOffset !== null}<button disabled={busy} onclick={() => { busy = true; void refresh(true).catch(cause => error = cause instanceof Error ? cause.message : $t('loading_impossible')).finally(() => busy = false); }}>{$t('load_previous_reports')}</button>{/if}
  {/if}
</div>

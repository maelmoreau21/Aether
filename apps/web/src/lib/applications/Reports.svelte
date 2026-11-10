<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import {
    applicationRequest, categoryLabels, checkLabels, dateLabel, features, formatPosition, historyLabels, loadFeatures,
    priorityLabels, statusLabels, statusTone, workHref, type MediaReport, type Priority
  } from './api';
  import './applications.css';
  import { t } from '../i18n';

  let { embedded = false }: { embedded?: boolean } = $props();
  const statuses = ['open', 'in_progress', 'resolved', 'closed'] as const;
  const categories = ['video', 'audio', 'subtitles', 'metadata', 'missing', 'other'] as const;
  const priorities: Priority[] = ['high', 'normal', 'low'];

  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let reports = $state<MediaReport[]>([]);
  let counts = $state<Record<string, number>>({});
  let nextOffset = $state<number | null>(null);
  let selected = $state<MediaReport | null>(null);
  let message = $state('');
  let status = $state<string>('active');
  let category = $state('');
  let priority = $state('');
  let sort = $state<'updated' | 'priority'>('priority');
  let mineOnly = $state(false);
  let brokenArt = $state<string[]>([]);

  const isAdmin = $derived($session?.user.role === 'administrator');
  const statusFilters = $derived([
    { value: 'active', label: $t('rp_filter_active'), count: (counts.open ?? 0) + (counts.in_progress ?? 0) },
    ...statuses.map(value => ({ value, label: statusLabels[value] ?? value, count: counts[value] ?? 0 })),
    { value: 'all', label: $t('rq_group_all'), count: statuses.reduce((total, value) => total + (counts[value] ?? 0), 0) }
  ]);
  const transitions: Record<string, string> = $derived({
    open: $t('reopen'), in_progress: $t('take_over'), resolved: $t('mark_resolved'), closed: $t('close_without_fix')
  });
  const contextLabels: Record<string, string> = $derived({
    engine: $t('rp_context_engine'), errorCode: $t('rp_context_error'), audioTrackId: $t('rp_context_audio'), subtitleTrackId: $t('rp_context_subtitle')
  });

  function listPath(offset: number | null) {
    const params = new URLSearchParams();
    if (status !== 'all') params.set('status', status);
    if (category) params.set('category', category);
    if (isAdmin && priority) params.set('priority', priority);
    if (isAdmin) params.set('sort', sort);
    if (isAdmin && mineOnly) params.set('mine', 'true');
    if (offset) params.set('offset', String(offset));
    const suffix = params.toString();
    return `/api/v1/reports${suffix ? `?${suffix}` : ''}`;
  }
  async function refresh(append = false) {
    const result = await applicationRequest<{ reports: MediaReport[]; nextOffset?: number | null; counts?: Record<string, number> }>(listPath(append ? nextOffset : null));
    reports = append ? [...reports, ...result.reports.filter(item => !reports.some(existing => existing.id === item.id))] : result.reports;
    nextOffset = result.nextOffset ?? null;
    counts = result.counts ?? counts;
  }
  async function run(task: () => Promise<void>, fallback: string) {
    busy = true; error = '';
    try { await task(); }
    catch (cause) { error = cause instanceof Error ? cause.message : fallback; }
    finally { busy = false; }
  }
  onMount(() => {
    void (async () => {
      try { const available = await loadFeatures(true); if (available.reports) await refresh(); }
      catch (cause) { error = cause instanceof Error ? cause.message : $t('reports_unavailable_short'); }
      finally { loading = false; }
    })();
  });
  const reload = () => run(() => refresh(), $t('loading_impossible'));
  async function setStatus(value: string) { status = value; selected = null; await reload(); }
  async function load(id: string) { selected = await applicationRequest<MediaReport>(`/api/v1/reports/${encodeURIComponent(id)}`); }
  async function show(id: string) {
    notice = ''; message = '';
    if (selected?.id === id) { selected = null; return; }
    await run(() => load(id), $t('detail_unavailable'));
  }
  /** A transition carries the typed message as its explanation, in the same request. */
  async function update(next: string) {
    if (!selected) return;
    const id = selected.id;
    const note = message.trim();
    await run(async () => {
      await applicationRequest(`/api/v1/reports/${encodeURIComponent(id)}/status`, { method: 'POST', body: JSON.stringify({ status: next, ...(note ? { message: note } : {}) }) });
      message = '';
      notice = $t('rp_status_changed', { status: statusLabels[next] ?? next });
      await load(id); await refresh();
    }, $t('modification_impossible'));
  }
  async function setPriority(value: string) {
    if (!selected) return;
    const id = selected.id;
    await run(async () => {
      await applicationRequest(`/api/v1/reports/${encodeURIComponent(id)}/priority`, { method: 'POST', body: JSON.stringify({ priority: value }) });
      await load(id); await refresh();
    }, $t('modification_impossible'));
  }
  async function comment(event: SubmitEvent) {
    event.preventDefault();
    if (!selected || !message.trim()) return;
    const id = selected.id;
    await run(async () => {
      await applicationRequest(`/api/v1/reports/${encodeURIComponent(id)}/comments`, { method: 'POST', body: JSON.stringify({ message: message.trim() }) });
      message = ''; await load(id);
    }, $t('comment_impossible'));
  }
  function title(report: MediaReport) { return report.subject?.title ?? categoryLabels[report.category] ?? $t('report'); }
  function subjectLine(report: MediaReport) {
    const subject = report.subject;
    if (!subject) return '';
    if (subject.kind === 'track') return [subject.artist, subject.album].filter(Boolean).join(' · ');
    return subject.year ? String(subject.year) : '';
  }
  function moment(report: MediaReport) {
    if (report.positionSeconds == null) return '';
    const start = formatPosition(report.positionSeconds);
    return report.endSeconds != null ? $t('rp_moment_range', { start, end: formatPosition(report.endSeconds) }) : $t('rp_moment_at', { start });
  }
  function image(report: MediaReport) {
    const key = report.subject?.image;
    return key && !brokenArt.includes(key) ? `${key}?width=160` : '';
  }
  const allowed = (report: MediaReport) => isAdmin
    ? statuses.filter(value => value !== report.status)
    : ['resolved', 'closed'].includes(report.status) ? ['open'] : [];
</script>

<div class="application-page reports-page">
  <header class="heading">
    <div>
      <p class="muted eyebrow">{$t('reports_subtitle')}</p>
      {#if !embedded}<h1>{$t('reports_title')}</h1>{/if}
      <p class="muted">{$t('reports_hint')}</p>
    </div>
    <button disabled={busy || loading || !$features.reports} onclick={reload}>{$t('refresh')}</button>
  </header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}

  {#if loading}<p class="empty" role="status">{$t('loading')}</p>
  {:else if !$features.reports}<p class="panel">{$t('reports_unavailable')}</p>
  {:else}
    {#if isAdmin}
      <div class="stat-row" aria-label={$t('rp_queue_summary')}>
        <div class="stat tone-err"><strong>{counts.urgent ?? 0}</strong><span>{$t('rp_urgent')}</span></div>
        <div class="stat tone-warn"><strong>{counts.open ?? 0}</strong><span>{statusLabels.open}</span></div>
        <div class="stat tone-info"><strong>{counts.in_progress ?? 0}</strong><span>{statusLabels.in_progress}</span></div>
        <div class="stat tone-ok"><strong>{counts.resolved ?? 0}</strong><span>{statusLabels.resolved}</span></div>
      </div>
    {/if}
    <div class="heading compact">
      <h2>{isAdmin && !mineOnly ? $t('all_reports') : $t('my_reports')}</h2>
      <div class="row">
        <label class="inline">{$t('rp_category')}
          <select bind:value={category} disabled={busy} onchange={reload}>
            <option value="">{$t('rp_all_categories')}</option>
            {#each categories as value (value)}<option {value}>{categoryLabels[value]}</option>{/each}
          </select>
        </label>
        {#if isAdmin}
          <label class="inline">{$t('rp_priority')}
            <select bind:value={priority} disabled={busy} onchange={reload}>
              <option value="">{$t('rp_all_priorities')}</option>
              {#each priorities as value (value)}<option {value}>{priorityLabels[value]}</option>{/each}
            </select>
          </label>
          <label class="inline">{$t('rp_sort')}
            <select bind:value={sort} disabled={busy} onchange={reload}>
              <option value="priority">{$t('rp_sort_priority')}</option>
              <option value="updated">{$t('rp_sort_updated')}</option>
            </select>
          </label>
          <label class="check"><input type="checkbox" bind:checked={mineOnly} disabled={busy} onchange={reload} />{$t('rq_only_mine')}</label>
        {/if}
      </div>
    </div>
    <div class="chips" role="group" aria-label={$t('status')}>
      {#each statusFilters as filter (filter.value)}
        <button class="chip" aria-pressed={status === filter.value} disabled={busy} onclick={() => setStatus(filter.value)}>{filter.label}<span class="count">{filter.count}</span></button>
      {/each}
    </div>

    <ul class="records">
      {#each reports as report (report.id)}
        <li class="record report-record" class:urgent={report.priority === 'high' && ['open', 'in_progress'].includes(report.status)}>
          <div class="record-main">
            <div class="art small" aria-hidden="true">
              {#if image(report)}<img src={image(report)} alt="" loading="lazy" onerror={() => brokenArt = [...brokenArt, report.subject?.image ?? '']} />{:else}<span>{title(report).charAt(0).toUpperCase()}</span>{/if}
            </div>
            <div class="record-text">
              <h3>{title(report)}{#if subjectLine(report)}<span class="year">{subjectLine(report)}</span>{/if}</h3>
              <small>
                {categoryLabels[report.category] ?? report.category}{#if moment(report)} · {moment(report)}{/if} · {dateLabel(report.updatedAt ?? report.createdAt)}{#if isAdmin && report.authorLabel} · {$t('rp_by', { name: report.authorLabel })}{/if}
              </small>
              <p class="text clamp">{report.description}</p>
            </div>
            <div class="badges end">
              {#if isAdmin && report.priority && report.priority !== 'normal'}<span class="badge priority-{report.priority}">{priorityLabels[report.priority]}</span>{/if}
              <span class="badge tone-{statusTone(report.status)}">{statusLabels[report.status] ?? report.status}</span>
            </div>
          </div>
          <div class="row actions">
            <button disabled={busy} aria-expanded={selected?.id === report.id} onclick={() => show(report.id)}>{selected?.id === report.id ? $t('hide_tracking') : $t('open_tracking')}</button>
            {#if report.subject?.workId}
              {@const href = workHref(report.subject.kind, report.subject.workId)}
              {#if href}<a class="button-link" href={resolve(href)}>{$t('rp_open_media')}</a>{/if}
            {/if}
          </div>

          {#if selected?.id === report.id}
            <div class="detail report-detail">
              <div class="stack">
                {#if notice}<p role="status" class="feedback success">{notice}</p>{/if}
                <dl class="facts">
                  {#if moment(selected)}<div><dt>{$t('rp_moment')}</dt><dd>{moment(selected)}</dd></div>{/if}
                  {#if selected.assetId}<div><dt>{$t('rp_copy')}</dt><dd><code>{selected.assetId}</code></dd></div>{/if}
                  {#each Object.entries(selected.context ?? {}).filter(([key, value]) => value && key in contextLabels) as [key, value] (key)}
                    <div><dt>{contextLabels[key]}</dt><dd><code>{value}</code></dd></div>
                  {/each}
                </dl>
                {#if isAdmin}
                  <div class="triage panel-soft">
                    <label class="inline">{$t('rp_priority')}
                      <select value={selected.priority ?? 'normal'} disabled={busy} onchange={(event) => setPriority(event.currentTarget.value)}>
                        {#each priorities as value (value)}<option {value}>{priorityLabels[value]}</option>{/each}
                      </select>
                    </label>
                    {#if selected.triage?.checks?.length}
                      <div>
                        <p class="muted small">{$t('rp_suggested_checks')}</p>
                        <ul class="checklist">{#each selected.triage.checks as check (check)}<li>{checkLabels[check] ?? check}</li>{/each}</ul>
                      </div>
                    {/if}
                  </div>
                {/if}

                <section aria-label={$t('comments')} class="thread">
                  <h3>{$t('exchanges')}</h3>
                  <article class="bubble own-start">
                    <small>{report.authorLabel ?? $t('historical_author')} · {dateLabel(selected.createdAt)}</small>
                    <p class="text">{selected.description}</p>
                  </article>
                  {#each selected.comments ?? [] as entry, itemIndex (entry.id ?? itemIndex)}
                    <article class="bubble" class:staff={entry.staff} class:own={entry.own}>
                      <small>{entry.authorLabel ?? $t('historical_author')}{#if entry.staff} · {$t('rp_staff')}{/if} · {dateLabel(entry.createdAt)}</small>
                      <p class="text">{entry.message}</p>
                    </article>
                  {/each}
                  <form class="stack" onsubmit={comment}>
                    <label>{$t('add_comment')}<textarea maxlength="8000" bind:value={message} placeholder={allowed(selected).length ? $t('rp_comment_or_note') : ''}></textarea></label>
                    <div class="row">
                      <button class="primary" disabled={busy || !message.trim()}>{$t('send')}</button>
                      {#each allowed(selected) as next (next)}
                        <button type="button" class:danger={next === 'closed'} disabled={busy} onclick={() => update(next)}>{transitions[next]}{#if message.trim()} {$t('rp_with_note')}{/if}</button>
                      {/each}
                    </div>
                  </form>
                </section>

                {#if selected.history?.length}
                  <details>
                    <summary>{$t('history')}</summary>
                    <ol class="timeline">
                      {#each selected.history as entry, itemIndex (itemIndex)}
                        <li class="tone-{statusTone(entry.status ?? '')}">
                          <span class="when">{dateLabel(entry.createdAt)}</span>
                          <span>{historyLabels[entry.kind ?? ''] ?? entry.kind}{#if entry.kind === 'report.status'} · {statusLabels[entry.status ?? ''] ?? entry.status}{/if}{#if entry.actorLabel} · {entry.actorLabel}{/if}</span>
                        </li>
                      {/each}
                    </ol>
                  </details>
                {/if}
              </div>
            </div>
          {/if}
        </li>
      {:else}<li class="empty">{$t('no_reports_selection')}</li>{/each}
    </ul>
    {#if nextOffset !== null}<button disabled={busy} onclick={() => run(() => refresh(true), $t('loading_impossible'))}>{$t('load_previous_reports')}</button>{/if}
  {/if}
</div>

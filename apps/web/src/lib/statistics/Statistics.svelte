<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import { session, getSessionGeneration, onSessionGenerationChange } from '../auth/session';
  import { statisticsRequest, duration, eventLabels, type Statistics, type Detail, type Distribution, type Insights, type Profile, type WrappedSettings } from './api';
  import { st, statLocale, type StatKey } from './i18n';
  import { addDays, browserTimeZone, localDate, localDayStart, monthName, shortDuration, weekdayName } from './insights';
  import Kpi from './charts/Kpi.svelte';
  import Bars from './charts/Bars.svelte';
  import Meter from './charts/Meter.svelte';
  import HourHeatmap from './charts/HourHeatmap.svelte';
  import CalendarHeatmap from './charts/CalendarHeatmap.svelte';
  import AreaChart from './charts/AreaChart.svelte';
  import Donut from './charts/Donut.svelte';
  import TopList from './charts/TopList.svelte';
  import { bucket, chartSeries, dailySeries } from './series';

  let { administrator = false, active = true, embedded = false }: { administrator?: boolean; active?: boolean; embedded?: boolean } = $props();
  const endpoint = $derived(administrator ? '/api/v1/admin/statistics' : '/api/v1/account/statistics');
  const zone = browserTimeZone();
  let mounted = $state(false), loading = $state(false), saving = $state(false), error = $state(''), message = $state('');
  let data = $state<Statistics | null>(null), detail = $state<Detail | null>(null), detailLoading = $state(false);
  let insights = $state<Insights | null>(null), insightsError = $state('');
  let profile = $state<Profile | null>(null), profileLoading = $state(false);
  let wrapped = $state<WrappedSettings | null>(null), wrappedDirty = $state(false);
  let dialog: HTMLDialogElement, profileDialog: HTMLDialogElement;
  const today = localDate(new Date());
  let from = $state(addDays(today, -29)), to = $state(today);
  let userId = $state(''), libraryId = $state(''), nodeId = $state(''), kind = $state(''), device = $state(''), rank = $state('users'), offset = $state(0);
  let settingsReady = $state(false), persistedEnabled = $state(false), usersCanView = $state(false);
  let tab = $state<'overview'|'activity'|'content'|'technical'|'history'|'settings'>('overview');
  const tabs = ['overview','activity','content','technical','history','settings'] as const;
  let enabled = $state(false), eventDays = $state(0), summaryDays = $state(0), dirty = $state(false), eraseConfirmed = $state(false);
  const RETENTION_PRESETS = [0, 365, 730, 1825] as const;
  const retentionPreset = $derived(
    eventDays === summaryDays && (RETENTION_PRESETS as readonly number[]).includes(eventDays)
      ? String(eventDays)
      : 'custom'
  );
  function applyRetentionPreset(value: string) {
    if (value === 'custom') return;
    const days = Number(value);
    eventDays = days;
    summaryDays = days;
    dirty = true;
  }
  let facets = $state<Statistics['facets']>({ libraries: [], nodes: [], devices: [] });
  let users = $state<Statistics['users']>([]);
  let pending: AbortController | undefined, detailPending: AbortController | undefined, profilePending: AbortController | undefined;
  let requestNumber = 0;
  let nextExportOffset = $state<number | null>(null);
  let filtersOpen = $state(false);
  const metricKeys = ['pauses', 'resumes', 'additionalSessions', 'musicSkips', 'seeksForward', 'seeksBackward', 'audioChanges', 'subtitleChanges', 'speedChanges', 'introSkips', 'creditsSkips', 'otherSkips', 'bufferingCount', 'errors'] as const;
  const knownMethods = ['direct', 'remux', 'separate_audio', 'prepared', 'unknown'];
  const thisYear = new Date().getFullYear();
  const wrappedYear = new Date().getMonth() === 0 ? thisYear - 1 : thisYear;
  const wrappedHref = resolve('statistics/wrapped/');

  const number = $derived(new Intl.NumberFormat($statLocale));
  const percent = $derived(new Intl.NumberFormat($statLocale, { style: 'percent', maximumFractionDigits: 0 }));
  function date(value: number) { return new Date(value * 1000).toLocaleString($statLocale); }
  function day(value: string) { return new Date(`${value}T12:00:00`).toLocaleDateString($statLocale, { weekday: 'short', day: 'numeric', month: 'long', year: 'numeric' }); }
  function statusLabel(status: string) { const key = `status_${status}` as StatKey; return ['active', 'completed', 'partial', 'interrupted'].includes(status) ? $st(key) : status; }
  function kindLabel(value: string) { return $st(value === 'music' ? 'kind_music' : value === 'episode' ? 'kind_episode' : value === 'series' ? 'kind_series' : 'kind_film'); }
  function methodLabel(value: string) { return knownMethods.includes(value) ? $st(`method_${value}` as StatKey) : value; }
  function hourLabel(hour: number) { return $st('hour_label', { hour }); }
  function plays(value: number) { return `${number.format(value)} ${$st('kpi_plays').toLowerCase()}`; }

  function params() {
    const values = new URLSearchParams({ from: String(localDayStart(from)), to: String(localDayStart(addDays(to, 1))), rank, offset: String(offset), tz: zone });
    for (const [key, value] of Object.entries({ userId, libraryId, nodeId, kind, device })) if (value) values.set(key, value);
    return values.toString();
  }
  async function load(full = true) {
    const generation = getSessionGeneration(), current = ++requestNumber;
    pending?.abort(); pending = new AbortController(); loading = true; error = '';
    const signal = pending.signal, query = params();
    try {
      if (administrator) {
        const config = await (await statisticsRequest(`${endpoint}/settings`, {signal})).json();
        if (current !== requestNumber || generation !== getSessionGeneration()) return;
        settingsReady = true;persistedEnabled=config.enabled;
        if (!dirty) { enabled=config.enabled;usersCanView=config.usersCanView===true;eventDays=config.eventRetentionDays;summaryDays=config.summaryRetentionDays; }
        if (!config.enabled) { data=null;insights=null;return; }
      }
      const [result, extra, settings] = await Promise.all([
        statisticsRequest(`${endpoint}?${query}`, { signal }).then(response => response.json() as Promise<Statistics>),
        full ? statisticsRequest(`${endpoint}/insights?${query}`, { signal }).then(response => response.json() as Promise<Insights>).catch(cause => { if (!signal.aborted) insightsError = cause instanceof Error ? cause.message : $st('error_generic'); return null; }) : Promise.resolve(undefined),
        full && administrator ? statisticsRequest(`${endpoint}/wrapped-settings`, { signal }).then(response => response.json() as Promise<WrappedSettings>).catch(() => null) : Promise.resolve(undefined)
      ]);
      if (current !== requestNumber || generation !== getSessionGeneration()) return;
      data = result;
      if (extra !== undefined) { insights = extra; if (extra) insightsError = ''; }
      if (settings && !wrappedDirty) wrapped = settings;
      if (!dirty) { enabled = result.enabled; eventDays = result.eventRetentionDays; summaryDays = result.summaryRetentionDays; }
      if (!userId) users = result.users;
      if (!libraryId && !nodeId && !device) facets = result.facets;
    } catch (cause) {
      if (current === requestNumber && generation === getSessionGeneration() && !(cause instanceof DOMException && cause.name === 'AbortError')) { data = null; error = cause instanceof Error ? cause.message : $st('error_generic'); }
    } finally { if (current === requestNumber) loading = false; }
  }
  function apply() { offset = 0; nextExportOffset = null; void load(); }
  function preset(days: number | 'year') {
    to = today; from = days === 'year' ? `${today.slice(0, 4)}-01-01` : addDays(today, -(days - 1)); apply();
  }
  async function save() {
    saving = true; message = ''; error = '';
    try {
      await statisticsRequest(`${endpoint}/settings`, { method: 'PUT', body: JSON.stringify({ enabled, usersCanView, eventRetentionDays: eventDays, summaryRetentionDays: summaryDays }) });
      persistedEnabled=enabled;
      if (wrapped && wrappedDirty) await statisticsRequest(`${endpoint}/wrapped-settings`, { method: 'PUT', body: JSON.stringify(wrapped) });
      dirty = false; wrappedDirty = false; message = $st('saved'); await load();
      window.dispatchEvent(new Event('statistics-settings-changed'));
    } catch (cause) { enabled=persistedEnabled;error = String(cause instanceof Error ? cause.message : cause); } finally { saving = false; }
  }
  async function cleanup() {
    saving = true; message = ''; error = '';
    try { const result = await (await statisticsRequest(`${endpoint}/purge`, { method: 'POST' })).json(); message = $st('purged', { count: result.eventsRemoved, limit: result.batchLimit }); await load(); }
    catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { saving = false; }
  }
  async function erase() {
    if (!eraseConfirmed) return;
    saving = true; error = '';
    try { await statisticsRequest(endpoint, { method: 'DELETE' }); eraseConfirmed = false; message = $st('erased'); await load(); }
    catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { saving = false; }
  }
  let timelineFilter = $state<'actions' | 'all'>('actions');
  const KEY_EVENT_KINDS = new Set(['subtitle_change', 'audio_change', 'speed_change', 'seek', 'skip_intro', 'skip_credits', 'skip_recap', 'skip_preview', 'pause', 'resume', 'start', 'stop', 'ended', 'error']);
  const visibleTimeline = $derived(detail ? (timelineFilter === 'all' ? detail.events : detail.events.filter(item => KEY_EVENT_KINDS.has(item.event.kind))) : []);
  async function openDetail(id: string) {
    const generation = getSessionGeneration();
    detailPending?.abort(); const controller = new AbortController(); detailPending = controller;
    detail = null; detailLoading = true; dialog.showModal();
    try { const result = await (await statisticsRequest(`${endpoint}/sessions/${encodeURIComponent(id)}`, { signal: controller.signal })).json(); if (generation === getSessionGeneration() && detailPending === controller) detail = result; }
    catch (cause) { if (!controller.signal.aborted && generation === getSessionGeneration()) { error = String(cause instanceof Error ? cause.message : cause); dialog.close(); } }
    finally { if (detailPending === controller) detailLoading = false; }
  }
  async function moreEvents() {
    if (!detail?.nextSequence) return;
    const previous = detail, generation = getSessionGeneration(); detailLoading = true;
    try {
      const next: Detail = await (await statisticsRequest(`${endpoint}/sessions/${encodeURIComponent(previous.session.id)}?after=${previous.nextSequence}`)).json();
      if (generation === getSessionGeneration() && detail?.session.id === previous.session.id) detail = { ...next, events: [...previous.events, ...next.events] };
    } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { detailLoading = false; }
  }
  async function openProfile(id: string) {
    const generation = getSessionGeneration();
    profilePending?.abort(); const controller = new AbortController(); profilePending = controller;
    profile = null; profileLoading = true; profileDialog.showModal();
    try { const result: Profile = await (await statisticsRequest(`${endpoint}/users/${encodeURIComponent(id)}?${params()}`, { signal: controller.signal })).json(); if (generation === getSessionGeneration() && profilePending === controller) profile = result; }
    catch (cause) { if (!controller.signal.aborted && generation === getSessionGeneration()) { error = String(cause instanceof Error ? cause.message : cause); profileDialog.close(); } }
    finally { if (profilePending === controller) profileLoading = false; }
  }
  async function download(exportOffset = 0) {
    error = ''; message = '';
    const generation = getSessionGeneration();
    try {
      const query = new URLSearchParams(params()); query.set('offset', String(exportOffset));
      const response = await statisticsRequest(`${endpoint}/export?${query}`);
      const blob = await response.blob(); if (generation !== getSessionGeneration()) return;
      const url = URL.createObjectURL(blob), anchor = document.createElement('a');
      anchor.href = url; anchor.download = `statistiques-${Math.floor(exportOffset / 2000) + 1}.csv`; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      nextExportOffset = response.headers.get('x-export-truncated') === 'true' ? exportOffset + 2000 : null;
      if (nextExportOffset !== null) message = $st('export_batch');
    } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); }
  }
  onMount(() => {
    mounted = true;
    let ticks = 0;
    // Live counters every 15 s; the heavier analytics refresh every minute.
    const timer = setInterval(() => { ticks++; if (active && !loading && !dirty && !wrappedDirty && document.visibilityState === 'visible') void load(ticks % 4 === 0); }, 15000);
    const off = onSessionGenerationChange(() => { requestNumber++; pending?.abort(); detailPending?.abort(); profilePending?.abort(); data = null; insights = null; settingsReady=false;enabled=false;usersCanView=false;dirty=false;wrappedDirty=false;wrapped=null;detail = null; profile = null; users = []; facets = { libraries: [], nodes: [], devices: [] }; dialog?.close(); profileDialog?.close(); });
    return () => { clearInterval(timer); off(); pending?.abort(); detailPending?.abort(); profilePending?.abort(); };
  });
  $effect(() => { const account = $session?.user.id; if (mounted && active && account) untrack(() => { void load(); }); });

  // Chart data, all in local time.
  const monthlyBars = $derived((insights?.monthly ?? []).map(item => ({ key: item.month, label: monthName(Number(item.month.slice(5)), $statLocale, 'short'), value: item.seconds, hint: `${monthName(Number(item.month.slice(5)), $statLocale)} ${item.month.slice(0, 4)} · ${plays(item.plays)}` })));
  const weekdayBars = $derived((insights?.weekdays ?? []).map(item => ({ key: String(item.weekday), label: weekdayName(item.weekday, $statLocale, 'short'), value: item.plays, hint: weekdayName(item.weekday, $statLocale) })));
  function lastDays<T extends { date: string }>(rows: T[], count: number, value: (row: T | undefined) => number) {
    const map = new Map(rows.map(row => [row.date, row]));
    const start = addDays(to, -(count - 1)) < from ? from : addDays(to, -(count - 1));
    const out: { key: string; label: string; value: number; hint: string }[] = [];
    for (let cursor = start; cursor <= to; cursor = addDays(cursor, 1)) out.push({ key: cursor, label: String(Number(cursor.slice(8))), value: value(map.get(cursor)), hint: day(cursor) });
    return out;
  }
  const dailyBars = $derived(insights ? lastDays(insights.calendar, 90, row => row?.seconds ?? 0) : []);
  const concurrencyBars = $derived(insights ? lastDays(insights.concurrency.daily, 60, row => row?.peak ?? 0) : []);
  const bucketOrder = ['completed', 'partial', 'dropped', 'abandoned', 'unknown'] as const;
  const bucketTotal = $derived(insights ? bucketOrder.reduce((sum, key) => sum + insights!.completion.buckets[key], 0) : 0);
  const growth = $derived(insights?.growth);
  const playsCount = $derived(insights?.totals.plays ?? data?.summary.qualifiedPlays ?? 0);

  // Dashboard (overview) data.
  function shortDay(value: string) { return new Date(`${value}T12:00:00`).toLocaleDateString($statLocale, { day: 'numeric', month: 'short' }); }
  const activity = $derived.by(() => {
    const series = chartSeries(insights?.calendar ?? [], from, to);
    return { grouped: series.grouped, items: series.points.map(point => ({ key: point.key, label: shortDay(point.from), value: point.seconds, hint: `${point.from === point.to ? day(point.from) : `${shortDay(point.from)} – ${shortDay(point.to)}`} · ${plays(point.plays)}` })) };
  });
  const sparkline = $derived(bucket(dailySeries(insights?.calendar ?? [], from, to), 30));
  const rangeDays = $derived(Math.max(1, Math.round((localDayStart(addDays(to, 1)) - localDayStart(from)) / 86400)));
  function kindPlural(value: string) { return value === 'music' ? $st('music') : value === 'episode' ? $st('episodes') : $st('films'); }
  const kindSlices = $derived((data?.distributions.kind ?? []).map(row => ({ key: row.label, label: kindPlural(row.label), value: row.seconds, detail: `${shortDuration(row.seconds)} · ${plays(row.plays)}`, tone: row.label === 'music' ? 'var(--ok)' : row.label === 'episode' ? 'var(--info)' : 'var(--ac)' })));
  const bucketTones: Record<(typeof bucketOrder)[number], string> = { completed: 'var(--ok)', partial: 'var(--info)', dropped: 'var(--warn)', abandoned: 'var(--err)', unknown: 'var(--mu-2)' };
  const completionSlices = $derived(insights ? bucketOrder.map(key => ({ key, label: $st(`bucket_${key}` as StatKey), value: insights!.completion.buckets[key], detail: number.format(insights!.completion.buckets[key]), tone: bucketTones[key] })) : []);
  /** Title page, opened on its statistics tab (films and series only). */
  function mediaHref(kindValue: string, id: string): string | undefined {
    if (kindValue === 'film') return `${resolve('films/')}?work=${encodeURIComponent(id)}&section=stats`;
    if (kindValue === 'series') return `${resolve('series/')}?work=${encodeURIComponent(id)}&section=stats`;
    return undefined;
  }
  const topTitles = $derived((insights?.titles ?? []).map(item => ({ key: item.id, title: item.title, sub: [kindLabel(item.kind), item.subtitle].filter(Boolean).join(' · '), image: item.image, href: mediaHref(item.kind, item.id), value: item.seconds, detail: shortDuration(item.seconds) })));
  const topSeries = $derived((insights?.series ?? []).map(item => ({ key: item.id, title: item.title, sub: $st('episodes_count', { count: item.items }), image: item.image, href: mediaHref('series', item.id), value: item.seconds, detail: shortDuration(item.seconds) })));
  const live = $derived((data?.sessions ?? []).filter(item => item.status === 'active'));
  const activeFilters = $derived([
    userId ? { key: 'user', label: $st('user'), value: users.find(user => user.id === userId)?.name ?? userId, clear: () => userId = '' } : null,
    kind ? { key: 'kind', label: $st('type'), value: kindPlural(kind), clear: () => kind = '' } : null,
    libraryId ? { key: 'library', label: $st('library'), value: facets.libraries.find(library => library.id === libraryId)?.name ?? libraryId, clear: () => libraryId = '' } : null,
    nodeId ? { key: 'node', label: $st('node'), value: nodeId, clear: () => nodeId = '' } : null,
    device ? { key: 'device', label: $st('device'), value: device, clear: () => device = '' } : null
  ].filter(item => item !== null));
  function clearFilters() { userId = ''; kind = ''; libraryId = ''; nodeId = ''; device = ''; rank = 'users'; apply(); }
</script>

<section class="statistics" aria-label={administrator ? $st('title_server') : $st('title_personal')}>
  {#if administrator && !enabled}
    {#if !embedded}<h2>{$st('title_server')}</h2>{/if}
    {#if !settingsReady}<p role="status">{$st('loading')}</p>{:else}<label class="check activation"><input type="checkbox" bind:checked={enabled} disabled={saving} onchange={() => { dirty=true;void save(); }} /> {$st('enable_collection')}</label>{/if}
    {#if error}<p class="notice error" role="alert">{error}</p>{/if}
  {:else}
  <header class="heading">
    <div>
      {#if !embedded}<p class="eyebrow">{administrator ? $st('eyebrow_server') : $st('eyebrow_personal')}</p>{#if administrator}<h2>{$st('title_server')}</h2>{:else}<h1>{$st('title_personal')}</h1>{/if}{/if}
      <p>{$st('subtitle')}</p>
    </div>
    <div class="heading-actions">
      {#if !administrator && data?.wrappedAvailable}
        <a class="wrapped-cta" href={wrappedHref} title={$st('wrapped_cta_hint')}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4h14v16H5z" /><path d="M9 4v16M5 9h4M5 15h4" /><path d="M13 9l4 3-4 3z" /></svg>
          <span>{$st('wrapped_cta', { year: wrappedYear })}</span>
        </a>
      {/if}
      {#if administrator}<label class="switch"><input type="checkbox" role="switch" bind:checked={enabled} disabled={saving} onchange={() => { dirty=true;void save(); }} /><span>{$st('enable_collection')}</span></label>{/if}
    </div>
  </header>
  {#if error}<p class="notice error" role="alert">{error}</p>{/if}
  {#if message}<p class="notice" role="status">{message}</p>{/if}
  <nav class="statistics-tabs" aria-label={$st('sections')}>
    {#each tabs as item (item)}<button type="button" class:active={tab===item} aria-current={tab===item?'page':undefined} onclick={() => tab=item}>{$st(`tab_${item}` as StatKey)}</button>{/each}
  </nav>
  {#if administrator && data && tab==='settings'}
    <section class="panel settings"><h3>{$st('settings_collection')}</h3><form onsubmit={event => { event.preventDefault(); void save(); }}>
      <label class="check"><input type="checkbox" bind:checked={usersCanView} disabled={saving} onchange={() => dirty = true} /> {$st('users_can_view')}</label>
      <p class="muted">{$st('collection_note')}</p>
      <div class="filters">
        <label>{$st('retention_preset')}
          <select value={retentionPreset} disabled={saving} onchange={event => applyRetentionPreset((event.currentTarget as HTMLSelectElement).value)}>
            <option value="0">{$st('retention_unlimited')}</option>
            <option value="365">{$st('retention_1_year')}</option>
            <option value="730">{$st('retention_2_years')}</option>
            <option value="1825">{$st('retention_5_years')}</option>
            <option value="custom">{$st('retention_custom')}</option>
          </select>
        </label>
        <label>{$st('event_days')}<input type="number" min="0" max="36500" bind:value={eventDays} oninput={() => dirty = true} required /></label>
        <label>{$st('summary_days')}<input type="number" min="0" max="36500" bind:value={summaryDays} oninput={() => dirty = true} required /></label>
      </div>
      {#if wrapped}
        <fieldset class="wrapped-settings" disabled={!usersCanView}><legend>{$st('wrapped_settings')}</legend>
          <label class="check"><input type="checkbox" bind:checked={wrapped.visible} onchange={() => wrappedDirty = true} /> {$st('wrapped_visible')}</label>
          <label class="check"><input type="checkbox" bind:checked={wrapped.periodEnabled} disabled={!wrapped.visible} onchange={() => wrappedDirty = true} /> {$st('wrapped_period')}</label>
          {#if wrapped.periodEnabled && wrapped.visible}
            <div class="filters">
              <label>{$st('wrapped_from')} · {$st('day_label')}<input type="number" min="1" max="31" bind:value={wrapped.startDay} oninput={() => wrappedDirty = true} required /></label>
              <label>{$st('wrapped_from')} · {$st('month_label')}<select bind:value={wrapped.startMonth} onchange={() => wrappedDirty = true}>{#each Array.from({ length: 12 }, (_, i) => i + 1) as month (month)}<option value={month}>{monthName(month, $statLocale)}</option>{/each}</select></label>
              <label>{$st('wrapped_to')} · {$st('day_label')}<input type="number" min="1" max="31" bind:value={wrapped.endDay} oninput={() => wrappedDirty = true} required /></label>
              <label>{$st('wrapped_to')} · {$st('month_label')}<select bind:value={wrapped.endMonth} onchange={() => wrappedDirty = true}>{#each Array.from({ length: 12 }, (_, i) => i + 1) as month (month)}<option value={month}>{monthName(month, $statLocale)}</option>{/each}</select></label>
            </div>
          {/if}
          <p class="muted">{$st('wrapped_settings_note')}</p>
        </fieldset>
      {/if}
      <div class="actions"><button class="primary" disabled={saving}>{$st('save')}</button><button type="button" onclick={() => void cleanup()} disabled={saving}>{$st('purge')}</button></div>
    </form></section>
  {/if}
  {#if data && tab==='settings'}
    <p class="collection-state">{data.enabled ? $st('collection_on') : $st('collection_off')}{#if data.firstEnabledAt} {$st('since', { date: date(data.firstEnabledAt) })}{:else} · {$st('no_history')}{/if}. {$st('retention', { events: data.eventRetentionDays ? $st('days_count', { count: data.eventRetentionDays }) : $st('summaries_forever'), summaries: data.summaryRetentionDays ? $st('days_count', { count: data.summaryRetentionDays }) : $st('summaries_forever') })}</p>
  {/if}
  {#if tab!=='settings'}
  <div class="toolbar">
    <div class="segmented" role="group" aria-label={$st('presets')}>
      {#each [[7, 'preset_7'], [30, 'preset_30'], [90, 'preset_90'], [365, 'preset_365']] as [days, key] (key)}
        <button type="button" aria-pressed={from === addDays(today, -((days as number) - 1)) && to === today} onclick={() => preset(days as number)}>{$st(key as StatKey)}</button>
      {/each}
      <button type="button" aria-pressed={from === `${today.slice(0, 4)}-01-01` && to === today} onclick={() => preset('year')}>{$st('preset_year')}</button>
    </div>
    <div class="range" role="group" aria-label={$st('custom_range')}>
      <label><span class="h-sr">{$st('from')}</span><input type="date" bind:value={from} max={to} required onchange={() => { if (from && to && from <= to) apply(); }} /></label>
      <span aria-hidden="true">→</span>
      <label><span class="h-sr">{$st('to')}</span><input type="date" bind:value={to} min={from} max={today} required onchange={() => { if (from && to && from <= to) apply(); }} /></label>
    </div>
    <button type="button" class="tool" aria-expanded={filtersOpen} aria-controls="statistics-filters" onclick={() => filtersOpen = !filtersOpen}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h16M7 12h10M10 18h4" /></svg>{$st('filters')}{#if activeFilters.length}<b class="count">{activeFilters.length}</b>{/if}
    </button>
    <span class="grow"></span>
    <button type="button" class="tool" onclick={() => void download()} disabled={!data}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>{$st('export')}</button>
    {#if nextExportOffset !== null}<button type="button" class="tool" onclick={() => void download(nextExportOffset ?? 0)}>{$st('export_next')}</button>{/if}
    <button type="button" class="tool icon" onclick={() => void load()} disabled={loading} title={$st('refresh')}><svg class:spin={loading} viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.3-5.6M20 4v5h-5" /></svg><span class="h-sr">{loading ? $st('loading') : $st('refresh')}</span></button>
  </div>
  {#if filtersOpen}
    <form id="statistics-filters" class="panel filter-form" aria-label={$st('advanced_filters')} onsubmit={event => { event.preventDefault(); apply(); }}>
      <div class="filters">
        {#if administrator}<label>{$st('user')}<select bind:value={userId}><option value="">{$st('all_m')}</option>{#each users as user (user.id)}<option value={user.id}>{user.name}</option>{/each}</select></label>{/if}
        <label>{$st('type')}<select bind:value={kind}><option value="">{$st('all_m')}</option><option value="film">{$st('films')}</option><option value="episode">{$st('episodes')}</option><option value="music">{$st('music')}</option></select></label>
        <label>{$st('library')}<select bind:value={libraryId}><option value="">{$st('all_f')}</option>{#each facets.libraries as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
        <label>{$st('node')}<select bind:value={nodeId}><option value="">{$st('all_m')}</option>{#each facets.nodes as node (node)}<option>{node}</option>{/each}</select></label>
        <label>{$st('device')}<select bind:value={device}><option value="">{$st('all_m')}</option>{#each facets.devices as item (item)}<option>{item}</option>{/each}</select></label>
        <label>{$st('ranking')}<select bind:value={rank}><option value="users">{$st('rank_users')}</option><option value="seconds">{$st('rank_seconds')}</option><option value="completion">{$st('rank_completion')}</option><option value="favorites">{$st('rank_favorites')}</option><option value="rating">{$st('rank_rating')}</option></select></label>
      </div>
      <div class="actions"><button class="primary" disabled={loading}>{$st('apply')}</button><button type="button" onclick={clearFilters} disabled={loading}>{$st('clear_filters')}</button></div>
    </form>
  {/if}
  {#if activeFilters.length}
    <div class="chips" role="group" aria-label={$st('filters_active')}>
      {#each activeFilters as item (item.key)}<button type="button" class="chip on" onclick={() => { item.clear(); apply(); }} aria-label={`${item.label} : ${item.value} · ${$st('clear_filters')}`}><span>{item.label}</span><b>{item.value}</b><span aria-hidden="true">×</span></button>{/each}
      <button type="button" class="chip link" onclick={clearFilters}>{$st('clear_filters')}</button>
    </div>
  {/if}
  {#if insightsError}<p class="notice error" role="alert">{insightsError}</p>{/if}
  {/if}
  {#if data}
    {#if tab==='overview'}
    <div class="kpis">
      <Kpi label={$st('kpi_time')} value={duration(data.summary.seconds)} growth={growth ? growth.seconds : undefined} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} spark={sparkline.map(point => point.seconds)} hint={$st('per_day', { value: shortDuration(data.summary.seconds / rangeDays) })} />
      <Kpi label={$st('kpi_plays')} value={number.format(playsCount)} growth={growth ? growth.plays : undefined} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} spark={sparkline.map(point => point.plays)} hint={`${number.format(data.summary.plays)} ${$st('kpi_opened').toLowerCase()}`} />
      {#if administrator}<Kpi label={$st('kpi_users')} value={number.format(data.summary.users)} growth={growth ? growth.users : undefined} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} />{/if}
      <Kpi label={$st('kpi_media')} value={number.format(data.summary.media)} growth={growth ? growth.media : undefined} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} />
      <Kpi label={$st('kpi_completed')} value={number.format(data.summary.completed)} growth={growth ? growth.completed : undefined} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} />
      <Kpi label={$st('kpi_active')} value={number.format(data.summary.active)} locale={$statLocale} hint={insights ? `${$st('kpi_peak')} : ${number.format(insights.concurrency.peak)}` : ''} />
    </div>
    {#if !data.summary.plays}<p class="panel empty">{$st('no_plays')}</p>{/if}
    {#if insights?.truncated}<p class="notice">{$st('truncated')}</p>{/if}
    {#if insights && data.summary.plays}
      {#if administrator && live.length}
        <section class="panel live" aria-labelledby="statistics-live">
          <h2 id="statistics-live"><span class="pulse" aria-hidden="true"></span>{$st('live_title')} <b>{live.length}</b></h2>
          <ul>{#each live.slice(0, 8) as item (item.id)}<li><strong>{item.title}</strong><span>{item.user} · {item.device}</span><time>{$st('live_since', { time: new Date(item.startedAt * 1000).toLocaleTimeString($statLocale, { hour: '2-digit', minute: '2-digit' }) })}</time></li>{/each}</ul>
        </section>
      {/if}
      <section class="panel">
        <div class="panel-head"><div><h2>{$st('activity_title')}</h2><p class="muted">{activity.grouped ? $st('activity_weekly') : $st('activity_daily')}</p></div><button type="button" class="link" onclick={() => tab = 'activity'}>{$st('see_all')} →</button></div>
        <AreaChart items={activity.items} label={$st('activity_title')} format={shortDuration} duration height={220} everyLabel={Math.max(1, Math.ceil(activity.items.length / 7))} />
      </section>
      <div class="dashboard">
        <section class="panel wide">
          <div class="panel-head"><div><h2>{$st('top_titles_title')}</h2><p class="muted">{$st('popular_note')}</p></div><button type="button" class="link" onclick={() => tab = 'content'}>{$st('see_all')} →</button></div>
          <TopList items={topTitles} empty={$st('no_data')} limit={8} />
        </section>
        <section class="panel">
          <h2>{$st('kinds_title')}</h2><p class="muted">{$st('kinds_note')}</p>
          <Donut items={kindSlices} total={shortDuration(data.summary.seconds)} caption={$st('kinds_title')} empty={$st('no_data')} percent={value => percent.format(value)} />
          <h2 class="spaced">{$st('completion_title')}</h2>
          <Donut items={completionSlices} total={number.format(bucketTotal)} caption={$st('completion_title')} empty={$st('no_data')} percent={value => percent.format(value)} />
        </section>
      </div>
      <div class="columns">
        <section class="panel"><h2>{$st('hours_title')}</h2><p class="muted">{$st('hours_note')}</p><HourHeatmap cells={insights.hours} label={$st('hours_title')} locale={$statLocale} {hourLabel} unit={plays} /></section>
        <section class="panel"><h2>{$st('top_series_title')}</h2><p class="muted">{$st('series_note')}</p><TopList items={topSeries} empty={$st('no_data')} limit={5} /></section>
      </div>
    {/if}
    {/if}
    {#if insights && data.summary.plays}
      {#if tab==='activity'}
      <div class="columns">
        <section class="panel"><h2>{$st('monthly_title')}</h2><p class="muted">{$st('monthly_note')}</p><Bars items={monthlyBars} label={$st('monthly_title')} format={shortDuration} /></section>
        <section class="panel"><h2>{$st('weekdays_title')}</h2><p class="muted">{$st('weekdays_note')}</p><Bars items={weekdayBars} label={$st('weekdays_title')} format={plays} /></section>
      </div>
      <section class="panel"><h2>{$st('calendar_title')}</h2><p class="muted">{$st('calendar_note')}</p>
        <CalendarHeatmap days={insights.calendar} {from} {to} label={$st('calendar_title')} locale={$statLocale} less={$st('less')} more={$st('more')} describe={(value, seconds, count) => `${day(value)} · ${shortDuration(seconds)} · ${plays(count)}`} />
      </section>
      <div class="columns">
        <section class="panel"><h2>{$st('daily_title')}</h2><p class="muted">{$st('daily_note')}</p><Bars items={dailyBars} label={$st('daily_title')} format={shortDuration} height={150} everyLabel={Math.max(1, Math.ceil(dailyBars.length / 12))} />
          <details><summary>{$st('daily_values')}</summary><ul class="values">{#each insights.calendar as item (item.date)}<li>{day(item.date)} : {duration(item.seconds)} · {plays(item.plays)}</li>{/each}</ul></details></section>
        <section class="panel"><h2>{$st('hours_title')}</h2><p class="muted">{$st('hours_note')}</p><HourHeatmap cells={insights.hours} label={$st('hours_title')} locale={$statLocale} {hourLabel} unit={plays} /></section>
      </div>
      {/if}
      {#if tab==='content'}
      <div class="columns">
        <section class="panel"><h2>{$st('genres_title')}</h2><p class="muted">{$st('genres_note')}</p><Meter items={insights.genres.map(item => ({ key: item.id, label: item.title, value: item.seconds, detail: `${shortDuration(item.seconds)} · ${plays(item.plays)}` }))} empty={$st('no_data')} /></section>
        <section class="panel"><h2>{$st('resolution_title')}</h2><p class="muted">{$st('resolution_note')}</p><Meter items={insights.resolution.map(item => ({ key: item.label, label: item.label === 'unknown' ? $st('res_unknown') : item.label, value: item.seconds, detail: `${shortDuration(item.seconds)} · ${plays(item.plays)}` })).sort((a, b) => b.value - a.value)} empty={$st('no_data')} /></section>
      </div>
      {/if}
      {#if tab==='technical'}
      <div class="columns">
        <section class="panel"><h2>{$st('methods_title')}</h2><p class="muted">{$st('methods_note')}</p><Meter items={insights.methods.map(item => ({ key: item.label, label: methodLabel(item.label), value: item.seconds, detail: `${shortDuration(item.seconds)} · ${plays(item.plays)}` })).sort((a, b) => b.value - a.value)} empty={$st('no_data')} /></section>
        <section class="panel"><h2>{$st('clients_title')}</h2><p class="muted">{$st('clients_note')}</p>
          {#if insights.clients.length}<div class="table-wrap"><table><thead><tr><th>{$st('client')}</th><th>{$st('col_time')}</th><th>{$st('direct')}</th><th>{$st('prepared')}</th><th>{$st('share')}</th></tr></thead><tbody>{#each insights.clients as client (client.label)}<tr><td>{client.label}</td><td>{shortDuration(client.seconds)}</td><td>{client.direct}</td><td>{client.transcoded}</td><td><span class="share" style:--w={client.transcodeShare}><span></span></span>{percent.format(client.transcodeShare)}</td></tr>{/each}</tbody></table></div>{:else}<p class="muted">{$st('no_data')}</p>{/if}
        </section>
      </div>
      {/if}
      {#if tab==='content'}
      <div class="columns">
        <section class="panel"><h2>{$st('completion_title')}</h2><p class="muted">{$st('completion_note')}</p>
          {#if bucketTotal}
            <div class="stack" role="img" aria-label={$st('completion_title')}>{#each bucketOrder as key (key)}{#if insights.completion.buckets[key]}<span class={`segment ${key}`} style:--w={insights.completion.buckets[key] / bucketTotal}></span>{/if}{/each}</div>
            <dl class="legend-list">{#each bucketOrder as key (key)}<dt><span class={`dot ${key}`}></span>{$st(`bucket_${key}` as StatKey)}</dt><dd>{insights.completion.buckets[key]} · {percent.format(insights.completion.buckets[key] / bucketTotal)}</dd>{/each}</dl>
          {:else}<p class="muted">{$st('no_data')}</p>{/if}
        </section>
        <section class="panel"><h2>{$st('dropoff_title')}</h2><p class="muted">{$st('dropoff_note')}</p>
          {#if insights.completion.dropoff.length}<div class="table-wrap"><table><thead><tr><th>{$st('col_media')}</th><th>{$st('starters')}</th><th>{$st('below25')}</th><th>{$st('below50')}</th><th>{$st('average')}</th></tr></thead><tbody>{#each insights.completion.dropoff as item (item.id)}<tr><td>{item.title}<small>{kindLabel(item.kind)}</small></td><td>{item.starters}</td><td>{item.below25}</td><td>{item.below50}</td><td>{percent.format(item.averageCompletion)}</td></tr>{/each}</tbody></table></div>{:else}<p class="muted">{$st('no_data')}</p>{/if}
        </section>
      </div>
      <div class="columns">
        <section class="panel"><h2>{$st('series_title')}</h2><p class="muted">{$st('series_note')}</p><Meter items={insights.series.map(item => ({ key: item.id, label: item.title, sub: $st('episodes_count', { count: item.items }), value: item.seconds, detail: `${shortDuration(item.seconds)} · ${plays(item.plays)}` }))} empty={$st('no_data')} /></section>
        <section class="panel"><h2>{$st('people_title')}</h2><p class="muted">{$st('people_note')}</p><Meter items={insights.people.map(person => ({ key: `${person.role}:${person.name}`, label: person.name, sub: person.role === 'director' ? $st('role_director') : $st('role_actor'), value: person.seconds, detail: $st('titles_count', { count: person.titles }) }))} empty={$st('no_data')} /></section>
      </div>
      {/if}
      {#if tab==='activity'}
      <section class="panel"><h2>{$st('concurrency_title')}</h2><p class="muted">{$st('concurrency_note')}{#if insights.concurrency.peakAt} {$st('peak_at', { count: insights.concurrency.peak, date: date(insights.concurrency.peakAt) })}.{/if}</p><Bars items={concurrencyBars} label={$st('concurrency_title')} format={value => String(value)} height={110} everyLabel={Math.max(1, Math.ceil(concurrencyBars.length / 12))} /></section>
      {/if}
    {/if}
      {#if tab==='content'}
    <section class="panel"><h2>{$st('popular_title')}</h2><p class="muted">{$st('popular_note')}</p><div class="table-wrap"><table><thead><tr><th>{$st('col_media')}</th><th>{$st('col_plays')}</th><th>{$st('col_people')}</th><th>{$st('col_time')}</th><th>{$st('col_completed')}</th><th>{$st('col_favorites')}</th><th>{$st('col_rating')}</th></tr></thead><tbody>{#each data.popular as media (`${media.kind}:${media.id}`)}{@const href = mediaHref(media.kind, media.id)}<tr><td>{#if href}
<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- resolved base path plus query. -->
<a class="title-link" {href} aria-label={$st('open_stats', { title: media.title })}>{media.title}</a>{:else}{media.title}{/if}<small>{kindLabel(media.kind)}</small></td><td>{media.plays}</td><td>{media.users}</td><td>{duration(media.seconds)}</td><td>{media.completed} · {Math.round(media.completed / media.plays * 100)} %</td><td>{media.favorites}</td><td>{media.rating ? media.rating.toFixed(1) : '—'}</td></tr>{/each}</tbody></table></div></section>
      {/if}
      {#if tab==='technical'}
    <div class="columns">
      <section class="panel"><h2>{$st('behaviour_title')}</h2><dl>{#each metricKeys as key (key)}<dt>{$st(`m_${key}` as StatKey)}</dt><dd>{data.behavior[key]}</dd>{/each}</dl></section>
      <section class="panel"><h2>{$st('quality_title')}</h2><dl><dt>{$st('startup')}</dt><dd>{Math.round(data.behavior.startupMs)} ms</dd><dt>{$st('buffering')}</dt><dd>{data.behavior.bufferingCount}</dd><dt>{$st('buffering_time')}</dt><dd>{duration(data.behavior.bufferingSeconds)}</dd><dt>{$st('errors')}</dt><dd>{data.behavior.errors}</dd></dl><p class="muted">{$st('quality_note')}</p></section>
    </div>
    <div class="columns">{@render distribution($st('dist_initial_audio'), data.distributions.initialAudio ?? [])}{@render distribution($st('dist_audio_time'), data.distributions.audioTime ?? [])}{@render distribution($st('dist_subtitles_time'), data.distributions.subtitlesTime ?? [])}{@render distribution($st('dist_method_time'), (data.distributions.methodTime ?? []).map(row => ({ ...row, label: methodLabel(row.label) })))}{@render distribution($st('dist_artists'), data.music.artist ?? [])}{@render distribution($st('dist_albums'), data.music.album ?? [])}</div>
      {/if}
      {#if tab==='history'}
    <section class="panel"><h2>{$st('history_title')}</h2><div class="table-wrap"><table><thead><tr><th>{$st('col_date')}</th>{#if administrator}<th>{$st('col_user')}</th>{/if}<th>{$st('col_media')}</th><th>{$st('col_device')}</th><th>{$st('col_time')}</th><th>{$st('col_coverage')}</th><th>{$st('col_status')}</th><th>{$st('col_detail')}</th></tr></thead><tbody>{#each data.sessions as item (item.id)}<tr><td>{date(item.startedAt)}</td>{#if administrator}<td>{item.user}</td>{/if}<td>{item.title}{#if item.artist}<small>{item.artist} · {item.album}</small>{/if}</td><td class="device">{item.device}</td><td>{duration(item.seconds)}</td><td>{item.duration ? `${Math.min(100, Math.round(item.coveredSeconds / item.duration * 100))} %` : $st('unknown')}</td><td>{statusLabel(item.status)}</td><td><button onclick={() => void openDetail(item.id)} aria-label={$st('detail_of', { title: item.title })}>{$st('view')}</button></td></tr>{/each}</tbody></table></div><div class="actions"><button disabled={offset === 0 || loading} onclick={() => { offset = Math.max(0, offset - 50); void load(false); }}>{$st('previous')}</button><span>{$st('page', { page: Math.floor(offset / 50) + 1 })}</span><button disabled={!data.hasMore || loading} onclick={() => { offset += 50; void load(false); }}>{$st('next')}</button></div></section>
    {#if administrator}<section class="panel"><h2>{$st('users_title')}</h2><div class="table-wrap"><table><thead><tr><th>{$st('col_user')}</th><th>{$st('col_plays')}</th><th>{$st('col_time')}</th><th>{$st('profile')}</th></tr></thead><tbody>{#each data.users as user (user.id)}<tr><td>{user.name}</td><td>{user.plays}</td><td>{duration(user.seconds)}</td><td><button onclick={() => void openProfile(user.id)} aria-label={$st('profile_of', { name: user.name })}>{$st('profile')}</button></td></tr>{/each}</tbody></table></div></section>{/if}
      {/if}
    {#if tab!=='settings'}<p class="muted footnote">{$st('dates_note', { zone: insights?.timezone ?? zone, seconds: insights?.minimumPlaySeconds ?? 60 })}</p>{/if}
    {#if tab==='settings'}
    <details class="panel"><summary>{$st('periods_title')}</summary><p>{$st('periods_note')}</p><ul>{#each data.periods as period, index (`${period.startedAt}-${index}`)}<li>{date(period.startedAt)} → {period.endedAt ? date(period.endedAt) : $st('ongoing')}</li>{/each}</ul><p>{$st('privacy_note')}</p></details>
    {#if !administrator}<details class="panel"><summary>{$st('erase_title')}</summary><p>{$st('erase_note')}</p><label class="check"><input type="checkbox" bind:checked={eraseConfirmed} /> {$st('erase_confirm')}</label><button disabled={!eraseConfirmed || saving} onclick={() => void erase()}>{$st('erase_action')}</button></details>{/if}
    {/if}
  {/if}
  {/if}
</section>
{#snippet distribution(title: string, rows: Distribution[])}<section class="panel"><h2>{title}</h2>{#if rows.length}<dl>{#each rows.slice(0, 12) as row (row.label)}<dt>{row.label || $st('not_set')}</dt><dd>{row.plays} · {duration(row.seconds)}</dd>{/each}</dl>{:else}<p class="muted">{$st('no_data')}</p>{/if}</section>{/snippet}
<dialog bind:this={dialog} class="session-dialog" onclose={() => { detailPending?.abort(); detail = null; }}><header class="heading"><h2>{detail?.session.title ?? $st('session_detail')}</h2><button onclick={() => dialog.close()}>{$st('close')}</button></header>{#if detailLoading}<p role="status">{$st('loading')}</p>{:else if detail}<p>{date(detail.session.startedAt)} · {detail.session.device} · {statusLabel(detail.session.status)}</p><dl><dt>{$st('col_time')}</dt><dd>{duration(detail.session.seconds)}</dd><dt>{$st('covered')}</dt><dd>{duration(detail.session.coveredSeconds)} / {duration(detail.session.duration)}</dd><dt>{$st('end')}</dt><dd>{detail.session.endReason || $st('playing_now')}</dd></dl>{#if detail.detailsExpired}<p>{$st('details_expired')}</p>{/if}<div class="tabs timeline-filter" role="group" aria-label={$st('filter_events')}><button type="button" class:active={timelineFilter === 'actions'} onclick={() => timelineFilter = 'actions'}>{$st('key_moments')}</button><button type="button" class:active={timelineFilter === 'all'} onclick={() => timelineFilter = 'all'}>{$st('all_count', { count: detail.events.length })}</button></div><ol class="timeline">{#each visibleTimeline as item (item.event.sequence)}<li><time>{date(item.occurredAt)}</time><strong>{eventLabels[item.event.kind] ?? item.event.kind}{#if item.event.kind === 'speed_change' && item.event.rate !== undefined} ×{item.event.rate}{/if}</strong><span>{duration(item.event.position)} · {item.event.origin === 'manual' ? $st('origin_manual') : item.event.origin === 'sync' ? $st('origin_sync') : $st('origin_auto')}{#if item.event.previousTrack} · {item.event.previousTrack.language} →{/if}{#if item.event.track} {item.event.track.language} ({item.event.track.id}){/if}{#if item.event.fromPosition !== undefined} · {$st('from_position', { position: duration(item.event.fromPosition) })}{/if}{#if item.event.errorCode} · {item.event.errorCode}{/if}</span></li>{/each}</ol>{#if detail.eventsTruncated}<button onclick={() => void moreEvents()} disabled={detailLoading}>{$st('more_events')}</button>{/if}{/if}</dialog>
<dialog bind:this={profileDialog} class="session-dialog profile-dialog" onclose={() => { profilePending?.abort(); profile = null; }}>
  <header class="heading"><h2>{profile ? $st('profile_of', { name: profile.user.name }) : $st('profile')}</h2><div class="heading-actions">
    {#if profile}
      <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- resolved base path plus query. -->
      <a class="wrapped-cta" href={`${wrappedHref}?user=${encodeURIComponent(profile.user.id)}`}>{$st('open_wrapped')}</a>
    {/if}
    <button onclick={() => profileDialog.close()}>{$st('close')}</button></div></header>
  {#if profileLoading}<p role="status">{$st('loading')}</p>{:else if profile}
    <div class="kpis">
      <Kpi label={$st('kpi_time')} value={duration(profile.totals.seconds)} growth={profile.growth.seconds} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} />
      <Kpi label={$st('kpi_plays')} value={number.format(profile.totals.plays)} growth={profile.growth.plays} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} />
      <Kpi label={$st('kpi_media')} value={number.format(profile.totals.media)} growth={profile.growth.media} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} />
      <Kpi label={$st('kpi_completed')} value={number.format(profile.totals.completed)} locale={$statLocale} />
    </div>
    <dl class="profile-dates">{#if profile.firstPlay}<dt>{$st('profile_first')}</dt><dd>{date(profile.firstPlay)}</dd>{/if}{#if profile.lastPlay}<dt>{$st('profile_last')}</dt><dd>{date(profile.lastPlay)}</dd>{/if}</dl>
    <div class="columns">
      <section><h3>{$st('weekdays_title')}</h3><Bars items={profile.weekdays.map(item => ({ key: String(item.weekday), label: weekdayName(item.weekday, $statLocale, 'short'), value: item.plays, hint: weekdayName(item.weekday, $statLocale) }))} label={$st('weekdays_title')} format={plays} height={120} /></section>
      <section><h3>{$st('hours_title')}</h3><HourHeatmap cells={profile.hours} label={$st('hours_title')} locale={$statLocale} {hourLabel} unit={plays} /></section>
      <section><h3>{$st('profile_top')}</h3><Meter items={profile.titles.map(item => ({ key: item.id, label: item.title, sub: kindLabel(item.kind), value: item.seconds, detail: shortDuration(item.seconds) }))} empty={$st('no_data')} limit={8} /></section>
      <section><h3>{$st('genres_title')}</h3><Meter items={profile.genres.map(item => ({ key: item.id, label: item.title, value: item.seconds, detail: shortDuration(item.seconds) }))} empty={$st('no_data')} limit={8} /></section>
      <section><h3>{$st('clients_title')}</h3><Meter items={profile.clients.map(item => ({ key: item.label, label: item.label, value: item.seconds, detail: `${shortDuration(item.seconds)} · ${percent.format(item.transcodeShare)} ${$st('prepared').toLowerCase()}` }))} empty={$st('no_data')} limit={6} /></section>
      <section><h3>{$st('profile_recent')}</h3>{#if profile.recent.length}<ol class="recent">{#each profile.recent as item (item.id)}<li><span>{item.title}<small>{kindLabel(item.kind)} · {item.device}</small></span><time>{date(item.startedAt)}</time></li>{/each}</ol>{:else}<p class="muted">{$st('no_data')}</p>{/if}</section>
    </div>
  {/if}
</dialog>
<style>
  .statistics-tabs{display:flex;gap:4px;border-bottom:1px solid var(--line);overflow-x:auto;scrollbar-width:none}
  .statistics-tabs button{flex:none;border:0;border-radius:0;background:none;color:var(--mu);font-weight:600;padding:12px 14px;margin-bottom:-1px;border-bottom:2px solid transparent}
  .statistics-tabs button:hover:not(:disabled){background:none;color:var(--tx)}
  .statistics-tabs button.active{color:var(--tx);border-bottom-color:var(--ac)}
  .activation{margin-block:16px}
  .switch{display:inline-flex;align-items:center;gap:10px;min-height:40px;padding:0 14px;border:1px solid var(--line);border-radius:var(--r-pill);background:var(--bg-2);font-size:13px;font-weight:600;cursor:pointer}
  .switch input{appearance:none;width:34px;min-height:20px;height:20px;padding:0;border-radius:var(--r-pill);background:var(--pn-3);border:0;position:relative;cursor:pointer;transition:background var(--t-fast) var(--ease)}
  .switch input::after{content:'';position:absolute;top:3px;left:3px;width:14px;height:14px;border-radius:50%;background:var(--tx);transition:transform var(--t-fast) var(--ease)}
  .switch input:checked{background:var(--ok)}.switch input:checked::after{transform:translateX(14px)}
  .switch input:focus-visible{outline:2px solid var(--ac);outline-offset:2px}
  .toolbar{display:flex;flex-wrap:wrap;align-items:center;gap:10px}
  .toolbar .grow{flex:1}
  .segmented{display:inline-flex;flex-wrap:wrap;padding:3px;border-radius:var(--r-pill);background:var(--pn);border:1px solid var(--line)}
  .segmented button{border:0;background:transparent;color:var(--mu);padding:6px 13px;min-height:32px;border-radius:var(--r-pill);font-size:12.5px;font-weight:600}
  .segmented button:hover:not(:disabled){background:transparent;color:var(--tx)}
  .segmented button[aria-pressed='true']{background:var(--bg-3);color:var(--tx);box-shadow:0 1px 0 var(--line-2) inset}
  .range{display:inline-flex;align-items:center;gap:6px;color:var(--mu)}.range label{display:block}.range input{width:auto;min-height:38px;padding:6px 10px;font-size:13px}
  .tool{display:inline-flex;align-items:center;gap:8px;min-height:38px;padding:0 14px;font-size:13px;font-weight:600;border-radius:var(--r-pill)}
  .tool svg{width:16px;height:16px;fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
  .tool[aria-expanded='true']{background:var(--ac-soft);border-color:var(--ac)}
  .tool.icon{width:38px;padding:0;justify-content:center}
  .count{display:inline-grid;place-items:center;min-width:18px;height:18px;padding:0 5px;border-radius:var(--r-pill);background:var(--ac);color:var(--on-ac);font-size:11px}
  .spin{animation:spin 1s linear infinite}@keyframes spin{to{transform:rotate(360deg)}}
  .chips{display:flex;flex-wrap:wrap;gap:8px}
  .chip{display:inline-flex;align-items:center;gap:6px}.chip b{font-weight:700}.chip.link{background:none;border-color:transparent;color:var(--mu);text-decoration:underline}
  .panel-head{display:flex;justify-content:space-between;align-items:flex-start;gap:12px}
  .link{background:none;border:0;padding:4px 0;color:var(--ac);font-weight:600;font-size:13px;white-space:nowrap}.link:hover:not(:disabled){background:none;text-decoration:underline}
  .dashboard{display:grid;grid-template-columns:minmax(0,2fr) minmax(0,1fr);gap:20px}
  .spaced{margin-top:26px !important}
  .live h2{display:flex;align-items:center;gap:10px}.live h2 b{font-size:12px;padding:2px 8px;border-radius:var(--r-pill);background:var(--info-soft);color:var(--info)}
  .live ul{list-style:none;margin:14px 0 0;padding:0;display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:10px}
  .live li{display:grid;gap:3px;padding:12px 14px;border-radius:var(--r-sm);background:var(--pn);font-size:13px;min-width:0}
  .live li strong,.live li span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.live li span,.live li time{color:var(--mu);font-size:12px}
  .pulse{width:9px;height:9px;border-radius:50%;background:var(--ok);box-shadow:0 0 0 0 var(--ok-soft);animation:pulse 1.6s ease-out infinite}
  @keyframes pulse{0%{box-shadow:0 0 0 0 var(--ok-soft)}70%{box-shadow:0 0 0 10px transparent}100%{box-shadow:0 0 0 0 transparent}}
  .title-link{color:var(--tx);font-weight:600;text-decoration:none}.title-link:hover{text-decoration:underline}
  .footnote{font-size:12px;margin:0}
  @media(prefers-reduced-motion:reduce){.spin,.pulse{animation:none}}
  .statistics{display:grid;gap:22px;color:var(--tx);width:100%;max-width:var(--maxw);margin:0 auto;min-width:0}
  .heading{display:flex;justify-content:space-between;align-items:center;gap:20px;flex-wrap:wrap}.heading h1{font-size:clamp(1.6rem,3vw,2.4rem);margin:4px 0}.heading h2{margin:4px 0}.heading p,.muted,.collection-state{color:var(--mu);line-height:1.6}
  .heading-actions{display:flex;flex-wrap:wrap;gap:10px;align-items:center}
  .eyebrow{font-size:.75rem;text-transform:uppercase;letter-spacing:.12em}
  .wrapped-cta{display:inline-flex;align-items:center;gap:10px;min-height:44px;padding:0 18px;border-radius:var(--r-md);background:var(--btn);color:var(--btn-tx);font-weight:800;transition:transform var(--t-fast) var(--ease)}
  .wrapped-cta:hover{transform:translateY(-1px)}.wrapped-cta svg{width:20px;height:20px;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linejoin:round}
  .panel{background:var(--bg-2);border:1px solid var(--line);border-radius:var(--r-md);padding:22px;min-width:0}.panel h2{font-size:1.05rem;margin:0 0 6px}.panel p{line-height:1.6}.panel > .muted{margin:0 0 18px;font-size:13px}
  .filters{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:16px}label{display:grid;gap:8px;font-size:.85rem}.check{display:flex;align-items:center;gap:10px;margin:16px 0}
  .actions{display:flex;flex-wrap:wrap;align-items:center;gap:12px;margin-top:20px}.primary{background:var(--btn);color:var(--btn-tx);border-color:transparent}
  button{padding:10px 15px;border:1px solid var(--line);border-radius:var(--r-sm);background:var(--bg-2);color:var(--tx);cursor:pointer}button:hover:not(:disabled){background:var(--pn-2)}button:disabled{opacity:.45;cursor:default}
  input,select{width:100%;box-sizing:border-box;min-height:42px;padding:9px 12px;background:var(--bg);border:1px solid var(--line);color:var(--tx);border-radius:var(--r-xs)}.check input{width:18px;min-height:18px}
  summary{cursor:pointer;font-weight:600}
  .presets{display:flex;flex-wrap:wrap;gap:8px;margin-bottom:16px}.chip{padding:6px 14px;min-height:34px;border-radius:var(--r-pill);background:var(--pn);font-size:13px}.chip.on{background:var(--ac-soft);border-color:var(--ac);color:var(--tx)}
  .wrapped-settings{border:1px solid var(--line);border-radius:var(--r-sm);padding:4px 16px 12px;margin:18px 0 0}.wrapped-settings legend{font-weight:700;padding:0 6px}
  .kpis{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:14px}
  .columns{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px}
  dl{display:grid;grid-template-columns:1fr auto;gap:12px;margin:0}dt{color:var(--mu);overflow-wrap:anywhere}dd{margin:0;text-align:right;font-variant-numeric:tabular-nums}
  .table-wrap{overflow:auto}table{width:100%;border-collapse:collapse;font-size:.85rem}th{text-align:left;color:var(--mu);font-weight:500}td,th{padding:12px;border-bottom:1px solid var(--line);vertical-align:middle}td:first-child,th:first-child{padding-left:0}td small{display:block;color:var(--mu);margin-top:4px}
  td.device{max-width:220px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .share{display:inline-block;width:56px;height:6px;border-radius:var(--r-pill);background:var(--pn);margin-right:8px;vertical-align:middle;overflow:hidden}.share span{display:block;height:100%;width:calc(var(--w) * 100%);background:var(--ac)}
  .stack{display:flex;height:14px;border-radius:var(--r-pill);overflow:hidden;background:var(--pn);margin-bottom:18px}.segment{flex:var(--w) 0 0;height:100%}
  .completed{background:var(--ok)}.partial{background:var(--info)}.dropped{background:var(--warn)}.abandoned{background:var(--err)}.unknown{background:var(--pn-3)}
  .legend-list dt{display:flex;align-items:center;gap:8px}.dot{display:inline-block;width:10px;height:10px;border-radius:50%}
  .values{max-height:240px;overflow:auto;font-size:13px;color:var(--mu)}
  .notice{border:1px solid var(--line);background:var(--bg-2);padding:16px;border-radius:var(--r-sm);margin:0}.error{color:var(--err)}.empty{text-align:center;color:var(--mu);padding:40px}
  .session-dialog{width:min(850px,calc(100% - 36px));max-height:85dvh;box-sizing:border-box;background:var(--bg-3);color:var(--tx);border:1px solid var(--line);border-radius:var(--r-lg);padding:26px}.session-dialog::backdrop{background:var(--media-shade);backdrop-filter:blur(8px)}
  .profile-dialog{width:min(1100px,calc(100% - 36px))}.profile-dialog .kpis{margin:12px 0}.profile-dialog h3{font-size:.95rem;margin:0 0 12px}.profile-dialog .columns section{min-width:0}
  .profile-dates{margin:8px 0 22px;max-width:520px}
  .recent{list-style:none;margin:0;padding:0;display:grid;gap:10px;font-size:13px}.recent li{display:flex;justify-content:space-between;gap:12px}.recent small{display:block;color:var(--mu)}.recent time{color:var(--mu);white-space:nowrap}
  .timeline{list-style:none;padding:0}.timeline li{display:grid;grid-template-columns:145px 150px 1fr;gap:12px;padding:13px 0;border-bottom:1px solid var(--line);font-size:.8rem}.timeline time,.timeline span{color:var(--mu)}
  @media(max-width:900px){.columns,.dashboard{grid-template-columns:1fr}}
  @media(max-width:640px){.panel{padding:16px}.heading{align-items:flex-start}.timeline li{grid-template-columns:1fr;gap:6px}.kpis{grid-template-columns:repeat(2,minmax(0,1fr))}.toolbar .grow{display:none}.range{width:100%}.range label{flex:1}.range input{width:100%}.wrapped-cta{width:100%;justify-content:center}}
</style>

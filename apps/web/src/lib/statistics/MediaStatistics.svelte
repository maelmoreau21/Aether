<script lang="ts">
  // Audience of one title (film or series), shown in the title's « Statistiques » tab.
  // Every number comes from the Core's statistics endpoints narrowed with `mediaId`,
  // so library permissions, parental policy and account isolation stay server-side.
  import { untrack } from 'svelte';
  import { session, getSessionGeneration } from '../auth/session';
  import { statisticsRequest, duration, type Insights, type Statistics, type Distribution } from './api';
  import { st, statLocale, type StatKey } from './i18n';
  import { addDays, browserTimeZone, localDate, localDayStart, shortDuration } from './insights';
  import { chartSeries } from './series';
  import Kpi from './charts/Kpi.svelte';
  import AreaChart from './charts/AreaChart.svelte';
  import Meter from './charts/Meter.svelte';
  import HourHeatmap from './charts/HourHeatmap.svelte';

  let { mediaId, kind = 'film' }: { mediaId: string; kind?: 'film' | 'series' } = $props();

  type Period = '30' | '90' | '365' | 'all';
  const periods: Period[] = ['30', '90', '365', 'all'];
  const periodKeys: Record<Period, StatKey> = { '30': 'preset_30', '90': 'preset_90', '365': 'preset_365', all: 'preset_all' };
  const zone = browserTimeZone();
  const administrator = $derived($session?.user.role === 'administrator');
  let period = $state<Period>('all');
  let scope = $state<'server' | 'me'>('server');
  let loading = $state(false), error = $state('');
  let insights = $state<Insights | null>(null), report = $state<Statistics | null>(null);
  let pending: AbortController | undefined;

  const today = localDate(new Date());
  const server = $derived(administrator && scope === 'server');
  const endpoint = $derived(server ? '/api/v1/admin/statistics' : '/api/v1/account/statistics');

  function query(limit?: number) {
    const from = period === 'all' ? 0 : localDayStart(addDays(today, -(Number(period) - 1)));
    const values = new URLSearchParams({ from: String(Math.max(0, from)), to: String(localDayStart(addDays(today, 1))), tz: zone, mediaId });
    if (limit) values.set('limit', String(limit));
    return values.toString();
  }
  async function load() {
    const generation = getSessionGeneration();
    pending?.abort(); const controller = new AbortController(); pending = controller;
    loading = true; error = '';
    try {
      const [extra, summary] = await Promise.all([
        statisticsRequest(`${endpoint}/insights?${query()}`, { signal: controller.signal }).then(response => response.json() as Promise<Insights>),
        statisticsRequest(`${endpoint}?${query(10)}`, { signal: controller.signal }).then(response => response.json() as Promise<Statistics>)
      ]);
      if (pending !== controller || generation !== getSessionGeneration()) return;
      insights = extra; report = summary;
    } catch (cause) {
      if (!controller.signal.aborted && generation === getSessionGeneration()) { insights = null; report = null; error = cause instanceof Error ? cause.message : $st('error_generic'); }
    } finally { if (pending === controller) loading = false; }
  }
  $effect(() => {
    void mediaId; void period; void endpoint; void $session?.user.id;
    untrack(() => { void load(); });
    return () => pending?.abort();
  });

  const number = $derived(new Intl.NumberFormat($statLocale));
  const percent = $derived(new Intl.NumberFormat($statLocale, { style: 'percent', maximumFractionDigits: 0 }));
  const media = $derived(insights?.media);
  const audience = $derived(media?.audience);
  function when(value: number | null | undefined) {
    if (!value) return $st('media_never');
    const at = new Date(value * 1000);
    return at.toLocaleDateString($statLocale, at.getFullYear() === new Date().getFullYear() ? { day: 'numeric', month: 'short' } : { day: 'numeric', month: 'short', year: 'numeric' });
  }
  function stamp(value: number) { return new Date(value * 1000).toLocaleString($statLocale, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' }); }
  function share(value: number | null | undefined) { return value === null || value === undefined ? '—' : percent.format(value); }
  function plays(value: number) { return `${number.format(value)} ${$st('kpi_plays').toLowerCase()}`; }
  function statusLabel(status: string) { return ['active', 'completed', 'partial', 'interrupted'].includes(status) ? $st(`status_${status}` as StatKey) : status; }

  const languages = $derived.by(() => { try { return new Intl.DisplayNames([$statLocale], { type: 'language' }); } catch { return null; } });
  function language(code: string) {
    if (!code || code === 'und') return $st('not_set');
    if (code === 'off') return $st('media_subtitles_off');
    try { const name = languages?.of(code) ?? code; return name.charAt(0).toLocaleUpperCase($statLocale) + name.slice(1); } catch { return code; }
  }
  function languageItems(rows: Distribution[]) {
    return rows.map(row => ({ key: row.label, label: language(row.label), value: row.seconds, detail: `${shortDuration(row.seconds)} · ${plays(row.plays)}` }));
  }

  const start = $derived(period === 'all'
    ? (audience?.firstPlay ? localDate(new Date(audience.firstPlay * 1000)) : addDays(today, -29))
    : addDays(today, -(Number(period) - 1)));
  const activity = $derived.by(() => {
    const series = chartSeries(insights?.calendar ?? [], start, today);
    const format = (from: string) => new Date(`${from}T12:00:00`).toLocaleDateString($statLocale, { day: 'numeric', month: 'short' });
    return {
      grouped: series.grouped,
      items: series.points.map(point => ({ key: point.key, label: format(point.from), value: point.seconds, hint: `${point.from === point.to ? format(point.from) : `${format(point.from)} – ${format(point.to)}`} · ${plays(point.plays)}` }))
    };
  });
  const retention = $derived((media?.retention ?? []).map(point => ({ key: String(point.percent), label: `${point.percent} %`, value: point.share, hint: `${$st('media_retention_point', { percent: point.percent })} · ${number.format(point.viewers)} ${$st('media_viewers').toLowerCase()}` })));
  const empty = $derived(!!insights && !insights.totals.opened);
</script>

<section class="media-stats" aria-labelledby="media-stats-title" aria-busy={loading}>
  <header class="head">
    <div>
      <h2 id="media-stats-title">{$st('media_title')}</h2>
      <p>{server ? $st('media_note_server') : $st('media_note_personal')}</p>
    </div>
    <div class="controls">
      {#if administrator}
        <div class="segmented" role="group" aria-label={$st('media_scope')}>
          <button type="button" aria-pressed={scope === 'server'} onclick={() => scope = 'server'}>{$st('media_scope_server')}</button>
          <button type="button" aria-pressed={scope === 'me'} onclick={() => scope = 'me'}>{$st('media_scope_me')}</button>
        </div>
      {/if}
      <div class="segmented" role="group" aria-label={$st('presets')}>
        {#each periods as item (item)}<button type="button" aria-pressed={period === item} onclick={() => period = item}>{$st(periodKeys[item])}</button>{/each}
      </div>
    </div>
  </header>

  {#if error}<p class="notice error" role="alert">{error}</p>{/if}
  {#if !insights && loading}
    <div class="kpis skeleton" aria-hidden="true">{#each Array.from({ length: 4 }, (_, i) => i) as i (i)}<span></span>{/each}</div>
    <p class="h-sr" role="status">{$st('loading')}</p>
  {:else if empty}
    <p class="empty">{$st('media_empty')}</p>
  {:else if insights && audience}
    <div class="kpis">
      <Kpi label={$st('kpi_plays')} value={number.format(audience.plays)} growth={period === 'all' ? undefined : insights.growth.plays} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} hint={[audience.sessions !== audience.plays ? `${number.format(audience.sessions)} ${$st('kpi_opened').toLowerCase()}` : '', audience.active ? `${number.format(audience.active)} ${$st('kpi_active').toLowerCase()}` : ''].filter(Boolean).join(' · ')} />
      <Kpi label={$st('kpi_time')} value={duration(audience.seconds)} growth={period === 'all' ? undefined : insights.growth.seconds} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} spark={activity.items.length > 1 ? activity.items.map(item => item.value) : undefined} />
      {#if server}<Kpi label={$st('media_viewers')} value={number.format(audience.viewers)} growth={period === 'all' ? undefined : insights.growth.users} locale={$statLocale} compare={$st('vs_previous')} none={$st('growth_none')} />{/if}
      <Kpi label={$st('media_avg_completion')} value={share(audience.averageCompletion)} locale={$statLocale} hint={`${$st('media_completion_rate')} : ${share(audience.completionRate)}`} />
      <Kpi label={$st('media_avg_session')} value={audience.averageSession === null ? '—' : shortDuration(audience.averageSession)} locale={$statLocale} />
      <Kpi label={$st('media_rewatches')} value={number.format(audience.rewatches)} locale={$statLocale} />
      <Kpi label={$st('media_last_play')} value={when(audience.lastPlay)} locale={$statLocale} hint={audience.firstPlay ? `${$st('media_first_play')} : ${when(audience.firstPlay)}` : ''} />
    </div>

    <div class="grid">
      <section class="card">
        <h3>{$st('media_retention_title')}</h3><p class="note">{$st('media_retention_note')}</p>
        <AreaChart items={retention} label={$st('media_retention_title')} format={value => percent.format(value)} max={1} height={180} everyLabel={2} tone="info" />
      </section>
      <section class="card">
        <h3>{$st('media_activity_title')}</h3><p class="note">{activity.grouped ? $st('activity_weekly') : $st('activity_daily')}</p>
        <AreaChart items={activity.items} label={$st('media_activity_title')} format={shortDuration} duration height={180} everyLabel={Math.max(1, Math.ceil(activity.items.length / 6))} />
      </section>
    </div>

    {#if kind === 'series' && media?.episodes.length}
      <section class="card">
        <h3>{$st('media_episodes_title')}</h3><p class="note">{$st('media_episodes_note')}</p>
        <div class="table-wrap"><table>
          <thead><tr><th>{$st('col_media')}</th><th class="num">{$st('col_plays')}</th>{#if server}<th class="num">{$st('media_viewers')}</th>{/if}<th class="num">{$st('col_time')}</th><th>{$st('media_avg_completion')}</th></tr></thead>
          <tbody>
            {#each media.episodes as item (item.id)}
              <tr>
                <td><span class="code">{item.season !== null && item.episode !== null ? $st('media_episode_code', { season: item.season, episode: item.episode }) : '—'}</span>{item.title}</td>
                <td class="num">{number.format(item.plays)}</td>
                {#if server}<td class="num">{number.format(item.users)}</td>{/if}
                <td class="num">{shortDuration(item.seconds)}</td>
                <td><span class="bar" style:--w={item.averageCompletion ?? 0}><span></span></span>{share(item.averageCompletion)}</td>
              </tr>
            {/each}
          </tbody>
        </table></div>
      </section>
    {/if}

    <div class="grid">
      <section class="card"><h3>{$st('hours_title')}</h3><p class="note">{$st('hours_note')}</p><HourHeatmap cells={insights.hours} label={$st('hours_title')} locale={$statLocale} hourLabel={hour => $st('hour_label', { hour })} unit={plays} /></section>
      <section class="card split">
        <div><h3>{$st('media_audio_title')}</h3><Meter items={languageItems(report?.distributions.audioTime ?? [])} empty={$st('no_data')} limit={5} /></div>
        <div><h3>{$st('media_subtitles_title')}</h3><Meter items={languageItems(report?.distributions.subtitlesTime ?? [])} empty={$st('no_data')} limit={5} /></div>
      </section>
    </div>

    {#if server && media?.viewers.length}
      <section class="card">
        <h3>{$st('media_viewers_title')}</h3><p class="note">{$st('media_viewers_note')}</p>
        <div class="table-wrap"><table>
          <thead><tr><th>{$st('col_user')}</th><th class="num">{$st('col_plays')}</th><th class="num">{$st('col_time')}</th><th>{$st('media_avg_completion')}</th><th>{$st('media_last_play')}</th></tr></thead>
          <tbody>{#each media.viewers as viewer (viewer.id)}<tr><td>{viewer.name}</td><td class="num">{number.format(viewer.plays)}</td><td class="num">{shortDuration(viewer.seconds)}</td><td><span class="bar" style:--w={viewer.averageCompletion ?? 0}><span></span></span>{share(viewer.averageCompletion)}</td><td>{when(viewer.lastPlay)}</td></tr>{/each}</tbody>
        </table></div>
      </section>
    {/if}

    {#if report?.sessions.length}
      <section class="card">
        <h3>{$st('media_recent_title')}</h3>
        <div class="table-wrap"><table>
          <thead><tr><th>{$st('col_date')}</th>{#if server}<th>{$st('col_user')}</th>{/if}{#if kind === 'series'}<th>{$st('col_media')}</th>{/if}<th>{$st('col_device')}</th><th class="num">{$st('col_time')}</th><th class="num">{$st('col_coverage')}</th><th>{$st('col_status')}</th></tr></thead>
          <tbody>{#each report.sessions as item (item.id)}<tr><td>{stamp(item.startedAt)}</td>{#if server}<td>{item.user}</td>{/if}{#if kind === 'series'}<td>{item.title}</td>{/if}<td class="device">{item.device}</td><td class="num">{duration(item.seconds)}</td><td class="num">{item.duration ? `${Math.min(100, Math.round(item.coveredSeconds / item.duration * 100))} %` : '—'}</td><td><span class={`status ${item.status}`}>{statusLabel(item.status)}</span></td></tr>{/each}</tbody>
        </table></div>
      </section>
    {/if}
    <p class="foot">{$st('media_collection_note')} {$st('dates_note', { zone: insights.timezone, seconds: insights.minimumPlaySeconds })}</p>
  {/if}
</section>

<style>
  .media-stats { display: grid; gap: 18px; min-width: 0; }
  .head { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: flex-end; gap: 14px; }
  .head h2 { margin: 0 0 4px; font-size: 1.15rem; }
  .head p { margin: 0; color: var(--mu); font-size: 13px; }
  .controls { display: flex; flex-wrap: wrap; gap: 10px; }
  .segmented { display: inline-flex; padding: 3px; border-radius: var(--r-pill); background: var(--pn); border: 1px solid var(--line); }
  .segmented button { border: 0; background: transparent; color: var(--mu); padding: 6px 13px; min-height: 32px; border-radius: var(--r-pill); font-size: 12.5px; font-weight: 600; cursor: pointer; }
  .segmented button:hover { color: var(--tx); }
  .segmented button[aria-pressed='true'] { background: var(--bg-3); color: var(--tx); box-shadow: 0 1px 0 var(--line-2) inset; }
  .kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 12px; }
  .skeleton span { height: 104px; border-radius: var(--r-md); background: var(--pn); animation: pulse 1.4s ease-in-out infinite; }
  .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 18px; }
  .card { background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-md); padding: 20px; min-width: 0; }
  .card h3 { margin: 0 0 4px; font-size: .98rem; }
  .note { margin: 0 0 16px; color: var(--mu); font-size: 12.5px; line-height: 1.5; }
  .split { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 22px; }
  .split h3 { margin-bottom: 14px; }
  .table-wrap { overflow: auto; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; color: var(--mu); font-weight: 500; font-size: 12px; }
  td, th { padding: 10px 12px; border-bottom: 1px solid var(--line); vertical-align: middle; }
  td:first-child, th:first-child { padding-left: 0; }
  tbody tr:last-child td { border-bottom: 0; }
  .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .code { display: inline-block; min-width: 64px; margin-right: 8px; color: var(--mu); font-variant-numeric: tabular-nums; font-size: 12px; }
  td.device { max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar { display: inline-block; width: 64px; height: 6px; margin-right: 8px; vertical-align: middle; border-radius: var(--r-pill); background: var(--pn); overflow: hidden; }
  .bar span { display: block; height: 100%; width: calc(var(--w) * 100%); background: var(--ok); }
  .status { display: inline-block; padding: 2px 8px; border-radius: var(--r-pill); font-size: 11.5px; font-weight: 600; background: var(--pn); color: var(--mu); }
  .status.completed { background: var(--ok-soft); color: var(--ok); }
  .status.active { background: var(--info-soft); color: var(--info); }
  .status.interrupted { background: var(--warn-soft); color: var(--warn); }
  .empty { margin: 0; padding: 36px 16px; text-align: center; color: var(--mu); border: 1px dashed var(--line-2); border-radius: var(--r-md); }
  .notice { margin: 0; padding: 14px; border-radius: var(--r-sm); border: 1px solid var(--line); background: var(--bg-2); }
  .error { color: var(--err); }
  .foot { margin: 0; color: var(--mu-2); font-size: 12px; line-height: 1.6; }
  @keyframes pulse { 50% { opacity: .5; } }
  @media (max-width: 900px) { .grid { grid-template-columns: 1fr; } }
  @media (max-width: 560px) { .split { grid-template-columns: 1fr; } .card { padding: 16px; } .kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); } .segmented { flex-wrap: wrap; } }
  @media (prefers-reduced-motion: reduce) { .skeleton span { animation: none; } }
</style>

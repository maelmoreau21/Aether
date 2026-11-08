<script lang="ts">
  import { adminRequest } from '../admin/request';
  import { t } from '../i18n';
  import Icon from '../ui/Icon.svelte';
  import {
    bestDetection, copySegment, effectiveSegment, formatClock, parseClock, seasonSummary, segmentKinds, segmentProblem,
    type SeasonSegments, type SegmentEpisode, type SegmentKind
  } from './segments';

  let { seasonId, seasonTitle }: { seasonId: string; seasonTitle: string } = $props();

  let data = $state<SeasonSegments | null>(null);
  let loading = $state(false), loadError = $state('');
  let notice = $state(''), actionError = $state('');
  let busy = $state(false), analysing = $state(false);
  let openAsset = $state<string | null>(null);
  let kind = $state<SegmentKind>('intro');
  let startText = $state(''), endText = $state('');
  let applyAll = $state(false);

  const kindLabel = (value: SegmentKind) => $t(`segment_kind_${value}`);
  const summary = $derived(data ? seasonSummary(data.episodes) : null);
  const current = $derived(data?.episodes.find(item => item.assetId === openAsset) ?? null);
  const start = $derived(parseClock(startText));
  const end = $derived(parseClock(endText));
  const problem = $derived(current ? segmentProblem(start, end, current.duration) : null);

  async function load() {
    const id = seasonId;
    loading = true; loadError = '';
    try {
      const result = await adminRequest<SeasonSegments>(`/api/v1/admin/seasons/${encodeURIComponent(id)}/markers`);
      if (id === seasonId) data = result;
    } catch (cause) { loadError = cause instanceof Error ? cause.message : $t('segment_load_failed'); }
    finally { loading = false; }
  }
  $effect(() => { void seasonId; data = null; openAsset = null; notice = ''; actionError = ''; void load(); });

  function fill(episode: SegmentEpisode, value: SegmentKind) {
    kind = value;
    const shown = effectiveSegment(episode, value);
    const detected = bestDetection(episode, value);
    const source = shown.source === 'manual' || shown.source === 'automatic' ? shown : detected;
    startText = source ? formatClock(source.start, true) : '';
    endText = source ? formatClock(source.end, true) : '';
    actionError = '';
  }
  function open(episode: SegmentEpisode) {
    if (openAsset === episode.assetId) { openAsset = null; return; }
    openAsset = episode.assetId; applyAll = false; notice = '';
    fill(episode, kind);
  }
  function nudge(field: 'start' | 'end', delta: number) {
    const value = (field === 'start' ? start : end) ?? 0;
    const next = formatClock(Math.max(0, Math.round((value + delta) * 10) / 10), true);
    if (field === 'start') startText = next; else endText = next;
  }
  function useChapter(at: number, field: 'start' | 'end') {
    if (field === 'start') startText = formatClock(at, true); else endText = formatClock(at, true);
  }

  async function send(assetId: string, body: unknown | null) {
    await adminRequest(`/api/v1/admin/assets/${encodeURIComponent(assetId)}/markers/${kind}`, body === null ? { method: 'DELETE' } : { method: 'PUT', body: JSON.stringify(body) });
  }
  async function run(action: () => Promise<string>) {
    if (busy) return;
    busy = true; actionError = ''; notice = '';
    try { notice = await action(); await load(); if (current) fill(current, kind); }
    catch (cause) { actionError = cause instanceof Error ? cause.message : $t('segment_save_failed'); }
    finally { busy = false; }
  }
  function save() {
    const episode = current;
    if (!episode || problem || start === null || end === null) return;
    void run(async () => {
      await send(episode.assetId, { enabled: true, start, end });
      if (!applyAll || !data) return $t('segment_saved');
      let copied = 0, skipped = 0;
      for (const other of data.episodes) {
        if (other.assetId === episode.assetId) continue;
        const copy = copySegment(kind, start, end, episode.duration, other.duration);
        if (!copy) { skipped++; continue; }
        await send(other.assetId, { enabled: true, ...copy });
        copied++;
      }
      return $t('segment_copied', { copied, skipped });
    });
  }
  function disable() {
    const episode = current;
    if (episode) void run(async () => { await send(episode.assetId, { enabled: false }); return $t('segment_disabled_saved'); });
  }
  function reset() {
    const episode = current;
    if (episode) void run(async () => { await send(episode.assetId, null); return $t('segment_reset_done'); });
  }
  async function analyse() {
    if (analysing) return;
    analysing = true; actionError = ''; notice = '';
    try {
      const result = await adminRequest<{ episodesCount: number; tasksQueued: number }>(`/api/v1/admin/seasons/${encodeURIComponent(seasonId)}/analyze-markers`, { method: 'POST' });
      notice = $t('intro_analysis_queued', { episodes: result.episodesCount, queued: result.tasksQueued });
      await load();
    } catch (cause) { actionError = cause instanceof Error ? cause.message : $t('analysis_unavailable'); }
    finally { analysing = false; }
  }

  function statusLabel(episode: SegmentEpisode): { text: string; tone: 'ok' | 'wait' | 'err' | 'mute'; detail?: string } {
    if (!episode.detectionEnabled) return { text: $t('segment_status_detection_off'), tone: 'mute' };
    const { fingerprint: fp, detection: det } = episode;
    if (det.status === 'error') return { text: $t('segment_status_detection_failed'), tone: 'err', detail: det.error ?? undefined };
    if (fp.status === 'error') return { text: $t('segment_status_audio_failed'), tone: 'err', detail: fp.error ?? undefined };
    if (['queued', 'paused'].includes(fp.status) || ['queued', 'paused'].includes(det.status)) return { text: $t('segment_status_queued'), tone: 'wait' };
    if (fp.status === 'running' || det.status === 'running') return { text: $t('segment_status_running'), tone: 'wait' };
    if (fp.status === 'completed' || det.status === 'completed') return { text: $t('segment_status_analysed'), tone: 'ok' };
    return { text: $t('segment_status_not_analysed'), tone: 'mute' };
  }
  const pct = (value: number, duration: number | null) => duration ? `${Math.min(100, Math.max(0, value / duration * 100))}%` : '0%';
</script>

<section class="segments" aria-labelledby="segments-title">
  <header class="seg-head">
    <div>
      <h2 id="segments-title">{$t('segments_title')}</h2>
      <p class="lead">{$t('segments_lead', { season: seasonTitle })}</p>
    </div>
    <div class="seg-actions">
      <button type="button" class="btn" disabled={loading} onclick={() => void load()}><Icon name="refresh" size={15} />{$t('segment_refresh')}</button>
      <button type="button" class="btn primary" disabled={analysing} onclick={analyse}><Icon name="search" size={15} />{analysing ? $t('analysing') : $t('segment_analyse_season')}</button>
    </div>
  </header>

  {#if data && data.providers.length === 0}
    <p class="banner warn" role="status">{$t('segment_no_provider')}</p>
  {:else if data && !data.providers.some(provider => provider.enabled)}
    <p class="banner warn" role="status">{$t('segment_provider_disabled')}</p>
  {/if}
  {#if summary && summary.total}
    <ul class="seg-summary" aria-label={$t('segment_summary')}>
      {#each segmentKinds as value (value)}<li><strong>{summary.covered[value]}/{summary.total}</strong> {kindLabel(value)}</li>{/each}
      {#if summary.pending}<li class="wait">{$t('segment_summary_pending', { count: summary.pending })}</li>{/if}
      {#if summary.failed}<li class="err">{$t('segment_summary_failed', { count: summary.failed })}</li>{/if}
      {#if summary.weak}<li class="mute">{$t('segment_summary_weak', { count: summary.weak })}</li>{/if}
    </ul>
  {/if}
  {#if notice}<p class="banner ok" role="status">{notice}</p>{/if}
  {#if actionError}<p class="banner err" role="alert">{actionError}</p>{/if}
  {#if loadError}<p class="banner err" role="alert">{loadError}</p>{/if}
  {#if loading && !data}<p role="status">{$t('segment_loading')}</p>{/if}

  {#if data}
    <ol class="seg-list">
      {#each data.episodes as episode (episode.assetId)}
        {@const status = statusLabel(episode)}
        {@const expanded = openAsset === episode.assetId}
        <li class="seg-row" class:open={expanded}>
          <button type="button" class="seg-line" aria-expanded={expanded} onclick={() => open(episode)}>
            <span class="ep-no">{episode.episodeNumber}</span>
            <span class="ep-main">
              <span class="ep-title">{episode.title}</span>
              <span class="timeline" aria-hidden="true">
                {#each segmentKinds as value (value)}
                  {@const shown = effectiveSegment(episode, value)}
                  {#if (shown.source === 'manual' || shown.source === 'automatic') && episode.duration}
                    <span class="bar k-{value}" class:manual={shown.source === 'manual'} style:left={pct(shown.start, episode.duration)} style:width={pct(shown.end - shown.start, episode.duration)}></span>
                  {/if}
                {/each}
              </span>
            </span>
            <span class="chips">
              {#each segmentKinds as value (value)}
                {@const shown = effectiveSegment(episode, value)}
                {#if shown.source === 'manual' || shown.source === 'automatic'}
                  <span class="chip k-{value}" title={shown.source === 'automatic' ? $t('segment_auto_title', { confidence: Math.round(shown.confidence * 100) }) : $t('segment_manual_title')}>
                    {kindLabel(value)} {formatClock(shown.start)}–{formatClock(shown.end)}{#if shown.source === 'manual'} ✎{/if}
                  </span>
                {:else if shown.source === 'disabled'}<span class="chip off">{kindLabel(value)} : {$t('segment_none')}</span>{/if}
              {/each}
            </span>
            <span class="state {status.tone}" title={status.detail}>{status.text}</span>
            <span class="caret" class:up={expanded}><Icon name="chevron" size={16} /></span>
          </button>

          {#if expanded}
            <div class="editor">
              <div class="kinds" role="radiogroup" aria-label={$t('segment_kind')}>
                {#each segmentKinds as value (value)}
                  <button type="button" role="radio" aria-checked={kind === value} class="kind k-{value}" onclick={() => fill(episode, value)}>{kindLabel(value)}</button>
                {/each}
              </div>
              {#if status.detail}<p class="banner err">{status.detail}</p>{/if}
              <div class="fields">
                {#each [['start', startText], ['end', endText]] as [field] (field)}
                  <div class="field">
                    <label for="seg-{field}">{field === 'start' ? $t('segment_start') : $t('segment_end')}</label>
                    <div class="time">
                      <button type="button" class="nudge" aria-label={$t('segment_minus', { seconds: 5 })} onclick={() => nudge(field as 'start' | 'end', -5)}>−5</button>
                      <button type="button" class="nudge" aria-label={$t('segment_minus', { seconds: 1 })} onclick={() => nudge(field as 'start' | 'end', -1)}>−1</button>
                      {#if field === 'start'}
                        <input id="seg-start" inputmode="decimal" placeholder="0:00" bind:value={startText} aria-invalid={start === null} />
                      {:else}
                        <input id="seg-end" inputmode="decimal" placeholder="1:30" bind:value={endText} aria-invalid={end === null} />
                      {/if}
                      <button type="button" class="nudge" aria-label={$t('segment_plus', { seconds: 1 })} onclick={() => nudge(field as 'start' | 'end', 1)}>+1</button>
                      <button type="button" class="nudge" aria-label={$t('segment_plus', { seconds: 5 })} onclick={() => nudge(field as 'start' | 'end', 5)}>+5</button>
                    </div>
                  </div>
                {/each}
                <p class="hint">{$t('segment_time_hint', { duration: formatClock(episode.duration) })}{#if start !== null && end !== null && end > start} · {$t('segment_length', { length: formatClock(end - start, true) })}{/if}</p>
                {#if problem && startText && endText}<p class="banner err" role="alert">{$t(problem, { duration: formatClock(episode.duration) })}</p>{/if}
              </div>

              {#if episode.chapters.length}
                <details class="chapters">
                  <summary>{$t('segment_chapters', { count: episode.chapters.length })}</summary>
                  <ul>
                    {#each episode.chapters as chapter (chapter.start)}
                      <li><span>{formatClock(chapter.start)} · {chapter.title || '—'}</span>
                        <button type="button" class="link" onclick={() => useChapter(chapter.start, 'start')}>{$t('segment_use_as_start')}</button>
                        <button type="button" class="link" onclick={() => useChapter(chapter.start, 'end')}>{$t('segment_use_as_end')}</button></li>
                    {/each}
                  </ul>
                </details>
              {/if}

              {#if episode.automatic.some(item => item.type === kind)}
                <ul class="detections" aria-label={$t('segment_detections')}>
                  {#each episode.automatic.filter(item => item.type === kind) as item (item.extensionId)}
                    <li>
                      <span>{formatClock(item.start, true)}–{formatClock(item.end, true)} · {item.extensionId} · {$t('segment_confidence', { confidence: Math.round(item.confidence * 100) })}</span>
                      {#if !item.usable}<span class="state mute">{$t('segment_below_floor', { floor: Math.round(episode.minConfidence * 100) })}</span>{/if}
                      <button type="button" class="link" onclick={() => { startText = formatClock(item.start, true); endText = formatClock(item.end, true); }}>{$t('segment_use_detection')}</button>
                    </li>
                  {/each}
                </ul>
              {/if}

              <label class="check"><input type="checkbox" bind:checked={applyAll} /> {kind === 'credits' ? $t('segment_apply_all_credits') : $t('segment_apply_all')}</label>
              <div class="editor-actions">
                <button type="button" class="btn primary" disabled={busy || !!problem} onclick={save}>{busy ? $t('saving') : $t('segment_save')}</button>
                <button type="button" class="btn" disabled={busy} onclick={disable}>{$t('segment_mark_none', { kind: kindLabel(kind) })}</button>
                {#if episode.manual.some(item => item.type === kind)}
                  <button type="button" class="btn" disabled={busy} onclick={reset}>{$t('segment_reset')}</button>
                {/if}
              </div>
            </div>
          {/if}
        </li>
      {:else}
        <li class="empty">{$t('segment_no_episode')}</li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .segments { display: flex; flex-direction: column; gap: 14px; }
  .seg-head { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: flex-end; gap: 12px; }
  h2 { margin: 0; font-size: 22px; font-weight: 800; color: var(--tx); }
  .lead { margin: 4px 0 0; color: var(--mu); font-size: 14px; max-width: 70ch; }
  .seg-actions, .editor-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .btn { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; padding: 0 14px; border: 0; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }
  .btn:hover:not(:disabled) { background: var(--pn-3); }
  .btn.primary { background: var(--btn); color: var(--btn-tx); }
  .btn:disabled { opacity: .6; cursor: default; }
  .banner { margin: 0; padding: 10px 14px; border-radius: var(--r-sm); background: var(--pn); font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .banner.err { color: var(--err); } .banner.ok { color: var(--ac); } .banner.warn { color: var(--tx); border-left: 3px solid var(--ac); }
  .seg-summary { display: flex; flex-wrap: wrap; gap: 8px; margin: 0; padding: 0; list-style: none; font-size: 13px; color: var(--mu); }
  .seg-summary li { padding: 6px 10px; border-radius: var(--r-pill); background: var(--pn); }
  .seg-summary strong { color: var(--tx); }
  .seg-list { display: flex; flex-direction: column; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .seg-row { border-radius: var(--r-md); background: var(--pn); }
  .seg-row.open { background: var(--pn-2); }
  .seg-line { display: grid; grid-template-columns: 36px minmax(0, 1fr) auto auto 16px; align-items: center; gap: 12px; width: 100%; padding: 10px 12px; border: 0; background: none; color: var(--tx); font: inherit; text-align: left; cursor: pointer; }
  .ep-no { font-weight: 800; color: var(--mu); text-align: center; }
  .ep-main { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .ep-title { font-weight: 700; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .timeline { position: relative; height: 6px; border-radius: 3px; background: var(--pn-3); overflow: hidden; }
  .bar { position: absolute; top: 0; bottom: 0; border-radius: 3px; opacity: .85; }
  .bar.manual { outline: 1px solid var(--tx); }
  .k-intro { --seg: var(--ac); } .k-credits { --seg: #a78bfa; } .k-recap { --seg: #f59e0b; } .k-preview { --seg: #34d399; }
  .bar { background: var(--seg); }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; justify-content: flex-end; }
  .chip { padding: 3px 8px; border-radius: var(--r-pill); font-size: 12px; font-weight: 700; background: color-mix(in srgb, var(--seg, var(--mu)) 18%, transparent); white-space: nowrap; }
  .chip.off { color: var(--mu); }
  .state { font-size: 12px; font-weight: 700; white-space: nowrap; }
  .state.ok { color: var(--ac); } .state.err { color: var(--err); } .state.wait { color: var(--tx); } .state.mute { color: var(--mu); }
  .editor { display: flex; flex-direction: column; gap: 12px; padding: 4px 16px 16px; }
  .kinds { display: flex; flex-wrap: wrap; gap: 6px; }
  .kind { min-height: 36px; padding: 0 14px; border: 1px solid var(--line-2); border-radius: var(--r-pill); background: none; color: var(--tx); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }
  .kind[aria-checked='true'] { border-color: var(--seg); background: color-mix(in srgb, var(--seg) 20%, transparent); }
  .fields { display: flex; flex-wrap: wrap; gap: 12px 24px; align-items: flex-end; }
  .field { display: flex; flex-direction: column; gap: 6px; }
  .field label { font-size: 13px; font-weight: 700; color: var(--mu); }
  .time { display: flex; align-items: center; gap: 4px; }
  .time input { width: 96px; min-height: 40px; padding: 0 10px; border: 1px solid var(--line-2); border-radius: var(--r-sm); background: var(--bg); color: var(--tx); font: inherit; font-variant-numeric: tabular-nums; text-align: center; }
  .time input[aria-invalid='true'] { border-color: var(--err); }
  .nudge { min-width: 36px; min-height: 40px; border: 0; border-radius: var(--r-sm); background: var(--pn-3); color: var(--tx); font: inherit; font-size: 12px; font-weight: 700; cursor: pointer; }
  .hint { flex-basis: 100%; margin: 0; font-size: 13px; color: var(--mu); }
  .chapters summary { cursor: pointer; font-size: 13px; font-weight: 700; color: var(--mu); }
  .chapters ul, .detections { display: flex; flex-direction: column; gap: 4px; margin: 6px 0 0; padding: 0; list-style: none; font-size: 13px; }
  .chapters li, .detections li { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .link { border: 0; background: none; color: var(--ac); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; padding: 4px; }
  .check { display: flex; align-items: center; gap: 8px; font-size: 14px; }
  .caret { display: inline-flex; transition: transform var(--t-fast) ease; } .caret.up { transform: rotate(180deg); }
  .empty { padding: 24px; color: var(--mu); text-align: center; }
  @media (max-width: 760px) {
    .seg-line { grid-template-columns: 28px minmax(0, 1fr) 16px; }
    .chips, .state { grid-column: 2 / 3; justify-content: flex-start; }
    .editor { padding: 4px 10px 14px; }
  }
</style>

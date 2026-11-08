<script lang="ts">
  import { untrack } from 'svelte';
  import { adminRequest } from '../admin/request';
  import { t } from '../i18n';
  import Icon from '../ui/Icon.svelte';
  import MediaSegments from './MediaSegments.svelte';
  import SegmentEditPanel from './SegmentEditPanel.svelte';
  import SegmentSteps from './SegmentSteps.svelte';
  import {
    SEASON_MARKERS_LIMIT, analysisActive, analysisState, effectiveSegment, formatClock, pollWhile, seasonSummary, segmentKinds,
    type SeasonSegments, type SegmentEpisode, type SegmentKind
  } from './segments';

  let { seasonId, seasonTitle, focusWork = null, focusAsset = null, focusTitle = '' }: {
    seasonId: string; seasonTitle: string;
    /** Opened from an episode's shortcut: that episode (its work, and the file played). */
    focusWork?: string | null; focusAsset?: string | null; focusTitle?: string;
  } = $props();

  let data = $state<SeasonSegments | null>(null);
  let loading = $state(false), loadError = $state('');
  let notice = $state(''), actionError = $state('');
  let analysing = $state(false);
  let openAsset = $state<string | null>(null);
  // Analysis of the open episode alone.
  let rowAnalysing = $state(false), rowNotice = $state(''), rowError = $state('');
  // The focused episode is unfolded once; when the season list does not include it (unavailable
  // file, or past the list's limit), its own editor is shown instead.
  let focusDone = false;
  let focusMissing = $state(false);
  let stopPolling: (() => void) | null = null;

  const kindLabel = (value: SegmentKind) => $t(`segment_kind_${value}`);
  const summary = $derived(data ? seasonSummary(data.episodes) : null);
  const providerOn = $derived(data?.providers.some(provider => provider.enabled) ?? false);

  async function load() {
    const id = seasonId;
    loading = true; loadError = '';
    try {
      const result = await adminRequest<SeasonSegments>(`/api/v1/admin/seasons/${encodeURIComponent(id)}/markers`);
      if (id === seasonId) {
        data = result;
        if (focusWork && !focusDone) {
          focusDone = true;
          const focused = result.episodes.find(item => item.workId === focusWork || item.assetId === focusAsset);
          if (focused) {
            openAsset = focused.assetId;
            requestAnimationFrame(() => document.getElementById(`segment-row-${focused.assetId}`)?.scrollIntoView({ block: 'center' }));
          } else focusMissing = true;
        }
      }
    } catch (cause) { loadError = cause instanceof Error ? cause.message : $t('segment_load_failed'); }
    finally { loading = false; }
  }
  function stopWatching() { stopPolling?.(); stopPolling = null; }
  $effect(() => {
    void seasonId;
    untrack(() => { stopWatching(); data = null; openAsset = null; focusMissing = false; notice = ''; actionError = ''; rowNotice = ''; rowError = ''; void load(); });
    return stopWatching;
  });

  // After an analysis request, refresh every few seconds while an episode's analysis waits or runs.
  function watch() {
    stopWatching();
    stopPolling = pollWhile(async () => {
      await load();
      return data ? data.episodes.some(episode => analysisActive(episode)) : false;
    });
  }

  function open(episode: SegmentEpisode) {
    openAsset = openAsset === episode.assetId ? null : episode.assetId;
    rowNotice = ''; rowError = '';
  }
  async function analyse() {
    if (analysing) return;
    analysing = true; actionError = ''; notice = '';
    try {
      const result = await adminRequest<{ episodesCount: number; tasksQueued: number }>(`/api/v1/admin/seasons/${encodeURIComponent(seasonId)}/analyze-markers`, { method: 'POST' });
      notice = $t('intro_analysis_queued', { episodes: result.episodesCount, queued: result.tasksQueued });
      await load();
      watch();
    } catch (cause) { actionError = cause instanceof Error ? cause.message : $t('analysis_unavailable'); }
    finally { analysing = false; }
  }
  // Reads the episode's chapters again and fingerprints it (and peers still missing one).
  async function analyseEpisode(episode: SegmentEpisode) {
    if (rowAnalysing) return;
    rowAnalysing = true; rowNotice = ''; rowError = '';
    try {
      await adminRequest(`/api/v1/admin/assets/${encodeURIComponent(episode.assetId)}/analyze-markers`, { method: 'POST' });
      rowNotice = providerOn ? $t('segment_analysis_queued_episode') : $t('segment_analysis_no_provider');
      await load();
      watch();
    } catch (cause) { rowError = cause instanceof Error ? cause.message : $t('analysis_unavailable'); }
    finally { rowAnalysing = false; }
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
  {#if data && data.episodes.length >= SEASON_MARKERS_LIMIT}
    <p class="banner warn" role="status">{$t('segment_list_truncated', { count: SEASON_MARKERS_LIMIT })}</p>
  {/if}
  {#if focusMissing}
    <p class="banner warn" role="status">{$t('segment_focus_not_listed')}</p>
    {#if focusAsset}<div class="focus-media"><MediaSegments assetId={focusAsset} title={focusTitle} /></div>{/if}
  {/if}

  {#if data}
    <ol class="seg-list">
      {#each data.episodes as episode (episode.assetId)}
        {@const status = analysisState(episode)}
        {@const expanded = openAsset === episode.assetId}
        <li class="seg-row" class:open={expanded} id="segment-row-{episode.assetId}">
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
            <span class="state {status.tone}" title={status.detail}>{$t(status.key)}</span>
            <span class="caret" class:up={expanded}><Icon name="chevron" size={16} /></span>
          </button>

          {#if expanded}
            <div class="editor-wrap">
              <div class="row-status">
                <SegmentSteps {episode} />
                <button type="button" class="btn" disabled={rowAnalysing} onclick={() => analyseEpisode(episode)}><Icon name="search" size={15} />{rowAnalysing ? $t('analysing') : $t('segment_analyse_media')}</button>
              </div>
              {#if rowNotice}<p class="banner ok" role="status">{rowNotice}</p>{/if}
              {#if rowError}<p class="banner err" role="alert">{rowError}</p>{/if}
              <SegmentEditPanel {episode} siblings={data.episodes} onChanged={load} />
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
  .seg-actions { display: flex; flex-wrap: wrap; gap: 8px; }
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
  .editor-wrap { display: flex; flex-direction: column; gap: 12px; padding: 4px 16px 16px; }
  .row-status { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: flex-start; gap: 10px 16px; padding-bottom: 12px; border-bottom: 1px solid var(--line-2); }
  .focus-media { padding: 14px 16px; border-radius: var(--r-md); background: var(--pn-2); }
  .caret { display: inline-flex; transition: transform var(--t-fast) ease; } .caret.up { transform: rotate(180deg); }
  .empty { padding: 24px; color: var(--mu); text-align: center; }
  @media (max-width: 760px) {
    .seg-line { grid-template-columns: 28px minmax(0, 1fr) 16px; }
    .chips, .state { grid-column: 2 / 3; justify-content: flex-start; }
    .editor-wrap { padding: 4px 10px 14px; }
  }
</style>

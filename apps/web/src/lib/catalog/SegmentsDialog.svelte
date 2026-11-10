<script lang="ts">
  import { onMount } from 'svelte';
  import { localizedTitle, playbackIds, type Work } from './api';
  import { locale, t } from '../i18n';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from '../admin/request';
  import MediaSegments from './MediaSegments.svelte';
  import SegmentEditor from './SegmentEditor.svelte';

  // « Intros & génériques » opened from the media options menu (⋯): the file of a film or an
  // episode, or a series season by season (with the copy to the whole season).
  let { work, assetId = null, season = null, focus = null, onclose }: {
    work: Work;
    /** File of a film or an episode; the work's first file when absent. */
    assetId?: string | null;
    /** Series: season shown first. */
    season?: number | null;
    /** Series: episode opened from its shortcut. */
    focus?: { work: string; asset: string | null; title: string } | null;
    onclose: () => void;
  } = $props();

  const series = $derived(work.kind === 'series');
  const fileId = $derived(series ? null : assetId ?? playbackIds(work)[0] ?? null);
  // svelte-ignore state_referenced_locally
  let seasonNumber = $state(season ?? work.seasons?.[0]?.number ?? 1);
  const activeSeason = $derived(work.seasons?.find(item => item.number === seasonNumber) ?? work.seasons?.[0]);
  const seasonLabel = (item?: { number: number; title: string }) =>
    !item ? '' : item.title || (item.number === 0 ? $t('segment_specials') : $t('segment_season_number', { number: item.number }));

  // Series: the detection runs again on every episode (the audio fingerprints already
  // extracted are reused; a correction is never replaced).
  let seriesBusy = $state(false), seriesNotice = $state(''), seriesError = $state('');
  async function rerunSeries() {
    if (seriesBusy) return;
    seriesBusy = true; seriesNotice = ''; seriesError = '';
    try {
      const result = await adminRequest<{ episodesCount: number; tasksQueued: number }>(`/api/v1/admin/works/${encodeURIComponent(work.id)}/analyze-markers`, { method: 'POST' });
      seriesNotice = $t('intro_analysis_queued', { episodes: result.episodesCount, queued: result.tasksQueued });
    } catch (cause) { seriesError = cause instanceof Error ? cause.message : $t('analysis_unavailable'); }
    finally { seriesBusy = false; }
  }

  let card = $state<HTMLDivElement | null>(null);
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    card?.querySelector<HTMLElement>('.close-btn')?.focus();
    return () => previous?.focus?.();
  });
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.stopPropagation(); onclose(); }
  }
</script>

<div class="overlay" role="dialog" aria-modal="true" aria-labelledby="segments-dialog-title" tabindex="-1" onkeydown={keydown}>
  <button type="button" class="backdrop" aria-label={$t('close_the_window')} tabindex="-1" onclick={onclose}></button>
  <div class="card" bind:this={card}>
    <header class="head">
      <div class="title">
        <Icon name="film" size={20} />
        <div>
          <h2 id="segments-dialog-title">{$t('segments_tab')}</h2>
          <p>{localizedTitle(work, $locale)}</p>
        </div>
      </div>
      <button type="button" class="close-btn" aria-label={$t('close')} onclick={onclose}><Icon name="close" size={18} /></button>
    </header>
    <div class="body">
      {#if series}
        <div class="series-bar">
          <button type="button" class="btn" disabled={seriesBusy} onclick={rerunSeries}><Icon name="search" size={15} />{seriesBusy ? $t('analysing') : $t('media_rerun_intro_series')}</button>
          {#if seriesNotice}<span class="series-note" role="status">{seriesNotice}</span>{/if}
          {#if seriesError}<span class="series-note err" role="alert">{seriesError}</span>{/if}
        </div>
        {#if activeSeason}
          {#if (work.seasons?.length ?? 0) > 1}
            <label class="season">{$t('segment_season')}
              <select bind:value={seasonNumber}>
                {#each work.seasons ?? [] as item (item.id)}<option value={item.number}>{seasonLabel(item)}</option>{/each}
              </select>
            </label>
          {/if}
          {#key activeSeason.id}
            <SegmentEditor seasonId={activeSeason.id} seasonTitle={seasonLabel(activeSeason)} focusWork={focus?.work ?? null} focusAsset={focus?.asset ?? null} focusTitle={focus?.title ?? ''} />
          {/key}
        {:else}
          <p class="empty">{$t('no_episodes_available_for_this_season')}</p>
        {/if}
      {:else if fileId}
        {#key fileId}<MediaSegments assetId={fileId} title={localizedTitle(work, $locale)} />{/key}
      {:else}
        <p class="empty">{$t('no_indexed_file_is_available_for')}</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; z-index: 2000; display: grid; place-items: center; padding: 24px; outline: none; }
  .backdrop { position: absolute; inset: 0; border: 0; background: rgb(6 8 14 / 0.55); backdrop-filter: blur(4px); -webkit-backdrop-filter: blur(4px); cursor: default; }
  .card {
    position: relative; z-index: 1; display: flex; flex-direction: column;
    width: min(980px, 100%); max-height: calc(100dvh - 48px);
    border: 1px solid var(--line); border-radius: var(--r-xl); background: var(--bg-3); color: var(--tx); box-shadow: var(--shadow);
  }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 22px 28px 12px; }
  .title { display: flex; align-items: center; gap: 12px; min-width: 0; color: var(--mu); }
  .title > div { min-width: 0; }
  h2 { margin: 0; font-size: 22px; font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  .title p { margin: 2px 0 0; font-size: 14px; color: var(--mu); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .close-btn { flex: none; display: grid; place-items: center; width: 44px; height: 44px; padding: 0; border: 0; border-radius: 14px; background: var(--pn); color: var(--tx); cursor: pointer; }
  .close-btn:hover { background: var(--pn-2); }
  .close-btn:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  .body { display: flex; flex-direction: column; gap: 14px; min-height: 0; padding: 8px 28px 28px; overflow-y: auto; overscroll-behavior: contain; }
  /* The editor's own heading repeats the dialog title: keep its lead and actions only. */
  .body :global(.media-segments h2) { display: none; }
  .season { display: inline-flex; align-items: center; gap: 10px; font-size: 14px; font-weight: 700; }
  .season select { min-height: 40px; padding: 0 12px; border: 1px solid var(--line-2); border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; }
  .empty { margin: 0; color: var(--mu); }
  .series-bar { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 14px; }
  .btn { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; padding: 0 14px; border: 0; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }
  .btn:hover:not(:disabled) { background: var(--pn-3); }
  .btn:disabled { opacity: .6; cursor: default; }
  .series-note { font-size: 13px; color: var(--ac); } .series-note.err { color: var(--err); }
  @media (max-width: 639px) {
    .overlay { padding: 0; place-items: stretch; }
    .card { width: 100%; max-height: 100dvh; height: 100dvh; border: 0; border-radius: 0; }
    .head { padding: calc(16px + env(safe-area-inset-top, 0px)) 16px 8px; }
    .body { padding: 8px 16px calc(20px + env(safe-area-inset-bottom, 0px)); }
  }
</style>

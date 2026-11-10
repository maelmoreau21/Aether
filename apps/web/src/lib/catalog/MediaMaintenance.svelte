<script lang="ts">
  import { untrack } from 'svelte';
  import { adminRequest } from '../admin/request';
  import { t } from '../i18n';
  import Icon from '../ui/Icon.svelte';
  import { chaptersLabel, mediaStatusActive, segmentsLabel, trickplayLabel, type MediaStatus } from './media-status';
  import { pollWhile } from './segments';

  // What was generated for one file, and the actions that rebuild it. The state is read from
  // the Core (not guessed): a thumbnail sheet is "generated" only when it is really prepared.
  let { assetId, onSegmentsChanged }: { assetId: string; onSegmentsChanged?: () => Promise<void> | void } = $props();

  let status = $state<MediaStatus | null>(null);
  let loadError = $state(''), notice = $state(''), actionError = $state('');
  let busy = $state<'' | 'trickplay' | 'intro' | 'chapters'>('');
  let stopPolling: (() => void) | null = null;

  async function load() {
    const id = assetId;
    try {
      const result = await adminRequest<MediaStatus>(`/api/v1/admin/assets/${encodeURIComponent(id)}/media-status`);
      if (id === assetId) { status = result; loadError = ''; }
    } catch (cause) { loadError = cause instanceof Error ? cause.message : $t('media_status_unavailable'); }
  }
  function stopWatching() { stopPolling?.(); stopPolling = null; }
  function watch() {
    stopWatching();
    stopPolling = pollWhile(async () => { await load(); return status ? mediaStatusActive(status) : false; });
  }
  $effect(() => {
    void assetId;
    untrack(() => { stopWatching(); status = null; notice = ''; actionError = ''; void load().then(() => { if (status && mediaStatusActive(status)) watch(); }); });
    return stopWatching;
  });

  async function run(action: 'trickplay' | 'intro' | 'chapters', path: string, done: (result: Record<string, unknown>) => string) {
    if (busy) return;
    busy = action; notice = ''; actionError = '';
    try {
      const result = await adminRequest<Record<string, unknown>>(`/api/v1/admin/assets/${encodeURIComponent(assetId)}/${path}`, { method: 'POST' });
      notice = done(result ?? {});
      await load();
      if (action !== 'trickplay') await onSegmentsChanged?.();
      watch();
    } catch (cause) { actionError = cause instanceof Error ? cause.message : $t('operation_failed'); }
    finally { busy = ''; }
  }
  // The only way to release locked corrections: the detections apply again (an administrator's
  // explicit choice, confirmed first because the corrections are deleted).
  async function resetToAuto() {
    if (busy || !status || !window.confirm($t('media_reset_auto_confirm', { count: status.segments.locked }))) return;
    busy = 'intro'; notice = ''; actionError = '';
    try {
      await adminRequest(`/api/v1/admin/assets/${encodeURIComponent(assetId)}/markers`, { method: 'DELETE' });
      notice = $t('media_reset_auto_done');
      await load();
      await onSegmentsChanged?.();
    } catch (cause) { actionError = cause instanceof Error ? cause.message : $t('operation_failed'); }
    finally { busy = ''; }
  }
  const regenerateTrickplay = () => run('trickplay', 'regenerate-trickplay', result =>
    result.purged === false && status && !status.trickplay.canPurge ? $t('media_trickplay_queued_remote') : $t('media_trickplay_queued'));
  const rerunIntro = () => run('intro', 'analyze-markers', () => $t('media_intro_queued'));
  const refreshChapters = () => run('chapters', 'refresh-chapters', () => $t('media_chapters_queued'));

  const trickplay = $derived(status ? trickplayLabel(status.trickplay) : null);
  const segments = $derived(status ? segmentsLabel(status.segments) : null);
  const chapters = $derived(status ? chaptersLabel(status.chapters) : null);
</script>

<section class="maintenance" aria-labelledby="media-tools-title">
  <h3 id="media-tools-title">{$t('media_tools_title')}</h3>
  {#if loadError}<p class="banner err" role="alert">{loadError}</p>{/if}
  {#if status && trickplay && segments && chapters}
    <ul class="rows">
      <li>
        <div class="what"><strong>{$t('media_tools_trickplay')}</strong><span class="state" class:ok={status.trickplay.status === 'generated'} class:err={status.trickplay.status === 'failed'}>{$t(trickplay.key, trickplay.params)}</span></div>
        <button type="button" class="btn" disabled={!!busy || !status.trickplay.enabled} onclick={regenerateTrickplay}><Icon name="refresh" size={15} />{busy === 'trickplay' ? $t('media_tools_working') : $t('media_regenerate_trickplay')}</button>
      </li>
      <li>
        <div class="what"><strong>{$t('media_tools_intro')}</strong><span class="state" class:ok={status.segments.locked === 0 && status.segments.automatic > 0}>{$t(segments.key, segments.params)}</span></div>
        <button type="button" class="btn" disabled={!!busy || !status.segments.detectionEnabled} onclick={rerunIntro}><Icon name="search" size={15} />{busy === 'intro' ? $t('media_tools_working') : $t('media_rerun_intro')}</button>
      </li>
      <li>
        <div class="what"><strong>{$t('media_tools_chapters')}</strong><span class="state">{$t(chapters.key, chapters.params)}{#if status.chapters.images === 'running'} · {$t('media_chapter_images_running')}{/if}</span></div>
        <button type="button" class="btn" disabled={!!busy} onclick={refreshChapters}><Icon name="refresh" size={15} />{busy === 'chapters' ? $t('media_tools_working') : $t('media_refresh_chapters')}</button>
      </li>
    </ul>
    {#if status.segments.locked > 0}
      <p class="hint">{$t('media_segments_locked_hint')}</p>
      <div><button type="button" class="btn" disabled={!!busy} onclick={resetToAuto}>{$t('media_reset_auto')}</button></div>
    {/if}
  {/if}
  {#if notice}<p class="banner ok" role="status">{notice}</p>{/if}
  {#if actionError}<p class="banner err" role="alert">{actionError}</p>{/if}
</section>

<style>
  .maintenance { display: flex; flex-direction: column; gap: 10px; padding: 14px 16px; border-radius: var(--r-md); background: var(--pn); }
  h3 { margin: 0; font-size: 16px; font-weight: 800; color: var(--tx); }
  .rows { display: flex; flex-direction: column; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .rows li { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px 16px; }
  .what { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .state { font-size: 13px; color: var(--mu); }
  .state.ok { color: var(--ac); } .state.err { color: var(--err); }
  .btn { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; padding: 0 14px; border: 0; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }
  .btn:hover:not(:disabled) { background: var(--pn-3); }
  .btn:disabled { opacity: .6; cursor: default; }
  .hint { margin: 0; font-size: 13px; color: var(--mu); }
  .banner { margin: 0; padding: 10px 14px; border-radius: var(--r-sm); background: var(--pn-2); font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .banner.err { color: var(--err); } .banner.ok { color: var(--ac); }
</style>

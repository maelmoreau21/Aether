<script lang="ts">
  import { untrack } from 'svelte';
  import { adminRequest } from '../admin/request';
  import { t } from '../i18n';
  import Icon from '../ui/Icon.svelte';
  import MediaMaintenance from './MediaMaintenance.svelte';
  import SegmentEditPanel from './SegmentEditPanel.svelte';
  import SegmentSteps from './SegmentSteps.svelte';
  import { analysisActive, analysisState, formatClock, pollWhile, type AssetSegments } from './segments';

  // Segments of one file (a film, or one episode): where its analysis stands, step by step,
  // and the editor to correct what was detected.
  let { assetId, title = '' }: { assetId: string; title?: string } = $props();

  let data = $state<AssetSegments | null>(null);
  let loading = $state(false), loadError = $state('');
  let analysing = $state(false), notice = $state(''), actionError = $state('');
  let stopPolling: (() => void) | null = null;

  const film = $derived(data?.kind === 'film');
  const overall = $derived(data ? analysisState(data.asset, film) : null);

  async function load() {
    const id = assetId;
    loading = true; loadError = '';
    try {
      const result = await adminRequest<AssetSegments>(`/api/v1/admin/assets/${encodeURIComponent(id)}/segments`);
      if (id === assetId) data = result;
    } catch (cause) { loadError = cause instanceof Error ? cause.message : $t('segment_load_failed_media'); }
    finally { loading = false; }
  }
  function stopWatching() { stopPolling?.(); stopPolling = null; }
  $effect(() => {
    void assetId;
    untrack(() => { stopWatching(); data = null; notice = ''; actionError = ''; void load(); });
    return stopWatching;
  });

  // After an analysis request, refresh every few seconds while a step waits or runs.
  function watch() {
    stopWatching();
    stopPolling = pollWhile(async () => {
      await load();
      return data ? analysisActive(data.asset, data.kind === 'film') : false;
    });
  }

  async function analyse() {
    if (analysing) return;
    analysing = true; notice = ''; actionError = '';
    try {
      await adminRequest(`/api/v1/admin/assets/${encodeURIComponent(assetId)}/analyze-markers`, { method: 'POST' });
      // Without an enabled detection extension, the chapters are not read again.
      const providerOn = data?.providers.some(provider => provider.enabled) ?? false;
      notice = !providerOn ? $t('segment_analysis_no_provider') : film ? $t('segment_analysis_queued_film') : $t('segment_analysis_queued_episode');
      await load();
      watch();
    } catch (cause) { actionError = cause instanceof Error ? cause.message : $t('analysis_unavailable'); }
    finally { analysing = false; }
  }
</script>

<section class="media-segments" aria-labelledby="media-segments-title">
  <header class="head">
    <div>
      <h2 id="media-segments-title">{$t('segments_title')}</h2>
      <p class="lead">{film ? $t('segments_lead_film', { title }) : $t('segments_lead_episode', { title })}</p>
    </div>
    <div class="actions">
      <button type="button" class="btn" disabled={loading} onclick={() => void load()}><Icon name="refresh" size={15} />{$t('segment_refresh')}</button>
      <button type="button" class="btn primary" disabled={analysing || !data} onclick={analyse}><Icon name="search" size={15} />{analysing ? $t('analysing') : $t('segment_analyse_media')}</button>
    </div>
  </header>

  {#if data && data.providers.length === 0}
    <p class="banner warn" role="status">{$t('segment_no_provider')}</p>
  {:else if data && !data.providers.some(provider => provider.enabled)}
    <p class="banner warn" role="status">{$t('segment_provider_disabled')}</p>
  {/if}
  {#if notice}<p class="banner ok" role="status">{notice}</p>{/if}
  {#if actionError}<p class="banner err" role="alert">{actionError}</p>{/if}
  {#if loadError}<p class="banner err" role="alert">{loadError}</p>{/if}
  {#if loading && !data}<p role="status">{$t('segment_loading')}</p>{/if}

  {#if data && overall}
    {#key assetId}<MediaMaintenance {assetId} onSegmentsChanged={load} />{/key}
    <div class="status">
      <p class="overall {overall.tone}">{$t(overall.key)}{#if data.asset.duration} · {$t('segment_media_duration', { duration: formatClock(data.asset.duration) })}{/if}</p>
      <SegmentSteps episode={data.asset} {film} />
      {#if film}<p class="hint">{$t('segment_film_hint')}</p>{/if}
    </div>
    <div class="panel"><SegmentEditPanel episode={data.asset} {film} onChanged={load} /></div>
  {/if}
</section>

<style>
  .media-segments { display: flex; flex-direction: column; gap: 14px; }
  .head { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: flex-end; gap: 12px; }
  h2 { margin: 0; font-size: 22px; font-weight: 800; color: var(--tx); }
  .lead { margin: 4px 0 0; color: var(--mu); font-size: 14px; max-width: 70ch; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .btn { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; padding: 0 14px; border: 0; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }
  .btn:hover:not(:disabled) { background: var(--pn-3); }
  .btn.primary { background: var(--btn); color: var(--btn-tx); }
  .btn:disabled { opacity: .6; cursor: default; }
  .banner { margin: 0; padding: 10px 14px; border-radius: var(--r-sm); background: var(--pn); font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .banner.err { color: var(--err); } .banner.ok { color: var(--ac); } .banner.warn { color: var(--tx); border-left: 3px solid var(--ac); }
  .status, .panel { padding: 14px 16px; border-radius: var(--r-md); background: var(--pn); }
  .status { display: flex; flex-direction: column; gap: 10px; }
  .overall { margin: 0; font-weight: 800; }
  .ok { color: var(--ac); } .err { color: var(--err); } .mute { color: var(--mu); }
  .hint { margin: 0; font-size: 13px; color: var(--mu); }
  @media (max-width: 760px) { .status, .panel { padding: 12px 10px; } }
</style>

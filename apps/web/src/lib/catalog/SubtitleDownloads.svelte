<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { clearDescriptorCache, type Work } from './api';
  import type { Media } from '../player/types';
  import Icon from '../ui/Icon.svelte';
  import { subtitleErrorText, subtitleMatchText, subtitleRequest, type SubtitleInventory, type SubtitleResult } from './subtitle-download-api';
  import { t, fmt } from '../i18n';

  let { work, selectedMedia, onclose }: { work: Work; selectedMedia?: Media; onclose: () => void } = $props();
  const copies = $derived.by(() => {
    const seen: string[] = [];
    const items: { id: string; label: string }[] = [];
    if (selectedMedia) {
      seen.push(selectedMedia.id);
      items.push({ id: selectedMedia.id, label: selectedMedia.title || $t('copy_selected') });
    }
    for (const edition of work.editions ?? []) for (const version of edition.versions) for (const source of version.sources) {
      const id = source.playbackMediaId;
      if (!id || seen.includes(id)) continue;
      seen.push(id);
      items.push({ id, label: [edition.label, version.label, source.nodeId].filter(Boolean).join(' · ') || `Copie ${items.length + 1}` });
    }
    return items;
  });
  let assetId = $state('');
  let inventory = $state<SubtitleInventory | null>(null);
  let providerId = $state('');
  let language = $state('fr');
  let forced = $state(false);
  let sdh = $state(false);
  let items = $state<SubtitleResult[]>([]);
  let searchId = $state('');
  let page = $state(1);
  let totalPages = $state(1);
  let searched = $state(false);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let dialog: HTMLDialogElement;
  let controller: AbortController | undefined;
  const base = $derived(`/api/v1/admin/subtitles/${encodeURIComponent(assetId)}`);
  const quota = $derived(inventory?.quotas?.find(entry => entry.providerId === providerId));

  onMount(() => {
    dialog.showModal();
    assetId = copies[0]?.id ?? '';
    if (assetId) void refresh();
    return () => controller?.abort();
  });

  async function run(action: (signal: AbortSignal) => Promise<void>) {
    controller?.abort();
    const current = new AbortController();
    controller = current;
    busy = true;
    error = '';
    message = '';
    try { await action(current.signal); }
    catch (cause) {
      if (!current.signal.aborted) error = cause instanceof Error && cause.name !== 'TimeoutError' ? cause.message : $t('loading_impossible');
    } finally { if (controller === current) busy = false; }
  }

  async function loadInventory(signal: AbortSignal) {
    const data = await subtitleRequest<SubtitleInventory>(base, signal, $session?.csrfToken ?? '');
    if (signal.aborted) return;
    inventory = data;
    if (!data.providers.some(provider => provider.id === providerId)) providerId = data.providers[0]?.id ?? '';
  }

  async function refresh() {
    inventory = null;
    items = [];
    searchId = '';
    searched = false;
    await run(loadInventory);
  }

  async function search(nextPage = 1) {
    await run(async signal => {
      const data = await subtitleRequest<{ searchId: string; items: SubtitleResult[]; totalPages: number }>(`${base}/search`, signal, $session?.csrfToken ?? '', 'POST', { providerId, language: language.trim(), forced: forced || undefined, sdh: sdh || undefined, page: nextPage });
      if (signal.aborted) return;
      searchId = data.searchId;
      items = data.items;
      page = nextPage;
      totalPages = Math.max(1, data.totalPages);
      searched = true;
    });
  }

  async function download(item: SubtitleResult) {
    await run(async signal => {
      const result = await subtitleRequest<{ id: string; status: string; remaining?: number | null; resetTimeUtc?: string | null }>(`${base}/download`, signal, $session?.csrfToken ?? '', 'POST', { searchId, fileId: item.id });
      if (signal.aborted) return;
      clearDescriptorCache();
      await loadInventory(signal);
      if (signal.aborted) return;
      message = result.status === 'completed' ? $t('available_in_player') : $t('refresh_list');
    });
  }

  async function remove(id: string) {
    await run(async signal => {
      await subtitleRequest<void>(`${base}/${encodeURIComponent(id)}`, signal, $session?.csrfToken ?? '', 'DELETE');
      if (signal.aborted) return;
      clearDescriptorCache();
      await loadInventory(signal);
      if (signal.aborted) return;
      message = $t('refresh_list');
    });
  }

  function cancel() {
    controller?.abort();
    busy = false;
    message = $t('loading_impossible');
  }
</script>

<dialog bind:this={dialog} onclose={onclose} aria-labelledby="subtitle-download-title">
  <div class="modal-header"><h3 id="subtitle-download-title">{$t('subtitles')} · {work.title}</h3><button type="button" aria-label={$t('close')} onclick={() => dialog.close()}><Icon name="close" size={18} /></button></div>
  <div class="modal-body" aria-busy={busy}>
    <p>{$t('subtitles_manual_note')}</p>
    {#if copies.length > 1}<label>{$t('copy_version')}<select bind:value={assetId} disabled={busy} onchange={() => void refresh()}>{#each copies as copy (copy.id)}<option value={copy.id}>{copy.label}</option>{/each}</select></label>{/if}
    {#if !assetId}<p role="status">{$t('no_copy_available')}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if message}<p role="status">{message}</p>{/if}
    {#if busy}<p role="status">{$t('operation_in_progress')}</p><button type="button" onclick={cancel}>{$t('interrupt_wait')}</button>{/if}
    {#if inventory}
      {#if !inventory.enabled}<p role="status">{subtitleErrorText(inventory.unavailableCode ?? '')}</p>{/if}
      {#if inventory.providers.length === 0}<p role="status">{$t('no_subtitle_provider')}</p>{/if}
      {#if quota}
        <p role="status">{$t('provider_quota_prefix')} {quota.remaining === null ? $t('unknown_balance') : $t('download_s_left', { remaining: quota.remaining })}.{quota.resetTimeUtc ? $t('reset_announced', { resetTimeUtc: quota.resetTimeUtc }) : ''}{quota.nextAllowedAt > Date.now() / 1000 ? ` ${$t('next_attempt_earliest', { date: $fmt.dateTime(quota.nextAllowedAt) })}` : ''}</p>
      {/if}
      <form onchange={() => { searched = false; items = []; searchId = ''; }} onsubmit={event => { event.preventDefault(); void search(); }}>
        <label>{$t('subtitle_provider')}<select bind:value={providerId} disabled={busy || !inventory.enabled}>{#each inventory.providers as provider (provider.id)}<option value={provider.id}>{provider.name}</option>{/each}</select></label>
        <label>{$t('language')}<input bind:value={language} required maxlength="35" pattern={'[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*'} placeholder="fr" disabled={busy || !inventory.enabled} /></label>
        <label class="check"><input type="checkbox" bind:checked={forced} disabled={busy || !inventory.enabled} /> {$t('subtitles_forced_only')}</label>
        <label class="check"><input type="checkbox" bind:checked={sdh} disabled={busy || !inventory.enabled} /> {$t('sdh_only')}</label>
        <p>{$t('subtitle_search_note')}</p>
        <button type="submit" disabled={busy || !inventory.enabled || !providerId}>{$t('search_action')}</button>
      </form>
      {#if searched}
        <h4>{$t('results')}</h4>
        {#each items as item (item.id)}
          <div class="subtitle-row"><div><strong>{item.label}</strong><p>{item.language}{item.forced ? $t('catalog_forced') : ''}{item.sdh ? ' · SDH' : ''}</p><p>{subtitleMatchText(item.match)}</p></div><button type="button" disabled={busy || !inventory.enabled || inventory.downloads.some(entry => entry.providerId === providerId && entry.fileId === item.id && entry.status === 'completed')} onclick={() => void download(item)}>{$t('download')}</button></div>
        {:else}<p>{$t('no_results_search')}</p>{/each}
        {#if totalPages > 1}<nav aria-label={$t('results')}><button type="button" disabled={busy || page <= 1} onclick={() => void search(page - 1)}>{$t('previous_page')}</button><span>{page} / {totalPages}</span><button type="button" disabled={busy || page >= totalPages} onclick={() => void search(page + 1)}>{$t('next_page')}</button></nav>{/if}
      {/if}
      <h4>{$t('downloaded_subtitles')}</h4>
      {#each inventory.downloads as entry (entry.id)}
        <div class="subtitle-row"><div><strong>{entry.label}</strong><p>{entry.language}{entry.forced ? $t('catalog_forced') : ''}{entry.sdh ? ' · SDH' : ''} · {entry.status === 'completed' ? $t('available_in_player') : entry.status === 'failed' ? 'Échec' : entry.status === 'interrupted' ? $t('interrupted_remove_retry') : $t('pending_or_running')}</p>{#if entry.errorCode}<p class="error">{subtitleErrorText(entry.errorCode)}</p>{/if}</div><button type="button" disabled={busy} onclick={() => void remove(entry.id)}>{$t('remove_download')}</button></div>
      {:else}<p>{$t('no_downloaded_subtitles')}</p>{/each}
    {/if}
    {#if assetId}<button type="button" disabled={busy} onclick={() => void refresh()}>{$t('refresh_list')}</button>{/if}
  </div>
</dialog>

<style>
  dialog { width: min(720px, calc(100vw - 48px)); max-width: none; max-height: calc(100dvh - 48px); padding: 0; border: 1px solid var(--line); border-radius: var(--r-xl); background: var(--bg-3); color: var(--tx); box-shadow: var(--shadow); overscroll-behavior: contain; }
  dialog::backdrop { background: rgb(6 8 14 / 0.55); backdrop-filter: blur(4px); -webkit-backdrop-filter: blur(4px); }
  .modal-header { position: sticky; top: 0; z-index: 1; display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 24px 28px 12px; background: var(--bg-3); }
  .modal-header button { flex: none; width: 44px; padding: 0; border-radius: 14px; }
  h3 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  h4 { margin: 28px 0 4px; font-size: 17px; font-weight: 700; }
  .modal-body { padding: 4px 28px 28px; }
  p { font-size: 13px; line-height: 1.6; color: var(--mu); }
  label { display: block; margin: 16px 0; font-size: 14px; font-weight: 700; color: var(--tx); }
  input:not([type=checkbox]), select { box-sizing: border-box; display: block; width: 100%; min-height: 46px; margin-top: 8px; padding: 0 14px; background: var(--pn); color: var(--tx); border: 1px solid transparent; border-radius: var(--r-sm); font: inherit; font-size: 14px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease); }
  input:not([type=checkbox]):focus, select:focus { outline: none; border-color: var(--ac); background: var(--bg-3); box-shadow: none; }
  select option { background: var(--bg-3); color: var(--tx); }
  input[type=checkbox] { flex: none; width: 18px; height: 18px; accent-color: var(--ac); }
  .check { display: flex; gap: 10px; align-items: center; min-height: 44px; margin: 4px 0; font-weight: 600; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 44px; padding: 0 16px; border: 1px solid transparent; border-radius: var(--r-md); background: var(--pn); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) var(--ease), opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-2); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button[type=submit] { background: var(--btn); color: var(--btn-tx); }
  button[type=submit]:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  .subtitle-row { display: flex; justify-content: space-between; gap: 16px; align-items: center; padding: 14px 0; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; }
  .subtitle-row strong { font-size: 14px; font-weight: 700; color: var(--tx); }
  .subtitle-row p { margin: 4px 0; }
  .subtitle-row button { flex: none; }
  .error { color: var(--err); }
  p.error[role=alert] { padding: 12px 16px; border-radius: var(--r-sm); background: var(--err-soft); font-weight: 600; }
  nav { display: flex; gap: 12px; align-items: center; margin-top: 16px; font-variant-numeric: tabular-nums; }
  @media (max-width: 639px) {
    dialog { width: 100vw; height: 100dvh; max-height: 100dvh; margin: 0; border: 0; border-radius: 0; }
    .modal-header { padding: 16px 16px 8px; }
    h3 { font-size: 20px; }
    .modal-body { padding: 4px 16px calc(24px + env(safe-area-inset-bottom, 0px)); }
    .subtitle-row { align-items: flex-start; flex-direction: column; }
  }
</style>

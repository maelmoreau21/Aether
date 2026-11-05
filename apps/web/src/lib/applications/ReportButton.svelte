<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { applicationRequest, categoryLabels, features, loadFeatures, reportPayload, type ReportContext } from './api';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  let { workId, trackId, assetId, title = 'ce média', positionSeconds, context = {} }: {
    workId?: string; trackId?: string; assetId?: string; title?: string; positionSeconds?: number; context?: ReportContext | (() => ReportContext);
  } = $props();
  let dialog: HTMLDialogElement;
  const componentId = $props.id();
  const titleId = `${componentId}-report-title`;
  let opener: HTMLElement | null = null;
  let category = $state('other');
  let description = $state('');
  let position = $state<number | undefined>();
  let end = $state<number | undefined>();
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let captured = $state<{ workId?: string; trackId?: string; assetId?: string; context: ReportContext; title: string }>({ context: {}, title: '' });
  onMount(() => { void loadFeatures().catch(() => {}); });
  async function open(event: MouseEvent) {
    opener = event.currentTarget as HTMLElement;
    captured = { workId, trackId, assetId, title, context: { ...(typeof context === 'function' ? context() : context) } };
    position = positionSeconds === undefined ? undefined : Math.round(positionSeconds * 10) / 10;
    end = undefined; description = ''; error = ''; message = ''; category = 'other';
    dialog.showModal();
    await tick();
    dialog.querySelector<HTMLSelectElement>('select')?.focus();
  }
  async function submit(event: SubmitEvent) {
    event.preventDefault(); error = ''; busy = true;
    try {
      const payload = reportPayload({ workId: captured.workId, trackId: captured.trackId, assetId: captured.assetId, context: captured.context, category, description, positionSeconds: position, endSeconds: end });
      await applicationRequest('/api/v1/reports', { method: 'POST', body: JSON.stringify(payload) });
      message = $t('report_sent');
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('report_send_failed'); }
    finally { busy = false; }
  }
</script>
{#if $features.reports && (workId || trackId)}
  <button type="button" class="report-trigger" onclick={open} title={$t('report_problem')}><Icon name="alert" size={16} /><span>{$t('report_problem')}</span></button>
{/if}
<dialog bind:this={dialog} class="report-dialog" aria-labelledby={titleId} onclose={() => opener?.focus()}>
  <header><div><p>{$t('report_problem')}</p><h2 id={titleId}>{captured.title}</h2></div><button type="button" aria-label={$t('close')} onclick={() => dialog.close()}><Icon name="close" /></button></header>
  {#if message}<p class="success" role="status">{message}</p><button type="button" onclick={() => dialog.close()}>{$t('close')}</button>
  {:else}
    <form onsubmit={submit}>
      <label>{$t('problem_type')}<select bind:value={category}>{#each Object.entries(categoryLabels) as [value, label] (value)}<option {value}>{label}</option>{/each}</select></label>
      <label>{$t('description')}<textarea bind:value={description} minlength="3" maxlength="8000" required rows="5" placeholder={$t('report_description_placeholder')}></textarea></label>
      <div class="positions"><label>{$t('start_optional')}<input type="number" bind:value={position} min="0" step="0.1" /></label><label>{$t('end_optional')}<input type="number" bind:value={end} min={position ?? 0} step="0.1" /></label></div>
      {#if captured.context.engine || captured.context.audioTrackId || captured.context.subtitleTrackId || captured.context.errorCode}
        <details><summary>{$t('attached_context')}</summary><dl>{#each Object.entries(captured.context).filter(([, value]) => value) as [key, value] (key)}<dt>{key === 'engine' ? $t('playback') : key === 'audioTrackId' ? $t('audio') : key === 'subtitleTrackId' ? $t('subtitles') : $t('error_code')}</dt><dd>{value}</dd>{/each}</dl></details>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <footer><button type="button" disabled={busy} onclick={() => dialog.close()}>{$t('cancel')}</button><button class="primary" type="submit" disabled={busy || !description.trim()}>{busy ? $t('submitting') : $t('send_report')}</button></footer>
    </form>
  {/if}
</dialog>
<style>
  .report-trigger{display:inline-flex;align-items:center;gap:7px;color:inherit;background:transparent;border:1px solid var(--line);border-radius:9px;padding:8px 11px;cursor:pointer;font:inherit;font-size:12px}
  .report-dialog{width:min(580px,calc(100vw - 32px));max-height:85dvh;overflow:auto;box-sizing:border-box;padding:24px;background:var(--bg-2);color:var(--tx);border:1px solid var(--line-2);border-radius:18px;box-shadow:var(--shadow)}.report-dialog::backdrop{background:#0009;backdrop-filter:blur(8px)}header,footer{display:flex;justify-content:space-between;align-items:center;gap:16px}header{margin-bottom:22px}header p{font-size:12px;color:var(--mu);margin:0 0 7px}h2{font-size:20px;margin:0;overflow-wrap:anywhere}form{display:grid;gap:16px}label{display:grid;gap:8px;font-size:13px;color:var(--mu)}input,select,textarea{box-sizing:border-box;width:100%;padding:12px;background:var(--bg-3);color:var(--tx);border:1px solid var(--line-2);border-radius:10px;font:inherit}textarea{resize:vertical}.positions{display:grid;grid-template-columns:1fr 1fr;gap:12px}button{display:inline-flex;align-items:center;justify-content:center;padding:10px 14px;background:var(--bg-3);color:var(--tx);border:1px solid var(--line);border-radius:10px;font:inherit;cursor:pointer}button:disabled{opacity:.5}.primary{background:var(--ac);color:var(--on-ac)}footer{justify-content:flex-end}.error{color:var(--err)}.success{color:var(--ok);line-height:1.6}details{font-size:12px;color:var(--mu)}dl{display:grid;grid-template-columns:auto 1fr;gap:8px}dd{margin:0;overflow-wrap:anywhere}:is(button,input,select,textarea,summary):focus-visible{outline:2px solid var(--ac);outline-offset:3px}@media(max-width:480px){.positions{grid-template-columns:1fr}.report-dialog{padding:18px}}
</style>

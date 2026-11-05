<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { applicationRequest, features, loadFeatures, reportPayload, type ReportContext } from './api';
  import Icon, { type IconName } from '../ui/Icon.svelte';
  import { t, type TranslationKey } from '../i18n';

  /**
   * `variant` matches the surrounding toolbar: `tool` (media page tool buttons),
   * `player` (video player top bar, over the picture) or `bar` (music player).
   */
  let { workId, trackId, assetId, title = 'ce média', positionSeconds, context = {}, variant = 'tool' }: {
    workId?: string; trackId?: string; assetId?: string; title?: string; positionSeconds?: number;
    context?: ReportContext | (() => ReportContext); variant?: 'tool' | 'player' | 'bar';
  } = $props();

  const MAX_DESCRIPTION = 8000;
  const categories: readonly { id: string; icon: IconName; label: TranslationKey; hint: TranslationKey }[] = [
    { id: 'video', icon: 'film', label: 'report_category_video', hint: 'report_category_video_hint' },
    { id: 'audio', icon: 'volume', label: 'report_category_audio', hint: 'report_category_audio_hint' },
    { id: 'subtitles', icon: 'captions', label: 'report_category_subtitles', hint: 'report_category_subtitles_hint' },
    { id: 'metadata', icon: 'info', label: 'report_category_metadata', hint: 'report_category_metadata_hint' },
    { id: 'missing', icon: 'eye-off', label: 'report_category_missing', hint: 'report_category_missing_hint' },
    { id: 'other', icon: 'more', label: 'report_category_other', hint: 'report_category_other_hint' }
  ];
  const contextLabels: Record<string, TranslationKey> = { engine: 'playback', audioTrackId: 'audio', subtitleTrackId: 'subtitles', errorCode: 'error_code' };

  let dialog: HTMLDialogElement;
  let closeButton = $state<HTMLButtonElement>();
  const componentId = $props.id();
  const titleId = `${componentId}-report-title`;
  const hintId = `${componentId}-report-hint`;
  let opener: HTMLElement | null = null;
  let category = $state('other');
  let description = $state('');
  let includeTime = $state(false);
  let position = $state<number | undefined>();
  let end = $state<number | undefined>();
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let captured = $state<{ workId?: string; trackId?: string; assetId?: string; context: ReportContext; title: string; timed: boolean }>({ context: {}, title: '', timed: false });
  const contextEntries = $derived(Object.entries(captured.context).filter(([, value]) => value));

  onMount(() => { void loadFeatures().catch(() => {}); });

  function clock(seconds: number | undefined): string {
    if (seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '';
    const whole = Math.floor(seconds);
    const h = Math.floor(whole / 3600), m = Math.floor((whole % 3600) / 60), s = whole % 60;
    return `${h ? `${h}:${String(m).padStart(2, '0')}` : m}:${String(s).padStart(2, '0')}`;
  }
  async function open(event: MouseEvent) {
    opener = event.currentTarget as HTMLElement;
    const timed = positionSeconds !== undefined && Number.isFinite(positionSeconds);
    captured = { workId, trackId, assetId, title, timed, context: { ...(typeof context === 'function' ? context() : context) } };
    position = timed ? Math.round((positionSeconds as number) * 10) / 10 : undefined;
    includeTime = timed;
    end = undefined; description = ''; error = ''; message = ''; busy = false;
    category = captured.context.errorCode ? 'video' : 'other';
    dialog.showModal();
    await tick();
    dialog.querySelector<HTMLInputElement>('input[name="report-category"]:checked')?.focus();
  }
  function close() { if (dialog.open) dialog.close(); }
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    error = ''; busy = true;
    try {
      const timed = captured.timed && includeTime;
      const payload = reportPayload({ workId: captured.workId, trackId: captured.trackId, assetId: captured.assetId, context: captured.context, category, description, positionSeconds: timed ? position : undefined, endSeconds: timed ? end : undefined });
      await applicationRequest('/api/v1/reports', { method: 'POST', body: JSON.stringify(payload) });
      message = $t('report_sent');
      await tick();
      closeButton?.focus();
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('report_send_failed'); }
    finally { busy = false; }
  }
  /**
   * The dialog lives under <body>: inside the video player it would inherit the
   * player's `inert` top bar once the controls hide, and its keys would reach the
   * player shortcuts (Escape would close the player instead of this dialog).
   */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }
  /** Keys typed in the dialog stay in the dialog; Escape still cancels it natively. */
  function isolate(event: KeyboardEvent) { event.stopPropagation(); }
  /** A press on the backdrop (outside the panel) closes the dialog. */
  function backdrop(event: PointerEvent) {
    if (event.target !== dialog) return;
    const box = dialog.getBoundingClientRect();
    if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) close();
  }
</script>

{#if $features.reports && (workId || trackId)}
  <button type="button" class="report-trigger {variant}" onclick={open} aria-label={$t('report_problem')} title={$t('report_problem')} aria-haspopup="dialog">
    <Icon name="warning" size={variant === 'player' ? 22 : 20} />
  </button>
{/if}

<!-- The wrapper keeps this component's place in the page once the dialog is moved. -->
<div class="report-host">
<dialog bind:this={dialog} use:portal class="report-dialog" aria-labelledby={titleId} aria-describedby={hintId} onclose={() => opener?.focus()} onpointerdown={backdrop} onkeydown={isolate}>
  <header class="head">
    <span class="badge" aria-hidden="true"><Icon name="warning" size={22} /></span>
    <div class="head-copy">
      <h2 id={titleId}>{$t('report_problem')}</h2>
      <p class="subject" title={captured.title}>{captured.title}</p>
    </div>
    <button type="button" class="icon-btn" aria-label={$t('close')} onclick={close}><Icon name="close" size={20} /></button>
  </header>

  {#if message}
    <div class="done" role="status">
      <span class="done-icon" aria-hidden="true"><Icon name="check" size={28} /></span>
      <h3>{$t('report_sent_title')}</h3>
      <p>{message}</p>
      <button type="button" class="btn primary" bind:this={closeButton} onclick={close}>{$t('close')}</button>
    </div>
  {:else}
    <form onsubmit={submit} aria-busy={busy}>
      <div class="body">
        <p class="hint" id={hintId}>{$t('report_dialog_hint')}</p>

        <fieldset class="categories" disabled={busy}>
          <legend>{$t('report_category_legend')}</legend>
          <div class="grid">
            {#each categories as item (item.id)}
              <label class="choice" class:selected={category === item.id}>
                <input type="radio" name="report-category" value={item.id} bind:group={category} />
                <span class="choice-icon" aria-hidden="true"><Icon name={item.icon} size={20} /></span>
                <span class="choice-copy"><strong>{$t(item.label)}</strong><small>{$t(item.hint)}</small></span>
              </label>
            {/each}
          </div>
        </fieldset>

        <label class="field">
          <span class="field-label">{$t('description')}</span>
          <textarea bind:value={description} minlength="3" maxlength={MAX_DESCRIPTION} required rows="4" placeholder={$t('report_description_placeholder')} disabled={busy}></textarea>
          <small class="count" aria-live="off">{$t('report_description_count', { count: description.length, max: MAX_DESCRIPTION })}</small>
        </label>

        {#if captured.timed}
          <div class="timing">
            <label class="toggle"><input type="checkbox" bind:checked={includeTime} disabled={busy} /> <span>{$t('report_include_time')}</span>{#if includeTime && clock(position)}<span class="at">{$t('report_time_at', { time: clock(position) })}</span>{/if}</label>
            {#if includeTime}
              <div class="positions">
                <label class="field"><span class="field-label">{$t('report_time_start')}</span><input type="number" bind:value={position} min="0" step="0.1" required disabled={busy} /></label>
                <label class="field"><span class="field-label">{$t('report_time_end')}</span><input type="number" bind:value={end} min={position ?? 0} step="0.1" disabled={busy} /></label>
              </div>
            {/if}
          </div>
        {/if}

        {#if contextEntries.length}
          <details class="context">
            <summary><Icon name="chevron" size={16} /><span>{$t('attached_context')}</span></summary>
            <dl>{#each contextEntries as [key, value] (key)}<div><dt>{$t(contextLabels[key] ?? 'error_code')}</dt><dd>{value}</dd></div>{/each}</dl>
          </details>
        {/if}

        {#if error}<p class="error" role="alert"><Icon name="alert" size={18} /><span>{error}</span></p>{/if}
      </div>

      <footer class="foot">
        <button type="button" class="btn ghost" onclick={close}>{$t('cancel')}</button>
        <button type="submit" class="btn primary" disabled={busy || description.trim().length < 3}>{busy ? $t('submitting') : $t('send_report')}</button>
      </footer>
    </form>
  {/if}
</dialog>
</div>

<style>
  /* ---------- Déclencheur : bouton icône assorti à la barre voisine ---------- */
  .report-trigger {
    display: inline-grid; place-items: center; flex: none;
    width: 44px; height: 44px; min-height: 44px; padding: 0; box-sizing: border-box;
    border: 1px solid transparent; border-radius: 14px;
    background: var(--pn); color: var(--tx); cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .report-trigger:hover { background: var(--pn-2); }
  .report-trigger:active { transform: scale(0.98); }
  .report-trigger:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .report-trigger.bar { background: transparent; }
  .report-trigger.bar:hover { background: var(--pn-2); }
  .report-trigger.player { width: 48px; height: 48px; border: 0; border-radius: 16px; background: transparent; color: #fff; }
  .report-trigger.player:hover { background: rgb(255 255 255 / 0.14); }
  @media (max-width: 700px) { .report-trigger.player { width: 42px; height: 42px; min-height: 42px; } }

  .report-host { display: contents; }

  /* ---------- Fenêtre ---------- */
  .report-dialog {
    width: min(600px, calc(100vw - 32px)); max-width: none; max-height: min(88dvh, 820px);
    margin: auto; padding: 0; box-sizing: border-box; overflow: hidden;
    border: 1px solid var(--line-2); border-radius: var(--r-xl);
    background: var(--bg-2); color: var(--tx); box-shadow: var(--shadow);
    font-family: var(--font);
  }
  .report-dialog[open] { display: flex; flex-direction: column; animation: rise var(--t-med) var(--ease); }
  .report-dialog::backdrop { background: rgb(6 8 14 / 0.6); backdrop-filter: blur(6px); }
  @keyframes rise { from { opacity: 0; transform: translateY(12px) scale(0.98); } }
  @keyframes sheet { from { transform: translateY(40px); } }
  @media (prefers-reduced-motion: reduce) { .report-dialog[open] { animation: none; } }

  .head { display: flex; align-items: center; gap: 14px; padding: 20px 20px 16px 24px; border-bottom: 1px solid var(--line); flex: none; }
  .badge { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: 14px; background: var(--warn-soft); color: var(--warn); }
  .head-copy { flex: 1; min-width: 0; }
  h2 { margin: 0; font-size: 19px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.2; }
  .subject { margin: 2px 0 0; overflow: hidden; color: var(--mu); font-size: 14px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .icon-btn { display: grid; place-items: center; flex: none; width: 44px; height: 44px; padding: 0; border: 0; border-radius: 14px; background: transparent; color: var(--mu); cursor: pointer; transition: background-color var(--t-fast) ease, color var(--t-fast) ease; }
  .icon-btn:hover { background: var(--pn-2); color: var(--tx); }

  form { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .body { display: grid; gap: 20px; padding: 20px 24px 24px; overflow-y: auto; overscroll-behavior: contain; min-height: 0; }
  .hint { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.5; }

  fieldset { min-width: 0; margin: 0; padding: 0; border: 0; }
  legend, .field-label { padding: 0; margin-bottom: 10px; color: var(--tx); font-size: 14px; font-weight: 700; }
  .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
  .choice { position: relative; display: flex; flex-direction: column; align-items: flex-start; gap: 10px; min-width: 0; padding: 14px; border: 1px solid var(--line); border-radius: var(--r-md); background: var(--pn); cursor: pointer; transition: background-color var(--t-fast) ease, border-color var(--t-fast) ease; }
  .choice:hover { background: var(--pn-2); }
  .choice.selected { border-color: var(--ac); background: var(--ac-soft); }
  .choice input { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; cursor: pointer; }
  .choice:has(input:focus-visible) { outline: 2px solid var(--ac); outline-offset: 2px; }
  .choice-icon { display: grid; place-items: center; width: 34px; height: 34px; border-radius: 10px; background: var(--pn-2); color: var(--tx); }
  .choice.selected .choice-icon { background: var(--ac); color: var(--on-ac); }
  .choice-copy { display: grid; gap: 2px; min-width: 0; }
  .choice-copy strong { font-size: 14px; font-weight: 700; }
  .choice-copy small { color: var(--mu); font-size: 12px; line-height: 1.35; }
  fieldset:disabled .choice { opacity: 0.6; cursor: default; }

  .field { display: grid; min-width: 0; }
  .field .field-label { margin-bottom: 8px; }
  textarea, input[type='number'] {
    box-sizing: border-box; width: 100%; min-width: 0; padding: 12px 14px;
    border: 1px solid transparent; border-radius: var(--r-sm);
    background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }
  textarea { min-height: 116px; resize: vertical; line-height: 1.5; }
  input[type='number'] { height: 46px; font-variant-numeric: tabular-nums; }
  textarea::placeholder { color: var(--mu-2); }
  textarea:hover, input[type='number']:hover { background: var(--pn-2); }
  textarea:focus, input[type='number']:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  .count { justify-self: end; margin-top: 6px; color: var(--mu-2); font-size: 12px; font-variant-numeric: tabular-nums; }

  .timing { display: grid; gap: 12px; padding: 14px 16px; border-radius: var(--r-md); background: var(--pn); }
  .toggle { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; min-height: 32px; font-size: 14px; font-weight: 700; cursor: pointer; }
  .toggle input { width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  .at { margin-left: auto; padding: 2px 10px; border-radius: var(--r-pill); background: var(--pn-2); color: var(--mu); font-size: 13px; font-variant-numeric: tabular-nums; }
  .positions { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .positions input[type='number'] { background: var(--bg-2); }

  .context { border: 1px solid var(--line); border-radius: var(--r-md); }
  .context summary { display: flex; align-items: center; gap: 8px; min-height: 44px; padding: 0 14px; color: var(--mu); font-size: 13px; font-weight: 700; cursor: pointer; list-style: none; border-radius: var(--r-md); }
  .context summary::-webkit-details-marker { display: none; }
  .context summary :global(svg) { transition: transform var(--t-fast) var(--ease); }
  .context[open] summary :global(svg) { transform: rotate(90deg); }
  .context summary:hover { color: var(--tx); }
  dl { display: grid; gap: 8px; margin: 0; padding: 0 14px 14px; font-size: 13px; }
  dl div { display: flex; justify-content: space-between; gap: 16px; }
  dt { color: var(--mu); }
  dd { margin: 0; overflow-wrap: anywhere; font-family: var(--font-mono); text-align: right; }

  .error { display: flex; align-items: flex-start; gap: 10px; margin: 0; padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-size: 14px; font-weight: 600; line-height: 1.45; }
  .error :global(svg) { flex: none; margin-top: 1px; }

  .foot { display: flex; justify-content: flex-end; gap: 10px; padding: 16px 24px; border-top: 1px solid var(--line); background: var(--bg-2); flex: none; }
  .btn { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 20px; border: 0; border-radius: var(--r-md); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  .btn:active:not(:disabled) { transform: scale(0.98); }
  .btn:disabled { opacity: 0.45; cursor: default; }
  .ghost { background: transparent; color: var(--tx); }
  .ghost:hover { background: var(--pn-2); }
  .primary { background: var(--btn); color: var(--btn-tx); }
  .primary:hover:not(:disabled) { opacity: 0.9; }

  .done { display: grid; justify-items: center; gap: 8px; padding: 36px 24px 28px; text-align: center; }
  .done-icon { display: grid; place-items: center; width: 56px; height: 56px; margin-bottom: 6px; border-radius: 50%; background: var(--ok-soft); color: var(--ok); }
  .done h3 { margin: 0; font-size: 18px; font-weight: 800; }
  .done p { max-width: 420px; margin: 0 0 14px; color: var(--mu); font-size: 14px; line-height: 1.55; }

  :is(.btn, .icon-btn, summary, .toggle input):focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  @media (max-width: 560px) {
    .report-dialog { width: 100%; max-height: 92dvh; margin: auto 0 0; border-width: 1px 0 0; border-radius: var(--r-xl) var(--r-xl) 0 0; }
    .report-dialog[open] { animation-name: sheet; }
    .head { padding: 16px 12px 14px 16px; }
    .body { padding: 16px; }
    .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .choice { padding: 12px; }
    .positions { grid-template-columns: 1fr; }
    .foot { padding: 12px 16px calc(12px + env(safe-area-inset-bottom, 0px)); }
    .foot .btn { flex: 1; }
    textarea, input[type='number'] { font-size: 16px; }
  }
</style>

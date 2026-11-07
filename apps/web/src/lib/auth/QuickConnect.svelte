<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { deviceRequest } from './quick-connect';
  import { pollDeviceConnection } from './session';
  import { t } from '../i18n';
  interface Challenge { secret: string; code: string; expiresIn: number; qrWidth: number; qrModules: boolean[]; }
  let challenge = $state<Challenge | null>(null);
  let error = $state('');
  let busy = $state(false);
  let expired = $state(false);
  let controller: AbortController | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let deadline = 0;
  const path = $derived(challenge?.qrModules.flatMap((dark, index) => dark ? [$t('m_h1v1h_1z', { value: index % challenge!.qrWidth + 4, value2: Math.floor(index / challenge!.qrWidth) + 4 })] : []).join('') ?? '');
  async function poll(current: Challenge, signal: AbortSignal) {
    if (signal.aborted) return;
    if (Date.now() >= deadline) { expired = true; return; }
    try {
      if (await pollDeviceConnection(current.secret, signal)) { await goto(resolve('/'), { replaceState: true }); return; }
      if (!signal.aborted) timer = setTimeout(() => void poll(current, signal), 2500);
    } catch (cause) {
      if (!signal.aborted) { error = cause instanceof Error ? cause.message : $t('sign_in_unavailable'); expired = true; }
    }
  }
  async function start() {
    controller?.abort(); clearTimeout(timer);
    controller = new AbortController();
    const signal = controller.signal;
    busy = true; error = ''; expired = false; challenge = null;
    try {
      const result = await deviceRequest<Challenge>('start', {}, undefined, signal);
      if (signal.aborted) return;
      challenge = result; deadline = Date.now() + result.expiresIn * 1000;
      timer = setTimeout(() => void poll(result, signal), 2500);
    } catch (cause) { if (!signal.aborted) error = cause instanceof Error ? cause.message : $t('sign_in_unavailable'); }
    finally { if (!signal.aborted) busy = false; }
  }
  onMount(() => () => { controller?.abort(); clearTimeout(timer); });
</script>

<section class="quick-connect" aria-labelledby="quick-connect-title">
  <h2 id="quick-connect-title">{$t('with_an_already_signed_in_device')}</h2>
  <p>{$t('scan_the_qr_code_with_your')}</p>
  {#if challenge && !expired}
    <svg class="qr" viewBox={`0 0 ${challenge.qrWidth + 8} ${challenge.qrWidth + 8}`} role="img" aria-label={$t('sign_in_qr_code_to_scan')} shape-rendering="crispEdges"><path class="qr-background" d={`M0 0h${challenge.qrWidth + 8}v${challenge.qrWidth + 8}H0z`} /><path d={path} /></svg>
    <p class="code" aria-label={$t('code', { code: challenge.code })}>{challenge.code.slice(0, 4)}–{challenge.code.slice(4)}</p>
    <p class="hint">{$t('or_enter_this_code_under_settings')}</p>
    <p role="status">{$t('waiting_for_your_confirmation')}</p>
  {/if}
  {#if expired && !error}<p role="status">{$t('this_qr_code_has_expired_generate')}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  <button type="button" disabled={busy} onclick={() => void start()}>{busy ? $t('creating_the_code') : challenge ? $t('generate_a_new_code') : $t('show_the_qr_code')}</button>
</section>

<style>
  .quick-connect { border-top: 1px solid var(--line); padding-top: 20px; text-align: center; }
  h2 { margin: 0 0 8px; font-size: 16px; }
  p { color: var(--mu); font-size: 13px; line-height: 1.6; }
  .qr { display: block; width: min(100%, 216px); height: auto; margin: 20px auto 12px; fill: #101010; border-radius: 12px; }
  .qr-background { fill: #fff; }
  .code { font: 700 26px var(--font-mono, monospace); letter-spacing: .12em; color: var(--tx); margin: 12px 0; }
  button { min-height: 44px; border: 1px solid var(--line); border-radius: var(--r-md); padding: 10px 16px; font: inherit; font-size: 14px; font-weight: 600; color: var(--tx); background: var(--pn); cursor: pointer; }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: .5; cursor: wait; }
  [role='alert'] { color: var(--err); }
</style>

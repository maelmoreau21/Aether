<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { session } from './session';
  import ClientIcon from '../ui/ClientIcon.svelte';
  import { clientInfo } from '../ui/client-info';
  import { t, fmt } from '../i18n';

  interface Device { id: string; device: string; createdAt: number; lastSeenAt: number; expiresAt: number; current: boolean }
  let devices = $state<Device[]>([]);
  let loading = $state(true);
  let loaded = $state(false);
  let busy = $state<string | null>(null);
  let error = $state('');
  let notice = $state('');
  let confirmAll = $state(false);
  let controller: AbortController | undefined;
  const ordered = $derived([...devices].sort((a, b) => Number(b.current) - Number(a.current) || b.lastSeenAt - a.lastSeenAt));

  function deviceName(raw: string): string {
    const client = clientInfo(raw);
    return [client.app, client.system].filter(Boolean).join(' · ') || $t('browser_generic');
  }
  function date(value: number): string {
    return $fmt.dateTime(value);
  }
  async function load() {
    controller?.abort();
    const current = new AbortController();
    controller = current;
    loading = true;
    error = '';
    try {
      const response = await fetch('/api/v1/account/sessions', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.any([current.signal, AbortSignal.timeout(10000)]) });
      if (!response.ok) throw new Error($t('unable_to_load_your_sessions_try'));
      const data = await response.json();
      if (!Array.isArray(data.sessions)) throw new Error($t('unable_to_load_your_sessions_try'));
      if (!current.signal.aborted) { devices = data.sessions; loaded = true; }
    } catch (cause) {
      if (!current.signal.aborted) error = cause instanceof Error && cause.name === 'TimeoutError' ? $t('the_server_is_taking_too_long') : $t('unable_to_load_your_sessions_try');
    } finally {
      if (!current.signal.aborted) loading = false;
    }
  }
  onMount(() => { void load(); return () => controller?.abort(); });

  async function revoke(device?: Device) {
    if (busy || loading) return;
    busy = device?.id ?? 'all';
    error = ''; notice = '';
    try {
      const response = await fetch('/api/v1/account/sessions' + (device ? '/' + encodeURIComponent(device.id) : ''), {
        method: 'DELETE', credentials: 'same-origin', cache: 'no-store',
        headers: { 'x-csrf-token': $session?.csrfToken ?? '' }, signal: AbortSignal.timeout(10000)
      });
      if (!response.ok && !(device && response.status === 404)) throw new Error($t('unable_to_sign_out_this_session'));
      if (!device || device.current) { window.location.reload(); return; }
      devices = devices.filter(item => item.id !== device.id);
      notice = $t('the_session_has_been_signed_out');
      await load();
    } catch (cause) {
      error = cause instanceof Error && cause.name === 'TimeoutError' ? $t('the_server_is_taking_too_long') : $t('unable_to_sign_out_this_session');
    } finally { busy = null; confirmAll = false; }
  }
</script>

<section class="sessions" aria-labelledby="sessions-title" aria-busy={loading}>
  <header class="sessions-header">
    <div><h3 id="sessions-title">{$t('your_sessions')} <span class="session-count">{loaded ? devices.length : '…'}</span></h3><p>{$t('review_your_sign_ins_and_stay')}</p></div>
    <button class="refresh" disabled={!!busy || loading} onclick={() => void load()}><Icon name="refresh" size={17} />{loading ? 'Actualisation…' : 'Actualiser'}</button>
  </header>

  {#if error}<p class="message error" role="alert"><Icon name="alert" size={18} />{error}</p>{/if}
  {#if notice}<p class="message notice" role="status"><Icon name="check" size={18} />{notice}</p>{/if}
  {#if loading && !loaded}
    <div class="empty" role="status"><Icon name="refresh" size={28} /><p>{$t('loading_your_sessions')}</p></div>
  {:else if loaded && devices.length === 0}
    <div class="empty"><Icon name="shield" size={28} /><strong>{$t('no_sessions_to_show')}</strong><p>{$t('refresh_the_list_to_check_your')}</p></div>
  {/if}

  <div class="session-list">
    {#each ordered as device (device.id)}
      {@const client = clientInfo(device.device)}
      <article class:current={device.current} aria-label={deviceName(device.device)} title={device.device || $t('browser_not_reported')}>
        <div class="session-top">
          <div class="device-icon"><ClientIcon kind={client.kind} size={24} /></div>
          <div class="device-heading"><strong>{client.system || 'Appareil'}</strong><span class="client-app">{client.app}</span></div>
          {#if device.current}<span class="current-badge"><span></span>{$t('this_device')}</span>{/if}
        </div>
        <dl>
          <div><dt>{$t('auth_last_activity')}</dt><dd>{date(device.lastSeenAt)}</dd></div>
          <div><dt>{$t('signed_in_since')}</dt><dd>{date(device.createdAt)}</dd></div>
        </dl>
        <button class="disconnect" disabled={!!busy || loading} aria-label={'Déconnecter ' + (device.current ? 'cette session' : deviceName(device.device))} onclick={() => void revoke(device)}>{busy === device.id ? 'Déconnexion…' : 'Déconnecter'}</button>
      </article>
    {/each}
  </div>

  <div class="session-info"><Icon name="shield" size={19} /><p>{$t('a_session_expires_after_7_days')}</p></div>
  {#if devices.length > 0}
    <footer>
      <div><strong>{$t('sign_out_everywhere')}</strong><p>{$t('close_all_sessions_of_your_account')}</p></div>
      {#if !confirmAll}<button class="danger" disabled={!!busy || loading} onclick={() => confirmAll = true}>{$t('sign_out_all_devices')}</button>{/if}
    </footer>
    {#if confirmAll}
      <div class="confirmation" role="group" aria-label={$t('confirm_sign_out_everywhere')}><p>{$t('you_will_need_to_sign_in')}</p><div><button disabled={!!busy} onclick={() => confirmAll = false}>{$t('sched_cancel')}</button><button class="danger" disabled={!!busy || loading} onclick={() => void revoke()}>{busy === 'all' ? 'Déconnexion…' : $t('confirm_sign_out')}</button></div></div>
    {/if}
  {/if}
</section>

<style>
  .sessions { display: grid; gap: 22px; min-width: 0; margin-top: 20px; }
  .sessions-header { display: flex; align-items: start; justify-content: space-between; gap: 20px; }
  h3 { display: flex; align-items: center; gap: 10px; margin: 0 0 7px; color: var(--tx); font-size: 18px; font-weight: 700; }
  p { margin: 0; color: var(--mu); font-size: 13px; line-height: 1.65; }
  .session-count { display: inline-flex; align-items: center; justify-content: center; min-width: 25px; height: 25px; padding: 0 7px; box-sizing: border-box; border-radius: var(--r-pill); background: var(--pn-2); color: var(--mu); font-size: 12px; }
  button { display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; gap: 8px; min-height: 44px; padding: 10px 16px; border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 13px; font-weight: 650; cursor: pointer; transition: background-color var(--t-fast) ease; }
  button:hover:not(:disabled) { background: var(--pn-2); }
  button:disabled { opacity: .45; cursor: default; }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .session-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 260px), 1fr)); gap: 12px; }
  article { display: grid; align-content: start; gap: 14px; min-width: 0; padding: 16px; border: 1px solid var(--line); border-radius: var(--r-md); background: var(--pn); }
  article.current { border-color: var(--ac); }
  .session-top { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .device-icon { display: grid; flex: 0 0 42px; place-items: center; height: 42px; border-radius: 12px; background: var(--pn-2); color: var(--tx); }
  .current .device-icon { background: var(--ac-soft); color: var(--ac); }
  .device-heading { display: grid; justify-items: start; min-width: 0; gap: 2px; flex: 1; }
  strong { color: var(--tx); font-size: 14px; font-weight: 700; overflow-wrap: anywhere; }
  .device-heading > strong { font-size: 15px; }
  .client-app { max-width: 100%; color: var(--mu); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .current-badge { display: inline-flex; align-items: center; flex: none; gap: 6px; padding: 3px 8px; border-radius: var(--r-pill); background: var(--ac-soft); font-size: 11px; font-weight: 700; color: var(--ac); }
  .current-badge span { width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .disconnect { width: 100%; min-height: 38px; padding: 8px 12px; background: transparent; }
  dl { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin: 0; }
  dt { color: var(--mu); font-size: 11px; font-weight: 600; }
  dd { margin: 5px 0 0; color: var(--tx); font-size: 12px; line-height: 1.55; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .session-info { display: flex; align-items: start; gap: 10px; color: var(--mu); }
  .session-info :global(svg), .message :global(svg) { flex-shrink: 0; margin-top: 2px; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 20px; padding-top: 22px; border-top: 1px solid var(--line); }
  footer p { margin-top: 5px; }
  .danger { border-color: transparent; background: var(--err-soft); color: var(--err); }
  .danger:hover:not(:disabled) { background: var(--err-soft); opacity: .85; }
  .confirmation { display: grid; gap: 14px; padding: 16px; border: 1px solid var(--line); border-radius: var(--r-md); }
  .confirmation > div { display: flex; justify-content: end; gap: 8px; flex-wrap: wrap; }
  .message { display: flex; align-items: start; gap: 9px; padding: 12px 14px; border-radius: var(--r-sm); }
  .error { color: var(--err); background: var(--err-soft); }
  .notice { color: var(--ac); background: var(--ac-soft); }
  .empty { display: grid; justify-items: center; gap: 10px; padding: 30px 16px; text-align: center; border: 1px dashed var(--line); border-radius: var(--r-lg); color: var(--mu); }
  @media (max-width: 650px) {
    .sessions-header { flex-wrap: wrap; gap: 12px; }
    .refresh { min-height: 40px; padding: 8px 12px; }
    article { padding: 16px; gap: 16px; }
    .device-icon { flex-basis: 42px; height: 42px; }
    dl { grid-template-columns: 1fr; gap: 12px; }
    dl > div { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
    dt { flex-shrink: 0; }
    dd { margin: 0; text-align: right; }
    footer { align-items: start; flex-direction: column; gap: 14px; }
    footer button { width: 100%; }
    .confirmation > div { display: grid; grid-template-columns: 1fr; }
  }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { session } from './session';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  import { type DeviceAccount, type DeviceAccounts, PinRequired, accountInitial, forgetAccount, loadAccounts, markAccountChosen, parkAccount, switchAccount } from './accounts';

  let device = $state<DeviceAccounts | null>(null);
  let error = $state('');
  let busy = $state(false);
  let managing = $state(false);
  const csrf = $derived($session?.csrfToken ?? '');
  const others = $derived(device ? device.accounts.filter((account) => !account.current) : []);
  // Signed out with accounts set aside: the launch screen of a shared device.
  const signedOut = $derived(!$session);
  let pinFor = $state<DeviceAccount | null>(null);
  let pin = $state('');
  let pinError = $state('');
  let firstKey = $state<HTMLButtonElement | null>(null);
  const KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9'];

  async function refresh() {
    try { device = await loadAccounts(); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('accounts_unavailable'); }
  }
  onMount(() => { void refresh(); });

  async function run(action: () => Promise<void>) {
    if (busy) return;
    busy = true; error = '';
    try { await action(); } catch (cause) { error = cause instanceof Error ? cause.message : $t('accounts_unavailable'); }
    finally { busy = false; }
  }

  // A full reload drops every cache filled by the previous account (catalogue, progress, preferences).
  function choose(account: DeviceAccount) {
    if (account.current) { markAccountChosen(); void goto(resolve('/')); return; }
    if (account.hasPin) { askPin(account); return; }
    void run(async () => {
      try { await switchAccount(csrf, account.id); }
      catch (cause) { if (cause instanceof PinRequired) { askPin(account); return; } throw cause; }
      opened();
    });
  }
  function opened() { markAccountChosen(); window.location.assign(resolve('/')); }

  // A short PIN stands in for the password; the keypad works with a TV remote.
  function askPin(account: DeviceAccount) {
    pinFor = account; pin = ''; pinError = '';
    queueMicrotask(() => firstKey?.focus());
  }
  function closePin() { pinFor = null; pin = ''; pinError = ''; }
  function press(digit: string) { if (pin.length < 6) pin += digit; pinError = ''; }
  async function submitPin() {
    const account = pinFor;
    if (!account || busy || pin.length < 4) return;
    busy = true; pinError = '';
    try { await switchAccount(csrf, account.id, pin); opened(); }
    catch (cause) { pinError = cause instanceof Error ? cause.message : $t('accounts_unavailable'); pin = ''; busy = false; }
  }
  function pinKeys(event: KeyboardEvent) {
    if (/^[0-9]$/.test(event.key)) { event.preventDefault(); press(event.key); }
    else if (event.key === 'Backspace') { event.preventDefault(); pin = pin.slice(0, -1); }
    else if (event.key === 'Enter' && pin.length >= 4 && !(event.target instanceof HTMLButtonElement)) { event.preventDefault(); void submitPin(); }
    else if (event.key === 'Escape') { event.preventDefault(); closePin(); }
  }
  // The active account stays signed in, set aside; the sign-in page opens for the new one.
  const add = () => run(async () => {
    if (!signedOut) await parkAccount(csrf);
    window.location.assign(resolve('login/'));
  });
  const forget = (account: DeviceAccount) => run(async () => { await forgetAccount(csrf, account.id); await refresh(); });
</script>

<main class="accounts">
  <h1>{managing ? $t('accounts_manage') : signedOut ? $t('accounts_who') : $t('accounts_title')}</h1>
  {#if !signedOut}<p class="hint">{$t('accounts_intro')}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if device}
    <ul class="grid">
      {#each device.accounts as account (account.id)}
        <li class="tile" class:current={account.current}>
          <button type="button" class="pick" disabled={busy || managing} onclick={() => choose(account)} aria-label={$t('accounts_open', { name: account.name })}>
            <span class="avatar" aria-hidden="true">{accountInitial(account.name)}</span>
            <span class="name">{account.name}</span>
            <span class="badges">
              {#if account.current}<span class="badge">{$t('accounts_active')}</span>
              {:else if account.hasPin}<span class="badge" title={$t('accounts_locked')}><Icon name="shield" size={11} /><span class="sr-only">{$t('accounts_locked')}</span></span>{/if}
            </span>
          </button>
          {#if managing && !account.current}
            <button type="button" class="danger" disabled={busy} onclick={() => forget(account)}>
              <Icon name="trash" size={13} />{$t('accounts_forget')}
            </button>
          {/if}
        </li>
      {/each}
      {#if !managing && device.accounts.length < device.max}
        <li class="tile">
          <button type="button" class="pick" disabled={busy} onclick={add}>
            <span class="avatar add" aria-hidden="true">+</span>
            <span class="name">{$t('accounts_add')}</span>
            <span class="badges"></span>
          </button>
        </li>
      {/if}
    </ul>

    <div class="row">
      {#if others.length || managing}
        <button type="button" onclick={() => { managing = !managing; }}>{managing ? $t('accounts_done') : $t('accounts_manage')}</button>
      {/if}
      {#if !signedOut}<a href={resolve('/')}>{$t('accounts_back')}</a>{/if}
    </div>
  {:else if !error}
    <p class="hint" role="status">{$t('loading')}</p>
  {/if}
</main>

{#if pinFor}
  <div class="pin-backdrop">
    <div class="pin" role="dialog" aria-modal="true" aria-labelledby="pin-title" tabindex="-1" onkeydown={pinKeys}>
      <span class="avatar small" aria-hidden="true">{accountInitial(pinFor.name)}</span>
      <h2 id="pin-title">{$t('accounts_pin_title', { name: pinFor.name })}</h2>
      <div class="dots" role="status" aria-label={$t('accounts_pin_required')}>
        {#each [0, 1, 2, 3, 4, 5] as slot (slot)}<span class:filled={slot < pin.length} class:optional={slot >= 4}></span>{/each}
      </div>
      {#if pinError}<p class="error" role="alert">{pinError}</p>{/if}
      <div class="keys">
        {#each KEYS as key, index (key)}
          {#if index === 0}<button type="button" class="key" bind:this={firstKey} disabled={busy} onclick={() => press(key)}>{key}</button>
          {:else}<button type="button" class="key" disabled={busy} onclick={() => press(key)}>{key}</button>{/if}
        {/each}
        <button type="button" class="key small-text" disabled={busy || !pin} onclick={() => { pin = pin.slice(0, -1); }}>{$t('accounts_pin_clear')}</button>
        <button type="button" class="key" disabled={busy} onclick={() => press('0')}>0</button>
        <button type="button" class="key ok small-text" disabled={busy || pin.length < 4} onclick={submitPin}>{$t('accounts_pin_open')}</button>
      </div>
      <button type="button" class="cancel" disabled={busy} onclick={closePin}>{$t('accounts_pin_cancel')}</button>
    </div>
  </div>
{/if}

<style>
  .accounts { min-height: 100dvh; box-sizing: border-box; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 24px; padding: 48px 16px; background: var(--color-canvas, var(--bg)); color: var(--tx); }
  h1 { margin: 0; font-size: clamp(26px, 4vw, 40px); font-weight: 800; letter-spacing: -0.02em; text-align: center; }
  .grid { display: flex; flex-wrap: wrap; justify-content: center; gap: 24px; margin: 0; padding: 0; list-style: none; }
  .tile { display: flex; flex-direction: column; align-items: center; gap: 10px; width: 140px; }
  .pick { display: flex; flex-direction: column; align-items: center; gap: 10px; width: 100%; padding: 0; border: 0; background: none; color: inherit; font: inherit; cursor: pointer; }
  .pick:disabled { cursor: default; }
  .avatar { display: grid; place-items: center; width: 120px; height: 120px; border-radius: var(--r-lg, 18px); border: 3px solid transparent; background: var(--pn-2); color: var(--tx); font-size: 48px; font-weight: 800; transition: border-color .15s ease, transform .15s ease; }
  .avatar.add { background: none; border: 2px dashed var(--line-2, var(--line)); color: var(--mu); font-weight: 400; }
  .current .avatar { border-color: var(--ac); }
  .pick:not(:disabled):hover .avatar, .pick:focus-visible .avatar { border-color: var(--tx); transform: scale(1.04); }
  .pick:focus-visible { outline: none; }
  .name { font-weight: 700; text-align: center; overflow-wrap: anywhere; }
  .badges { display: flex; justify-content: center; min-height: 20px; }
  .badge { padding: 2px 8px; border-radius: 999px; background: var(--pn); color: var(--mu); font-size: 12px; font-weight: 600; }
  button:not(.pick), a { display: inline-flex; align-items: center; gap: 6px; min-height: 36px; padding: 0 14px; border: 1px solid var(--line); border-radius: var(--r-sm, 10px); background: var(--pn); color: var(--tx); font: inherit; font-weight: 700; text-decoration: none; cursor: pointer; }
  button.danger { min-height: 30px; padding: 0 10px; color: var(--danger, #e5484d); font-size: 13px; }
  button:disabled { opacity: .5; cursor: not-allowed; }
  button:focus-visible, a:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  .row { display: flex; flex-wrap: wrap; justify-content: center; gap: 10px; }
  .error { margin: 0; color: var(--danger, #e5484d); font-weight: 600; text-align: center; }
  .hint { margin: 0; max-width: 52ch; color: var(--mu); font-size: 14px; text-align: center; }
  .badge { display: inline-flex; align-items: center; gap: 4px; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .pin-backdrop { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 16px; background: color-mix(in srgb, var(--bg, #000) 82%, transparent); }
  .pin { display: flex; flex-direction: column; align-items: center; gap: 16px; width: min(360px, 100%); box-sizing: border-box; padding: 28px 20px; border-radius: var(--r-lg, 18px); background: var(--pn); color: var(--tx); }
  .pin:focus { outline: none; }
  .pin h2 { margin: 0; font-size: 20px; font-weight: 800; text-align: center; }
  .avatar.small { width: 64px; height: 64px; font-size: 28px; }
  .dots { display: flex; gap: 12px; }
  .dots span { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--mu); }
  .dots span.optional:not(.filled) { opacity: .4; }
  .dots span.filled { background: var(--tx); border-color: var(--tx); }
  .keys { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; width: 100%; }
  button.key { justify-content: center; min-height: 56px; font-size: 22px; }
  button.key.small-text { font-size: 14px; }
  button.key.ok { background: var(--btn, var(--ac)); color: var(--btn-tx, #fff); }
  button.cancel { background: none; border: 0; color: var(--mu); }
  @media (max-width: 480px) { .tile { width: 108px; } .avatar { width: 96px; height: 96px; font-size: 38px; } }
  @media (prefers-reduced-motion: reduce) { .avatar { transition: none; } }
</style>

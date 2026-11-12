<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { session } from './session';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  import { type DeviceAccount, type DeviceAccounts, accountInitial, forgetAccount, loadAccounts, parkAccount, switchAccount } from './accounts';

  let device = $state<DeviceAccounts | null>(null);
  let error = $state('');
  let busy = $state(false);
  let managing = $state(false);
  const csrf = $derived($session?.csrfToken ?? '');
  const others = $derived(device ? device.accounts.filter((account) => !account.current) : []);

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
    if (account.current) { void goto(resolve('/')); return; }
    void run(async () => { await switchAccount(csrf, account.id); window.location.assign(resolve('/')); });
  }
  // The active account stays signed in, set aside; the sign-in page opens for the new one.
  const add = () => run(async () => { await parkAccount(csrf); window.location.assign(resolve('login/')); });
  const forget = (account: DeviceAccount) => run(async () => { await forgetAccount(csrf, account.id); await refresh(); });
</script>

<main class="accounts">
  <h1>{managing ? $t('accounts_manage') : $t('accounts_title')}</h1>
  <p class="hint">{$t('accounts_intro')}</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if device}
    <ul class="grid">
      {#each device.accounts as account (account.id)}
        <li class="tile" class:current={account.current}>
          <button type="button" class="pick" disabled={busy || managing} onclick={() => choose(account)} aria-label={$t('accounts_open', { name: account.name })}>
            <span class="avatar" aria-hidden="true">{accountInitial(account.name)}</span>
            <span class="name">{account.name}</span>
            <span class="badges">
              {#if account.current}<span class="badge">{$t('accounts_active')}</span>{/if}
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
      <a href={resolve('/')}>{$t('accounts_back')}</a>
    </div>
  {:else if !error}
    <p class="hint" role="status">{$t('loading')}</p>
  {/if}
</main>

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
  @media (max-width: 480px) { .tile { width: 108px; } .avatar { width: 96px; height: 96px; font-size: 38px; } }
  @media (prefers-reduced-motion: reduce) { .avatar { transition: none; } }
</style>

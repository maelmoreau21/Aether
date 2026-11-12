<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from './session';
  import DeviceSessions from './DeviceSessions.svelte';
  import QuickConnectApprove from './QuickConnectApprove.svelte';
  import TwoFactorSettings from './TwoFactorSettings.svelte';
  import AccountPinSettings from './AccountPinSettings.svelte';
  import { t } from '../i18n';
  let { mode = 'security' }: { mode?: 'security' | 'devices' } = $props();
  let canCreate=$state(false),login=$state('');
  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmation = $state('');
  let error = $state('');
  let busy = $state(false);
  onMount(() => { if (mode !== 'security') return; void fetch('/api/v1/account/local-credentials',{cache:'no-store'}).then(r=>r.ok?r.json():{canCreate:false}).then(v=>canCreate=v.canCreate).catch(()=>{}); });
  async function mutate(path: string, method: string, body?: unknown) {
    error = ''; busy = true;
    try {
      const response = await fetch(path, { method, headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' }, body: body === undefined ? undefined : JSON.stringify(body) });
      if (!response.ok) {
        const code = (await response.json()).error;
        throw new Error(({ invalid_current_password: $t('the_current_password_is_incorrect'), invalid_password: $t('use_15_to_128_characters_with'), local_credentials_required: $t('the_password_is_managed_by_your'), recent_login_required: $t('sign_in_again_with_sso_then'), sso_local_accounts_blocked: $t('creating_local_credentials_is_disabled'), local_credentials_conflict: $t('username_unavailable_or_account_already'), try_later: $t('too_many_attempts_try_again_in') } as Record<string,string>)[code] ?? $t('operation_not_possible_try_again'));
      }
      return true;
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('operation_failed'); return false; }
    finally { busy = false; currentPassword = ''; newPassword = ''; confirmation = ''; }
  }
  async function change(event: SubmitEvent) {
    event.preventDefault();
    if (newPassword !== confirmation) { error = $t('the_two_new_passwords_must_be'); return; }
    if (await mutate($session?.user.sso ? '/api/v1/account/local-credentials' : '/api/v1/account/password', $session?.user.sso ? 'POST' : 'PUT', $session?.user.sso ? {login,password:newPassword} : { currentPassword, newPassword })) window.location.reload();
  }
</script>
<section>
{#if mode === 'security'}<h3>{$t('password_sign_in')}</h3>
{#if $session?.user.sso}<p>{$t('your_password_is_managed_by_your')}</p>{/if}
{#if !$session?.user.sso || canCreate}
<form onsubmit={change}>
{#if $session?.user.sso}<p>{$t('sign_in_again_with_sso_before')}</p><label>{$t('local_username')}<input required minlength="3" maxlength="64" pattern="[a-z0-9._-]+" autocomplete="username" bind:value={login} disabled={busy} /></label>{:else}
  <label>{$t('current_password')}<input type="password" autocomplete="current-password" required maxlength="512" bind:value={currentPassword} disabled={busy} /></label>
  {/if}
  <label>{$t('new_password')}<input type="password" autocomplete="new-password" required minlength="15" maxlength="128" bind:value={newPassword} disabled={busy} /></label>
  <label>{$t('confirm_the_new_password')}<input type="password" autocomplete="new-password" required minlength="15" maxlength="128" bind:value={confirmation} disabled={busy} /></label>
  {#if !$session?.user.sso}<p>{$t('all_your_sessions_will_be_signed')}</p>{/if}<button disabled={busy}>{$session?.user.sso ? $t('create_my_local_credentials') : $t('change_the_password')}</button>
</form>{/if}
{#if !$session?.user.sso}<TwoFactorSettings />{/if}
<AccountPinSettings />
{:else}<QuickConnectApprove /><DeviceSessions />
{/if}
{#if error}<p role="alert">{error}</p>{/if}</section>
<style>
  section { display: grid; gap: 12px; margin-top: 24px; min-width: 0; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; color: var(--tx); }
  p { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  form { display: grid; gap: 16px; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input { width: 100%; max-width: 520px; height: 46px; min-height: 46px; box-sizing: border-box; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:hover:not(:disabled) { background: var(--pn-2); }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  input:disabled { opacity: 0.45; }
  button { justify-self: start; display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  form button { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); }
  form button:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  [role='alert'] { padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
</style>

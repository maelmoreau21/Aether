<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from './session';
  import { createPasskey, passkeysSupported } from './webauthn';
  import { t } from '../i18n';

  interface Passkey { id: string; name: string; createdAt: number; lastUsedAt: number | null }
  interface Status { localAccount: boolean; totpEnabled: boolean; recoveryCodesLeft: number; passkeys: Passkey[] }
  interface Enrolment { secret: string; uri: string; qrWidth: number; qrModules: boolean[] }

  let status = $state<Status | null>(null);
  let password = $state('');
  let code = $state('');
  let passkeyName = $state('');
  let enrolment = $state<Enrolment | null>(null);
  let recoveryCodes = $state<string[]>([]);
  let error = $state('');
  let busy = $state(false);
  let canUsePasskeys = $state(false);

  const qrPath = $derived(enrolment?.qrModules.flatMap((dark, index) => dark ? [$t('m_h1v1h_1z', { value: index % enrolment!.qrWidth + 4, value2: Math.floor(index / enrolment!.qrWidth) + 4 })] : []).join('') ?? '');

  const messages = (): Record<string, string> => ({
    invalid_current_password: $t('the_current_password_is_incorrect'),
    invalid_code: $t('mfa_code_incorrect'),
    totp_setup_expired: $t('mfa_setup_expired_start_again'),
    totp_already_enabled: $t('mfa_already_enabled'),
    passkey_limit: $t('passkey_limit_reached'),
    passkey_rejected: $t('passkey_already_registered'),
    invalid_passkey: $t('passkey_not_recognised'),
    unsupported_algorithm: $t('passkey_unsupported'),
    try_later: $t('too_many_attempts_try_again_in')
  });

  async function load() {
    const response = await fetch('/api/v1/account/security', { cache: 'no-store' }).catch(() => null);
    if (response?.ok) status = await response.json();
  }
  onMount(() => { canUsePasskeys = passkeysSupported(); void load(); });

  async function call<T>(path: string, method: string, body?: unknown): Promise<T> {
    const response = await fetch(path, {
      method, cache: 'no-store',
      headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
    if (!response.ok) {
      const failure = (await response.json().catch(() => null))?.error as string | undefined;
      throw new Error((failure && messages()[failure]) || $t('operation_not_possible_try_again'));
    }
    return response.status === 204 ? (undefined as T) : response.json();
  }

  async function run(action: () => Promise<void>) {
    if (busy) return;
    error = ''; busy = true;
    try { await action(); }
    catch (cause) { error = cause instanceof DOMException ? $t('passkey_cancelled_or_unavailable') : cause instanceof Error ? cause.message : $t('operation_failed'); }
    finally { busy = false; password = ''; code = ''; }
  }

  const startTotp = (event: SubmitEvent) => { event.preventDefault(); return run(async () => { recoveryCodes = []; enrolment = await call<Enrolment>('/api/v1/account/security/totp/setup', 'POST', { password }); }); };
  const confirmTotp = (event: SubmitEvent) => { event.preventDefault(); return run(async () => { recoveryCodes = (await call<{ recoveryCodes: string[] }>('/api/v1/account/security/totp/confirm', 'POST', { code: code.replace(/\s/g, '') })).recoveryCodes; enrolment = null; await load(); }); };
  const disableTotp = () => run(async () => { await call('/api/v1/account/security/totp/disable', 'POST', { password }); recoveryCodes = []; await load(); });
  const regenerate = () => run(async () => { recoveryCodes = (await call<{ recoveryCodes: string[] }>('/api/v1/account/security/recovery-codes', 'POST', { password })).recoveryCodes; await load(); });
  const addPasskey = () => run(async () => {
    const options = await call<Parameters<typeof createPasskey>[0]>('/api/v1/account/security/passkeys/options', 'POST', { password });
    const credential = await createPasskey(options);
    await call('/api/v1/account/security/passkeys', 'POST', { ...credential, name: passkeyName.trim() || undefined });
    passkeyName = '';
    await load();
  });
  const removePasskey = (id: string) => run(async () => { await call(`/api/v1/account/security/passkeys/${encodeURIComponent(id)}`, 'DELETE', { password }); await load(); });
  const date = (seconds: number) => new Date(seconds * 1000).toLocaleDateString();
</script>

{#if status?.localAccount}
<section aria-labelledby="mfa-title">
  <h3 id="mfa-title">{$t('two_factor_and_passkeys')}</h3>
  <p>{$t('two_factor_explanation')}</p>

  {#if recoveryCodes.length}
    <div class="codes" role="status">
      <strong>{$t('recovery_codes_shown_once')}</strong>
      <ul>{#each recoveryCodes as recovery (recovery)}<li><code>{recovery}</code></li>{/each}</ul>
    </div>
  {/if}

  {#if enrolment}
    <form onsubmit={confirmTotp}>
      <p>{$t('scan_this_qr_code_with_your')}</p>
      <svg class="qr" viewBox={`0 0 ${enrolment.qrWidth + 8} ${enrolment.qrWidth + 8}`} role="img" aria-label={$t('authenticator_qr_code')} shape-rendering="crispEdges"><path class="qr-background" d={`M0 0h${enrolment.qrWidth + 8}v${enrolment.qrWidth + 8}H0z`} /><path d={qrPath} /></svg>
      <p>{$t('or_enter_this_key_manually')} <code class="secret">{enrolment.secret}</code></p>
      <label>{$t('authenticator_app_code')}<input autocomplete="one-time-code" inputmode="numeric" maxlength="7" required bind:value={code} disabled={busy} /></label>
      <div class="row"><button class="primary" disabled={busy}>{$t('enable_two_factor')}</button><button type="button" onclick={() => (enrolment = null)} disabled={busy}>{$t('cancel')}</button></div>
    </form>
  {:else}
    <label>{$t('current_password')}<input type="password" autocomplete="current-password" maxlength="512" bind:value={password} disabled={busy} /></label>
    <p class="hint">{$t('password_needed_for_security_changes')}</p>

    <h4>{$t('authenticator_app')}</h4>
    {#if status.totpEnabled}
      <p>{$t('two_factor_enabled_codes_left', { value: status.recoveryCodesLeft })}</p>
      <div class="row">
        <button onclick={regenerate} disabled={busy || !password}>{$t('new_recovery_codes')}</button>
        <button onclick={disableTotp} disabled={busy || !password}>{$t('disable_two_factor')}</button>
      </div>
    {:else}
      <form onsubmit={startTotp}><button class="primary" disabled={busy || !password}>{$t('set_up_an_authenticator_app')}</button></form>
    {/if}

    <h4>{$t('passkeys')}</h4>
    {#if status.passkeys.length}
      <ul class="keys">
        {#each status.passkeys as key (key.id)}
          <li><span><strong>{key.name}</strong> · {$t('passkey_added_on', { value: date(key.createdAt) })}{#if key.lastUsedAt} · {$t('passkey_last_used', { value: date(key.lastUsedAt) })}{/if}</span><button onclick={() => removePasskey(key.id)} disabled={busy || !password}>{$t('remove')}</button></li>
        {/each}
      </ul>
    {:else}
      <p>{$t('no_passkey_yet')}</p>
    {/if}
    {#if canUsePasskeys}
      <div class="row">
        <input aria-label={$t('passkey_name')} placeholder={$t('passkey_name')} maxlength="64" bind:value={passkeyName} disabled={busy} />
        <button class="primary" onclick={addPasskey} disabled={busy || !password}>{$t('add_a_passkey')}</button>
      </div>
    {:else}
      <p class="hint">{$t('passkey_unsupported')}</p>
    {/if}
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
{/if}

<style>
  section { display: grid; gap: 12px; margin-top: 24px; min-width: 0; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; color: var(--tx); }
  h4 { margin: 8px 0 0; font-size: 15px; font-weight: 700; color: var(--tx); }
  p { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .hint { font-size: 13px; }
  form { display: grid; gap: 12px; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input { width: 100%; max-width: 520px; height: 46px; box-sizing: border-box; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; }
  input:hover:not(:disabled) { background: var(--pn-2); }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  input:disabled { opacity: 0.45; }
  .row { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; }
  .row input { flex: 1 1 220px; width: auto; }
  button { justify-self: start; display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease; }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .primary { background: var(--btn); color: var(--btn-tx); }
  .primary:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  .qr { width: 200px; max-width: 100%; height: auto; border-radius: var(--r-sm); }
  .qr path { fill: #000; }
  .qr .qr-background { fill: #fff; }
  code { font-family: var(--font-mono); font-size: 14px; color: var(--tx); }
  .secret { overflow-wrap: anywhere; }
  .codes { display: grid; gap: 8px; padding: 14px; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); }
  .codes ul { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 6px; margin: 0; padding: 0; list-style: none; }
  .keys { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .keys li { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; justify-content: space-between; padding: 10px 14px; border-radius: var(--r-sm); background: var(--pn); color: var(--mu); font-size: 14px; min-width: 0; }
  .keys strong { color: var(--tx); }
  [role='alert'] { padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
</style>

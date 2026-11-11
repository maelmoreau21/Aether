<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from './session';
  import { t } from '../i18n';
  import { isSafeHttpUrl } from '../safe-url';
  let { embedded = false }: { embedded?: boolean } = $props();
  let enabled = $state(false);
  let message = $state('');
  let password = $state('');
  let busy = $state(false);
  onMount(() => {
    const controller = new AbortController();
    fetch('/api/v1/auth/oidc/status', { signal: controller.signal }).then(r => r.json()).then(s => { enabled = s.enabled === true; }).catch(() => { message = $t('sso_status_unavailable'); });
    return () => controller.abort();
  });
  async function link(event: SubmitEvent) {
    event.preventDefault(); busy = true; message = '';
    try {
      const response = await fetch('/api/v1/auth/oidc/link', { method: 'POST', headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' }, body: JSON.stringify({ password }) });
      if (!response.ok) throw new Error($t('link_refused_check_your_local_password'));
      const data: { url: string } = await response.json();
      if (!data.url || !isSafeHttpUrl(data.url)) throw new Error($t('link_unavailable'));
      window.location.assign(data.url);
    } catch (failure) { message = failure instanceof Error ? failure.message : $t('link_unavailable'); busy = false; }
    finally { password = ''; }
  }
</script>
<section class:embedded aria-label={embedded ? $t('sso_link') : undefined} aria-labelledby={embedded ? undefined : 'oidc-title'}>
  {#if !embedded}<h2 id="oidc-title">{$t('sso_connection')}</h2>{/if}
  <p>{enabled ? $t('sso_sign_in_enabled') : $t('sso_disabled')}</p>
  {#if enabled && $session?.user.sso}<p>{$t('your_account_uses_sso_sign_in')}</p>{/if}
  {#if enabled && !$session?.user.sso}
    <form onsubmit={link}>
      <p>{$t('to_link_your_local_account_to')}</p>
      <label>{$t('local_password')} <input type="password" autocomplete="current-password" required maxlength="512" bind:value={password} disabled={busy} /></label>
      <button type="submit" disabled={busy}>{busy ? 'Connexion…' : $t('link_my_local_account_to_sso')}</button>
    </form>
  {/if}
  {#if message}<p role="alert">{message}</p>{/if}
</section>
<style>
  section { margin-bottom: 20px; max-width: 700px; padding: 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); color: var(--tx); overflow-wrap: anywhere; }
  section.embedded { background: transparent; padding: 0; border: 0; border-radius: 0; margin: 0; max-width: none; }
  h2 { margin: 0 0 12px; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  p { margin: 0 0 12px; color: var(--mu); font-size: 14px; line-height: 1.6; }
  form { display: grid; gap: 16px; }
  form p { margin: 0; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input { width: 100%; max-width: 520px; height: 46px; min-height: 46px; box-sizing: border-box; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:hover:not(:disabled) { background: var(--pn-2); }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  button { justify-self: start; display: inline-flex; align-items: center; justify-content: center; height: 48px; padding: 0 22px; border: 0; border-radius: var(--r-md); background: var(--btn); color: var(--btn-tx); font: inherit; font-size: 15px; font-weight: 700; cursor: pointer; transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { opacity: 0.9; }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  [role='alert'] { margin: 12px 0 0; padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  @media (max-width: 640px) { section:not(.embedded) { padding: 16px; border-radius: var(--r-md); } }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from './session';
  import { t } from '../i18n';
  import { loadAccounts, setAccountPin } from './accounts';

  let hasPin = $state(false);
  let pin = $state('');
  let busy = $state(false);
  let message = $state('');
  let error = $state('');

  onMount(() => {
    void loadAccounts()
      .then((device) => { hasPin = device.accounts.some((account) => account.current && account.hasPin); })
      .catch(() => undefined);
  });

  async function apply(next: string | null) {
    const user = $session?.user.id;
    if (!user || busy) return;
    busy = true; message = ''; error = '';
    try {
      await setAccountPin($session?.csrfToken ?? '', user, next);
      hasPin = next !== null;
      pin = '';
      message = next === null ? $t('accounts_pin_removed') : $t('accounts_pin_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('accounts_unavailable');
    } finally {
      busy = false;
    }
  }

  function save(event: SubmitEvent) {
    event.preventDefault();
    if (!/^[0-9]{4,6}$/.test(pin)) { error = $t('accounts_pin_format'); return; }
    void apply(pin);
  }
</script>

<h3>{$t('accounts_pin_section')}</h3>
<p>{$t('accounts_pin_help')}</p>
{#if hasPin}<p class="state">{$t('accounts_pin_active')}</p>{/if}
<form onsubmit={save}>
  <label>{$t('accounts_pin_new')}
    <input type="password" inputmode="numeric" autocomplete="off" pattern={'[0-9]{4,6}'} minlength="4" maxlength="6" required bind:value={pin} disabled={busy} />
  </label>
  <div class="actions">
    <button disabled={busy}>{$t('accounts_pin_save')}</button>
    {#if hasPin}<button type="button" class="secondary" disabled={busy} onclick={() => apply(null)}>{$t('accounts_pin_remove')}</button>{/if}
  </div>
</form>
{#if message}<p role="status">{message}</p>{/if}
{#if error}<p role="alert">{error}</p>{/if}

<style>
  h3 { margin: 16px 0 0; font-size: 17px; font-weight: 700; color: var(--tx); }
  p { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .state { color: var(--tx); font-weight: 600; }
  form { display: grid; gap: 16px; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input { width: 100%; max-width: 220px; height: 46px; box-sizing: border-box; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 18px; letter-spacing: .3em; }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  .actions { display: flex; flex-wrap: wrap; gap: 10px; }
  button { min-height: 48px; padding: 0 22px; border: 0; border-radius: var(--r-md); background: var(--btn); color: var(--btn-tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; }
  button.secondary { background: var(--pn-2); color: var(--tx); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  [role='alert'] { padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
</style>

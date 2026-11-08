<script lang="ts">
  import { session } from '../auth/session';
  import { t } from '../i18n';
  let { userId }: { userId: string } = $props();
  const kindOptions = [{ id: 'films', get label() { return $t('films'); } }, { id: 'series', get label() { return $t('series'); } }, { id: 'music', get label() { return $t('music'); } }] as const;
  let kinds = $state<string[]>(['films', 'series', 'music']), effective = $state<string[]>([]);
  let allow = $state(false), loaded = $state(false), busy = $state(false), error = $state(''), notice = $state('');
  $effect(() => {
    const id = userId, controller = new AbortController(); loaded = false; error = ''; notice = '';
    fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/media-policy`, { signal: controller.signal, cache: 'no-store' })
      .then(async response => { if (!response.ok) throw new Error($t('permissions_unavailable')); return response.json(); })
      .then(value => { if (!controller.signal.aborted) { allow = value.allowMediaDownload; kinds = Array.isArray(value.allowedKinds) ? value.allowedKinds : ['films', 'series', 'music']; effective = Array.isArray(value.effectiveKinds) ? value.effectiveKinds : kinds; loaded = true; } })
      .catch(cause => { if (!controller.signal.aborted) error = cause.message; });
    return () => controller.abort();
  });
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (busy || !loaded) return;
    busy = true; error = ''; notice = ''; const id = userId;
    try {
      const response = await fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/media-policy`, { method:'PUT', headers:{'content-type':'application/json','x-csrf-token':$session?.csrfToken ?? ''}, body:JSON.stringify({allowMediaDownload:allow, allowedKinds:kindOptions.map(option => option.id).filter(kind => kinds.includes(kind))}) });
      if (!response.ok) throw new Error($t('permissions_not_saved'));
      if (id === userId) { notice = $t('permissions_saved'); effective = effective.filter(kind => kinds.includes(kind)); }
    } catch (cause) { if (id === userId) error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }
</script>
<form onsubmit={save}><fieldset disabled={busy || !loaded}><legend>{$t('allowed_media_types')}</legend>
  {#each kindOptions as option (option.id)}
    <label><input type="checkbox" value={option.id} bind:group={kinds} />{option.label}</label>
  {/each}
  <p>{$t('an_unticked_type_disappears_from_the')}</p>
  {#if loaded && effective.length < 3}<p>{$t('effective_kinds', { kinds: effective.length ? kindOptions.filter(option => effective.includes(option.id)).map(option => option.label).join(', ') : $t('none_lower') })}</p>{/if}
</fieldset><fieldset disabled={busy || !loaded}><legend>{$t('download_and_sync')}</legend>
  <label><input type="checkbox" bind:checked={allow} />{$t('allow_downloading_original_files_and_new')}</label>
  <p>{$t('this_right_controls_the_download_offered')}</p>
  <button>{$t('save_permissions')}</button>
</fieldset>{#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}</form>
<style>
form { display: grid; gap: 16px; font-family: var(--font-sans); color: var(--color-text); }
  fieldset { display: grid; gap: 12px; min-width: 0; margin: 0; padding: 0; border: 0; }
  fieldset:disabled { opacity: 0.7; }
  legend { margin: 0 0 16px; padding: 0; font-size: 17px; font-weight: 700; }
  label { display: flex; align-items: center; gap: 12px; min-height: 44px; font-size: 14px; font-weight: 600; cursor: pointer; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }
  p { max-width: 720px; margin: 0; color: var(--color-muted); font-size: 13px; line-height: 1.5; }
  button {
    justify-self: start;
    height: 48px;
    margin-top: 8px;
    padding: 0 22px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--color-accent);
    color: var(--color-on-accent);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity var(--transition-fast), transform var(--transition-fast);
  }
  button:hover:not(:disabled) { background: var(--color-accent-hover); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  p[role='alert'], p[role='status'] { max-width: none; padding: 12px 16px; border-radius: var(--radius-md); color: var(--color-text); font-size: 14px; }
  p[role='alert'] { background: var(--color-danger-soft); }
  p[role='status'] { background: var(--color-success-soft); }
  @media (max-width: 640px) {
    button { width: 100%; justify-self: stretch; }
  }
  @media (prefers-reduced-motion: reduce) {
    button { transition: none; }
  }
</style>

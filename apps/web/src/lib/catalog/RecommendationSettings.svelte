<script lang="ts">
  import { session } from '../auth/session';
  import { t } from '../i18n';
  type Options = { enabled: boolean; excludeWatched: boolean; historyDays: number };
  let options = $state<Options>({ enabled: true, excludeWatched: true, historyDays: 180 });
  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let notice = $state('');
  $effect(() => {
    const owner = $session?.user.id;
    options = { enabled: true, excludeWatched: true, historyDays: 180 };
    error = ''; notice = ''; loading = true;
    const controller = new AbortController();
    if (!owner) { loading = false; return; }
    void fetch('/api/v1/account/recommendations/preferences', { credentials: 'same-origin', cache: 'no-store', signal: controller.signal })
      .then(async response => { if (!response.ok) throw new Error(); const value = await response.json(); if (!controller.signal.aborted) options = value; })
      .catch(() => { if (!controller.signal.aborted) error = $t('unable_to_load_the_preferences'); })
      .finally(() => { loading = false; });
    return () => controller.abort();
  });
  async function save(event: SubmitEvent) {
    event.preventDefault(); saving = true; error = ''; notice = '';
    const owner = $session?.user.id;
    try {
      const response = await fetch('/api/v1/account/recommendations/preferences', {
        method: 'PUT', credentials: 'same-origin', cache: 'no-store',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify(options), signal: AbortSignal.timeout(10000)
      });
      if (!response.ok) throw new Error();
      const value = await response.json();
      if ($session?.user.id !== owner) return;
      options = value; notice = $t('preferences_saved');
      window.dispatchEvent(new Event('media-recommendations-changed'));
    } catch { if ($session?.user.id === owner) error = $t('saving_failed_check_your_session_and'); }
    finally { saving = false; }
  }
</script>
<section class="card" aria-labelledby="recommendations-heading">
  <h2 id="recommendations-heading">{$t('personal_recommendations')}</h2>
  <p>{$t('computed_locally_from_the_genres_people')}</p>
  <form onsubmit={save}>
    <fieldset disabled={loading || saving}>
      <label><input type="checkbox" bind:checked={options.enabled} /> {$t('enable_recommendations_on_the_home_page')}</label>
      <label><input type="checkbox" bind:checked={options.excludeWatched} /> {$t('exclude_content_already_watched')}</label>
      <label>{$t('history_used_days')}<input type="number" min="1" max="3650" required bind:value={options.historyDays} /></label>
      <p>{$t('this_duration_limits_the_computation_it')}</p>
      <button type="submit">{saving ? 'Enregistrement…' : $t('save_recommendations')}</button>
    </fieldset>
  </form>
  {#if loading}<p role="status">{$t('loading_settings_short')}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  .card { display: grid; gap: 14px; min-width: 0; padding: 24px; margin-bottom: 20px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); color: var(--tx); overflow-wrap: anywhere; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  p { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  form { margin: 0; }
  fieldset { display: grid; gap: 14px; min-width: 0; margin: 0; padding: 0; border: 0; }
  fieldset:disabled { opacity: 0.7; }
  label { display: flex; align-items: center; gap: 12px; min-height: 44px; flex-wrap: wrap; font-size: 14px; font-weight: 600; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  input[type='number'] { width: 120px; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; font-variant-numeric: tabular-nums; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input[type='number']:hover { background: var(--pn-2); }
  input[type='number']:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  button { justify-self: start; height: 48px; padding: 0 22px; border: 0; border-radius: var(--r-md); background: var(--btn); color: var(--btn-tx); font: inherit; font-size: 15px; font-weight: 700; cursor: pointer; transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { opacity: 0.9; }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  [role='alert'] { padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  @media (max-width: 640px) { .card { padding: 16px; border-radius: var(--r-md); } }
</style>

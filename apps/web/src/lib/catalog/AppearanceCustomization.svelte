<script lang="ts">
  import { preferenceState, savePreferences, DEFAULT_HOME_SECTIONS, type HomeSection } from '../preferences';
  import { t, translatedLabels } from '../i18n';
  const names: Readonly<Record<HomeSection, string>> = translatedLabels({ hero: 'featured', continue: 'resume_watching', tonight: 'for_tonight', films: 'recent_films', series: 'recent_series', rated: 'top_rated' });
  const sections = $derived($preferenceState.homeSections ?? DEFAULT_HOME_SECTIONS);
  let saveError = $state('');
  async function save(patch: Parameters<typeof savePreferences>[0]) {
    saveError = await savePreferences(patch) ? '' : $t('saving_failed_change_the_setting_again');
  }
  function move(index: number, direction: number) {
    const next = [...sections];
    [next[index], next[index + direction]] = [next[index + direction], next[index]];
    void save({ homeSections: next });
  }
  function toggle(section: HomeSection) {
    void save({ homeSections: sections.includes(section) ? sections.filter(item => item !== section) : [...sections, section] });
  }
</script>
<section class="customization" aria-labelledby="customization-title">
  <h2 id="customization-title">{$t('customise_the_display')}</h2>
  <p>{$t('these_choices_follow_your_account_across')}</p>
  <div class="choices">
    <label>{$t('poster_size')}
      <select value={$preferenceState.cardSize ?? 'medium'} onchange={event => void save({ cardSize: event.currentTarget.value as 'small' | 'medium' | 'large' })}>
        <option value="small">{$t('small')}</option><option value="medium">{$t('stats_average')}</option><option value="large">{$t('large')}</option>
      </select>
    </label>
    <label>{$t('card_spacing')}
      <select value={$preferenceState.cardDensity ?? 'comfortable'} onchange={event => void save({ cardDensity: event.currentTarget.value as 'comfortable' | 'compact' })}>
        <option value="comfortable">{$t('comfortable')}</option><option value="compact">{$t('compact')}</option>
      </select>
    </label>
  </div>
  <h3>{$t('home_sections')}</h3>
  <ol>
    {#each sections as section, index (section)}
      <li><span>{names[section]}</span>
        <button type="button" aria-label={$t('move_up', { value: names[section] })} disabled={index === 0} onclick={() => move(index, -1)}>↑</button>
        <button type="button" aria-label={$t('move_down', { value: names[section] })} disabled={index === sections.length - 1} onclick={() => move(index, 1)}>↓</button>
        <button type="button" onclick={() => toggle(section)}>{$t('hide')}</button>
      </li>
    {/each}
  </ol>
  {#each DEFAULT_HOME_SECTIONS.filter(section => !sections.includes(section)) as section (section)}
    <button class="restore" type="button" onclick={() => toggle(section)}>{$t('show', { value: names[section] })}</button>
  {/each}
  <button type="button" onclick={() => void save({ homeSections: [...DEFAULT_HOME_SECTIONS], cardSize: 'medium', cardDensity: 'comfortable' })}>{$t('restore_default_display')}</button>
  {#if saveError}<p role="alert">{saveError}</p>{/if}
</section>
<style>
  .customization { display: grid; gap: 16px; min-width: 0; padding: 24px; margin-bottom: 20px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); color: var(--tx); overflow-wrap: anywhere; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  h3 { margin: 8px 0 0; padding-top: 20px; border-top: 1px solid var(--line); font-size: 17px; font-weight: 700; }
  p { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .choices { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px 20px; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; }
  select { width: 100%; max-width: 520px; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  select:hover { background-color: var(--pn-2); }
  select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  button { justify-self: start; display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, select:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  [role='alert'] { margin: 0; padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  ol { display: grid; margin: 0; padding: 0; list-style: none; border-radius: var(--r-md); background: var(--pn); overflow: hidden; }
  li { display: flex; align-items: center; gap: 8px; min-height: 56px; padding: 6px 8px 6px 16px; border-top: 1px solid var(--line); flex-wrap: wrap; }
  li:first-child { border-top: 0; }
  li span { flex: 1; min-width: 120px; font-size: 15px; font-weight: 600; }
  li button { min-width: 44px; padding: 0 14px; background: transparent; color: var(--mu); }
  li button:hover:not(:disabled) { background: var(--pn-2); color: var(--tx); }
  .restore { margin: 0 8px 0 0; }
  @media (max-width: 640px) { .customization { padding: 16px; border-radius: var(--r-md); } }
</style>

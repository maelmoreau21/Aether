<script lang="ts">
  import { onMount } from 'svelte';
  import { fmt, locale, localeOptions, setLocale, t, type SupportedLocale } from '../i18n';
  import { ensurePreferences, preferenceState, savePreferences, type PreferencePatch } from '../preferences';
  import { METADATA_LANGUAGES, MAX_FALLBACK_LANGUAGES, cleanFallbacks } from '../i18n/languages';
  import Icon from '../ui/Icon.svelte';

  // Five independent choices. Each one is saved on its own; a missing value means "follow the
  // setting shown in its description", never a hidden copy of another language.
  const prefs = $derived($preferenceState);
  let status = $state<'' | 'saved' | 'failed'>('');
  let pendingFallback = $state('');

  onMount(() => { void ensurePreferences().catch(() => {}); });

  async function save(patch: PreferencePatch) {
    status = '';
    status = (await savePreferences(patch)) ? 'saved' : 'failed';
  }
  const valueOf = (event: Event) => (event.currentTarget as HTMLSelectElement).value;
  const optional = (value: string) => (value === '' ? null : value);

  const fallbacks = $derived(prefs.metadataFallbackLocales ?? []);
  const metadataLanguage = $derived(prefs.metadataLocale ?? '');
  const audioChoices = $derived(['original', ...METADATA_LANGUAGES.map(language => language.split('-')[0]).filter((code, index, all) => all.indexOf(code) === index)]);

  function addFallback() {
    if (!pendingFallback) return;
    const next = cleanFallbacks([...fallbacks, pendingFallback], metadataLanguage || $locale);
    pendingFallback = '';
    void save({ metadataFallbackLocales: next.length ? next : null });
  }
  function removeFallback(code: string) {
    const next = fallbacks.filter(language => language !== code);
    void save({ metadataFallbackLocales: next.length ? next : null });
  }
  const choosable = $derived(METADATA_LANGUAGES.filter(code => !fallbacks.includes(code) && code !== (metadataLanguage || $locale)));
</script>

<section class="card languages" aria-labelledby="languages-heading">
  <h2 id="languages-heading">{$t('lang_page_title')}</h2>
  <p class="muted">{$t('lang_page_intro')}</p>

  <div class="language-block">
    <h3><Icon name="globe" size={16} /> {$t('lang_interface_title')}</h3>
    <label>
      <span>{$t('language')}</span>
      <select value={$locale} onchange={(event) => setLocale(valueOf(event) as SupportedLocale)}>
        {#each localeOptions as option (option.code)}<option value={option.code}>{$fmt.language(option.code)}</option>{/each}
      </select>
    </label>
    <p class="muted">{$t('lang_interface_note')}</p>
  </div>

  <div class="language-block">
    <h3><Icon name="film" size={16} /> {$t('lang_metadata_title')}</h3>
    <label>
      <span>{$t('lang_metadata_label')}</span>
      <select value={metadataLanguage} onchange={(event) => void save({ metadataLocale: optional(valueOf(event)) })}>
        <option value="">{$t('lang_follow_interface')}</option>
        {#each METADATA_LANGUAGES as code (code)}<option value={code}>{$fmt.language(code)}</option>{/each}
      </select>
    </label>
    <p class="muted">{$t('lang_metadata_note')}</p>

    <fieldset class="fallbacks">
      <legend>{$t('lang_fallbacks')}</legend>
      {#if fallbacks.length}
        <ol>
          {#each fallbacks as code, index (code)}
            <li>
              <span>{index + 1}. {$fmt.language(code)}</span>
              <button type="button" class="link-button" onclick={() => removeFallback(code)} aria-label={$t('lang_fallback_remove', { language: $fmt.language(code) })}>{$t('remove')}</button>
            </li>
          {/each}
        </ol>
      {:else}
        <p class="muted">{$t('lang_fallbacks_none')}</p>
      {/if}
      {#if fallbacks.length < MAX_FALLBACK_LANGUAGES}
        <div class="fallback-add">
          <label>
            <span class="visually-hidden">{$t('lang_fallback_add')}</span>
            <select bind:value={pendingFallback} aria-label={$t('lang_fallback_add')}>
              <option value="">{$t('lang_fallback_add')}</option>
              {#each choosable as code (code)}<option value={code}>{$fmt.language(code)}</option>{/each}
            </select>
          </label>
          <button type="button" disabled={!pendingFallback} onclick={addFallback}>{$t('lang_add_button')}</button>
        </div>
      {:else}
        <p class="muted">{$t('lang_fallbacks_limit')}</p>
      {/if}
    </fieldset>
    <p class="muted">{$t('lang_availability_note')}</p>
  </div>

  <div class="language-block">
    <h3><Icon name="image" size={16} /> {$t('lang_artwork_title')}</h3>
    <label>
      <span>{$t('lang_artwork_label')}</span>
      <select value={prefs.artworkLocale ?? ''} onchange={(event) => void save({ artworkLocale: optional(valueOf(event)) })}>
        <option value="">{$t('lang_follow_metadata')}</option>
        {#each METADATA_LANGUAGES as code (code)}<option value={code}>{$fmt.language(code)}</option>{/each}
      </select>
    </label>
    <p class="muted">{$t('lang_artwork_note')}</p>
  </div>

  <div class="language-block">
    <h3><Icon name="play" size={16} /> {$t('lang_audio_title')}</h3>
    <label>
      <span>{$t('settings_pref_audio')}</span>
      <select value={prefs.audioLanguage ?? ''} onchange={(event) => void save({ audioLanguage: optional(valueOf(event)) })}>
        <option value="">{$t('lang_follow_interface')}</option>
        {#each audioChoices as code (code)}<option value={code}>{code === 'original' ? $t('original_version') : $fmt.language(code)}</option>{/each}
      </select>
    </label>
    <p class="muted">{$t('lang_audio_note')}</p>
  </div>

  <div class="language-block">
    <h3><Icon name="info" size={16} /> {$t('lang_subtitles_title')}</h3>
    <label>
      <span>{$t('lang_subtitles_label')}</span>
      <select value={prefs.subtitleLanguage ?? ''} onchange={(event) => void save({ subtitleLanguage: optional(valueOf(event)) })}>
        <option value="">{$t('lang_follow_metadata')}</option>
        {#each audioChoices.filter(code => code !== 'original') as code (code)}<option value={code}>{$fmt.language(code)}</option>{/each}
      </select>
    </label>
    <p class="muted">{$t('lang_subtitles_note')}</p>
  </div>

  <p class="status" role="status" aria-live="polite">
    {#if status === 'saved'}{$t('lang_saved')}{:else if status === 'failed'}{$t('save_preferences_failed')}{/if}
  </p>
</section>

<style>
  .languages { display: grid; gap: var(--space-4, 1.25rem); }
  .language-block { display: grid; gap: 0.5rem; padding-top: 0.75rem; border-top: 1px solid var(--border, rgba(127, 127, 127, 0.25)); }
  .language-block h3 { display: flex; align-items: center; gap: 0.5rem; margin: 0; }
  label { display: grid; gap: 0.25rem; }
  .fallbacks { border: 0; padding: 0; margin: 0.25rem 0 0; display: grid; gap: 0.5rem; }
  .fallbacks legend { font-weight: 600; padding: 0; }
  .fallbacks ol { list-style: none; margin: 0; padding: 0; display: grid; gap: 0.25rem; }
  .fallbacks li { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; }
  .fallback-add { display: flex; gap: 0.5rem; align-items: end; }
  .fallback-add label { flex: 1; }
  .link-button { background: none; border: 0; padding: 0; color: inherit; text-decoration: underline; cursor: pointer; }
  .visually-hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .status { min-height: 1.25em; margin: 0; }
</style>

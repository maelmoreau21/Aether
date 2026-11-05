<script lang="ts">
  import type { LibraryOptions } from './types';
  import AdvancedLibraryOptions from './AdvancedLibraryOptions.svelte';
  import { t } from '../i18n';
  let { options = $bindable(), kind, section = 'metadata' }: { options: LibraryOptions; kind: string; section?: 'metadata' | 'indexing' | 'advanced' } = $props();
  const images = [
    ['fetchPoster', 'Affiche', 'Portrait · fiche et catalogue'],
    ['fetchBackdrop', 'Fond', 'Paysage · arrière-plan'],
    ['fetchLogo', 'Logo transparent', 'Identité visuelle du titre'],
    ['fetchThumb', 'Miniature', 'Aperçu du média'],
    ['fetchBanner', 'Bannière', 'Illustration panoramique'],
    ['fetchDisc', 'Disque', 'Illustration du disque']
  ] as const;
</script>

<div class="library-options">
  {#if section === 'metadata'}
    <section class="option-section" aria-labelledby="metadata-options-heading">
      <h3 id="metadata-options-heading">{$t('metadata_language_section')}</h3>
      <p>{$t('library_options_intro')}</p>
      <div class="language-grid">
        <div><label for="library-metadata-language">{$t('preferred_language')}</label><input id="library-metadata-language" bind:value={options.preferredMetadataLanguage} required minlength="2" maxlength="35" placeholder="fr" aria-describedby="language-hint" /><p id="language-hint" class="hint">{$t('language_code_hint')}</p></div>
        <div><label for="library-metadata-country">{$t('classification_country')}</label><input id="library-metadata-country" bind:value={options.metadataCountryCode} required pattern={'[A-Z]{2}'} maxlength="2" placeholder="FR" aria-describedby="country-hint" /><p id="country-hint" class="hint">{$t('country_code_hint')}</p></div>
      </div>
      {#if kind !== 'music'}<label class="toggle-row"><input type="checkbox" bind:checked={options.readLocalNfo} aria-label={$t('local_nfo')} /><span><strong>{$t('local_nfo')}</strong><small>{$t('local_nfo_hint')}</small></span></label>
      {:else}<p>{$t('music_embedded_tags')}</p>{/if}
    </section>
    {#if kind !== 'music'}
      <section class="option-section" aria-labelledby="local-images-heading"><h3 id="local-images-heading">{$t('local_images')}</h3><p>{$t('local_images_note')}</p>
        <div class="image-grid">{#each images as [key, label, description] (key)}<label class="image-option"><input type="checkbox" bind:checked={options[key]} aria-label={label} /><span><strong>{label}</strong><small>{description}</small></span></label>{/each}</div>
      </section>
      <section class="option-section" aria-labelledby="image-quality-heading">
        <h3 id="image-quality-heading">{$t('provider_images')}</h3>
        <p>{$t('provider_images_note')}</p>
        <label class="toggle-row"><input type="checkbox" bind:checked={options.artworkDeduplicate} /><span><strong>{$t('deduplicate_artwork')}</strong><small>{$t('deduplicate_artwork_hint')}</small></span></label>
        {#if options.artworkDeduplicate}<div><label for="artwork-similarity">{$t('similarity_tolerance')}</label><input id="artwork-similarity" type="number" min="0" max="16" step="1" required bind:value={options.artworkSimilarity} /><p class="hint">{$t('artwork_size_note')}</p></div>{/if}
        <div class="language-grid"><div><label for="artwork-min-width">{$t('poster_min_width')}</label><input id="artwork-min-width" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterWidth} /></div><div><label for="artwork-min-height">{$t('poster_min_height')}</label><input id="artwork-min-height" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterHeight} /></div></div>
        <p class="hint">{$t('artwork_size_note')}</p>
      </section>
      <section class="option-section" aria-labelledby="artwork-rotation-heading"><h3 id="artwork-rotation-heading">{$t('artwork_rotation_section')}</h3>
        <p>{$t('artwork_rotation_note_library')}</p>
      </section>
    {/if}
  {:else if section === 'indexing'}
    {#if kind !== 'music'}
      <section class="option-section" aria-labelledby="local-scan-options-heading"><h3 id="local-scan-options-heading">{$t('import_playback')}</h3>
        <label class="toggle-row"><input type="checkbox" bind:checked={options.preferEmbeddedTitles} aria-label={$t('prefer_embedded_titles')} /><span><strong>{$t('prefer_embedded_titles')}</strong><small>{$t('prefer_embedded_titles_hint')}</small></span></label>
        <label for="embedded-subtitles">{$t('embedded_subtitles')}</label><select id="embedded-subtitles" bind:value={options.embeddedSubtitlePolicy}><option value="allow_all">{$t('all_embedded_subtitles')}</option><option value="allow_text">{$t('text_subtitles_only')}</option><option value="block_all">{$t('no_embedded_subtitles')}</option></select>
        <p class="hint">{$t('embedded_subtitles_note')}</p>
      </section>
      <section class="option-section" aria-labelledby="local-refresh-heading"><h3 id="local-refresh-heading">{$t('local_refresh')}</h3>
        <label for="library-metadata-refresh">{$t('reread_local_metadata')}</label><select id="library-metadata-refresh" bind:value={options.automaticRefreshIntervalDays}><option value={0}>{$t('never_automatically')}</option><option value={30}>{$t('every_days', { count: 30 })}</option><option value={60}>{$t('every_days', { count: 60 })}</option><option value={90}>{$t('every_days', { count: 90 })}</option></select>
        <p class="hint">{$t('local_refresh_note')}</p>
      </section>
    {:else}<p>{$t('music_tags_note')}</p>{/if}
  {:else}
    <AdvancedLibraryOptions bind:options {kind} />
  {/if}
</div>

<style>
  .library-options { display: grid; gap: 28px; min-width: 0; font-family: var(--font); color: var(--tx); }
  .option-section { min-width: 0; }
  .option-section + .option-section { padding-top: 24px; border-top: 1px solid var(--color-line-subtle); }
  h3 { margin: 0; font-size: 20px; font-weight: 800; letter-spacing: -0.02em; }
  p, .hint { font-size: 14px; line-height: 1.6; color: var(--mu); }
  .hint { margin: 8px 0 0; font-size: 13px; }
  .language-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px; }
  label { display: block; margin: 20px 0 8px; font-size: 14px; font-weight: 700; line-height: 1.5; color: var(--tx); }
  input:not([type='checkbox']), select { width: 100%; min-width: 0; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-sm); background-color: var(--color-surface); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  input:not([type='checkbox']):hover, select:hover:not(:disabled) { background-color: var(--color-surface-hover); border-color: var(--color-line); }
  input:not([type='checkbox']):focus, select:focus, select:focus-visible { outline: none; border-color: var(--ac); background-color: var(--color-surface); }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 2px 0 0; accent-color: var(--ac); }
  .toggle-row { display: flex; align-items: flex-start; gap: 12px; min-height: 44px; margin: 8px 0 0; padding-block: 12px; font-weight: 400; cursor: pointer; }
  strong { font-size: 15px; font-weight: 700; }
  small { display: block; margin-top: 4px; font-size: 13px; font-weight: 500; line-height: 1.5; color: var(--mu); }
  .image-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; margin-top: 16px; }
  .image-option { display: flex; align-items: flex-start; gap: 12px; min-height: 44px; margin: 0; padding: 14px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-sm); background: var(--color-surface); font-weight: 400; cursor: pointer; transition: background-color var(--t-fast) ease, border-color var(--t-fast) ease; }
  .image-option:hover { background: var(--color-surface-hover); }
  .image-option:has(input:checked) { border-color: var(--ac); }
  .image-option:has(input:focus-visible) { outline: 2px solid var(--ac); outline-offset: 3px; }
  @media (max-width: 640px) {
    .language-grid, .image-grid { grid-template-columns: minmax(0, 1fr); }
    .language-grid { gap: 0; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { input, select, .image-option { transition: none; } }
</style>

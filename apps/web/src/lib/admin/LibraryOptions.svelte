<script lang="ts" module>
  export type LibraryOptionsSection = 'metadata' | 'images' | 'scan' | 'playback' | 'providers' | 'subtitles';
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import type { LibraryOptions } from './types';
  import ProviderPriority from './ProviderPriority.svelte';
  import { t } from '../i18n';
  let { options = $bindable(), kind, section, excludedPatterns = $bindable('') }: { options: LibraryOptions; kind: string; section: LibraryOptionsSection; excludedPatterns?: string } = $props();
  const languages = [{ value: 'fr', label: 'Français' }, { value: 'fr-CA', label: 'Français (Canada)' }, { value: 'en', label: 'Anglais' }, { value: 'de', label: 'Allemand' }, { value: 'es', label: 'Espagnol' }, { value: 'it', label: 'Italien' }, { value: 'pt', label: 'Portugais' }, { value: 'ja', label: 'Japonais' }, { value: 'ko', label: 'Coréen' }, { value: 'zh', label: 'Chinois' }];
  const images = [['fetchPoster', 'Affiches'], ['fetchBackdrop', 'Fonds'], ['fetchLogo', 'Logos'], ['fetchThumb', 'Miniatures'], ['fetchBanner', 'Bannières'], ['fetchDisc', 'Disques']] as const;
  let customLanguage = $state(false);
  // Image pool languages are edited as « fr, en » and stored as an ordered list.
  const artworkLanguagesText = {
    get value() { return (options.artworkLanguages ?? ['fr', 'en']).join(', '); },
    set value(text: string) { options.artworkLanguages = [...new Set(text.split(',').map(value => value.trim()).filter(Boolean))].slice(0, 5); }
  };
  type Suggestion = { id: string; name: string };
  let metadataProviders = $state<Suggestion[]>([]), imageProviders = $state<Suggestion[]>([]), segmentProviders = $state<Suggestion[]>([]), subtitleProviders = $state<Suggestion[]>([]);
  const nonVideoMetadataIds = new Set(['discogs', 'musicbrainz', 'lrclib', 'fanart-tv', 'poster-rotator']);
  const orderedMetadataProviders = $derived.by(() => {
    const filtered = kind === 'music'
      ? metadataProviders
      : metadataProviders.filter(item => !nonVideoMetadataIds.has(item.id));
    const preferred = kind === 'series' ? 'tvdb' : kind === 'films' ? 'tmdb' : '';
    return [...filtered].sort((a, b) => {
      if (a.id === preferred && b.id !== preferred) return -1;
      if (b.id === preferred && a.id !== preferred) return 1;
      return a.name.localeCompare(b.name);
    });
  });
  let providerError = $state('');
  onMount(() => {
    const controller = new AbortController();
    void fetch('/api/v1/admin/extensions', { credentials: 'same-origin', cache: 'no-store', signal: controller.signal })
      .then(async response => {
        if (!response.ok) throw new Error();
        const data = await response.json();
        const extensions = (data.extensions ?? []) as { manifest: { id: string; name: string; permissions?: string[] } }[];
        const having = (permission: string) => extensions.filter(entry => entry.manifest.permissions?.includes(permission)).map(entry => ({ id: entry.manifest.id, name: entry.manifest.name }));
        metadataProviders = having('metadata.read'); imageProviders = having('images'); segmentProviders = having('playback.markers'); subtitleProviders = having('subtitles');
      }).catch(() => { if (!controller.signal.aborted) providerError = $t('the_extension_list_is_unavailable_try'); });
    return () => controller.abort();
  });
</script>

<div class="library-options">
  {#if section === 'metadata'}
    <h3>{$t('metadata')}</h3><p class="lead">{$t('language_identification_sources_and_update')}</p>
    <div class="group">
      <label for="library-metadata-language">{$t('metadata_language')}</label>
      {#if customLanguage}
        <input id="library-metadata-language" bind:value={options.preferredMetadataLanguage} required minlength="2" maxlength="35" placeholder={$t('fr_ca')} />
        <button type="button" class="language-back" onclick={() => customLanguage = false}>{$t('choose_from_the_list')}</button>
      {:else}
        <select id="library-metadata-language" value={options.preferredMetadataLanguage} onchange={event => { if (event.currentTarget.value === '_custom') customLanguage = true; else options.preferredMetadataLanguage = event.currentTarget.value; }}>
          {#if !languages.some(language => language.value === options.preferredMetadataLanguage)}<option value={options.preferredMetadataLanguage}>{options.preferredMetadataLanguage}</option>{/if}
          {#each languages as language (language.value)}<option value={language.value}>{language.label}</option>{/each}
          <option value="_custom">{$t('other_language')}</option>
        </select>
      {/if}
      <label for="library-metadata-country">{$t('classification_country')}</label><input id="library-metadata-country" bind:value={options.metadataCountryCode} required pattern={'[A-Z]{2}'} maxlength="2" placeholder="FR" />
    </div>
    {#if kind !== 'music'}
      <div class="group">
        <h4>{$t('identification')}</h4>
        <label class="check"><input type="checkbox" bind:checked={options.readLocalNfo} /><span>{$t('read_local_nfo_files')}<small>{$t('identifiers_written_by_radarr_sonarr_or')}</small></span></label>
        <p class="hint">{$t('an_unrecognised_media_item_is_left')}</p>
        <p class="hint">{$t('provider_order_is_set_under_providers')}</p>
      </div>
      <div class="group">
        <h4>{$t('updates')}</h4>
        <label for="online-refresh">{$t('refresh_online_metadata')}</label>
        <select id="online-refresh" bind:value={options.onlineRefreshIntervalDays}><option value={0}>{$t('during_scans_only')}</option><option value={30}>{$t('every_30_days_recommended')}</option><option value={60}>{$t('every_60_days')}</option><option value={90}>{$t('every_90_days')}</option></select>
        <label for="library-metadata-refresh">{$t('reread_local_metadata')}</label><select id="library-metadata-refresh" bind:value={options.automaticRefreshIntervalDays}><option value={0}>{$t('during_scans_only')}</option><option value={30}>{$t('every_30_days')}</option><option value={60}>{$t('every_60_days')}</option><option value={90}>{$t('every_90_days')}</option></select>
        <p class="hint">{$t('manual_corrections_are_kept_provider_keys')}</p>
      </div>
      {#if kind === 'films'}<div class="group"><h4>{$t('collections')}</h4><label class="check"><input type="checkbox" bind:checked={options.automaticCollections} /><span>{$t('automatically_group_a_collection_s_films')}</span></label></div>{/if}
    {:else}<p class="hint">{$t('information_embedded_in_music_files_is')}</p>{/if}
  {:else if section === 'images'}
    <h3>{$t('admin_artwork_tab')}</h3><p class="lead">{$t('artwork_fetched_online_images_already_in')}</p>
    <div class="group">
      <h4>{$t('artwork_to_fetch')}</h4><div class="image-grid">{#each images as [key, label] (key)}<label class="check"><input type="checkbox" bind:checked={options[key]} /><span>{label}</span></label>{/each}</div>
    </div>
    <div class="group">
      <h4>{$t('image_pool')}</h4>
      <p class="hint">{$t('artwork_single_image_note')}</p>
      <label class="toggle-row"><span><strong>{$t('artwork_multi_image_mode')}</strong><small>{$t('artwork_multi_image_mode_note')}</small></span><input type="checkbox" role="switch" bind:checked={() => options.artworkPoolEnabled ?? true, value => options.artworkPoolEnabled = value} /></label>
      <label for="artwork-languages">{$t('languages_searched_in_order')}</label>
      <input id="artwork-languages" maxlength="100" placeholder={$t('fr_en')} required bind:value={artworkLanguagesText.value} />
      <label class="check"><input type="checkbox" bind:checked={() => options.artworkIncludeNeutral ?? true, value => options.artworkIncludeNeutral = value} /><span>{$t('include_images_with_no_language_backdrops')}</span></label>
      {#if options.artworkPoolEnabled ?? true}
        <label for="artwork-max-images">{$t('images_added_per_media_item_and')}</label>
        <input id="artwork-max-images" type="number" min="1" max="12" step="1" required bind:value={() => options.artworkMaxImages ?? 12, value => options.artworkMaxImages = value} />
      {/if}
    </div>
    <div class="group">
      <h4>{$t('duplicates_and_quality')}</h4>
      <label class="check"><input type="checkbox" bind:checked={options.artworkDeduplicate} /><span>{$t('skip_images_similar_to_those_already')}</span></label>
      {#if options.artworkDeduplicate}<label for="artwork-similarity">{$t('difference_tolerance_0_16_also_used')}</label><input id="artwork-similarity" type="number" min="0" max="16" step="1" required bind:value={options.artworkSimilarity} />{/if}
      <div class="grid"><div><label for="artwork-min-width">{$t('minimum_poster_width')}</label><input id="artwork-min-width" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterWidth} /></div><div><label for="artwork-min-height">{$t('minimum_poster_height')}</label><input id="artwork-min-height" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterHeight} /></div></div>
      <p class="hint">{$t('a_value_of_0_keeps_every')}</p>
    </div>
  {:else if section === 'scan'}
    <div class="group">
      <h4>{$t('file_analysis')}</h4>
      <label class="toggle-row"><span><strong>{$t('watch_folders_for_changes')}</strong><small>{$t('new_media_is_analysed_automatically')}</small></span><input type="checkbox" role="switch" bind:checked={options.realtimeMonitoring} /></label>
      <label for="library-excluded">{$t('excluded_patterns')}</label><input id="library-excluded" bind:value={excludedPatterns} placeholder={$t('bonus_trailers')} autocomplete="off" />
      <p class="hint">{$t('system_folders_nomedia_and_ignore_are')}</p>
    </div>
    {#if kind !== 'music'}
      <div class="group"><label class="check"><input type="checkbox" bind:checked={options.preferEmbeddedTitles} /><span>{$t('prefer_titles_embedded_in_files')}</span></label></div>
    {/if}
  {:else if section === 'playback'}
    <h3>{$t('play')}</h3><p class="lead">{$t('previews_chapters_and_intros')}</p>
    <div class="group">
      <label class="toggle-row"><span><strong>{$t('previews_in_the_playback_bar')}</strong><small>{$t('trickplay_thumbnails_to_find_a_scene')}</small></span><input type="checkbox" role="switch" bind:checked={options.trickplayEnabled} /></label>
      {#if options.trickplayEnabled}
        <label class="check"><input type="checkbox" bind:checked={options.trickplayDuringScan} /><span>{$t('prepare_thumbnails_after_media_analysis')}</span></label>
        <div class="grid three"><div><label for="trickplay-interval">{$t('interval_s')}</label><input id="trickplay-interval" type="number" min="1" max="60" step="1" required bind:value={options.trickplayIntervalSeconds} /></div><div><label for="trickplay-width">{$t('width_px')}</label><input id="trickplay-width" type="number" min="160" max="1920" step="1" required bind:value={options.trickplayWidth} /></div><div><label for="trickplay-quality">{$t('jpeg_quality')}</label><input id="trickplay-quality" type="number" min="1" max="31" step="1" required bind:value={options.trickplayQuality} /></div></div>
        <div class="grid"><div><label for="trickplay-columns">{$t('trickplay_columns')}</label><input id="trickplay-columns" type="number" min="1" max="20" step="1" required bind:value={() => options.trickplayColumns ?? 10, value => options.trickplayColumns = value} /></div><div><label for="trickplay-rows">{$t('trickplay_rows')}</label><input id="trickplay-rows" type="number" min="1" max="20" step="1" required bind:value={() => options.trickplayRows ?? 10, value => options.trickplayRows = value} /></div></div>
        <p class="hint">{$t('n_320_px_one_preview_every_10')}</p>
      {/if}
      <label class="check"><input type="checkbox" bind:checked={options.chapterImagesEnabled} /><span>{$t('show_chapter_images')}<small>{$t('prepared_on_demand_from_the_files')}</small></span></label>
    </div>
    <div class="group">
      <h4>{$t('intros_credits')}</h4>
      <label class="check"><input type="checkbox" bind:checked={options.introDetectionEnabled} /><span>{$t('detect_intros_with_enabled_extensions')}<small>{$t('extension_order_is_set_under_providers')}</small></span></label>
      <label class="range">{$t('marker_min_confidence', { value: options.markerMinConfidence ?? 80 })}<input type="range" min="50" max="99" step="1" value={options.markerMinConfidence ?? 80} oninput={(event) => options.markerMinConfidence = Number(event.currentTarget.value)} /><small>{$t('marker_min_confidence_hint')}</small></label>
    </div>
  {:else if section === 'providers'}
    <h3>{$t('providers')}</h3><p class="lead">{$t('tick_the_extensions_to_use_and')}</p>
    {#if providerError}<p role="status">{providerError}</p>{/if}
    <ProviderPriority id="metadata-providers" label={$t('metadata')} description={$t('titles_summaries_cast_and_identifiers')} bind:value={options.metadataProviders} suggestions={orderedMetadataProviders} defaultEnabledIds={kind === 'series' ? ['tvdb'] : kind === 'films' ? ['tmdb'] : undefined} />
    <ProviderPriority id="image-providers" label="Images" description={$t('posters_backdrops_logos_and_thumbnails')} bind:value={options.imageProviders} suggestions={imageProviders} />
    <ProviderPriority id="segment-providers" label={$t('intros_credits')} description={$t('detection_of_intros_recaps_and_credits')} bind:value={options.segmentProviders} suggestions={segmentProviders} />
    <ProviderPriority id="subtitle-providers" label="Sous-titres" description={$t('manual_search_from_a_media_item')} bind:value={options.subtitleProviders} suggestions={subtitleProviders} />
  {:else if section === 'subtitles'}
    <h3>{$t('report_category_subtitles')}</h3><p class="lead">{$t('subtitles_embedded_in_files_and_online')}</p>
    <div class="group">
      <label for="embedded-subtitles">{$t('admin_embedded_subtitles')}</label><select id="embedded-subtitles" bind:value={options.embeddedSubtitlePolicy}><option value="allow_all">{$t('all_subtitles')}</option><option value="allow_text">{$t('admin_text_subtitles_only')}</option><option value="block_all">{$t('admin_no_embedded_subtitles')}</option></select>
    </div>
    <div class="group">
      <h4>{$t('online_subtitles')}</h4>
      <label class="check"><input type="checkbox" bind:checked={options.subtitleDownloadEnabled} /><span>{$t('allow_manual_subtitle_search')}<small>{$t('from_a_media_item_s_page')}</small></span></label>
      {#if options.subtitleDownloadEnabled}
        <label class="check"><input type="checkbox" bind:checked={options.subtitleRequireHash} /><span>{$t('require_an_exact_file_match_hash')}<small>{$t('currently_unavailable_this_requirement')}</small></span></label>
      {/if}
    </div>
  {/if}
</div>

<style>
  .library-options { min-width: 0; color: var(--tx); font-family: var(--font); }
  h3 { margin: 0; font-size: 17px; font-weight: 700; }
  h4 { margin: 0 0 4px; font-size: 13px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; color: var(--mu); }
  p { margin: 8px 0 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .lead { margin-bottom: 8px; }
  .group { padding: 18px 0; border-bottom: 1px solid var(--color-line-subtle); }
  .group:last-child { border-bottom: 0; }
  .hint { font-size: 13px; }
  label { display: block; margin: 16px 0 8px; font-size: 14px; font-weight: 600; }
  input:not([type='checkbox']), select { width: 100%; min-width: 0; min-height: 46px; padding: 10px 14px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-sm); background-color: var(--color-surface); color: var(--tx); font: inherit; }
  input:not([type='checkbox']):focus, select:focus { outline: none; border-color: var(--ac); }
  select { padding-right: 40px; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 1px 0 0; accent-color: var(--ac); }
  .toggle-row { display: flex; justify-content: space-between; align-items: center; gap: 20px; min-height: 56px; margin: 0; cursor: pointer; }
  .toggle-row strong { font-size: 14px; font-weight: 600; }
  small { display: block; margin-top: 2px; color: var(--mu); font-size: 13px; line-height: 1.5; font-weight: 400; }
  .toggle-row input { appearance: none; position: relative; width: 42px; height: 24px; border-radius: var(--r-pill); background: var(--line-2); cursor: pointer; }
  .toggle-row input::after { content: ''; position: absolute; top: 3px; left: 3px; width: 18px; height: 18px; border-radius: var(--r-pill); background: var(--tx); }
  .toggle-row input:checked { background: var(--ac); }
  .toggle-row input:checked::after { transform: translateX(18px); background: var(--on-ac); }
  button { min-height: 44px; padding: 10px 16px; border: 1px solid var(--line-2); border-radius: var(--r-sm); background: transparent; color: var(--tx); font: inherit; font-size: 13px; cursor: pointer; }
  button:hover { background: var(--pn); }
  .language-back { margin-top: 8px; }
  .grid, .image-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 20px; }
  .grid.three { grid-template-columns: repeat(3, minmax(0, 1fr)); }
  .image-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); }
  .check { display: flex; align-items: flex-start; gap: 12px; min-height: 44px; margin: 4px 0; padding: 8px 0; font-weight: 600; cursor: pointer; }
  .range { display: flex; flex-direction: column; gap: 6px; margin: 8px 0; font-weight: 600; }
  .range input { width: min(100%, 360px); accent-color: var(--ac); }
  .range small { font-weight: 400; color: var(--mu); }
  .image-grid .check { font-weight: 500; }
  :is(input, select, button):focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  @media (max-width: 640px) { .grid, .grid.three { grid-template-columns: minmax(0, 1fr); } .image-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } input:not([type='checkbox']), select { font-size: 16px; } }
</style>

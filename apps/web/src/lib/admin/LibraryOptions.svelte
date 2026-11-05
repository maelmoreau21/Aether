<script lang="ts">
  import type { LibraryOptions } from './types';
  import { recommendedLibraryOptions } from './types';
  import AdvancedLibraryOptions from './AdvancedLibraryOptions.svelte';
  let { options = $bindable(), kind, excludedPatterns = $bindable('') }: { options: LibraryOptions; kind: string; excludedPatterns?: string } = $props();
  const languages = [{ value: 'fr', label: 'Français' }, { value: 'fr-CA', label: 'Français (Canada)' }, { value: 'en', label: 'Anglais' }, { value: 'de', label: 'Allemand' }, { value: 'es', label: 'Espagnol' }, { value: 'it', label: 'Italien' }, { value: 'pt', label: 'Portugais' }, { value: 'ja', label: 'Japonais' }, { value: 'ko', label: 'Coréen' }, { value: 'zh', label: 'Chinois' }];
  const images = [['fetchPoster', 'Affiches'], ['fetchBackdrop', 'Fonds'], ['fetchLogo', 'Logos'], ['fetchThumb', 'Miniatures'], ['fetchBanner', 'Bannières'], ['fetchDisc', 'Disques']] as const;
  let presetApplied = $state(false);
  let customLanguage = $state(false);
</script>

<div class="library-options">
  <header><h3>Réglages de la bibliothèque</h3><p>Les nouvelles bibliothèques sont préconfigurées. Vous pouvez conserver ces choix ou les adapter.</p></header>
  <div class="essentials">
    <label for="library-metadata-language">Langue des métadonnées</label>
    {#if customLanguage}
      <input id="library-metadata-language" bind:value={options.preferredMetadataLanguage} required minlength="2" maxlength="35" placeholder="fr-CA" />
      <button type="button" class="language-back" onclick={() => customLanguage = false}>Choisir dans la liste</button>
    {:else}
      <select id="library-metadata-language" value={options.preferredMetadataLanguage} onchange={event => { if (event.currentTarget.value === '_custom') customLanguage = true; else options.preferredMetadataLanguage = event.currentTarget.value; }}>
        {#if !languages.some(language => language.value === options.preferredMetadataLanguage)}<option value={options.preferredMetadataLanguage}>{options.preferredMetadataLanguage}</option>{/if}
        {#each languages as language (language.value)}<option value={language.value}>{language.label}</option>{/each}
        <option value="_custom">Autre langue…</option>
      </select>
    {/if}
    <label class="toggle-row"><span><strong>Suivre les changements des dossiers</strong><small>Les nouveaux médias sont analysés automatiquement.</small></span><input type="checkbox" role="switch" bind:checked={options.realtimeMonitoring} /></label>
    {#if kind !== 'music'}
      <label class="toggle-row"><span><strong>Aperçus dans la barre de lecture</strong><small>Miniatures trickplay pour retrouver une scène.</small></span><input type="checkbox" role="switch" bind:checked={options.trickplayEnabled} /></label>
    {/if}
  </div>
  <details class="custom-settings">
    <summary>Personnaliser les réglages</summary>
    <div class="preset-row"><p>Appliquer les réglages recommandés à cette bibliothèque.</p><button type="button" onclick={() => { options = recommendedLibraryOptions(options); presetApplied = true; }}>Appliquer les réglages recommandés</button></div>
    {#if presetApplied}<p role="status">Réglages recommandés appliqués. Enregistrez la bibliothèque pour les conserver.</p>{/if}
    <details class="option-section"><summary>Métadonnées et images</summary>
      <label for="library-metadata-country">Pays des classifications</label><input id="library-metadata-country" bind:value={options.metadataCountryCode} required pattern={'[A-Z]{2}'} maxlength="2" placeholder="FR" />
      {#if kind !== 'music'}
        <label class="check"><input type="checkbox" bind:checked={options.readLocalNfo} /> Lire les fichiers NFO locaux</label>
        <h4>Illustrations à récupérer</h4><div class="image-grid">{#each images as [key, label] (key)}<label class="check"><input type="checkbox" bind:checked={options[key]} />{label}</label>{/each}</div>
        <label class="check"><input type="checkbox" bind:checked={options.artworkDeduplicate} /> Éviter les images en double</label>
        {#if options.artworkDeduplicate}<label for="artwork-similarity">Tolérance aux différences</label><input id="artwork-similarity" type="number" min="0" max="16" step="1" required bind:value={options.artworkSimilarity} />{/if}
        <div class="grid"><div><label for="artwork-min-width">Largeur minimale des affiches</label><input id="artwork-min-width" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterWidth} /></div><div><label for="artwork-min-height">Hauteur minimale des affiches</label><input id="artwork-min-height" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterHeight} /></div></div>
        <p>Une valeur de 0 conserve toutes les tailles disponibles.</p>
      {:else}<p>Les informations intégrées aux fichiers musicaux sont lues pendant l’analyse.</p>{/if}
    </details>
    <details class="option-section"><summary>Analyse des fichiers</summary>
      <label for="library-excluded">Dossiers ou motifs à exclure</label><input id="library-excluded" bind:value={excludedPatterns} placeholder="bonus, trailers" autocomplete="off" />
      <p>Les dossiers système, .nomedia et .ignore sont déjà pris en compte.</p>
      {#if kind !== 'music'}
        <label class="check"><input type="checkbox" bind:checked={options.preferEmbeddedTitles} /> Préférer les titres intégrés aux fichiers</label>
        <label for="embedded-subtitles">Sous-titres intégrés</label><select id="embedded-subtitles" bind:value={options.embeddedSubtitlePolicy}><option value="allow_all">Tous les sous-titres</option><option value="allow_text">Sous-titres texte uniquement</option><option value="block_all">Aucun sous-titre intégré</option></select>
        <label for="library-metadata-refresh">Relire les métadonnées locales</label><select id="library-metadata-refresh" bind:value={options.automaticRefreshIntervalDays}><option value={0}>Lors des analyses uniquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
      {/if}
    </details>
    <AdvancedLibraryOptions bind:options {kind} />
  </details>
</div>

<style>
  .library-options { min-width: 0; color: var(--tx); font-family: var(--font); }
  h3 { margin: 0; font-size: 20px; font-weight: 800; }
  h4 { margin: 20px 0 8px; font-size: 14px; }
  p { margin: 8px 0 16px; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .essentials { margin-top: 24px; }
  label { display: block; margin: 20px 0 8px; font-size: 14px; font-weight: 600; }
  input:not([type='checkbox']), select { width: 100%; min-width: 0; min-height: 46px; padding: 10px 14px; border: 1px solid var(--line-2); border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font: inherit; }
  select { padding-right: 40px; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  .toggle-row { display: flex; justify-content: space-between; align-items: center; gap: 20px; min-height: 64px; margin: 0; padding: 16px 0; border-bottom: 1px solid var(--line); cursor: pointer; }
  .toggle-row strong { font-size: 14px; font-weight: 600; }
  small { display: block; margin-top: 4px; color: var(--mu); font-size: 13px; line-height: 1.5; font-weight: 400; }
  .toggle-row input { appearance: none; position: relative; width: 42px; height: 24px; border-radius: var(--r-pill); background: var(--line-2); cursor: pointer; }
  .toggle-row input::after { content: ''; position: absolute; top: 3px; left: 3px; width: 18px; height: 18px; border-radius: var(--r-pill); background: var(--tx); }
  .toggle-row input:checked { background: var(--ac); }
  .toggle-row input:checked::after { transform: translateX(18px); background: var(--on-ac); }
  .custom-settings { margin-top: 24px; border-top: 1px solid var(--line); }
  .option-section { border-bottom: 1px solid var(--line); padding-bottom: 12px; }
  summary { display: flex; align-items: center; gap: 12px; min-height: 56px; padding: 12px 0; cursor: pointer; list-style: none; font-size: 14px; font-weight: 600; }
  summary::-webkit-details-marker { display: none; }
  summary::after { content: '+'; margin-left: auto; color: var(--mu); font-size: 20px; }
  details[open] > summary::after { content: '−'; }
  .custom-settings > summary { color: var(--mu); }
  .preset-row { padding: 4px 0 20px; }
  button { min-height: 44px; padding: 10px 16px; border: 1px solid var(--line-2); border-radius: var(--r-sm); background: transparent; color: var(--tx); font: inherit; font-size: 13px; cursor: pointer; }
  button:hover { background: var(--pn); }
  .language-back { margin-top: 8px; }
  .grid, .image-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 20px; }
  .check { display: flex; align-items: center; gap: 12px; min-height: 44px; margin: 4px 0; font-weight: 400; cursor: pointer; }
  :is(input, select, button, summary):focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  @media (max-width: 640px) { .grid { grid-template-columns: minmax(0, 1fr); } input:not([type='checkbox']), select { font-size: 16px; } }
</style>

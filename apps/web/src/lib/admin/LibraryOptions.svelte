<script lang="ts" module>
  export type LibraryOptionsSection = 'metadata' | 'images' | 'scan' | 'playback' | 'providers' | 'subtitles';
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import type { LibraryOptions } from './types';
  import ProviderPriority from './ProviderPriority.svelte';
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
      }).catch(() => { if (!controller.signal.aborted) providerError = 'La liste des extensions est indisponible. Réessayez plus tard.'; });
    return () => controller.abort();
  });
</script>

<div class="library-options">
  {#if section === 'metadata'}
    <h3>Métadonnées</h3><p class="lead">Langue, sources d’identification et fréquence de mise à jour.</p>
    <div class="group">
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
      <label for="library-metadata-country">Pays des classifications</label><input id="library-metadata-country" bind:value={options.metadataCountryCode} required pattern={'[A-Z]{2}'} maxlength="2" placeholder="FR" />
    </div>
    {#if kind !== 'music'}
      <div class="group">
        <h4>Identification</h4>
        <label class="check"><input type="checkbox" bind:checked={options.readLocalNfo} /><span>Lire les fichiers NFO locaux<small>Les identifiants écrits par Radarr, Sonarr ou Kodi sont utilisés en priorité.</small></span></label>
        <p class="hint">Un média non reconnu est laissé tel quel puis réessayé automatiquement plus tard. S’il reste inconnu, utilisez « Identifier » sur sa fiche.</p>
        <p class="hint">L’ordre des fournisseurs se règle dans Fournisseurs.</p>
      </div>
      <div class="group">
        <h4>Mises à jour</h4>
        <label for="online-refresh">Actualiser les métadonnées en ligne</label>
        <select id="online-refresh" bind:value={options.onlineRefreshIntervalDays}><option value={0}>Lors des analyses uniquement</option><option value={30}>Tous les 30 jours (recommandé)</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
        <label for="library-metadata-refresh">Relire les métadonnées locales</label><select id="library-metadata-refresh" bind:value={options.automaticRefreshIntervalDays}><option value={0}>Lors des analyses uniquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
        <p class="hint">Les corrections manuelles sont conservées. Les clés des fournisseurs se configurent dans Extensions.</p>
      </div>
      {#if kind === 'films'}<div class="group"><h4>Collections</h4><label class="check"><input type="checkbox" bind:checked={options.automaticCollections} /><span>Regrouper automatiquement les films d’une collection</span></label></div>{/if}
    {:else}<p class="hint">Les informations intégrées aux fichiers musicaux sont lues pendant l’analyse.</p>{/if}
  {:else if section === 'images'}
    <h3>Images</h3><p class="lead">Illustrations récupérées en ligne. Les images déjà présentes dans le dossier du média sont gardées et ne sont pas re-téléchargées.</p>
    <div class="group">
      <h4>Illustrations à récupérer</h4><div class="image-grid">{#each images as [key, label] (key)}<label class="check"><input type="checkbox" bind:checked={options[key]} /><span>{label}</span></label>{/each}</div>
    </div>
    <div class="group">
      <h4>Pool d’images</h4>
      <p class="hint">Utilisé par « Collecter les images » pour remplir les variantes affichées en rotation.</p>
      <label for="artwork-languages">Langues recherchées, dans l’ordre</label>
      <input id="artwork-languages" maxlength="100" placeholder="fr, en" required bind:value={artworkLanguagesText.value} />
      <label class="check"><input type="checkbox" bind:checked={() => options.artworkIncludeNeutral ?? true, value => options.artworkIncludeNeutral = value} /><span>Inclure les images sans langue (fonds, scènes)</span></label>
      <label for="artwork-max-images">Images ajoutées par média et par collecte (1–12)</label>
      <input id="artwork-max-images" type="number" min="1" max="12" step="1" required bind:value={() => options.artworkMaxImages ?? 12, value => options.artworkMaxImages = value} />
    </div>
    <div class="group">
      <h4>Doublons et qualité</h4>
      <label class="check"><input type="checkbox" bind:checked={options.artworkDeduplicate} /><span>Écarter les images similaires à celles déjà enregistrées</span></label>
      {#if options.artworkDeduplicate}<label for="artwork-similarity">Tolérance aux différences (0–16, utilisée aussi par « Repérer les doublons »)</label><input id="artwork-similarity" type="number" min="0" max="16" step="1" required bind:value={options.artworkSimilarity} />{/if}
      <div class="grid"><div><label for="artwork-min-width">Largeur minimale des affiches</label><input id="artwork-min-width" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterWidth} /></div><div><label for="artwork-min-height">Hauteur minimale des affiches</label><input id="artwork-min-height" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterHeight} /></div></div>
      <p class="hint">Une valeur de 0 conserve toutes les tailles disponibles.</p>
    </div>
  {:else if section === 'scan'}
    <div class="group">
      <h4>Analyse des fichiers</h4>
      <label class="toggle-row"><span><strong>Suivre les changements des dossiers</strong><small>Les nouveaux médias sont analysés automatiquement.</small></span><input type="checkbox" role="switch" bind:checked={options.realtimeMonitoring} /></label>
      <label for="library-excluded">Dossiers ou motifs à exclure</label><input id="library-excluded" bind:value={excludedPatterns} placeholder="bonus, trailers" autocomplete="off" />
      <p class="hint">Les dossiers système, .nomedia et .ignore sont déjà pris en compte.</p>
    </div>
    {#if kind !== 'music'}
      <div class="group"><label class="check"><input type="checkbox" bind:checked={options.preferEmbeddedTitles} /><span>Préférer les titres intégrés aux fichiers</span></label></div>
    {/if}
  {:else if section === 'playback'}
    <h3>Lecture</h3><p class="lead">Aperçus, chapitres et intros.</p>
    <div class="group">
      <label class="toggle-row"><span><strong>Aperçus dans la barre de lecture</strong><small>Miniatures trickplay pour retrouver une scène.</small></span><input type="checkbox" role="switch" bind:checked={options.trickplayEnabled} /></label>
      {#if options.trickplayEnabled}
        <label class="check"><input type="checkbox" bind:checked={options.trickplayDuringScan} /><span>Préparer les miniatures après l’analyse des médias</span></label>
        <div class="grid three"><div><label for="trickplay-interval">Intervalle (s)</label><input id="trickplay-interval" type="number" min="1" max="60" step="1" required bind:value={options.trickplayIntervalSeconds} /></div><div><label for="trickplay-width">Largeur (px)</label><input id="trickplay-width" type="number" min="160" max="1920" step="1" required bind:value={options.trickplayWidth} /></div><div><label for="trickplay-quality">Qualité JPEG (1–31)</label><input id="trickplay-quality" type="number" min="1" max="31" step="1" required bind:value={options.trickplayQuality} /></div></div>
        <p class="hint">320 px, un aperçu toutes les 10 secondes et une qualité de 7 offrent un bon compromis.</p>
      {/if}
      <label class="check"><input type="checkbox" bind:checked={options.chapterImagesEnabled} /><span>Afficher les images des chapitres<small>Préparées à la demande à partir des chapitres des fichiers.</small></span></label>
    </div>
    <div class="group">
      <h4>Intros et génériques</h4>
      <label class="check"><input type="checkbox" bind:checked={options.introDetectionEnabled} /><span>Détecter les intros avec les extensions activées<small>L’ordre des extensions se règle dans Fournisseurs.</small></span></label>
    </div>
  {:else if section === 'providers'}
    <h3>Fournisseurs</h3><p class="lead">Cochez les extensions à utiliser et classez-les par ordre de préférence : la première qui répond est utilisée. Les clés et réglages des extensions se configurent dans Extensions.</p>
    {#if providerError}<p role="status">{providerError}</p>{/if}
    <ProviderPriority id="metadata-providers" label="Métadonnées" description="Titres, résumés, distribution et identifiants." bind:value={options.metadataProviders} suggestions={metadataProviders} />
    <ProviderPriority id="image-providers" label="Images" description="Affiches, fonds, logos et miniatures." bind:value={options.imageProviders} suggestions={imageProviders} />
    <ProviderPriority id="segment-providers" label="Intros et génériques" description="Détection des intros, récapitulatifs et génériques." bind:value={options.segmentProviders} suggestions={segmentProviders} />
    <ProviderPriority id="subtitle-providers" label="Sous-titres" description="Recherche manuelle depuis la fiche d’un média." bind:value={options.subtitleProviders} suggestions={subtitleProviders} />
  {:else if section === 'subtitles'}
    <h3>Sous-titres</h3><p class="lead">Sous-titres intégrés aux fichiers et recherche en ligne.</p>
    <div class="group">
      <label for="embedded-subtitles">Sous-titres intégrés</label><select id="embedded-subtitles" bind:value={options.embeddedSubtitlePolicy}><option value="allow_all">Tous les sous-titres</option><option value="allow_text">Sous-titres texte uniquement</option><option value="block_all">Aucun sous-titre intégré</option></select>
    </div>
    <div class="group">
      <h4>Sous-titres en ligne</h4>
      <label class="check"><input type="checkbox" bind:checked={options.subtitleDownloadEnabled} /><span>Autoriser la recherche manuelle de sous-titres<small>Depuis la fiche d’un média, avec une extension configurée.</small></span></label>
      {#if options.subtitleDownloadEnabled}
        <label class="check"><input type="checkbox" bind:checked={options.subtitleRequireHash} /><span>Exiger une correspondance exacte du fichier (hash)<small>Indisponible actuellement : cette exigence bloque les recherches.</small></span></label>
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
  .image-grid .check { font-weight: 500; }
  :is(input, select, button):focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  @media (max-width: 640px) { .grid, .grid.three { grid-template-columns: minmax(0, 1fr); } .image-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } input:not([type='checkbox']), select { font-size: 16px; } }
</style>

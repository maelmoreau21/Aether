<script lang="ts">
  import { onMount } from 'svelte';
  import type { LibraryOptions } from './types';
  import ProviderPriority from './ProviderPriority.svelte';
  let { options = $bindable(), kind }: { options: LibraryOptions; kind: string } = $props();
  let providers = $state<{ id: string; name: string }[]>([]);
  let subtitleProviders = $state<{ id: string; name: string }[]>([]);
  let providerError = $state('');
  let languages = $derived(options.subtitleLanguages.join(', '));
  onMount(() => {
    const controller = new AbortController();
    void fetch('/api/v1/admin/extensions', { credentials:'same-origin', cache:'no-store', signal:controller.signal })
      .then(async response => {
        if (!response.ok) throw new Error();
        const data = await response.json();
        const extensions = (data.extensions ?? []) as { manifest: { id: string; name: string; permissions?: string[] } }[];
        providers = extensions.map(entry => ({ id:entry.manifest.id, name:entry.manifest.name }));
        subtitleProviders = extensions.filter(entry => entry.manifest.permissions?.includes('subtitles')).map(entry => ({ id:entry.manifest.id, name:entry.manifest.name }));
      }).catch(() => { if (!controller.signal.aborted) providerError = 'Les suggestions d’extensions sont indisponibles. Vous pouvez saisir leur identifiant.'; });
    return () => controller.abort();
  });
</script>

<details class="advanced-section"><summary id="library-providers-heading"><span>Fournisseurs et enrichissement</span></summary>
  <p class="notice">Les fournisseurs de métadonnées, images et segments activés sont utilisés dans l’ordre choisi. Une liste vide conserve la politique globale ; une liste renseignée limite les fournisseurs autorisés. Les clés et comptes se configurent dans Extensions.</p>
  {#if providerError}<p role="status">{providerError}</p>{/if}
  <ProviderPriority id="metadata-providers" label="Métadonnées" bind:value={options.metadataProviders} suggestions={providers} />
  <ProviderPriority id="image-providers" label="Images" bind:value={options.imageProviders} suggestions={providers} />
  <ProviderPriority id="similar-providers" label="Articles similaires (moteur à venir)" bind:value={options.similarItemsProviders} suggestions={providers} />
  <label for="online-refresh">Actualiser les métadonnées depuis Internet</label>
  <select id="online-refresh" bind:value={options.onlineRefreshIntervalDays}><option value={0}>Jamais automatiquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
  <p class="hint">Les fournisseurs installés et autorisés actualisent les champs automatiques à cette cadence. Les corrections manuelles restent prioritaires. Cette cadence est distincte de l’actualisation des NFO locaux.</p>
</details>

{#if kind !== 'music'}
<details class="advanced-section"><summary id="library-analysis-heading"><span>Analyse et organisation</span></summary>
  <p class="notice">La surveillance vérifie les dossiers via leur Media Node toutes les 60 secondes, selon les tâches en attente. Les collections réunissent dans Playlists les films accessibles appartenant à une même collection identifiée, à partir de deux films.</p>
  <label class="check"><input type="checkbox" bind:checked={options.realtimeMonitoring} /> Surveiller les modifications (vérification toutes les 60 secondes)</label>
  <label class="check"><input type="checkbox" bind:checked={options.automaticCollections} /> Ajouter automatiquement aux collections</label>
  <label class="check"><input type="checkbox" bind:checked={options.introDetectionEnabled} /> Activer la détection d’intro</label>
  <ProviderPriority id="segment-providers" label="Segments média" bind:value={options.segmentProviders} suggestions={providers} />
</details>

<details class="advanced-section"><summary id="library-trickplay-heading"><span>Trickplay</span></summary>
  <p class="notice">Génération native de planches de miniatures et réutilisation automatique des planches Jellyfin existantes.</p>
  <label class="check"><input type="checkbox" bind:checked={options.trickplayEnabled} /> Activer l’extraction d’images trickplay</label>
  <label class="check"><input type="checkbox" bind:checked={options.trickplayDuringScan} /> Extraire les images trickplay pendant le scan</label>
  <div class="grid"><div><label for="trickplay-interval">Intervalle entre les aperçus (secondes)</label><input id="trickplay-interval" type="number" min="1" max="60" step="1" required bind:value={options.trickplayIntervalSeconds} /></div><div><label for="trickplay-width">Largeur des aperçus (pixels)</label><input id="trickplay-width" type="number" min="160" max="1920" step="1" required bind:value={options.trickplayWidth} /></div><div><label for="trickplay-quality">Qualité JPEG (1–31, 7 recommandé)</label><input id="trickplay-quality" type="number" min="1" max="31" step="1" required bind:value={options.trickplayQuality} /></div></div>
  <p class="hint">Un intervalle plus court et des images plus grandes demanderont davantage de calcul et de stockage.</p>
</details>

<details class="advanced-section"><summary id="library-chapters-heading"><span>Images des chapitres</span></summary>
  <p class="notice">Aperçus des chapitres réellement présents, générés à la demande dans le stockage applicatif du Media Node. Largeur et qualité utilisent les valeurs de prévisualisation Trickplay ci-dessus. Aucun chapitre artificiel ni écriture dans les médias.</p>
  <label class="check"><input type="checkbox" bind:checked={options.chapterImagesEnabled} /> Activer l’extraction des images de chapitres</label>
  <label class="check"><input type="checkbox" bind:checked={options.chapterImagesDuringScan} disabled /> Extraire pendant le scan (planification indisponible)</label>
</details>

<details class="advanced-section"><summary id="library-subtitle-heading"><span>Téléchargement des sous-titres</span></summary>
  <p class="notice">Les administrateurs peuvent rechercher et télécharger explicitement des sous-titres depuis les actions d’un film ou d’un épisode. Configurez le compte et la clé du fournisseur signé dans Extensions. Les fichiers sont conservés dans les données applicatives, jamais dans les médias. Aucun téléchargement automatique n’est effectué.</p>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleDownloadEnabled} /> Autoriser la recherche et le téléchargement manuels des sous-titres</label>
  <label for="subtitle-languages">Langues des sous-titres (par ordre de préférence)</label>
  <input id="subtitle-languages" value={languages} onchange={event => options.subtitleLanguages = event.currentTarget.value.split(',').map(value => value.trim()).filter(Boolean)} maxlength="575" placeholder="fr, en" aria-describedby="subtitle-languages-hint" />
  <p id="subtitle-languages-hint" class="hint">Préférence réservée à une future automatisation ; la langue de recherche manuelle est choisie sur la fiche du média. Codes séparés par des virgules, par exemple fr, en ou fr-CA. Maximum 16 langues distinctes.</p>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleOnlyMissing} disabled /> Télécharger uniquement les sous-titres manquants (automatisation indisponible)</label>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleMatchAudio} disabled /> Rechercher uniquement les langues correspondant à l’audio (automatisation indisponible)</label>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleRequireHash} /> Exiger une correspondance exacte du fichier (hash)</label>
  <p class="hint">La correspondance par hash n’est pas encore disponible. Cette exigence bloque la recherche et le téléchargement manuels tant qu’elle reste activée. Une correspondance par identifiant ou titre ne garantit pas la synchronisation.</p>
  <ProviderPriority id="subtitle-providers" label="Sous-titres" bind:value={options.subtitleProviders} suggestions={subtitleProviders} />
</details>
{/if}

<details class="advanced-section"><summary id="library-storage-heading"><span>Enregistrement dans les dossiers médias</span><small>Préférences préparées</small></summary>
  <p class="notice">Les médias restent en lecture seule. Ces choix mémorisent votre intention ; ils n’autorisent aucune écriture et ne modifient pas les montages Docker. Un futur mode d’écriture devra être autorisé et configuré séparément.</p>
  <label class="check"><input type="checkbox" bind:checked={options.saveNfoToMedia} /> Enregistrer les métadonnées au format NFO dans les médias</label>
  <label class="check"><input type="checkbox" bind:checked={options.saveImagesToMedia} /> Enregistrer les illustrations dans les dossiers médias</label>
  {#if kind !== 'music'}
  <label class="check"><input type="checkbox" bind:checked={options.trickplaySaveToMedia} /> Enregistrer les images trickplay à côté des médias</label>
  <label class="check"><input type="checkbox" bind:checked={options.chapterImagesSaveToMedia} /> Enregistrer les images de chapitres à côté des médias</label>
  <label class="check"><input type="checkbox" bind:checked={options.saveSubtitlesToMedia} /> Enregistrer les sous-titres dans les dossiers médias</label>
  {/if}
</details>

<style>
  .advanced-section { min-width: 0; padding-bottom: 12px; border-bottom: 1px solid var(--color-line-subtle); font-family: var(--font); color: var(--tx); }
  .advanced-section:first-of-type { border-top: 1px solid var(--color-line-subtle); }
  summary { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px 16px; min-height: 56px; padding: 8px 0; font-size: 15px; font-weight: 700; list-style: none; cursor: pointer; border-radius: var(--r-xs); }
  summary::-webkit-details-marker { display: none; }
  summary::after { content: '+'; margin-left: auto; color: var(--mu); font-size: 20px; font-weight: 600; line-height: 1; }
  details[open] > summary::after { content: '−'; }
  summary:hover::after { color: var(--tx); }
  summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  summary small { display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--color-raised); color: var(--mu); font-size: 12px; font-weight: 600; }
  p { font-size: 14px; line-height: 1.6; color: var(--mu); }
  .notice { margin: 4px 0 12px; padding: 12px 14px; border-radius: var(--r-sm); background: var(--color-surface); }
  .hint { margin-top: 8px; font-size: 13px; }
  label { display: block; margin: 20px 0 8px; font-size: 14px; font-weight: 700; line-height: 1.5; color: var(--tx); }
  .check { display: flex; align-items: flex-start; gap: 12px; min-height: 44px; margin: 0; padding: 12px 0; font-weight: 600; cursor: pointer; }
  .check:has(input:disabled) { color: var(--mu); cursor: not-allowed; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 2px 0 0; accent-color: var(--ac); }
  input:not([type='checkbox']), select { width: 100%; min-width: 0; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid var(--color-line-subtle); border-radius: var(--r-sm); background-color: var(--color-surface); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  input:not([type='checkbox']):hover, select:hover:not(:disabled) { background-color: var(--color-surface-hover); border-color: var(--color-line); }
  input:not([type='checkbox']):focus, select:focus, select:focus-visible { outline: none; border-color: var(--ac); background-color: var(--color-surface); }
  .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 20px; }
  @media (max-width: 640px) {
    .grid { grid-template-columns: minmax(0, 1fr); gap: 0; }
    input:not([type='checkbox']), select { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { input, select { transition: none; } }
</style>

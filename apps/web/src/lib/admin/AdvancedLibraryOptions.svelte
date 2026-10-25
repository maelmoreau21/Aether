<script lang="ts">
  import { onMount } from 'svelte';
  import type { LibraryOptions } from './types';
  import ProviderPriority from './ProviderPriority.svelte';
  let { options = $bindable(), kind }: { options: LibraryOptions; kind: string } = $props();
  let providers = $state<{ id: string; name: string }[]>([]);
  let providerError = $state('');
  let languages = $derived(options.subtitleLanguages.join(', '));
  onMount(() => {
    const controller = new AbortController();
    void fetch('/api/v1/admin/extensions', { credentials:'same-origin', cache:'no-store', signal:controller.signal })
      .then(async response => {
        if (!response.ok) throw new Error();
        const data = await response.json();
        providers = (data.extensions ?? []).map((entry: { manifest: { id: string; name: string } }) => ({ id:entry.manifest.id, name:entry.manifest.name }));
      }).catch(() => { if (!controller.signal.aborted) providerError = 'Les suggestions d’extensions sont indisponibles. Vous pouvez saisir leur identifiant.'; });
    return () => controller.abort();
  });
</script>

<section class="advanced-section" aria-labelledby="library-providers-heading">
  <div class="heading"><h3 id="library-providers-heading">Fournisseurs et enrichissement</h3><span>Préférences préparées</span></div>
  <p class="notice">Ces choix sont sauvegardés par bibliothèque. L’exécution automatique des fournisseurs sera disponible lorsque leur raccordement au scan sera ajouté. Les clés et comptes se configurent dans Extensions.</p>
  {#if providerError}<p role="status">{providerError}</p>{/if}
  <ProviderPriority id="metadata-providers" label="Métadonnées" bind:value={options.metadataProviders} suggestions={providers} />
  <ProviderPriority id="image-providers" label="Images" bind:value={options.imageProviders} suggestions={providers} />
  <ProviderPriority id="similar-providers" label="Articles similaires" bind:value={options.similarItemsProviders} suggestions={providers} />
  <label for="online-refresh">Actualiser les métadonnées depuis Internet</label>
  <select id="online-refresh" bind:value={options.onlineRefreshIntervalDays}><option value={0}>Jamais automatiquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
  <p class="hint">Cadence réservée aux futurs fournisseurs ; distincte de l’actualisation des NFO locaux.</p>
</section>

{#if kind !== 'music'}
<section class="advanced-section" aria-labelledby="library-analysis-heading">
  <div class="heading"><h3 id="library-analysis-heading">Analyse et organisation</h3><span>Préférences préparées</span></div>
  <p class="notice">La surveillance native et les regroupements automatiques ne sont pas encore exécutés. Vos préférences sont conservées pour leur future activation.</p>
  <label class="check"><input type="checkbox" bind:checked={options.realtimeMonitoring} /> Activer la surveillance en temps réel</label>
  <label class="check"><input type="checkbox" bind:checked={options.automaticCollections} /> Ajouter automatiquement aux collections</label>
  <label class="check"><input type="checkbox" bind:checked={options.introDetectionEnabled} /> Activer la détection d’intro</label>
  <ProviderPriority id="segment-providers" label="Segments média" bind:value={options.segmentProviders} suggestions={providers} />
</section>

<section class="advanced-section" aria-labelledby="library-trickplay-heading">
  <div class="heading"><h3 id="library-trickplay-heading">Trickplay</h3><span>Moteur à venir</span></div>
  <p class="notice">Préparez les aperçus affichés pendant un déplacement dans la vidéo. Aucun aperçu n’est encore généré par ces réglages.</p>
  <label class="check"><input type="checkbox" bind:checked={options.trickplayEnabled} /> Activer l’extraction d’images trickplay</label>
  <label class="check"><input type="checkbox" bind:checked={options.trickplayDuringScan} /> Extraire les images trickplay pendant le scan</label>
  <div class="grid"><div><label for="trickplay-interval">Intervalle entre les aperçus (secondes)</label><input id="trickplay-interval" type="number" min="1" max="60" step="1" required bind:value={options.trickplayIntervalSeconds} /></div><div><label for="trickplay-width">Largeur des aperçus (pixels)</label><input id="trickplay-width" type="number" min="160" max="1920" step="1" required bind:value={options.trickplayWidth} /></div></div>
  <p class="hint">Un intervalle plus court et des images plus grandes demanderont davantage de calcul et de stockage.</p>
</section>

<section class="advanced-section" aria-labelledby="library-chapters-heading">
  <div class="heading"><h3 id="library-chapters-heading">Images des chapitres</h3><span>Moteur à venir</span></div>
  <p class="notice">Configuration conservée pour la future génération des miniatures de chapitres.</p>
  <label class="check"><input type="checkbox" bind:checked={options.chapterImagesEnabled} /> Activer l’extraction des images de chapitres</label>
  <label class="check"><input type="checkbox" bind:checked={options.chapterImagesDuringScan} /> Extraire les images des chapitres pendant le scan</label>
</section>

<section class="advanced-section" aria-labelledby="library-subtitle-heading">
  <div class="heading"><h3 id="library-subtitle-heading">Téléchargement des sous-titres</h3><span>Fournisseurs à venir</span></div>
  <p class="notice">Ces préférences seront utilisées par les futurs fournisseurs de sous-titres. Aucun téléchargement n’est effectué actuellement.</p>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleDownloadEnabled} /> Activer le téléchargement automatique des sous-titres</label>
  <label for="subtitle-languages">Langues des sous-titres (par ordre de préférence)</label>
  <input id="subtitle-languages" value={languages} onchange={event => options.subtitleLanguages = event.currentTarget.value.split(',').map(value => value.trim()).filter(Boolean)} maxlength="575" placeholder="fr, en" aria-describedby="subtitle-languages-hint" />
  <p id="subtitle-languages-hint" class="hint">Codes de langue séparés par des virgules, par exemple fr, en ou fr-CA. Maximum 16 langues distinctes.</p>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleOnlyMissing} /> Télécharger uniquement les sous-titres manquants</label>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleMatchAudio} /> Rechercher uniquement les langues correspondant à l’audio</label>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleRequireHash} /> Exiger une correspondance exacte du fichier (hash)</label>
  <ProviderPriority id="subtitle-providers" label="Sous-titres" bind:value={options.subtitleProviders} suggestions={providers} />
</section>
{/if}

<section class="advanced-section" aria-labelledby="library-storage-heading">
  <div class="heading"><h3 id="library-storage-heading">Enregistrement dans les dossiers médias</h3><span>Préférences préparées</span></div>
  <p class="notice">Les médias restent en lecture seule. Ces choix mémorisent votre intention ; ils n’autorisent aucune écriture et ne modifient pas les montages Docker. Un futur mode d’écriture devra être autorisé et configuré séparément.</p>
  <label class="check"><input type="checkbox" bind:checked={options.saveNfoToMedia} /> Enregistrer les métadonnées au format NFO dans les médias</label>
  <label class="check"><input type="checkbox" bind:checked={options.saveImagesToMedia} /> Enregistrer les illustrations dans les dossiers médias</label>
  {#if kind !== 'music'}
  <label class="check"><input type="checkbox" bind:checked={options.trickplaySaveToMedia} /> Enregistrer les images trickplay à côté des médias</label>
  <label class="check"><input type="checkbox" bind:checked={options.chapterImagesSaveToMedia} /> Enregistrer les images de chapitres à côté des médias</label>
  <label class="check"><input type="checkbox" bind:checked={options.saveSubtitlesToMedia} /> Enregistrer les sous-titres dans les dossiers médias</label>
  {/if}
</section>

<style>
  .advanced-section { min-width:0;border:1px solid var(--border-subtle);border-radius:var(--radius-lg);padding:24px;background:var(--bg-surface); }
  .heading { display:flex;align-items:center;justify-content:space-between;gap:16px; } h3 { margin:0;font-size:19px; } .heading span { flex-shrink:0;font-size:11px;padding:5px 9px;border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-secondary); }
  p { color:var(--text-secondary);font-size:13px;line-height:1.6; }.notice { border-left:3px solid var(--color-info);padding-left:12px; }.hint { font-size:12px; }
  label { display:block;margin:18px 0 8px;font-size:14px;line-height:1.5; }.check { display:flex;align-items:flex-start;gap:12px;padding:12px;background:var(--bg-main);border-radius:var(--radius-md);cursor:pointer; }
  input[type=checkbox] { width:18px;height:18px;flex-shrink:0;margin-top:2px;accent-color:var(--color-play); }
  input:not([type=checkbox]),select { width:100%;box-sizing:border-box;min-width:0;min-height:44px;padding:10px 12px;font:inherit;background:var(--bg-main);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:var(--radius-md); }
  .grid { display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px; }
  @media(max-width:600px) { .advanced-section { padding:18px; }.heading { align-items:flex-start;flex-direction:column;gap:10px; }.grid { grid-template-columns:minmax(0,1fr);gap:0; } }
</style>

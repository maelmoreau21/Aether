<script lang="ts">
  import { onMount } from 'svelte';
  import type { LibraryOptions } from './types';
  import ProviderPriority from './ProviderPriority.svelte';
  let { options = $bindable(), kind }: { options: LibraryOptions; kind: string } = $props();
  let providers = $state<{ id: string; name: string }[]>([]);
  let subtitleProviders = $state<{ id: string; name: string }[]>([]);
  let providerError = $state('');
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

<details class="advanced-section"><summary id="library-providers-heading">Fournisseurs et enrichissement</summary>
  <p class="notice">Les sources activées sont utilisées automatiquement. Personnalisez leur ordre uniquement si nécessaire.</p>
  {#if providerError}<p role="status">{providerError}</p>{/if}
  <ProviderPriority id="metadata-providers" label="Métadonnées" bind:value={options.metadataProviders} suggestions={providers} />
  <ProviderPriority id="image-providers" label="Images" bind:value={options.imageProviders} suggestions={providers} />
  <label for="online-refresh">Actualiser les métadonnées en ligne</label>
  <select id="online-refresh" bind:value={options.onlineRefreshIntervalDays}><option value={0}>Lors des analyses uniquement</option><option value={30}>Tous les 30 jours (recommandé)</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
  <p class="hint">Les corrections manuelles sont conservées. Les clés des fournisseurs se configurent dans Extensions.</p>
</details>

{#if kind !== 'music'}
<details class="advanced-section"><summary id="library-analysis-heading">Collections et détection d’intro</summary>
  {#if kind === 'films'}<label class="check"><input type="checkbox" bind:checked={options.automaticCollections} /> Regrouper automatiquement les films d’une collection</label>{/if}
  <label class="check"><input type="checkbox" bind:checked={options.introDetectionEnabled} /> Détecter les intros avec les extensions activées</label>
  <ProviderPriority id="segment-providers" label="Détection d’intro et segments" bind:value={options.segmentProviders} suggestions={providers} />
</details>

<details class="advanced-section"><summary id="library-trickplay-heading">Qualité et préparation des aperçus</summary>
  {#if options.trickplayEnabled}
    <label class="check"><input type="checkbox" bind:checked={options.trickplayDuringScan} /> Préparer les miniatures après l’analyse des médias</label>
    <div class="grid"><div><label for="trickplay-interval">Intervalle entre les aperçus (secondes)</label><input id="trickplay-interval" type="number" min="1" max="60" step="1" required bind:value={options.trickplayIntervalSeconds} /></div><div><label for="trickplay-width">Largeur des aperçus (pixels)</label><input id="trickplay-width" type="number" min="160" max="1920" step="1" required bind:value={options.trickplayWidth} /></div><div><label for="trickplay-quality">Qualité JPEG (1–31, 7 recommandé)</label><input id="trickplay-quality" type="number" min="1" max="31" step="1" required bind:value={options.trickplayQuality} /></div></div>
    <p class="hint">320 px, un aperçu toutes les 10 secondes et une qualité de 7 offrent un bon compromis entre précision, calcul et stockage.</p>
  {:else}<p>Activez les aperçus dans la barre de lecture pour personnaliser le trickplay.</p>{/if}
  <label class="check"><input type="checkbox" bind:checked={options.chapterImagesEnabled} /> Afficher les images des chapitres</label>
  <p>Les images de chapitres sont préparées à la demande, à partir des chapitres présents dans les fichiers.</p>
</details>

<details class="advanced-section"><summary id="library-subtitle-heading">Sous-titres en ligne</summary>
  <label class="check"><input type="checkbox" bind:checked={options.subtitleDownloadEnabled} /> Autoriser la recherche manuelle de sous-titres</label>
  {#if options.subtitleDownloadEnabled}
    <p>Recherchez les sous-titres depuis la fiche d’un média avec une extension configurée.</p>
    <label class="check"><input type="checkbox" bind:checked={options.subtitleRequireHash} /> Exiger une correspondance exacte du fichier (hash)</label>
    <p class="hint">La correspondance par hash est indisponible actuellement. Cette exigence bloque donc les recherches.</p>
    <ProviderPriority id="subtitle-providers" label="Sous-titres" bind:value={options.subtitleProviders} suggestions={subtitleProviders} />
  {/if}
</details>
{/if}
<style>
  .advanced-section { min-width: 0; padding-bottom: 12px; border-bottom: 1px solid var(--color-line-subtle); font-family: var(--font); color: var(--tx); }
  .advanced-section:first-of-type { border-top: 1px solid var(--color-line-subtle); }
  summary { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px 16px; min-height: 56px; padding: 8px 0; font-size: 15px; font-weight: 700; list-style: none; cursor: pointer; border-radius: var(--r-xs); }
  summary::-webkit-details-marker { display: none; }
  summary::after { content: '+'; margin-left: auto; color: var(--mu); font-size: 20px; font-weight: 600; line-height: 1; }
  details[open] > summary::after { content: '−'; }
  summary:hover::after { color: var(--tx); }
  summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
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

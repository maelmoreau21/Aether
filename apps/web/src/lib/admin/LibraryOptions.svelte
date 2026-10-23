<script lang="ts">
  import type { LibraryOptions } from './types';
  let { options = $bindable(), kind }: { options: LibraryOptions; kind: string } = $props();
  const images = [ ['fetchPoster','Affiche'], ['fetchBackdrop','Fond'], ['fetchLogo','Logo transparent'], ['fetchThumb','Miniature'], ['fetchBanner','Bannière'], ['fetchDisc','Disque'] ] as const;
</script>
<details open><summary>Métadonnées et langue</summary>
  <label>Langue préférée<input bind:value={options.preferredMetadataLanguage} required minlength="2" maxlength="35" placeholder="fr" /></label>
  <label>Pays des classifications<input bind:value={options.metadataCountryCode} required pattern={'[A-Z]{2}'} maxlength="2" placeholder="FR" /></label>
  <p>Préférences enregistrées pour les fournisseurs. La lecture des NFO locaux ne traduit pas leurs textes. Les classifications françaises connues sont prises en compte par le contrôle parental.</p>
  {#if kind !== 'music'}
    <label><input type="checkbox" bind:checked={options.readLocalNfo} />Utiliser les fichiers NFO locaux</label>
    <label for="library-metadata-refresh">Actualiser les métadonnées locales</label><select id="library-metadata-refresh" bind:value={options.automaticRefreshIntervalDays}><option value={0}>Jamais automatiquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
    <p>Le scan planifié réutilise les analyses techniques inchangées et relit les métadonnées et les images locales. Il ne remplace pas les corrections manuelles.</p>
  {:else}<p>Musique : analyse des tags intégrés, sans recherche externe.</p>{/if}
</details>
{#if kind !== 'music'}<details><summary>Images locales</summary>
  <p>Types d’images indexés au prochain scan. Les médias restent en lecture seule.</p>
  {#each images as [key, label] (key)}<label><input type="checkbox" bind:checked={options[key]} />{label}</label>{/each}
</details>{/if}
<details><summary>Fonctions supplémentaires</summary>
  <p>Les fournisseurs en ligne se configurent dans Extensions. Leur priorité par bibliothèque n’est pas encore raccordée au scan.</p>
  <p>Multi-dossiers, écriture NFO/images dans les médias, surveillance temps réel, miniatures de chapitres, trickplay, détection d’intro, regroupements automatiques et téléchargement de sous-titres restent à implémenter.</p>
</details>
<style>
  details { border-top:1px solid var(--border-subtle);margin-top:20px;padding-top:16px; } summary { cursor:pointer;font-weight:600; } label { display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-top:14px;font-size:14px; } input:not([type=checkbox]),select { width:100%;padding:10px;background:var(--bg-surface);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius: var(--radius-md); } p { font-size:13px;color:var(--text-secondary);line-height:1.5; }
</style>

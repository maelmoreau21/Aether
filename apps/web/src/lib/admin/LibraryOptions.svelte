<script lang="ts">
  import type { LibraryOptions } from './types';
  import AdvancedLibraryOptions from './AdvancedLibraryOptions.svelte';
  let { options = $bindable(), kind }: { options: LibraryOptions; kind: string } = $props();
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
  <div class="scan-notice"><strong>Appliquer les changements</strong><p>Enregistrez vos réglages, puis actualisez la bibliothèque pour les appliquer aux médias déjà présents. Les corrections manuelles sont conservées.</p></div>
  <section class="option-section" aria-labelledby="metadata-options-heading">
    <div class="section-heading"><div><h3 id="metadata-options-heading">Métadonnées et langue</h3><p>Choisissez les informations à importer et les préférences de la bibliothèque.</p></div><span class="status">Sources locales</span></div>
    <div class="language-grid">
      <div><label for="library-metadata-language">Langue préférée</label><input id="library-metadata-language" bind:value={options.preferredMetadataLanguage} required minlength="2" maxlength="35" placeholder="fr" aria-describedby="language-hint" /><p id="language-hint" class="hint">Code de langue, par exemple fr ou fr-CA. Préférence enregistrée pour les fournisseurs ; les NFO ne sont pas traduits.</p></div>
      <div><label for="library-metadata-country">Pays des classifications</label><input id="library-metadata-country" bind:value={options.metadataCountryCode} required pattern={'[A-Z]{2}'} maxlength="2" placeholder="FR" aria-describedby="country-hint" /><p id="country-hint" class="hint">Code pays à deux lettres, par exemple FR. Les classifications françaises connues sont utilisées par le contrôle parental.</p></div>
    </div>
    {#if kind !== 'music'}
      <label class="toggle-row"><input type="checkbox" bind:checked={options.readLocalNfo} aria-label="Utiliser les fichiers NFO locaux" /><span><strong>Utiliser les fichiers NFO locaux</strong><small>Importer les descriptions et les informations disponibles à côté des médias.</small></span></label>
      <label class="toggle-row"><input type="checkbox" bind:checked={options.preferEmbeddedTitles} aria-label="Préférer les titres intégrés aux médias" /><span><strong>Préférer les titres intégrés aux médias</strong><small>Utiliser le titre des tags plutôt que le nom du fichier lorsque les métadonnées locales ne fournissent pas de titre. Appliqué au prochain scan.</small></span></label>
      <label for="embedded-subtitles">Sous-titres intégrés proposés par le lecteur</label><select id="embedded-subtitles" bind:value={options.embeddedSubtitlePolicy}><option value="allow_all">Tous les sous-titres intégrés</option><option value="allow_text">Uniquement les sous-titres texte</option><option value="block_all">Ne proposer aucun sous-titre intégré</option></select>
      <p class="hint">Ne modifie pas les fichiers ni les sous-titres externes. Les pistes natives d’un fichier en lecture directe peuvent rester accessibles au navigateur.</p>
      <div class="refresh-setting"><label for="library-metadata-refresh">Actualiser les métadonnées locales</label><select id="library-metadata-refresh" bind:value={options.automaticRefreshIntervalDays}><option value={0}>Jamais automatiquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select><p class="hint">Le scan planifié relit les NFO et les images locales, et réutilise les analyses techniques inchangées. Aucune recherche Internet n’est déclenchée.</p></div>
    {:else}<div class="info-row"><strong>Tags intégrés aux fichiers audio</strong><p>La musique utilise les tags intégrés. L’actualisation automatique des métadonnées locales concerne les films et les séries.</p></div>{/if}
  </section>
  {#if kind !== 'music'}
    <details class="option-section images-section"><summary>Images locales</summary><p>Sélectionnez les illustrations à importer au prochain scan. Les originaux restent dans leurs dossiers, en lecture seule.</p>
      <div class="image-grid">{#each images as [key, label, description] (key)}<label class="image-option"><input type="checkbox" bind:checked={options[key]} aria-label={label} /><span><strong>{label}</strong><small>{description}</small></span></label>{/each}</div>
    </details>
  {/if}
  <AdvancedLibraryOptions bind:options {kind} />
</div>

<style>
  .library-options { display:grid;gap:24px;margin-top:28px;min-width:0; }
  .scan-notice { border:1px solid var(--border-subtle);border-left:3px solid var(--color-info);border-radius:var(--radius-md);padding:16px 20px;background:var(--bg-surface); }
  .scan-notice strong { font-size:14px; } .scan-notice p { margin:6px 0 0; }
  .option-section { min-width:0;border:1px solid var(--border-subtle);border-radius:var(--radius-lg);padding:24px;background:var(--bg-surface); }
  .section-heading { display:flex;align-items:flex-start;justify-content:space-between;gap:16px;margin-bottom:20px; }
  h3 { margin:0;font-size:19px;font-weight:600; } .section-heading p { margin:7px 0 0; }
  .status { flex-shrink:0;padding:5px 9px;border-radius:var(--radius-sm);background:var(--bg-elevated);font-size:11px;color:var(--text-secondary); }
  p,.hint { font-size:13px;color:var(--text-secondary);line-height:1.6; } .hint { margin:8px 0 0;font-size:12px; }
  .language-grid { display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px; }
  label { display:block;font-size:14px;line-height:1.5;margin-bottom:8px; }
  input:not([type=checkbox]),select { box-sizing:border-box;width:100%;min-width:0;min-height:44px;padding:10px 12px;background:var(--bg-main);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:var(--radius-md);font:inherit; }
  input[type=checkbox] { width:19px;height:19px;flex-shrink:0;accent-color:var(--color-play);margin:2px 0 0; }
  .toggle-row { display:flex;align-items:flex-start;gap:12px;margin:24px 0;padding:16px;background:var(--bg-main);border-radius:var(--radius-md);cursor:pointer; }
  strong { font-weight:550; } small { display:block;margin-top:4px;font-size:12px;line-height:1.5;color:var(--text-secondary); }
  .refresh-setting { max-width:640px; }
  summary { cursor:pointer;font-weight:600;font-size:19px;line-height:1.5; } summary:focus-visible { outline:2px solid var(--color-play);outline-offset:5px; }
  .image-grid { display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:12px;margin-top:20px; }
  .image-option { display:flex;align-items:flex-start;gap:12px;padding:16px;margin:0;border:1px solid var(--border-subtle);border-radius:var(--radius-md);background:var(--bg-main);cursor:pointer; }
  .image-option:has(input:checked) { border-color:var(--color-play); } .image-option:has(input:focus-visible) { outline:2px solid var(--color-play);outline-offset:2px; }
  .info-row p { margin-bottom:0; }
  @media(max-width:750px) { .image-grid { grid-template-columns:repeat(2,minmax(0,1fr)); } .section-heading { flex-direction:column;gap:10px; } }
  @media(max-width:520px) { .option-section { padding:18px; } .language-grid,.image-grid { grid-template-columns:minmax(0,1fr); } }
</style>

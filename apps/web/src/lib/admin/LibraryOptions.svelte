<script lang="ts">
  import type { LibraryOptions } from './types';
  import AdvancedLibraryOptions from './AdvancedLibraryOptions.svelte';
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
      <h3 id="metadata-options-heading">Métadonnées et langue</h3>
      <p>Choisissez les informations à importer et les préférences de la bibliothèque.</p>
      <div class="language-grid">
        <div><label for="library-metadata-language">Langue préférée</label><input id="library-metadata-language" bind:value={options.preferredMetadataLanguage} required minlength="2" maxlength="35" placeholder="fr" aria-describedby="language-hint" /><p id="language-hint" class="hint">Code de langue, par exemple fr ou fr-CA. Les NFO ne sont pas traduits.</p></div>
        <div><label for="library-metadata-country">Pays des classifications</label><input id="library-metadata-country" bind:value={options.metadataCountryCode} required pattern={'[A-Z]{2}'} maxlength="2" placeholder="FR" aria-describedby="country-hint" /><p id="country-hint" class="hint">Code pays à deux lettres, par exemple FR. Les classifications françaises connues sont utilisées par le contrôle parental.</p></div>
      </div>
      {#if kind !== 'music'}<label class="toggle-row"><input type="checkbox" bind:checked={options.readLocalNfo} aria-label="Utiliser les fichiers NFO locaux" /><span><strong>Utiliser les fichiers NFO locaux</strong><small>Importer les descriptions disponibles à côté des médias.</small></span></label>
      {:else}<p>La musique utilise les tags intégrés aux fichiers audio.</p>{/if}
    </section>
    {#if kind !== 'music'}
      <section class="option-section" aria-labelledby="local-images-heading"><h3 id="local-images-heading">Images locales</h3><p>Illustrations à importer au prochain scan. Les originaux restent en lecture seule.</p>
        <div class="image-grid">{#each images as [key, label, description] (key)}<label class="image-option"><input type="checkbox" bind:checked={options[key]} aria-label={label} /><span><strong>{label}</strong><small>{description}</small></span></label>{/each}</div>
      </section>
      <section class="option-section" aria-labelledby="image-quality-heading">
        <h3 id="image-quality-heading">Images des fournisseurs</h3>
        <p>Ces réglages s’appliquent aux prochaines collectes de cette bibliothèque. Les images déjà enregistrées et les ajouts manuels restent inchangés.</p>
        <label class="toggle-row"><input type="checkbox" bind:checked={options.artworkDeduplicate} /><span><strong>Écarter les images visuellement similaires</strong><small>Les images d’une même œuvre, d’un même type et d’une même langue sont comparées après téléchargement et décodage. Cette estimation peut ne pas reconnaître tous les doublons.</small></span></label>
        {#if options.artworkDeduplicate}<div><label for="artwork-similarity">Tolérance aux différences</label><input id="artwork-similarity" type="number" min="0" max="16" step="1" required bind:value={options.artworkSimilarity} /><p class="hint">De 0 à 16. Une valeur élevée écarte davantage d’images proches, mais risque aussi d’écarter des affiches différentes. Valeur recommandée : 5.</p></div>{/if}
        <div class="language-grid"><div><label for="artwork-min-width">Largeur minimale d’une affiche (px)</label><input id="artwork-min-width" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterWidth} /></div><div><label for="artwork-min-height">Hauteur minimale d’une affiche (px)</label><input id="artwork-min-height" type="number" min="0" max="4096" step="1" required bind:value={options.artworkMinPosterHeight} /></div></div>
        <p class="hint">0 désactive la limite correspondante. Les dimensions sont vérifiées après téléchargement.</p>
      </section>
      <section class="option-section" aria-labelledby="artwork-rotation-heading"><h3 id="artwork-rotation-heading">Rotation des affiches</h3>
        <p>Chaque utilisateur choisit les bibliothèques concernées dans ses préférences d’affichage. Les images choisies manuellement restent prioritaires.</p>
      </section>
    {/if}
  {:else if section === 'indexing'}
    {#if kind !== 'music'}
      <section class="option-section" aria-labelledby="local-scan-options-heading"><h3 id="local-scan-options-heading">Import et lecture</h3>
        <label class="toggle-row"><input type="checkbox" bind:checked={options.preferEmbeddedTitles} aria-label="Préférer les titres intégrés aux médias" /><span><strong>Préférer les titres intégrés aux médias</strong><small>Utiliser le titre des tags plutôt que le nom du fichier lorsque les métadonnées locales ne fournissent pas de titre. Appliqué au prochain scan.</small></span></label>
        <label for="embedded-subtitles">Sous-titres intégrés proposés par le lecteur</label><select id="embedded-subtitles" bind:value={options.embeddedSubtitlePolicy}><option value="allow_all">Tous les sous-titres intégrés</option><option value="allow_text">Uniquement les sous-titres texte</option><option value="block_all">Ne proposer aucun sous-titre intégré</option></select>
        <p class="hint">Ne modifie pas les fichiers ni les sous-titres externes. Les pistes natives en lecture directe peuvent rester accessibles au navigateur.</p>
      </section>
      <section class="option-section" aria-labelledby="local-refresh-heading"><h3 id="local-refresh-heading">Actualisation locale</h3>
        <label for="library-metadata-refresh">Relire les métadonnées locales</label><select id="library-metadata-refresh" bind:value={options.automaticRefreshIntervalDays}><option value={0}>Jamais automatiquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
        <p class="hint">Le scan planifié relit les NFO et images locales, puis réutilise les analyses techniques inchangées. Aucune recherche Internet n’est déclenchée par ce réglage.</p>
      </section>
    {:else}<p>Le scan musical lit les tags intégrés. L’actualisation des NFO locaux concerne les films et les séries.</p>{/if}
  {:else}
    <AdvancedLibraryOptions bind:options {kind} />
  {/if}
</div>

<style>
  .library-options { display:grid; gap:28px; min-width:0; }
  .option-section { min-width:0; } .option-section + .option-section { border-top:1px solid var(--color-line); padding-top:24px; }
  h3 { margin:0; font-size:20px; font-weight:600; letter-spacing:-.02em; }
  p,.hint { font-size:13px; color:var(--color-muted); line-height:1.6; } .hint { margin:8px 0 0; font-size:12px; }
  .language-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:20px; }
  label { display:block; font-size:14px; line-height:1.5; margin:18px 0 8px; }
  input:not([type=checkbox]),select { width:100%; min-width:0; min-height:44px; padding:10px 12px; background:var(--color-surface); color:var(--color-text); border:1px solid var(--color-line-strong); border-radius:var(--radius-sm); font-size:16px; }
  input[type=checkbox] { width:20px; height:20px; flex:none; accent-color:var(--color-play); margin:2px 0 0; }
  .toggle-row { display:flex; align-items:flex-start; gap:12px; min-height:44px; padding-block:12px; cursor:pointer; }
  strong { font-weight:550; } small { display:block; margin-top:4px; font-size:12px; line-height:1.5; color:var(--color-muted); }
  .image-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:12px; }
  .image-option { display:flex; align-items:flex-start; gap:12px; padding:12px; margin:0; border:1px solid var(--color-line); border-radius:var(--radius-sm); cursor:pointer; }
  .image-option:has(input:checked) { border-color:var(--color-accent); }
  .image-option:has(input:focus-visible) { outline:2px solid var(--color-accent); outline-offset:2px; }
  @media(max-width:520px) { .language-grid,.image-grid { grid-template-columns:minmax(0,1fr); } }
</style>

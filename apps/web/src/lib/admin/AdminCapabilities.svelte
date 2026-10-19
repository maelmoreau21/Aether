<script lang="ts">
  let { section }: { section: string } = $props();
</script>

<div class="capabilities">
  {#if section === 'extensions-section'}
  <section id="extensions-section" aria-labelledby="extensions-heading">
    <div class="section-top"><h2 id="extensions-heading">Extensions</h2><span>Parcours en préparation</span></div>
    <p>Enrichir les fiches de vos films, connecter des sources de métadonnées et ajouter des statistiques : les extensions font partie de la direction retenue pour le serveur.</p>
    <p class="availability" role="status">Les dépôts, le catalogue, l'installation et l'exécution des extensions ne sont pas encore disponibles. Les éléments ci-dessous décrivent le parcours prévu.</p>
    <div class="detail">
      <h3>Du dépôt à l'activation</h3>
      <ol>
        <li>Ajouter un dépôt et vérifier son éditeur.</li>
        <li>Parcourir son catalogue : fonction, version, compatibilité et provenance.</li>
        <li>Examiner les droits demandés et choisir les bibliothèques concernées.</li>
        <li>Installer puis activer l'extension après accord de l'administrateur.</li>
      </ol>
      <p>Une mise à jour demandant de nouveaux droits nécessitera un nouvel accord. Désactivation, révocation des droits et journal des actions sont prévus.</p>
    </div>
    <div class="detail">
      <h3>Des autorisations propres à chaque extension</h3>
      <dl>
        <div><dt>Métadonnées</dt><dd>Autoriser séparément la lecture des fiches et leur enrichissement.</dd></div>
        <div><dt>Statistiques</dt><dd>Distinguer les chiffres agrégés des historiques personnels.</dd></div>
        <div><dt>Réseau</dt><dd>Limiter les échanges aux domaines explicitement approuvés.</dd></div>
        <div><dt>Bibliothèques et tâches</dt><dd>Sélectionner les bibliothèques accessibles et approuver les tâches automatiques.</dd></div>
        <div><dt>Fichiers médias</dt><dd>Lecture seule par défaut. Toute écriture ou suppression demandera une autorisation distincte et explicite.</dd></div>
      </dl>
      <p>Installer une extension ne lui donnera pas automatiquement tous les droits du compte administrateur.</p>
    </div>
    <div class="detail">
      <h3>Intégrations du serveur</h3>
      <p>Les Media Nodes et le fournisseur OIDC se configurent dans leurs sections dédiées. Ils font partie des fonctionnalités du serveur et ne nécessitent aucune extension.</p>
    </div>
  </section>
  {/if}
</div>

<style>
  .capabilities { display: grid; gap: 1.5rem; }
  section { background: var(--bg-surface); border: 1px solid var(--border-subtle); border-radius: 16px; padding: 1.5rem; scroll-margin-top: 6rem; }
  .section-top { display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; }
  h2 { color: var(--text-primary); font-size: 1.3rem; margin: 0; }
  h3 { color: var(--text-primary); font-size: .95rem; margin: 0 0 .5rem; }
  .section-top span { color: var(--text-secondary); background: var(--bg-elevated); border-radius: 999px; font-size: .75rem; padding: .4rem .7rem; }
  p { color: var(--text-secondary); line-height: 1.65; }
  .detail { border-inline-start: 2px solid var(--border-subtle); padding-inline-start: 1rem; margin-top: 1.25rem; }
  .detail p { margin-bottom: 0; }
  .availability { padding: 1rem; background: var(--bg-elevated); border-radius: .65rem; }
  ol { padding-inline-start: 1.25rem; color: var(--text-secondary); line-height: 1.7; }
  li { padding-inline-start: .2rem; margin-block: .4rem; }
  dl { display: grid; gap: .85rem; }
  dt { color: var(--text-primary); font-size: .9rem; font-weight: 600; }
  dd { margin: .25rem 0 0; color: var(--text-secondary); font-size: .9rem; line-height: 1.6; }
  @media (max-width: 600px) { section { padding: 1.1rem; } }
</style>

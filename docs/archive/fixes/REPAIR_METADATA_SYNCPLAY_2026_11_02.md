# Métadonnées, plugins et SyncPlay — 2 novembre 2026

## Organisation des dépôts

Le dossier `C:/Users/Mael/Documents/GitHub/Aether` contient les deux dépôts
`Aether` et `Aether-Plugins`. L'ancienne copie de publication de Poster Rotator
a été déplacée, sans suppression, vers
`C:/Users/Mael/Documents/Aether-archives/Aether-Plugins-poster-release-2026-11-02`.
`git worktree repair` a rétabli ses liens Git ; sa branche et son contenu sont
conservés. Poster Rotator et les avatars restent des fonctionnalités natives.

## Identification

Les identifiants NFO du fournisseur permettent maintenant de demander ses
détails directement, sans imposer la correspondance du titre local. Le lecteur
NFO comprend aussi les attributs historiques Jellyfin et les variantes de noms
d'identifiants. Le champ `synopsis` du SDK est effectivement enregistré ; les
corrections manuelles restent prioritaires.

La fenêtre Identifier montre les IDs TMDb, IMDb et TVDb et autorise une recherche
par ID seul. Vider ces champs transmet `{externalIds:{}}` pour rechercher par
titre. Les recherches sans NFO conservent le titre et l'année ; les fournisseurs
corrigés gèrent également la conversion entre identifiants et le repli sans année.

Les paquets TMDb et IMDb 0.1.1 signés sont embarqués dans le Core. Le démarrage
conserve désormais l'activation et le périmètre choisis par l'administrateur pour
les extensions déjà installées. Les autres sources, paramètres et paquets locaux
se trouvent dans le dépôt voisin :
[rapport plugins](../../../../Aether-Plugins/docs/METADATA_REPAIR_2026_11_02.md).

## Mise à jour des plugins : limite conservée

Le changement de version dans l'interface utilisait une désinstallation suivie
d'une installation. Il passe désormais une demande `upgrade:true` à la route
du catalogue, qui applique les mêmes contrôles que la route d'upgrade.
Un refus conserve la version et ses paramètres.

La bascule des versions existantes reste **refusée par le Core** : voir
[le contrat de mise à jour](../../PLUGIN_UPDATE_LIFECYCLE.md). Les nouveaux paquets
embarqué/local ne remplacent donc pas automatiquement une installation existante.
Ni la désinstallation ni un remplacement SQL ne constituent une migration sûre.
Le catalogue public signé reste à sa version publiée ; aucune release n'a été
effectuée par cette intervention.

## Comparaison des implémentations Jellyfin

Sources locales examinées dans `C:/Users/Mael/Downloads/Jellyfin` :

- Fournisseur TMDb : résolution des identifiants avant la recherche textuelle.
- NFO : variantes historiques des identifiants et attributs du champ `id`.
- Client `PlaybackCore.js` : commandes programmées et notifications du lecteur.
- `WaitingGroupState.cs` : attente de chargement distincte de la pause demandée.

Les changements SyncPlay et ses contrats sont décrits dans [SYNCPLAY.md](../../SYNCPLAY.md).

## Vérifications et portée

- Chrome 156.0.8078.74, véritable interface Svelte avec réponses API simulées :
  IDs NFO préremplis/transmis, suppression des IDs pour recherche textuelle,
  recherche avec ID seul, rendu 1440/390 px et absence d'erreurs JavaScript.
  Captures et rapport : `output/playwright/identify-*`.
- Requête HTTPS réelle TMDb, film 27205, langue fr-FR, configuration par défaut
  du Core : HTTP 200, titre Inception, résumé et affiche présents. Cette requête
  seule ne prouve pas le parcours complet Core/Wasm/import d'images sur le NAS.
- Tests Rust ciblés : NFO, métadonnées/identification avec worker signé,
  conservation des corrections manuelles et protocole SyncPlay.
- Résultats : 7 tests NFO, 26 tests unitaires extensions (un navigateur ignoré),
  un scénario HTTP d'identification, un scénario de persistance du résumé et
  10 tests unitaires Rust SyncPlay réussis. Les 7 tests frontend SyncPlay et les
  51 tests natifs fournisseurs réussissent également.
- SyncPlay : scénario HTTP/TCP avec deux comptes et deux vrais WebSockets
  réussi (18,71 s d'exécution), couvrant création/adhésion, ping, play/pause/seek,
  refus de contrôle participant, compte à rebours/reconnexion et reprise après
  buffering ou déconnexion. Il a permis de corriger la référence SQL à une
  colonne d'épisode inexistante, qui empêchait toute création de séance.
- Vérifications Svelte et ESLint ; tests fournisseurs natifs, ABI Wasm,
  signatures et exécution des guests avec HTTP simulé dans Aether-Plugins.
- Clippy des bibliothèques Core/Node avec `-D warnings`, formatage Rust et
  `git diff --check` réussis.

Les résultats finaux des commandes sont consolidés dans `MEMORY.md`.
Ces preuves sont locales. Aucune session de deux appareils physiques sur le NAS,
aucune validation d'endurance et aucun déploiement ne sont revendiqués.

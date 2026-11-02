# Refonte UX — audit et premier jalon, 28 octobre 2026

## Périmètre et méthode

Lecture des règles, architecture, spécification, direction visuelle, sources Svelte,
styles, sélection des images, contrats frontend et tests. Inspection du worktree :
les changements préexistants backend, Docker, utilisateurs/groupes sont conservés.
Les skills frontend-design, web-design-guidelines et playwright ne sont pas
installés dans les répertoires disponibles. Playwright du projet est utilisé.

Le premier jalon couvre le shell de navigation et la découverte sur l’accueil.
Les fiches sont concernées par le composant Poster commun, mais leur architecture
et leurs actions techniques ne sont pas refondues dans ce jalon. Aucune modification
de Core, schéma, API, autorisations, moteur vidéo ou médias.

## Diagnostic priorisé du code réel

| Priorité | Parcours / observation | Impact / fréquence | Complexité / décision |
|---|---|---|---|
| Haute | Header : bibliothèques et destinations dans une seule rangée ; libellés masqués sur petit écran | Navigation difficile, chaque visite mobile | Moyenne ; corrigé par navigation inférieure nommée et sélecteur de bibliothèques |
| Haute | Accueil : hasard avant récence, mêmes œuvres répétées dans jusqu’à neuf rangées contextuelles | Découverte peu prévisible, chaque accueil | Faible ; reprise, ajouts films, ajouts séries, notes disponibles seulement |
| Haute | Film : feedback, outils, téléchargement et versions avant ou autour de la lecture ; caractéristiques techniques ouvertes | Lecture difficile à repérer, chaque fiche | Moyenne ; prochain jalon : action essentielle puis divulgation progressive |
| Haute | Série : cartes de saisons ET select dans les épisodes ; titre et logo redondants ; descriptions longues | Deux commandes pour le même choix | Moyenne ; prochain jalon : un seul sélecteur, épisodes 16:9 et action lisible |
| Haute | Administration : navigation modale même en desktop ; très grande route mêlant état, requêtes et sections masquées | Repérage et maintenance, chaque session admin | Élevée ; futur shell avec sidebar persistante et compatibilité des fragments existants |
| Moyenne | Cartes : badge film/série malgré rangée explicite, genres, année absente, bouton de lecture décoratif menant à une fiche | Bruit visuel, toutes les grilles | Faible ; corrigé : affiche, titre sur deux lignes, année/note disponibles, états utiles |
| Moyenne | Reprise : flèches aria-hidden et exclues du clavier, état non observé après chargement ; poster possible dans cadre paysage | Accessibilité et cadrage, utilisateurs avec historique | Faible ; corrigé : observation, clavier, reduced motion, repli sans poster étiré |
| Moyenne | Recherche / filtres : deux composants de filtres existent ; filtres techniques mélangés aux critères éditoriaux | Charge cognitive, recherche avancée | Moyenne ; conserver contrats et pagination, harmoniser dans un jalon dédié |
| Moyenne | Musique : présentation morceau/album/artiste sur page chargée ; contrôles et métadonnées techniques denses sur téléphone | Usage fréquent des bibliothèques musicales | Moyenne ; navigation commune et dégagement du lecteur vérifiés, composition à revoir |
| Moyenne | Playlists : création en tête, paramètres de partage, recherche et administration du partage dans le même parcours | Confusion édition / écoute / administration | Moyenne ; harmonisation ultérieure sans retirer partage ni droits |
| Moyenne | Paramètres : sections personnelles déjà présentes ; options d’affiches et avancées nombreuses | Utilisation occasionnelle, formulaires longs | Moyenne ; architecture conservée, menu profil borné à la hauteur de l’écran |
| Moyenne | Bibliothèques admin : éditeur déjà en dialogue ; nombreuses options avancées et commandes proches « scanner / actualiser » | Risque de mauvais choix, configuration | Élevée ; simplifier par tâche, conserver options et erreurs |
| Moyenne | Utilisateurs / groupes : éditeurs et corrections de focus déjà en cours dans le worktree | Opérations sensibles | Moyenne ; ne pas écraser ce travail, garder les deux rôles |
| Moyenne | Nodes / tâches : diagnostic, appairage ancien et avancé ; plusieurs familles de tâches et d’anomalies | Opérations techniques, fréquence variable | Élevée ; regrouper l’état et l’action, détails techniques à la demande |
| Basse | Téléchargements : route autonome accessible sans session réseau, retour vers médiathèque, copie locale et erreurs explicites | Usage hors ligne | Moyenne ; conserver cette indépendance, harmoniser ultérieurement |
| À préserver | Connexion/bootstrap, lecteur immersif et pistes, permissions Core, langues, thèmes, pagination et cache | Fonctionnement critique | Élevée ; ne pas réécrire ces mécanismes pour ce jalon |

L’audit navigateur de premier niveau couvre accueil, grilles, fiches film/série,
recherche, musique, playlists, paramètres, vue d’ensemble admin et téléchargements.
Il utilise des APIs interceptées, pas les données personnelles du serveur.
Les autres rubriques administratives, le bootstrap/SSO et le décodage vidéo sont
inspectés dans le code mais ne constituent pas une validation fonctionnelle réelle
exhaustive dans ce jalon.

## Décisions livrées

- Cinéma OLED / Pierre Douce, tokens, polices et logo officiels conservés.
- Desktop : destinations stables ; bibliothèques personnalisées dans un sélecteur
  nommé. Mobile/tablette jusqu’à 900 px : header marque/profil et quatre destinations
  en bas, avec safe area. Téléchargements dans le sélecteur et lien desktop direct.
- Dialogue natif : clavier, Échap, retour au déclencheur ; liste défilante pour les
  bibliothèques longues, fermeture et téléchargements hors de la liste défilante.
- Hero stable sur la page reçue, favorisant un vrai backdrop sélectionné par la
  politique d’artwork existante. Logo ou titre, métadonnées essentielles, résumé
  limité, action et informations. Aucun agrandissement de poster en panorama.
  Images cassées : repli textuel/neutre. Aucun endpoint supplémentaire.
- Accueil borné à 16 affiches par rangée récente. « Tout voir » conserve le tri
  ajouté-descendant. Une seule rangée de notes si au moins quatre œuvres ont une
  note >= 6 ; aucun classement personnalisé revendiqué.
- Reprise fondée sur l’historique reçu ; étiquette « Reprendre » pour le film
  correspondant ; le lancement utilise toujours le parcours existant. Les séries
  ouvrent leur fiche via « Voir les épisodes ».
- Affiches 2:3, titre sur deux lignes, progression accessible, état indisponible et
  visionné conservés. Suppression du faux bouton de lecture au survol des affiches.
- Le lecteur musical reste au-dessus de la navigation mobile ; le dialogue vidéo
  natif reste dans la top layer, sans modification du moteur.

## Validation reproductible

Depuis `apps/web` : `npm run check`, `npm run lint`, `npm test`, `npm run build`.
Le check conserve l’avertissement préexistant `CatalogGrid.svelte:90`
(`state_referenced_locally`). Ne pas assimiler cet avertissement à une erreur.

Depuis `apps/web`, lancer `npx vite preview --host 127.0.0.1 --port 5182`, puis,
depuis le dépôt : `node scripts/browser-ux-foundations.cjs`.
Variables optionnelles : `MEDIA_UI_URL` (URL finissant par `/app/`) et
`MEDIA_REPORT_DIR`. Preuves locales dans `target/ux-foundations/`, rapport JSON,
captures avant et après. Le script échoue sur les assertions du jalon ; il
consigne aussi les problèmes d’audit des autres routes et échoue s’il en reste.

Résultats finaux : check sans erreur (un avertissement existant), lint et build
statique réussis ; 177 tests réussis dans 29 fichiers. Banc Chrome 156.0.8078.48 :
25 scénarios réussis, 100 prises de capture, aucune anomalie restante dans le
rapport. Captures inspectées : accueil mobile/tablette/desktop, menus longs dans
les deux thèmes, paysage, repli d’images, logo, lecteur musical et écrans d’audit.
Le script historique `browser-cinema-ui.cjs` s’arrêtait sur les paramètres avec
une fixture d’avatars incomplète (champ `packs` absent). Le nouveau banc fournit
ce contrat et valide le parcours ; aucun changement du produit pour ce problème
de fixture. Aucun résultat de serveur média réel n’est déduit de ces tests.

Les fixtures SVG et données synthétiques sont strictement dans le script de test.
Aucun média fictif n’est ajouté au produit. Navigateur Chrome Windows headless ;
largeurs 320, 360, 390, 430, 768, 1024, 1440, 1920 dans les deux thèmes ; paysage
844×390 avec toucher simulé. Vérification des actions hero, proportions, menu
avec 21 bibliothèques, titres longs, focus visible, Échap et retour focus,
navigation bibliothèque, cartes hors écran atteignables au clavier, lecteur
musical dégagé, états chargement/vide/erreur/images cassées/logo/sans backdrop et
indisponibilité. Ce contrôle ne prouve ni le décodage AV1/Opus ni une lecture
musicale audible ; le serveur média simulé ne fournit pas de flux lisible.

Pas de mesure comparative de performance, d’audit WCAG complet, de preuve NAS,
Authentik, Safari, téléphone physique ou réseau distant. Les captures utilisent
les images synthétiques du banc ; la qualité éditoriale de vrais visuels reste
à vérifier sur une bibliothèque personnelle autorisée.

## Jalons suivants

État au 29 octobre : les jalons fiches film/série et cadre administrateur sont
implémentés et validés localement ; voir [Fiches UX](UX_DETAILS_2026_10_29.md) et
[Administration UX](UX_ADMIN_2026_10_29.md). Les parcours internes de configuration
des bibliothèques restent la prochaine priorité. Les limites de serveur réel
restent distinctes.

1. Fiches film/série : lecture avant les outils secondaires, logo/titre unique,
   informations techniques repliables, sélection unique de saison et épisodes.
2. Administration : extraire un shell et navigation persistante desktop ; préserver
   tous les fragments et états d’édition, puis travailler bibliothèque par tâche.
3. Harmoniser recherche, musique, playlists, compte et téléchargements ; séparer
   les préférences personnelles de l’administration dans les parcours concernés.
4. Tests réels avec Core, flux AV1/Opus et médias synthétiques légaux, puis
   vérification manuelle tactile et technologies d’assistance.

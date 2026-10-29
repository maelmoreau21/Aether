# Refonte UX — fiches film et série, 29 octobre 2026

## Changements du jalon

L’action principale passe avant les outils d’organisation, le suivi et les détails
techniques. `DetailHero.svelte` partage la composition des deux fiches : titre ou
logo unique avec repli en cas d’erreur, année/classification/note disponibles,
durée connue du descripteur pour le film, résumé court extensible et genres.
L’affiche reste en 2:3 sur desktop ; sur téléphone avec backdrop, le contenu
éditorial et la lecture passent au premier plan. Aucun poster n’est utilisé comme
panorama, même lorsqu’il n’existe pas de backdrop. Les affiches masquées sur mobile
restent en chargement différé. Les illustrations auparavant accumulées sans contexte
ne sont plus affichées en galerie automatique.

Pour le film :

- Lecture/reprise et disponibilité avant les commandes secondaires.
- Choix de version, meilleur fichier disponible et inventaire vidéo/audio/textes
  regroupés dans « Versions et informations techniques », fermé initialement.
- Téléchargement conservé auprès des outils secondaires ; statut vu, favoris et
  note personnelle dans « Mon suivi et mes préférences ».
- Distribution, provenance et outils de gestion existants conservés. Les callbacks
  de lecture et la sélection manuelle de version restent ceux du catalogue.

Pour la série :

- Une seule commande de saison, le select au-dessus des épisodes. Les cartes de
  saisons concurrentes sont retirées. Le visuel propre à la saison sélectionnée
  reste affichable à côté du select lorsqu’il existe.
- Action « Voir les épisodes » dans le hero : déplacement local du focus et du
  scroll, sans navigation/rechargement de la route. Le contrôle paysage vérifie
  que le titre des épisodes est réellement visible. Préférences après la liste.
- Épisodes en lignes desktop et en colonne mobile avec vignette 16:9, numéro,
  titre, progression/état réellement connu et action de lecture visible.
- Résumé complet et suivi disponibles dans une section repliable ; version et
  téléchargement dans une autre section. Aucun préchargement de tous les
  descripteurs pour inventer des durées : durée affichée seulement si déjà connue.
- Pagination de 24 épisodes et contrat `episodesPage` conservés, annulation des
  demandes et erreurs explicites. Changer de saison ramène à la première page.

Aucune dépendance ajoutée. Aucun changement de rôle, API, base, moteur vidéo ou
média personnel. Les dialogues historiques de `WorkTools` et l’administration
ne sont pas refondus dans ce jalon.

## Vérification

Commandes Web : `npm run check`, `npm run lint`, `npm test`, `npm run build`.
Le check conserve l’avertissement existant `CatalogGrid.svelte:90`.

Résultats du build final : check sans erreur, lint et build statique réussis,
177 tests unitaires réussis dans 29 fichiers. Le banc des fiches passe 23 scénarios
et deux lancements avec décodage MP4 ; la non-régression des fondations passe
25 scénarios sans anomalie. Chrome 156.0.8078.48, Windows headless. Captures
mobile, desktop, épisodes, versions, absence d’images et lecteur inspectées.

Banc ciblé : `node scripts/browser-ux-details.cjs`, après le build et
`npx vite preview --host 127.0.0.1 --port 5182` depuis `apps/web`.
Variables : `MEDIA_UI_URL`, `MEDIA_REPORT_DIR`, `MEDIA_UI_MEDIA_FIXTURE`.
Le test réutilise les fixtures d’authentification/images du banc des fondations,
avec une API catalogue et des descripteurs synthétiques spécifiques aux fiches.

Couverture : 320, 360, 390, 430, 768, 1024, 1440, 1920 px, deux thèmes, clavier,
focus, version film et épisode, changement de saison sans navigation, pagination
locale et distante simulée, retour d’erreur, textes longs, absence d’images,
images cassées, indisponibilité et série vide ; paysage avec toucher simulé.
Non-régression du jalon précédent : `node scripts/browser-ux-foundations.cjs`.

Le test facultatif de lecture utilise `target/streaming-video-window-poc/gop15.mp4`,
fixture AV1 10 bits + Opus créée par `scripts/browser-video-window-poc.cjs` à partir
de `testsrc2` et `sine`. Il n’exécute pas ce générateur, ne réencode aucun fichier
et ne nécessite pas FFmpeg pour lire la fixture présente. Il observe des images
décodées après lancement depuis la fiche film puis un épisode, pause/reprise,
seek clavier et sortie du lecteur (Échap quitte d’abord le plein écran).
Les données et les requêtes Range sont interceptées par Playwright : ceci ne
valide pas le Core/Node, l’accès NAS, les signatures de streaming ou le protocole
HTTP Range réel. Pas d’écoute humaine ni de nouveau résultat de décodage MKV.

Preuves locales : `target/ux-details/report.json`, captures et dossier `before/`.
Les images éditoriales restent des SVG de test, exclusivement dans le banc.
Pas de mesure comparative de performance ni d’audit WCAG exhaustif revendiqué.

## Suite

Le cadre administrateur a depuis été livré et vérifié localement :
[Administration UX](UX_ADMIN_2026_10_29.md). La suite porte sur ses parcours internes.

La prochaine priorité est le shell administrateur : navigation persistante sur
large écran, menu accessible sur mobile et conservation des fragments existants,
avant de simplifier chaque parcours d’administration. Les changements backend et
administrateur déjà présents dans le worktree sont indépendants et conservés.

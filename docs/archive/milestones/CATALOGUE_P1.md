# P1 — Premier parcours de catalogue local

15 octobre 2026. Ce jalon remplace la présentation de démo par une navigation
Accueil / Films / Séries / Recherche / Paramètres et un catalogue obtenu par API.
Le lecteur immersif existant et son moteur natif sont conservés. Le serveur
reste un banc de fixtures sur 127.0.0.1, pas un standalone sécurisé terminé.

## Parcours et données

Routes statiques : `/app/`, `/app/films/`, `/app/series/`, `/app/search/`,
`/app/settings/`. Une fiche est ouverte par le paramètre `work`, par exemple
`/app/films/?work=fixture-film`. Les routes se rechargent directement depuis
Axum ; le post-traitement du build externalise récursivement les bootstraps
SvelteKit pour conserver la CSP sans script inline.

Films et Séries ont des listes distinctes. La recherche et le tri alphabétique
sont transmis à l'API ; la pagination affiche 24 œuvres à la fois. Cela borne
les cartes affichées, sans constituer une mesure de performance à grande échelle.

Le film `fixture-film` possède une édition, une MediaVersion et deux
MediaSources explicites MKV/MP4. Les assets FR/EN et leurs Tracks réutilisent
l'inventaire de lecture existant. Aucune fusion par titre ou nom de fichier.
La série `fixture-series` contient la saison 1 et l'épisode 1
`fixture-episode-1` : cet épisode réutilise explicitement la même mire que le
film pour vérifier la navigation. Ce sont des associations synthétiques,
pas une identification de véritables œuvres. Les épisodes ne sont pas listés
comme des films indépendants.

La lecture d'une fiche est limitée aux sources associées à son œuvre ou à son
épisode. Les commandes audio, texte, volume, vitesse, seek et plein écran restent
celles du lecteur immersif. Les scripts de non-régression amorcent désormais
la fiche du film, sans retirer leurs assertions de décodage et d'erreur.

Année, affiches et métadonnées réelles sont absentes. Les affiches de repli sont
typographiques et signalées. Historique, reprise, dates d'ajout, séries en cours
et recommandations ne sont pas inventés : les rubriques correspondantes
expliquent leur indisponibilité. Les collections de fixtures disponibles sont
présentées séparément des rubriques de récence non disponibles.

## Limite de sécurité et prochain jalon

Bootstrap administrateur, authentification, sessions, RBAC, SQLite et scanner
ne sont pas implémentés. Les identifiants d'association et bibliothèques de
fixtures ne constituent pas un catalogue persistant ni des autorisations.
Aucun dossier personnel n'est configuré, scanné ou exposé ; aucun original
modifié. Le confinement loopback et les contrôles Host/Origin restent en place.

Prochain petit jalon : bootstrap administrateur local sécurisé, sessions et
contrôles backend `administrator` / `user`, avec persistance SQLite et tests
d'échecs. L'ajout d'un dossier explicitement configuré et monté `ro`, puis un
scan hors requêtes HTTP et limité en ressources, vient après ce prérequis.

## Vérifications finales

- Backend : `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`
  et `cargo test --locked` réussis, 15 tests Rust.
- Web : check Svelte/TypeScript zéro erreur et zéro avertissement, lint,
  8 tests Vitest et build réussis après les ajustements visuels.
- `node scripts/browser-p1.cjs` : Chrome 155.0.8059.40, deux parcours MKV/MP4 réussis
  après adaptation de l'amorçage ; assertions pixels/PCM, audio, seek,
  sous-titres, fin et erreurs conservées.
- `node scripts/browser-catalog.cjs` : Chrome 155.0.8059.40 sous Windows,
  headless, viewports 390/768/1440 px. Navigation, fiches, série puis lecture
  MP4 de l'épisode, recherche sans résultat, catalogue vide par interception,
  erreur 503 puis réessai, fiche 404 et pagination simulée de 72 œuvres limitée
  à 24 cartes réussis. Captures Accueil/Films/Séries desktop et mobile inspectées.
  Dernière relance : état de chargement explicite et refus de présenter un
  épisode comme film via un paramètre `work` détourné également vérifiés.
- `node scripts/browser-p1-layout.cjs` : 360/390/768/1280/1440/1920 px,
  aucun débordement horizontal ni erreur CSP ; audio 404 avec une tentative,
  sous-titre 404 et Core 503 correctement signalés.
- `node scripts/browser-immersive.cjs` : plein écran réel et refus simulé,
  menus, clavier et tactile simulé 390×844 / 844×390 réussis.

Preuves : `evidence/2026-10-15/catalogue/report.json`, et sous-dossiers
`regression/`, `layout/`, `immersive/`. La pagination sur données simulées
ne mesure pas un catalogue réel de 72 médias. Aucun nouveau résultat
Edge/Firefox/Safari ni nouvelle expérience P0 n'est revendiqué.

Les commandes de build/serveur restent celles de `P1_WEB_INTEGRATION.md`.
Les résultats sont locaux, sans CI distante, preuve NAS, catalogue personnel,
charge, films longs ou client mobile physique. La pagination seule ne démontre
pas une capacité de catalogue ou de lectures simultanées.


## Ajustement de navigation et profil

La navigation principale contient désormais Accueil / Films / Séries / Recherche.
La recherche présente une loupe SVG et son texte. Paramètres reste à
`/app/settings/`, accessible depuis l'avatar en haut à droite. Le menu indique
l'absence de profil réel ; Profil est désactivé et marqué « Bientôt ».
Ce composant reste frontend, sans compte ni changement de permissions.
Clavier, clic extérieur, Échap, focus et formats desktop/mobile ont été vérifiés
par browser-profile.cjs ; catalogue et lecteur immersif rejoués avec succès.
Preuves : evidence/2026-10-15/profile-navigation/.


### Clarification des responsabilités (15 octobre 2026)

Mon profil (bientôt) concerne avatar, identité, mot de passe et préférences de
langue/audio/sous-titres. Paramètres concerne les réglages personnels de
l’application : aucun ajout ni gestion de bibliothèques dans cet écran.
Bibliothèques Films/Séries, dossiers, scans, utilisateurs, permissions et
configuration serveur appartiennent à la future Administration. Cette entrée
reste absente jusqu’à une identité administrator vérifiée ; ses opérations
nécessiteront des contrôles backend. Déconnexion reste absente sans auth.
Aucune fonction ni rôle n’est simulé. Check/lint/build et test navigateur profil
ciblé réussis ; preuves evidence/2026-10-15/personal-settings/.

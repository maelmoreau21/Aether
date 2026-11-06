# Mémoire Aether — point d’entrée

Le code et les tests actuels font foi. Cette mémoire est un index de décisions et
preuves historiques, pas une garantie de comportement ni de déploiement actuel.
Consulter les documents du domaine et les sources avant toute modification.

- Architecture et frontières : [ARCHITECTURE.md](../ARCHITECTURE.md).
- Sécurité : [docs/SECURITY.md](../docs/SECURITY.md).
- Lecture et résultats observés : [docs/STREAMING_COMPATIBILITY.md](../docs/STREAMING_COMPATIBILITY.md).
- Déploiement : [docs/DEPLOYMENT_DOCKER.md](../docs/DEPLOYMENT_DOCKER.md).
- Extensions : [docs/PLUGIN_UPDATE_LIFECYCLE.md](../docs/PLUGIN_UPDATE_LIFECYCLE.md).
- Historique complet jusqu’au 3 novembre 2026, y compris les travaux locaux :
  [archive de la mémoire](../docs/archive/MEMORY_2026-11-03.md). Les entrées y
  sont datées mais pas strictement chronologiques ; rechercher par sujet et date.

Les futures entrées restent courtes : périmètre, décision, vérifications réellement
exécutées et limites ; les détails propres à un domaine vont dans son document.
Archiver périodiquement les entrées anciennes sans les modifier ni perdre leur contexte.

## 6 novembre 2026 — CI formatage et attentes des tests d’avatars

- Formatage de `extensions/automated.rs` corrigé. Test `account` aligné sur le
  contrat natif actuel (`allowCustom=true` par défaut), avec indépendance des
  extensions vérifiée avant/après activation de la politique persistée.
- Test `settings_avatars` : politique restrictive configurée explicitement par
  l’API admin ; envoi dépassant 2 Mio attendu en 413 (limite HTTP), petits envois
  interdits en 403. Aucun changement du comportement produit.
- Vérifié sous Windows : `cargo fmt --check`, `git diff --check`, tests
  `account` (4/4) et `settings_avatars` (2/2), avec `--locked --all-features`.
  Shard complet et confirmation Linux/GitHub CI non exécutés.

## 6 novembre 2026 — Activation et visibilité des statistiques

- Collecte conservée désactivée par défaut. Admin : seul contrôle d’activation
  tant qu’elle est arrêtée ; ensuite six sous-menus (vue d’ensemble, activité,
  contenus, technique, historique, paramètres), filtres avancés repliés.
- Visibilité personnelle indépendante (`usersCanView`, défaut false) ; collecte
  active requise pour les utilisateurs, liens personnels masqués et refus Core
  sur rapports, analyses, exports, détails et Wrapped. Historique conservé et
  administration/API conservée ; effacement personnel toujours possible par API.
- Bandeau global de collecte supprimé. Voir `docs/STATISTICS.md`.
- Vérifié : 12 tests Rust statistiques/SQLite réussis (11 au premier passage,
  puis nouveau test de visibilité après correction de son jeton synthétique) ;
  deux contrats PostgreSQL ignorés. Douze tests Web ciblés réussis, check sans
  erreur/avertissement et lint réussis. Chrome/API synthétique : activation,
  six sous-menus, visibilité personnelle dans les quatre combinaisons et retrait
  du bandeau, aperçu mobile 390 px. Build final et formatage Rust ciblé réussis.
  Aucun déploiement ni validation PostgreSQL/lectures réelles supplémentaire.

## 6 novembre 2026 — Menus playlist, thème et drapeaux

- Menus playlist média/musique : fermeture au clic extérieur et Échap, avec
  retour au déclencheur au clavier. Les clics internes restent possibles.
- Menu profil : sélection du thème réellement affiché, même en mode système,
  mise à jour lors des changements du système ; FR/EN remplacés par les
  drapeaux SVG locaux existants, avec noms accessibles et état sélectionné.
- Vérifié : Web check (0 erreur/avertissement), lint, build et diff. Chrome avec
  API synthétique : clic interne/externe, réouverture, ajout à une playlist,
  Échap/focus, largeur 390 px ; changement système clair→sombre, sélection
  explicite et langue. Captures dans `output/playwright/menu-fixes/`.
  Pas de validation du serveur déployé ni du parcours musical séparé.

## 6 novembre 2026 — Paramètres d’avatars et chemins Docker

- Politique d’avatars native à nouveau persistante : import personnel et attribution
  à la création configurables ; obligation distincte, activée par défaut. Import
  normalisé restauré dans API/profil, suppression réservée aux avatars facultatifs.
  Les avatars facultatifs supprimés ne sont plus réattribués au démarrage.
- Compose autonome sans répétition des chemins assets/backups déjà définis dans
  l’image ; repli PostgreSQL vers les dossiers persistants présents. Diagnostic
  explicite du mode privé 0700 ; procédure UID/GID personnalisé dans
  `docs/ADMIN_DEPLOYMENT.md`. Aucun droit de volume NAS modifié.
- Vérifié localement : Web check (0 erreur/avertissement), lint, build, Compose
  config, fmt/diff ; huit tests Rust ciblés réussis (avatars, HTTP/galeries avec
  politique persistée et redémarrage, sauvegardes). Aucun déploiement ni validation
  PostgreSQL/NAS ou navigateur réelle ; test Unix de permissions ajouté, non
  exécuté sous Windows.

## 6 novembre 2026 — Isolation de la panne simulée multi-Node

- Le test arrête désormais le runtime propre à chaque Node et attend sa fin :
  arrêter le seul serveur HTTP laissait vivre les tâches de scan et leurs callbacks.
  Les assertions de bascule et les 50 flux concurrents sont conservés. Le scénario
  ciblé réussit localement sous Windows ; confirmation Linux/GitHub CI nécessaire.

## 6 novembre 2026 — Diagnostic CI du basculement multi-Node

- Le test de basculement à 50 requêtes a échoué en CI avec le média primaire
  sélectionné au lieu de la copie de secours. L'assertion conserve son exigence
  et affiche désormais les statuts et la télémétrie des deux Nodes en cas d'échec.
- Le scénario ciblé a réussi deux fois localement sous Windows, dont une après
  ajout du diagnostic ; `cargo fmt --check` réussit. Cause non établie et
  reproduction Linux/CI encore nécessaire ; aucun correctif produit revendiqué.

## 5 novembre 2026 — Tâches post-scan, permissions par type, images et lecteur

- Tâches non-scan (trickplay, chapitres, analyses) suivies par `active_task_runs`
  (`migrations/task_runs.sql`) : elles ne réécrivent plus `scan_jobs`, ne bloquent
  plus un rescan (409) ni le statut du scan. Résultats trickplay distants acceptés.
  Images de chapitres préparées après scan si `chapterImagesDuringScan`.
  `introDetectionEnabled` respecté ; le premier épisode d'une saison est réanalysé.
- Migrations : FK suspendues autour de l'init SQLite ; marqueurs `recap`/`preview`
  autorisés (SQLite reconstruit, PostgreSQL ALTER idempotent, non testé sur PG réel).
- Droits : types de médias refusés par utilisateur/groupe (`media_kinds.sql`) intégrés
  à la vue `effective_library_permissions` ; paliers d'âge, classifications normalisées
  (FR:-16, FSK, TV-MA…) et héritées de la série ; SyncPlay vérifie chaque membre.
  Non fait : contrôle parental par groupe.
- Web : `fitArtwork` publie le ratio réel (`--artwork-ratio`), plus de fond flouté ;
  aperçu trickplay mis à l'échelle sans rognage ; chapitres WebP ; bouton « Passer »
  hors du dock masqué ; saut auto du récap ; sous-titre en échec non bloquant ;
  reprise conservée au basculement.
- Vérifié : tests Rust ciblés (unitaires, scanner, séries, sauvegarde, permissions,
  SyncPlay, automation, tâches), suite complète avant derniers correctifs ; Web 345
  tests, check, lint, build ; clippy core/node. Aucun commit ni navigateur réel.

## 5 novembre 2026 — Dossiers, extensions, groupes et réglages

- Nouveau dossier de bibliothèque : le sélecteur ouvre la racine du Node (`/`
  sous Linux), sans proposer sa racine interne temporaire comme chemin média.
- Extensions : fin de l'installation automatique TMDb/IMDb ; actualisation des
  dépôts à l'ouverture, choix main/develop conservé après redémarrage. Voir
  `docs/PLUGIN_UPDATE_LIFECYCLE.md` ; installations existantes conservées.
- Galerie native initiale : 12 avatars locaux « Orbites », supprimables sans
  réapparition ; curseur d'attente réservé aux traitements réels. Groupes système
  Utilisateurs/Administrateurs déterminés par le rôle, comptes existants inclus,
  sans modifier les groupes personnalisés. Voir `docs/ADMIN_ACCESS.md`.
- Menus simples habillés en Halo avec repli natif ; paramètres numériques des
  extensions compatibles avec les décimales (Intro Skipper 0,75). Voir
  `docs/UI_DIRECTION.md` et `docs/PLUGIN_SETTINGS_AND_COLLECTIONS.md`.
- Contrôles locaux : check Svelte 0/0, lint, build Web, 18 tests Web ciblés ;
  Chrome avec APIs simulées à 390/820/1440 px pour menus/décimales et 390/1440
  pour dossiers, avatars et dépôts. Core groupes 5/5, extensions 3/3, galerie
  native et collections 3/3 ; contrôle SQL PostgreSQL 18
  sur base jetable ; fmt et Clippy Core lib/bin sans avertissement. Aucun
  déploiement, média personnel ou fournisseur réel validé.

## 5 novembre 2026 — Personnes et portraits

- Distribution et équipe : liens vers une page personne paginée avec films et
  séries accessibles, filtrés par nom. Les homonymes peuvent être regroupés.
  Portraits d'équipe fournis par TMDb 0.1.4 et servis par le proxy Core autorisé ;
  les anciennes métadonnées exigent une actualisation. Voir `docs/CATALOGUE_PRESENTATION.md`.
- Tests locaux : crédits Web 5/5, parcours navigateur 390/1440 sur API simulée,
  filtre Core avec parental/pagination 1/1 et portraits 5/5. Le fournisseur
  externe réel et un déploiement NAS n'ont pas été vérifiés.

## 5 novembre 2026 — Avatars, recherche Demandes et images des fiches

- Import GitHub retiré de l'UI et des routes natives/extensions ; uploads locaux
  et collections existantes conservés. Contrat : `docs/PLUGIN_SETTINGS_AND_COLLECTIONS.md`.
- Recherche Demandes : validation native à la place d'un bouton désactivé dès
  l'absence de saisie ; curseur d'attente réservé à la recherche en cours.
- Fiches : cadrage plus conservateur, illustration sans superposition sur un fond
  de scène, logos adaptés et sélection linguistique. Limites : `docs/UI_DIRECTION.md`.
- Vérifié localement : check Svelte 0/0, lint, build Web, 21 tests Web ciblés,
  2 tests Rust collections, Chrome sur fixtures à 390/1920 px (recherche, galerie,
  trois ratios). Aucun déploiement ni recherche ARR réelle effectués.

## 5 novembre 2026 — Extensions et préférences d'affiches

- Cartes d'extensions ouvrables en entier ; épinglage sur la fiche, versions du
  catalogue et instantané de restauration réunis dans la liste. Rétrogradation
  explicite bornée, paquet signé, sans données privées ni changement de schéma ;
  un instantané existant est préservé. Voir `docs/PLUGIN_UPDATE_LIFECYCLE.md`.
- Affiches : deux choix personnels, rotation active par défaut et langue de
  l'interface ou originale ; TMDb transmet désormais `originalLanguage`, repli
  interface si absent. Publication du plugin et enrichissement des œuvres
  existantes non effectués. Voir `docs/ARTWORK_ROTATION.md`.
- Pages utilisateurs et groupes allégées ; filtres compacts. Validation ciblée
  Web, Core et plugin sur fixtures ; aucun déploiement ni fournisseur réel validé.

## 5 novembre 2026 — Connexion QR, accueil et sauvegardes

- Connexion d'un appareil par QR ou code, avec confirmation explicite par une
  session locale ou SSO ; secrets distincts, expiration et consommation unique.
  Contrat et limites : `docs/LOCAL_SESSIONS.md` et `docs/API.md`.
- Accueil : recommandations actives par défaut, après Pour ce soir ; choix de
  désactivation existant conservé. Sélecteur d'accent et lien Demandes du header
  retirés. Sessions redessinées ; voir `docs/UI_DIRECTION.md`.
- NAS : volume de sauvegardes privé de nouveau possédé par 10001:10001 alors que
  Core tourne sous 1039:65539. Propriétaire du seul dossier corrigé, lecture et
  sonde privée réussies. Cause de récidive non établie ; risque de copie Docker
  sur volume vide et prévention documentés dans `docs/ADMIN_BACKUPS.md`.
- Contrôles ciblés : tests QR et recommandations SQLite, frontend et Chrome avec
  APIs synthétiques ; décodage QR généré avec ZXing. Aucun fournisseur SSO réel,
  appareil physique, PostgreSQL ou archive NAS complète validé ; code non déployé.

## 5 novembre 2026 — Administration, profil et sauvegardes

- Administration pleine largeur ; rubrique Extensions, épingles Demandes/Signalements
  locales au navigateur et par compte ; Clés API dans Système. Liens personnels
  Demandes/Signalements/statistiques déplacés dans Moi des paramètres. Voir
  `docs/UI_DIRECTION.md`.
- NAS : volume `/backups` recréé sous 10001:10001, Core sous 1039:65539, mode0700.
  Propriétaire du seul dossier corrigé sans récursion ; lecture/écriture/relecture
  d’une sonde privée réussies, aucune archive ni média touché, sans redémarrage.
  Diagnostic API local amélioré ; prévention : `docs/ADMIN_BACKUPS.md`.
- Vérifié : check Svelte 0/0, lint, 11 tests Web ciblés, build Web ; Chrome avec APIs
  synthétiques à 2560/390 px et épinglage après rechargement ; test Rust admin_backups
  2/2 et fmt. Scénario permissions Unix exclu sous Windows. Aucun test HTTP authentifié
  NAS ni sauvegarde/restauration complète de production, aucun déploiement de code.

## 4 novembre 2026 — Portraits « Distribution & équipe »

- Cause : les crédits TMDb contenaient `thumbUrl` (`image.tmdb.org`), mais l'interface le
  chargeait directement et la CSP `img-src 'self'` le bloquait ; les NFO ignoraient `<thumb>`.
- Décision : route Core `/api/v1/works/{work}/people/{key}/image` (clé = SHA-256 tronqué de
  l'URL), téléchargement paresseux via les hôtes d'une extension autorisée, JPEG ≤ 400×600 en
  cache d'assets (`person_images`). Détails : `docs/CATALOGUE_PRESENTATION.md`, `docs/API.md`.
- Limites : rendu visuel non vérifié dans un navigateur réel ; dossiers `.actors/` non lus.

## 4 novembre 2026 — Fiche « Informations média » par fichier

- Retrait du panneau « Versions et informations techniques » de `FilmDetail.svelte` ; le
  choix de version reste dans le Hero (`aria-label` « Sélectionner la version … »).
- `GET /api/v1/assets/{id}/details` (`crates/core/src/inventory/details.rs`) : Node, bibliothèque,
  pistes, conteneur ; chemins relatif/absolu côté Node **administrator uniquement**.
  La modale `WorkTools.svelte` regroupe par fichier (version ou copie multi-Node).
- Vérifié : `cargo test -p media-poc --test asset_details`, tests unitaires, check/lint/test Web,
  captures Playwright sur fixtures synthétiques (375/768/1440/1920). Scripts `browser-*.cjs`
  adaptés mais non relancés intégralement.

## 4 novembre 2026 — Assainissement du dépôt et de `crates/core`

Branche `chore/core-restructure`, partie de `feat/requests-and-reports` (WIP commité d'abord).
- Git : `crates/node/data`, `crates/core/data`, ainsi que les PNG/JSONL de
  `docs/evidence/2026-10-15`, sont ignorés et retirés de l'index (copies locales gardées,
  historique non réécrit). Les fichiers de suivi IA passent dans `.ai/`.
- SQL : `schema.sql` et les `*_schema.sql` sont dans `crates/core/migrations/` (pas des
  migrations sqlx : schéma courant et scripts idempotents de `initialize_fresh_schema`).
- Extensions : plus aucun `extension_*.rs` à la racine de `src/`. Les modules publics sont
  devenus `extensions::{runtime,storage,data,objects,files,data_lifecycle}`.
- Tests unitaires : convention `module/tests.rs`, plus de `#[path]` (ils dépendent d'éléments
  privés, donc pas de déplacement vers `tests/`). Filtre CI : `product::tests::…`.
- Expériences lecteur (mse, shaka, bench, audio-seek-policy) : dans `crates/core/experiments/`,
  toujours servies et testées. La page OIDC est dans `src/oidc/complete.html`.
- Binaire unique `src/bin/aether-core.rs` (`default-run`) ; `src/main.rs` supprimé. Les fixtures
  ne sont chargées que si `MEDIA_FIXTURE_DIR` est défini.
- Vérifié : `cargo check` et `cargo clippy --all-targets --all-features` passent
  (avertissements clippy seulement dans le WIP acquisition/reports), `cargo test --test p0
  --features fixture-bench` 11/11, `node --test scripts/audio-seek-policy.test.cjs` 10/10.
  `scripts/test-fresh-schema.py` échoue (6 planifications pour 5 attendues), comme avant le
  déplacement ; le WIP en est la cause, pas le déplacement.
- Non vérifié : suite `cargo test` complète (disque C: plein, le linker échoue avec LNK1318),
  build Docker, tests navigateur `--ignored`, PostgreSQL.

## 4 novembre 2026 — Correctifs visuels de la refonte

- Recherche : la règle globale des champs (spécificité 0,3,1) écrasait les styles
  des champs avec loupe ; passée sous `:where()` dans `tokens.css`.
- Fiches : vu (coche seule), favori (cœur) et note passent dans la rangée du
  bouton Lecture ; l'ancien panneau « Mon suivi » est retiré.
- Images : dimensions mesurées sur l'original, plus sur la vignette `?width=320` ;
  `lib/ui/fit-artwork.ts` évite les rognages forts (image entière + fond flouté),
  affiche floutée en repli sans image large, bannières 16:9 sur téléphone.
- Lecteur : timeline et commandes dans un dock vitré. Accueil et fiches : fil
  sans fin (`DiscoverFeed.svelte`, `discover.ts`), 24 similaires au lieu de 16.
- Vérifié : `npm run check`, `npm run lint`, vitest 260/260, build, et
  `browser_series` + `browser_scanner` (`--ignored`, Chrome) passent. Fil sans
  fin non exercé sur un grand catalogue réel.
- Lecteur, second lot : zone déjà chargée sur la timeline, pointeur masqué avec
  les commandes (micro-mouvements < 4 px ignorés), carte vitrée « À suivre »
  (vignette, saison/épisode, Lire maintenant, masquer) au début du générique ou
  30 s avant la fin d'un épisode de plus de 5 min ; elle remplace « Passer le
  générique ». Distinct du bouton permanent retiré le 22/10. `browser_series`
  vérifie carte et zone chargée via un marqueur injecté côté test ; passe.
- CI `browser_series` (commit dccdc67) : la grille du fil d'accueil réaffichait
  une œuvre déjà visible, d'où deux cartes pour un locator strict. Grille
  dédupliquée contre toute la page (renommée « Encore plus à découvrir ») ;
  le scénario vérifie l'absence de doublon après chargement complet du fil. Passe en local.

## 3 novembre 2026 — Documentation UX

- Les rapports UX datés sont conservés comme preuves historiques, sans feuille de
  route implicite. `docs/UI_DIRECTION.md` porte les règles durables et renvoie
  vers les jalons détaillés.
- Les sections « prochaine priorité » des rapports fiches, administration,
  bibliothèques, tâches et musique ont été neutralisées lorsqu’elles étaient
  dépassées. Aucun rapport ni historique n’a été supprimé.
- Les cinq avertissements CSS de `AppearanceCustomization.svelte` provenaient de
  sélecteurs `input` sans élément `input` dans le composant ; ces sélecteurs morts
  ont été retirés. `npm run check` et `npm run lint` passent sans erreur ni
  avertissement.

## 3 novembre 2026 — Maintenance ciblée

- Mémoire précédente archivée à l’identique ; point d’entrée réduit. Les anciennes
  décisions restent historiques et les règles de sécurité sont conservées.
- Caches Python suivis et journal de build déjà supprimés en grande partie avant
  ce lot ; dernier cache supprimé, règles d’exclusion préexistantes vérifiées.
- Chemins de configuration, lien local et commentaire Docker corrigés. Vérifié :
  `docker compose config --quiet`, `npm run lint`, `npm run check` (0 erreur,
  5 avertissements CSS), contrôle de liens local ciblé et `git diff --check`.
  Aucun build d’image, déploiement ni essai de sauvegarde réelle.
- Les consignes P0/P1 et des sections historiques d’architecture restent à
  réconcilier avec l’évolution du produit avant toute modification de priorité.

## 3 novembre 2026 — Mission fonctionnalités : validations locales ciblées

OpenSubtitles manuel, TVDB optionnel, images de vrais chapitres à la demande,
recommandations locales et socle de notifications sont raccordés. Preuves et
limites : docs/archive/missions/MISSION_FEATURES_2026_11_03.md, docs/archive/missions/MISSION_CORE_VALIDATION_2026_11_03.md
et docs/archive/missions/MISSION_WEB_VALIDATION_2026_11_03.md. Seize tests Core ciblés et deux
contrats PostgreSQL ont réussi, ainsi que le lecteur Chrome avec dérivé après
redémarrage. Deux vrais Core/PostgreSQL : panne d'une écriture Wasm bloquée,
zéro effet de la tentative tuée puis un effet après relance explicite vérifiés.
Multi-Core toujours expérimental ; routage Node et fencing global ouverts.
Fournisseurs réels, matériel ARM et NAS non attestés. Builds Docker suivis dans
docs/archive/ci/DOCKER_MULTIARCH_2026_11_03.md. Aucun push, publication ni déploiement ici.
La refonte visuelle concurrente est préservée et n'appartient pas à ce lot.
- Halo étendu à tout le reste du Web (4 novembre) : sections internes de l'administration (utilisateurs, groupes, contrôle parental, clés API, OIDC, Media Nodes, extensions, tâches, sauvegardes, vue d'ensemble, bibliothèques, sélecteur de dossiers, options, images, fournisseurs, avatars, paramètres serveur), outils et fenêtres des fiches (outils, identification, images, sous-titres, téléchargement, statut vu, note, distribution, chapitres, épisodes, playlists), menu profil, connexion, listes, téléchargements, réglages du compte, squelettes, Watch Party (fenêtre et panneau sur la vidéo). Styles réécrits avec les jetons ; balisage modifié seulement pour des classes, des icônes à la place d'emoji décoratifs et le libellé « Gestion du média · admin ». Check, lint, 246 tests et build réussis ; aucun essai visuel en navigateur.

## 4 novembre 2026 — Notifications contenu et budget TVDB

Notifications nouveau contenu avec opt-in, droits et parental : six tests locaux et un contrat PostgreSQL 18 passent. Le vrai module TVDB optimisé passe recherche/détails/images sous limites Wasmi Core sur fixtures (1/1, 9,98 s). Preuves : docs/archive/missions/MISSION_CORE_VALIDATION_2026_11_03.md et Aether-Plugins/docs/TVDB_2026_11_03.md. Aucun service externe validé ni publié ; trois images AMD64 smoke réussis, reprise ARM64 sous émulation en cours. Travail visuel concurrent conservé, multi-Core expérimental.

- Compléments Halo (4 novembre) : accueil avec sélecteur d'œuvre à l'affiche (3 vignettes 16:9), teinte d'ambiance calculée côté client depuis l'image (canvas 16×9, sans teinte si échec) et mosaïque « Pour ce soir » (affichée après « Reprendre » ; pas une section enregistrée, car `crates/core/src/preferences.rs` n'accepte que 5 sections connues — à étendre si on veut la rendre masquable). Administration : recherche de réglage dans la barre latérale (`searchAdmin`, testée) et bibliothèques en tableau. Lecteur : réglages en panneau latéral à onglets sur ordinateur. Musique : file d'attente et paroles en panneau latéral. CSP : les attributs `style` en ligne sont bloqués ; les couleurs du dégradé du logo passent par le CSS des composants. Check, lint, 257 tests et build réussis ; aucun essai visuel.
- Correction (même jour) : `tonight` est maintenant une section d'accueil enregistrable et masquable. Le Core l'accepte (`crates/core/src/preferences.rs`, 6 sections au plus), test `crates/core/tests/preferences.rs` étendu et réussi, `docs/PREFERENCES.md` à jour. Les listes déjà enregistrées ne l'affichent qu'après réactivation dans Compte › Apparence et accueil.
Chapitres : HEAD inverse corrigé, scénario TCP direct/inverse sans repli réussi (13,66 s), source intacte ; voir docs/CHAPTER_IMAGES.md. ARM64 non validé : deuxième interruption et moteur Docker absent ; conserver les journaux, ne pas confondre avec succès AMD64. Les événements Node/fournisseur restent non implémentés.

## 4 novembre 2026 — Modularisation du backend Rust

- `inventory.rs`, `nodes.rs` et `server.rs` remplacés par des modules privés par
  responsabilité et des façades compatibles. Les 200 éléments Rust et 82 noms
  publics/internes sont conservés, avec revue des imports/visibilités. Transactions,
  gardes, signatures et correctif HEAD préexistant préservés ; aucune logique
  métier réécrite, aucune nouvelle crate ni état global.
- Fmt global, Clippy locked/workspace/all-targets/all-features avec `-D warnings`
  et tests ciblés passent : 13 unitaires Core, 12 Node ; intégrations après lots
  inventaire/Nodes/serveur : 18/6/7. TLS réel reste ignoré faute de certificats.
  Mesures catalogue ponctuelles sans gain revendiqué. Audit, organisation,
  commandes, problèmes reportés et limites : [modules backend](../docs/BACKEND_MODULES.md).
  Aucun push, publication, déploiement ou validation NAS/PostgreSQL/Linux ici.

## 4 novembre 2026 — Optimisation CI, branche perf/ci-build-once

Audit develop `743cc61` et trois runs réussis : 20 min 44 s à 21 min 57 s, 72–80 minutes cumulées de runners. Version calculée avant build ; images natives smoke-testées transférées en archives SHA-256/IDs/labels, puis promotion sans recompiler après toutes les gates. Release main appelle la CI complète ; tag bêta après publication. Nettoyage disque conditionnel, poids des 62 cibles actualisés ; profils production conservés, COPY Docker ciblés et CLI packaging non livré retiré du build. Contrats locaux release/archives/manifests/partitions réussis ; CI distante et transfert réel à confirmer avant conclusion. Détails et preuves : [optimisation CI](../docs/CI_PERFORMANCE_2026_11_04.md). Aucune fusion, publication expérimentale ni protection de branche modifiée.

Essai de promotion sur registre local jetable réussi avec six images synthétiques et trois index ; compatibilité des IDs Docker classique/containerd vérifiée. Ce test n'est pas une preuve d'exécution Aether ARM64. PR [#36](https://github.com/maelmoreau21/Aether/pull/36) en brouillon ; CI distante en cours, aucune publication GHCR déclenchée.

Premier run PR : contrôles Rust/PG 462 s (655 s avant), frontend/navigateur/sécurité et ARM64 réussis ; archive réelle ARM64 145 Mo, save/gzip/contrôle/load 7 s + upload 2 s. Première répartition insuffisante (scanner 226 s contre 104 s) : poids finaux = maxima de deux runs, 461 s estimées par partition. Smoke AMD64 bloqué comme dans un run antérieur : reprise sur révision suivante, réutilisation FFmpeg hôte si disponible et limites explicites de job/step/exec, sans masquer d'erreur. Premier run non considéré comme succès complet ; validation de la révision finale requise.

Diagnostic du blocage AMD64 confirmé après annulation : apt téléchargeait 81 paquets / 66,4 Mo, le smoke n'avait pas démarré. Remplacement final par le cache de fixtures Rust en lecture seule, contrôle réel AV1 dans les images avant extraction ; générateur FFmpeg épinglé seulement sur miss. Contrats locaux renforcés ; nouvelle CI complète requise. Travail poursuivi en worktree isolé pour préserver des changements visuels concurrents.

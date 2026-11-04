# Mémoire Aether — point d’entrée

Le code et les tests actuels font foi. Cette mémoire est un index de décisions et
preuves historiques, pas une garantie de comportement ni de déploiement actuel.
Consulter les documents du domaine et les sources avant toute modification.

- Architecture et frontières : [ARCHITECTURE.md](ARCHITECTURE.md).
- Sécurité : [docs/SECURITY.md](docs/SECURITY.md).
- Lecture et résultats observés : [docs/STREAMING_COMPATIBILITY.md](docs/STREAMING_COMPATIBILITY.md).
- Déploiement : [docs/DEPLOYMENT_DOCKER.md](docs/DEPLOYMENT_DOCKER.md).
- Extensions : [docs/PLUGIN_UPDATE_LIFECYCLE.md](docs/PLUGIN_UPDATE_LIFECYCLE.md).
- Historique complet jusqu’au 3 novembre 2026, y compris les travaux locaux :
  [archive de la mémoire](docs/archive/MEMORY_2026-11-03.md). Les entrées y
  sont datées mais pas strictement chronologiques ; rechercher par sujet et date.

Les futures entrées restent courtes : périmètre, décision, vérifications réellement
exécutées et limites ; les détails propres à un domaine vont dans son document.
Archiver périodiquement les entrées anciennes sans les modifier ni perdre leur contexte.

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
limites : docs/MISSION_FEATURES_2026_11_03.md, docs/MISSION_CORE_VALIDATION_2026_11_03.md
et docs/MISSION_WEB_VALIDATION_2026_11_03.md. Seize tests Core ciblés et deux
contrats PostgreSQL ont réussi, ainsi que le lecteur Chrome avec dérivé après
redémarrage. Deux vrais Core/PostgreSQL : panne d'une écriture Wasm bloquée,
zéro effet de la tentative tuée puis un effet après relance explicite vérifiés.
Multi-Core toujours expérimental ; routage Node et fencing global ouverts.
Fournisseurs réels, matériel ARM et NAS non attestés. Builds Docker suivis dans
docs/DOCKER_MULTIARCH_2026_11_03.md. Aucun push, publication ni déploiement ici.
La refonte visuelle concurrente est préservée et n'appartient pas à ce lot.
- Halo étendu à tout le reste du Web (4 novembre) : sections internes de l'administration (utilisateurs, groupes, contrôle parental, clés API, OIDC, Media Nodes, extensions, tâches, sauvegardes, vue d'ensemble, bibliothèques, sélecteur de dossiers, options, images, fournisseurs, avatars, paramètres serveur), outils et fenêtres des fiches (outils, identification, images, sous-titres, téléchargement, statut vu, note, distribution, chapitres, épisodes, playlists), menu profil, connexion, listes, téléchargements, réglages du compte, squelettes, Watch Party (fenêtre et panneau sur la vidéo). Styles réécrits avec les jetons ; balisage modifié seulement pour des classes, des icônes à la place d'emoji décoratifs et le libellé « Gestion du média · admin ». Check, lint, 246 tests et build réussis ; aucun essai visuel en navigateur.

## 4 novembre 2026 — Notifications contenu et budget TVDB

Notifications nouveau contenu avec opt-in, droits et parental : six tests locaux et un contrat PostgreSQL 18 passent. Le vrai module TVDB optimisé passe recherche/détails/images sous limites Wasmi Core sur fixtures (1/1, 9,98 s). Preuves : docs/MISSION_CORE_VALIDATION_2026_11_03.md et Aether-Plugins/docs/TVDB_2026_11_03.md. Aucun service externe validé ni publié ; trois images AMD64 smoke réussis, reprise ARM64 sous émulation en cours. Travail visuel concurrent conservé, multi-Core expérimental.

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
  commandes, problèmes reportés et limites : [modules backend](docs/BACKEND_MODULES.md).
  Aucun push, publication, déploiement ou validation NAS/PostgreSQL/Linux ici.

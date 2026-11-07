# Index de la documentation Aether

Point d'entrée thématique de `docs/`. Rappel : **les documents sont des exigences ou des comptes rendus, pas des preuves d'implémentation** ; seuls le code et les tests effectivement exécutés font foi (voir [`../AGENTS.md`](../AGENTS.md), sections 2 et 11).

## Sources de vérité (chemins à ne pas déplacer)

| Document | Rôle |
| --- | --- |
| [`../AGENTS.md`](../AGENTS.md) | Règles de contribution et ordre des priorités |
| [`../ARCHITECTURE.md`](../ARCHITECTURE.md) | Composants et frontières |
| [`../.ai/MEMORY.md`](../.ai/MEMORY.md) | État courant et décisions (anciennes entrées : [`archive/MEMORY_2026-11-03.md`](archive/MEMORY_2026-11-03.md)) |
| [`SPECIFICATION.md`](SPECIFICATION.md) | Exigences et périmètre |
| [`MEDIA_FORMATS.md`](MEDIA_FORMATS.md) | Modèle des pistes et formats |
| [`STREAMING_COMPATIBILITY.md`](STREAMING_COMPATIBILITY.md) | Preuves de lecture observées |
| [`SECURITY.md`](SECURITY.md) | Modèle de menaces et authentification |
| [`API.md`](API.md) | Contrats d'API provisoires |
| [`TEST_PLAN.md`](TEST_PLAN.md) | Vérifications et jeux d'essai |
| [`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md) | Phases et critères de passage |

## Démarrer

- [`../README.md`](../README.md) : présentation, installation rapide, fonctionnalités.
- [`INSTALLATION_LOCALE.md`](INSTALLATION_LOCALE.md) : installer Aether sur un PC Windows.
- [`LOCAL_ADMIN.md`](LOCAL_ADMIN.md) : SQLite et premier administrateur local.
- [`../.ai/DEV_STATE.md`](../.ai/DEV_STATE.md) : journal d'état de développement.
- [`../.ai/ORIGINAL_REQUEST.md`](../.ai/ORIGINAL_REQUEST.md) et [`../.ai/PROMPT_DEMARRAGE_IA.md`](../.ai/PROMPT_DEMARRAGE_IA.md) : demande initiale et premier développement confié à l'IA.
- [`PRODUCT_FEATURES.md`](PRODUCT_FEATURES.md) et [`PRODUCT_BACKLOG.md`](PRODUCT_BACKLOG.md) : fonctions produit et demandes suivies.
- [`JELLYFIN_FUNCTIONAL_GAPS.md`](JELLYFIN_FUNCTIONAL_GAPS.md) : comparaison fonctionnelle Jellyfin / Aether.
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) et [`../CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md) : contribution et conduite.

## Architecture

- [`../ARCHITECTURE.md`](../ARCHITECTURE.md) : vue d'ensemble Core / Node / Web.
- [`ARCHITECTURE_ROADMAP.md`](ARCHITECTURE_ROADMAP.md) : audit Rust et feuille de route.
- [`BACKEND_MODULES.md`](BACKEND_MODULES.md) : modules internes du backend.
- [`DISTRIBUTED_TASKS.md`](DISTRIBUTED_TASKS.md), [`SCHEDULED_TASKS.md`](SCHEDULED_TASKS.md), [`AUTOMATION.md`](AUTOMATION.md) : moteur de tâches, planification, automatisation événementielle.
- [`MULTI_NODE_RELIABILITY.md`](MULTI_NODE_RELIABILITY.md) : un Core, plusieurs Media Nodes — audit, correctifs, mesures avant/après, limites.
- [`MULTI_CORE.md`](MULTI_CORE.md), [`MULTI_CORE_READINESS.md`](MULTI_CORE_READINESS.md) et [`MULTI_CORE_ROADMAP.md`](MULTI_CORE_ROADMAP.md) : préparation de plusieurs Core (expérimental) et feuille de route.
- [`POSTGRESQL.md`](POSTGRESQL.md), [`SQL_PERFORMANCE.md`](SQL_PERFORMANCE.md) : PostgreSQL facultatif, performance SQL du catalogue.
- [`API.md`](API.md) et [`extensions-v1.openapi.json`](extensions-v1.openapi.json) : contrats d'API.

## Média, bibliothèques et streaming

- Formats et lecture : [`MEDIA_FORMATS.md`](MEDIA_FORMATS.md), [`STREAMING_COMPATIBILITY.md`](STREAMING_COMPATIBILITY.md), [`STREAMING_PERFORMANCE.md`](STREAMING_PERFORMANCE.md), [`STREAMING_SYSTEM_BENCHMARK.md`](STREAMING_SYSTEM_BENCHMARK.md), [`NAS_PLAYBACK_DIAGNOSTICS.md`](NAS_PLAYBACK_DIAGNOSTICS.md), [`NEXT_MEDIA_TESTS.md`](NEXT_MEDIA_TESTS.md).
- Pistes et sources séparées : [`SEPARATED_MEDIA.md`](SEPARATED_MEDIA.md), [`PLAYER_TRACK_PREPARATION.md`](PLAYER_TRACK_PREPARATION.md).
- Sous-titres, images, vignettes : [`SUBTITLE_ASS_RENDERING.md`](SUBTITLE_ASS_RENDERING.md), [`OPENSUBTITLES.md`](OPENSUBTITLES.md), [`CHAPTER_IMAGES.md`](CHAPTER_IMAGES.md), [`TRICKPLAY.md`](TRICKPLAY.md), [`ARTWORK_ROTATION.md`](ARTWORK_ROTATION.md), [`INTRO_SKIPPER.md`](INTRO_SKIPPER.md).
- Bibliothèques et scanner : [`LIBRARIES_AND_SCANNER.md`](LIBRARIES_AND_SCANNER.md), [`LIBRARY_FOLDERS.md`](LIBRARY_FOLDERS.md), [`LIBRARY_OPTIONS.md`](LIBRARY_OPTIONS.md), [`LIBRARY_SCANNING_PLAN.md`](LIBRARY_SCANNING_PLAN.md).
- Films locaux : [`LOCAL_FILM_SCANNER.md`](LOCAL_FILM_SCANNER.md), [`LOCAL_FILM_NFO.md`](LOCAL_FILM_NFO.md), [`LOCAL_FILM_POSTERS.md`](LOCAL_FILM_POSTERS.md).
- Catalogue, musique et usages : [`CATALOGUE_PRESENTATION.md`](CATALOGUE_PRESENTATION.md), [`PLAYLISTS_AND_MUSIC.md`](PLAYLISTS_AND_MUSIC.md), [`RECOMMENDATIONS.md`](RECOMMENDATIONS.md), [`NOTIFICATIONS.md`](NOTIFICATIONS.md), [`SYNCPLAY.md`](SYNCPLAY.md).

## Sécurité et comptes

- [`SECURITY.md`](SECURITY.md) : modèle de menaces et règles d'authentification (la politique de signalement est dans [`../SECURITY.md`](../SECURITY.md)).
- [`OIDC.md`](OIDC.md) : SSO OpenID Connect.
- [`LOCAL_SESSIONS.md`](LOCAL_SESSIONS.md) : sessions locales et jalon OIDC.
- [`ADMIN_ACCESS.md`](ADMIN_ACCESS.md), [`ADMIN_USERS.md`](ADMIN_USERS.md) : groupes, clés API, comptes.

## Administration et déploiement

- Console : [`ADMIN_OVERVIEW.md`](ADMIN_OVERVIEW.md), [`ADMIN_CONSOLE.md`](ADMIN_CONSOLE.md), [`ADMIN_DEPLOYMENT.md`](ADMIN_DEPLOYMENT.md).
- Sauvegardes : [`ADMIN_BACKUPS.md`](ADMIN_BACKUPS.md) (planification), [`BACKUP_BUNDLE.md`](BACKUP_BUNDLE.md) (format du paquet), [`BACKUP_RESTORE.md`](BACKUP_RESTORE.md) (restauration), [`BACKUP_RECOVERY.md`](BACKUP_RECOVERY.md) (récupération).
- Déploiement : [`DEPLOYMENT.md`](DEPLOYMENT.md) (historique, renvoie vers Docker), [`DEPLOYMENT_DOCKER.md`](DEPLOYMENT_DOCKER.md), [`DEPLOYMENT_SYNOLOGY.md`](DEPLOYMENT_SYNOLOGY.md).
- Publication : [`CI_GHCR.md`](CI_GHCR.md), [`RELEASES_AND_VERSIONING.md`](RELEASES_AND_VERSIONING.md), [`GITHUB_GOVERNANCE_AND_MIGRATION.md`](GITHUB_GOVERNANCE_AND_MIGRATION.md).
- Réglages : [`SETTINGS_AND_REFRESH.md`](SETTINGS_AND_REFRESH.md), [`SETTINGS_REVIEW.md`](SETTINGS_REVIEW.md), [`PREFERENCES.md`](PREFERENCES.md).
- Métadonnées traduites par compte : [`METADATA_LOCALIZATION.md`](METADATA_LOCALIZATION.md).
- Stockage en lecture seule, caches, NFO, identifiants et renommages : [`STORAGE_AND_CACHES.md`](STORAGE_AND_CACHES.md).
- Interface bilingue, langues séparées, formats et caches : [`I18N.md`](I18N.md).

## Extensions (plugins)

- [`EXTENSIONS.md`](EXTENSIONS.md) : décision et parcours vertical.
- [`EXTENSIONS_API.md`](EXTENSIONS_API.md) et [`extensions-v1.openapi.json`](extensions-v1.openapi.json) : API v1.
- [`EXTENSION_REPOSITORIES.md`](EXTENSION_REPOSITORIES.md), [`STARTUP_AND_PLUGIN_CATALOGUE.md`](STARTUP_AND_PLUGIN_CATALOGUE.md) : dépôts et catalogue.
- [`EXTENSION_STORAGE.md`](EXTENSION_STORAGE.md) et [`PLUGIN_STORAGE.md`](PLUGIN_STORAGE.md) : stockage des extensions (deux angles, à lire ensemble).
- [`PLUGIN_UPDATE_LIFECYCLE.md`](PLUGIN_UPDATE_LIFECYCLE.md), [`PLUGIN_SETTINGS_AND_COLLECTIONS.md`](PLUGIN_SETTINGS_AND_COLLECTIONS.md) : mises à jour, paramètres, collections.
- [`FUTURE_PLUGIN_ARCHITECTURE.md`](FUTURE_PLUGIN_ARCHITECTURE.md) : écosystème futur (prospectif).

## Player et UX

- [`UI_DIRECTION.md`](UI_DIRECTION.md) : système de design durable.
- [`IMMERSIVE_PLAYER.md`](IMMERSIVE_PLAYER.md) : lecteur immersif.
- [`NAVIGATION_PERFORMANCE.md`](NAVIGATION_PERFORMANCE.md), [`RESPONSIVENESS_AND_ARTWORK.md`](RESPONSIVENESS_AND_ARTWORK.md), [`WEB_PERFORMANCE.md`](WEB_PERFORMANCE.md) : réactivité et performances Web.
- [`PWA.md`](PWA.md) : PWA et hors ligne.

## Tests et CI

- [`TEST_PLAN.md`](TEST_PLAN.md) : plan de tests de référence.
- [`MANUAL_BROWSER_CHECKS.md`](MANUAL_BROWSER_CHECKS.md) : vérifications humaines restantes.
- [`CI_GHCR.md`](CI_GHCR.md) : CI et publication GHCR.
- `evidence/` : preuves datées (journaux, captures, rapports) ; **ne pas réorganiser**, des scripts et la CI y écrivent.

## Archive

Rapports datés, missions et expériences terminées, conservés pour l'historique (liens depuis les documents vivants mis à jour).

| Dossier | Contenu |
| --- | --- |
| [`archive/`](archive/) | Anciennes entrées de [`.ai/MEMORY.md`](archive/MEMORY_2026-11-03.md) |
| `archive/missions/` | Missions datées du 3 novembre 2026 (`MISSION_*`) et mission streaming 3.2 |
| `archive/audits/` | Audits ponctuels : couverture backlog, sortie P1, extensions, lecteur |
| `archive/ci/` | Bilans CI/Docker datés (lot A01, trois P0, multi-arch, résultats Docker P2) |
| `archive/fixes/` | Correctifs ponctuels (NAS, métadonnées, UI, sélection audio AV1/Opus) |
| `archive/ux/` | Jalons de refonte UX d'octobre 2026 (`UX_*`) |
| `archive/player-experiments/` | Expériences P0C (MSE, Shaka, marges audio) et rapports lecteur datés |
| `archive/milestones/` | Jalons P1 / P3-P4 et livrable de refonte du stockage |

## Conventions de documentation (CONTRIBUTING-docs)

- **Langue** : français, ton factuel ; distinguer ce qui est exécuté, non exécuté et non vérifié. Ne pas déduire une réalisation d'un plan.
- **Nommage** : `MAJUSCULES_AVEC_UNDERSCORES.md`, nom par domaine (`LIBRARY_OPTIONS.md`). Un document vivant n'a **pas** de date dans son nom ; sa date de dernière mise à jour figure dans le texte si utile.
- **Préfixe / suffixe de date** : un rapport lié à un jour précis porte la date en suffixe `_AAAA_MM_JJ` (ex. `MISSION_FEATURES_2026_11_03.md`). Les rapports datés vont directement dans `archive/<thème>/` une fois la mission terminée.
- **Quand archiver** : mission, audit, expérience, correctif ou jalon terminé et dont le contenu utile a été reporté dans un document vivant ; ou document remplacé par un autre. Utiliser `git mv` (historique conservé), jamais de suppression sans trace.
- **Après un déplacement** : corriger tous les liens entrants (`*.md`, scripts, `.github/`, code) et ajouter le document à l'index ci-dessus. Vérifier que les liens relatifs Markdown résolvent.
- **Sources de vérité** : les chemins listés plus haut sont référencés par `AGENTS.md` et ne se déplacent pas.
- **Preuves** : les journaux et captures vont dans `evidence/<date>/<sujet>/`, pas dans les documents ; un document cite la preuve, il ne la remplace pas.
- **Secrets** : aucune clé, jeton ni mot de passe réel dans la documentation.
- **Doublons** : privilégier un renvoi croisé à une fusion risquée ; en cas de contenu réellement redondant, fusionner dans le document vivant et archiver l'ancien avec renvoi.

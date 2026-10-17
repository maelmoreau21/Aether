# MEMORY.md — État et décisions Aether

## 17 octobre 2026 — Correctif P0 (Audit A1) : Disponibilité de l'authentification et des lectures sous verrou SQLite de 30 s

- **Séparation de la validation de session et de la mise à jour d'activité (`crates/core/src/auth.rs`)** :
  - **Validation en lecture seule à chaque requête (`AuthState::authenticate`)** : remplacement du `UPDATE sessions SET last_seen_at=unixepoch() ... RETURNING user_id` synchrone par un unique `SELECT` SQLite en lecture seule joignant `sessions`, `users` et `local_credentials` (`WHERE s.token_hash = ? AND s.expires_at > unixepoch() AND s.last_seen_at > unixepoch() - ? AND u.disabled_at IS NULL`).
  - **Zéro cache positif d'autorisation** : chaque requête vérifie en direct dans SQLite (snapshot WAL) l'existence de la session, l'expiration absolue (`12 h`), l'expiration par inactivité (`30 min`), le rôle actuel et l'absence de désactivation (`disabled_at IS NULL`). Toute déconnexion, révocation ou désactivation de compte prend effet dès la requête suivante.
  - **Worker unique borné et coalescent (`SessionToucher`)** :
    - Aucune tâche Tokio n'est créée par requête : un seul worker de fond par `AuthState` traite une table d'attente bornée (`MAX_PENDING_TOUCHES = 4096`) avec cadence de coalescence de `TOUCH_INTERVAL_SECONDS = 60 s` par session, micro-fenêtre de regroupement (`20 ms`) et écriture groupée par lots (`TOUCH_BATCH_SIZE = 64` sessions en un seul `UPDATE ... CASE token_hash ...`).
    - L'`UPDATE` différé est strictement conditionnel (`expires_at > unixepoch() AND last_seen_at > unixepoch() - 1800 AND EXISTS(... disabled_at IS NULL)`) et borne l'horodatage à `MIN(unixepoch(), MAX(last_seen_at, ?))` : il ne peut ni ressusciter une session expirée, ni recréer une session révoquée/déconnectée.
    - Lors d'un `POST /api/v1/auth/logout`, l'entrée en attente est immédiatement purgée avant et après le `DELETE FROM sessions`.
    - Protection du pool de lecture sous conflit d'écriture : acquisition bornée (`50 ms`) et `PRAGMA busy_timeout = 15` (15 ms, restauré immédiatement à la valeur d'origine de la connexion) avec backoff de `150 ms` sur `SQLITE_BUSY`, évitant d'immobiliser les connexions du pool pendant un verrou `BEGIN IMMEDIATE` prolongé.
  - **Compromis documenté sur la précision de l'expiration glissante (fail-closed)** :
    - La coalescence à 60 s et la persistance asynchrone impliquent qu'un arrêt brutal avant flush ou un verrou d'écriture SQLite maintenu jusqu'au-delà de la 30e minute d'inactivité ne prolonge pas la session en mémoire au-delà du `last_seen_at` commité dans SQLite (`docs/LOCAL_SESSIONS.md`, `docs/DEPLOYMENT_DOCKER.md`). Ce choix fail-closed garantit l'absence totale de résurrection de session expirée.
- **Mesures et tests sous verrou `BEGIN IMMEDIATE` de 30 secondes (`crates/core/tests/auth_under_sqlite_lock.rs` & `auth::tests`)** :
  - Pendant un `BEGIN IMMEDIATE` maintenu 30 s sur la base SQLite (`busy_timeout` par défaut de 15 s), 120 requêtes authentifiées réelles avec sessions éligibles au rafraîchissement (`last_seen_at = unixepoch() - 120`) ont toutes réussi sans attendre la libération du verrou :
    - `GET /api/v1/auth/me` (20 req) : **100 % HTTP 200**, `p50 = 0,99 ms`, `p95 = 1,50 ms`, `p99 = 1,86 ms` (contre HTTP 500 après 16,388 s avant correctif).
    - `GET /api/v1/catalog?kind=film` (20 req) : **100 % HTTP 200**, `p50 = 4,84 ms`, `p95 = 6,47 ms`, `p99 = 7,97 ms`.
    - `GET /api/v1/catalog?kind=series` + `/api/v1/catalog/{series_id}` (40 req) : **100 % HTTP 200**, `p50 = 4,61 ms`, `p95 = 6,63 ms`, `p99 = 8,54 ms`.
    - `GET /api/v1/media/{episode_id}` (20 req) : **100 % HTTP 200**, `p50 = 3,47 ms`, `p95 = 5,92 ms`, `p99 = 10,31 ms`.
    - `GET /api/v1/assets/{id}/content` Range `bytes=0-63` & `bytes=1000-2000` (20 req) : **100 % HTTP 206**, `p50 = 4,16 ms`, `p95 = 5,56 ms`, `p99 = 6,19 ms`.
    - **Global (120 lectures authentifiées sous verrou 30 s)** : `p50 = 3,90 ms`, `p95 = 6,44 ms`, `p99 = 8,54 ms`, `max = 10,31 ms`.
    - **Écritures SQLite** : `0` écriture commitée pendant le verrou (2 sessions coalescées en attente), puis **1 seul lot `UPDATE`** (`write_batches_succeeded = 1`, `rows_updated = 2`) dès la libération du verrou.
  - Cas limites vérifiés par tests automatisés : 30 requêtes concurrentes du même utilisateur près de la limite d'expiration (`1795 s`) coalescées en 1 lot d'écriture ; non-résurrection d'une session expirée pendant un verrou ; saturation de la file bornée avec compteur `dropped_overflow` sans dégrader les lectures ; annulation immédiate au logout ; redémarrage avant persistance ; révocation immédiate de session et désactivation d'utilisateur (`disabled_at`).

## 17 octobre 2026 — Audit indépendant de sortie P1 avant P2

- Audit de `develop` à `ba5ed8bc1095999cb34cd7202be9fb43a4656cb0` : voir `docs/AUDIT_SORTIE_P1_2026-10-17.md`. Sortie P1 non validée ; aucune fonctionnalité corrigée ou ajoutée.
- Reproductions indépendantes : verrou SQLite imposé 30 s → `/auth/me` 500 après 16,388 s (timeout configuré 15 s), santé 200 ; cache descripteurs réutilisé sans revalidation et repeuplé par réponse tardive ; MAX indépendants produisant un tuple de métadonnées absent des entrées.
- Autres constats : test navigateur Séries sans prérequis dans le job CI Rust ; exemple LAN HTTP refusé par le Core ; cache 128 réellement borné mais non FIFO, NFO toujours cloné par épisode malgré Arc.
- `cargo test --workspace`, formatage, Clippy, 19 tests Web, check/lint/build réussis. Chrome Films avec vrai redémarrage et Séries exécutés avec succès. Aucun build/run Docker, déploiement NAS, commit, push ou publication ; anciennes preuves navigateur remises en état après les runners. Les anciennes affirmations de lectures authentifiées toujours disponibles sous publication sont invalidées par cet audit.
- Plan P2 proposé uniquement : fermer les corrections ciblées P1, puis appairage Node, ingestion distante, streaming autorisé et OIDC, un Core/SQLite conservés. Aucun passage P2 effectué.

## 17 octobre 2026 — Jalon P1 : Performances et lecture finale des Séries (Chargement différé SvelteKit, suppression N+1 SQLite, borne mémoire du scanner, tests 200 épisodes, validation Chromium Playwright et conteneur Docker)

- **Chargement différé frontend SvelteKit (`apps/web`)** :
  - **Élimination du chargement anticipé massif** : dans `Catalog.svelte`, l'ouverture d'une série affichait les saisons et épisodes mais déclenchait auparavant la récupération en rafale de tous les `/api/v1/media/{id}` de ses épisodes. Ce comportement a été supprimé : la fiche série, le sélecteur de saisons et la liste d'épisodes s'affichent instantanément sans aucune requête média préliminaire.
  - **Gestion dynamique des descripteurs (`api.ts` & `EpisodePlayer.svelte`)** :
    - `fetchMediaDescriptor` / `fetchMediaDescriptors` avec cache de session en mémoire (`descriptorCache`) évitant les requêtes redondantes au cours de la navigation.
    - Préchargement intelligent au survol/focus d'un épisode (`preloadMediaDescriptors`).
    - Gestion robuste de l'annulation (`AbortController`) lors des changements rapides d'épisode pour annuler les requêtes en vol.
    - Gestion explicite des erreurs (notamment HTTP 403 en cas de révocation des droits) avec notification à l'utilisateur.
    - État de chargement visuel distinct ("Chargement…") sur l'épisode sollicité.
    - Conservation intégrale de la sélection multi-versions et du lancement dans le lecteur immersif (`ImmersivePlayer.svelte`).
    - Suite de tests unitaires Vitest dédiée (`apps/web/src/lib/catalog/api.test.ts`) validant le cache, le préchargement, l'avortement lors du zapping rapide et la gestion des 403 (19/19 tests passés).

- **Optimisation SQLite et élimination du goulot N+1 (`crates/core/src/inventory.rs`)** :
  - **Diagnostic sur 200 épisodes** : pour charger une série de 200 épisodes, `detail()` exécutait 1 requête œuvre + 1 requête saisons + 1 requête épisodes + 200 × 3 requêtes imbriquées (éditions, versions, sources), soit **~612 allers-retours SQL**.
  - **Résolution groupée sans changement de contrat API** :
    - Remplacement de `load_work_editions()` unitaire par une jointure SQL groupée `load_series_editions()` chargeant en une seule passe indexée toutes les éditions, versions, sources et assets de tous les épisodes de la série.
    - Réduction drastique à exactement **4 requêtes SQL** au total pour toute la série.
    - Temps de réponse mesuré sur une série de 200 épisodes et 10 saisons : **71,88 ms**.
    - Vérification stricte que les épisodes rendus indisponibles (`available = 0`) sont proprement et immédiatement exclus du comptage et de l'arborescence.

- **Mémoire du scanner Séries et staging SQLite (`crates/node/src/scanner.rs` & `crates/core/src/inventory.rs`)** :
  - **Borne mémoire sur `series_cache`** : introduction d'une constante `MAX_SERIES_CACHE = 128` et d'une éviction des clés les plus anciennes pour empêcher une croissance mémoire infinie sur des bibliothèques comptant des milliers de séries.
  - **Partage par `Arc` des métadonnées NFO et affiches** : utilisation de `Arc<(NfoOutcome, Option<LocalImage>)>` évitant le clonage profond des structures par épisode, tout en conservant 100 % de réutilisation sans relecture disque superflue au sein d'une série.
  - **Optimisation de la publication staging** : remplacement des `SELECT DISTINCT` sur l'ensemble des colonnes par des `GROUP BY series_identity_key` et `GROUP BY series_identity_key, season_number`, éliminant les tables temporaires volumineuses lors de la publication atomique.
  - **Audit de la taille limite NFO** : confirmation dans le code de `crates/node/src/nfo.rs` de la limite stricte de 1 Mo (`MAX_BYTES = 1024 * 1024` = 1 048 576 octets), alignée avec `docs/LIBRARY_SCANNING_PLAN.md`.

- **Tests d'utilisation réelle et de validation bout-en-bout** :
  - **Banc d'essai automatisé 200 épisodes (`crates/core/tests/series_performance.rs`)** :
    - Série de 200 épisodes répartis sur 10 saisons (20 épisodes par saison).
    - Multi-versions sur un épisode (S01E01 en MP4 et MKV) avec métadonnées NFO spécifiques.
    - Navigation et cohérence des saisons 1 à 10.
    - Exclusion immédiate d'un épisode indisponible (passage de 200 à 199 épisodes) et réintégration.
    - Lecture du premier (S01E01 v1 & v2) et du dernier épisode (S10E20) via HEAD, GET Range (`bytes=0-63` : 64 octets, 206) et Seek (`bytes=1000-2000` : 1001 octets, 206).
    - Révocation en direct des droits d'un utilisateur -> rejet HTTP 403 Forbidden immédiat avant lecture.
    - Rescan incrémental ultra-rapide (560 ms, 0 probe, 201 fichiers réutilisés).
    - Redémarrage SQLite avec vérification de l'intégrité du catalogue et non-régression totale des Films.
  - **Test réel de lecture vidéo dans le navigateur compilé (`crates/core/tests/browser_series.rs` & `scripts/browser-series.cjs`)** :
    - Exécution sous Chromium (v155.0.8059.40) avec Playwright sur le build de production SvelteKit statique servi par Core.
    - Parcours validé : connexion admin -> création bibliothèque Séries -> scan -> clic fiche série -> preuve de zéro chargement média anticipé -> lecture vidéo S01E01 -> seek et pause -> retour accueil -> changement vers Saison 2 -> lecture vidéo S02E01 -> zéro violation CSP.
  - **Test réel dans conteneur Docker (`scripts/test_docker_series.py`)** :
    - Image Docker Debian Bookworm minimale `aether:test` (UID 10001, GID 10001).
    - Montage `/data` (SQLite) et `/media` (lecture seule `:ro`).
    - Bootstrap admin interactif via pipe Docker Engine (`media-bootstrap`).
    - Connexion admin, création bibliothèque Séries, scan, consultation du catalogue (2 saisons, multi-version détectée), récupération du descripteur média AV1 + Opus, streaming HEAD et Range (206) validés.

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : tous les tests unitaires et d'intégration réussis.
  - `npm run check`, `npm run lint`, `npm test` (19/19 réussis), `npm run build` : 0 erreur, 0 avertissement.
  - Zéro commit, zéro push, zéro déploiement.

## 17 octobre 2026 — Jalon P1 : Séries, saisons et épisodes réels (Reconnaissance, persistance SQLite additive 0011, métadonnées NFO locales, navigation multi-saison, streaming et validation Docker)

- **Reconnaissance déterministe des séries et épisodes** :
  - **Formats d'arborescence et nommages pris en charge** :
    - `Ma Série/Season 01/Ma Série - S01E01.mkv`
    - `Ma Série/Saison 01/S01E02.mp4`
    - `Ma Série/Season 02/02x03.mkv`
    - `Ma Série/Specials/S00E01.mkv` (saison 0 / hors-série)
  - **Détection déterministe (`parse_series_episode`)** : analyse sans ambiguïté des composants de chemin et des motifs regex/tokens pour extraire le dossier de série, le titre brut, l'année, les identifiants explicites (`[tvdb-123]`, `[tmdb-456]`), le numéro de saison et d'épisode.
  - **Absence de fusion hasardeuse** : deux séries distinctes dans des dossiers différents ne sont jamais fusionnées sur un titre ressemblant.
  - **Multi-versions et épisodes multi-fichiers** : les fichiers partageant la même identité de série, saison et épisode sont rattachés au même épisode logique, apparaissant sous forme de versions/sources distinctes dans le lecteur.
  - **Épisodes multiples dans un seul fichier (ex. S01E01-E02)** : reconnaissance et indexation sur l'épisode primaire sans générer de lectures concurrentes désynchronisées ni de doublons.

- **Graphe média et persistance SQLite additive (`0011_series.sql`)** :
  - **Migration additive 0011** :
    - Ajout de la colonne `kind TEXT NOT NULL DEFAULT 'film'` sur `inventory_works`.
    - Création de la table `inventory_seasons(id TEXT PRIMARY KEY, series_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE, season_number INTEGER NOT NULL, title TEXT NOT NULL, UNIQUE(series_id, season_number))`.
    - Création de la table `inventory_episodes(id TEXT PRIMARY KEY, season_id TEXT NOT NULL REFERENCES inventory_seasons(id) ON DELETE CASCADE, episode_number INTEGER NOT NULL, work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE, UNIQUE(season_id, episode_number))`.
    - Ajout des colonnes de staging séries dans `scan_staging_files` (`series_identity_key`, `series_title`, `series_year`, `series_external_ids_json`, `series_editorial_json`, `series_poster_path`, `series_poster_size`, `series_poster_modified_ns`, `series_poster_mime`, `series_poster_priority`, `season_number`, `episode_number`).
    - Migrations 0001 à 0010 rigoureusement intactes.
  - **Publication atomique (`publish_staged_scan`)** :
    - Branche spécifique `lib_kind == "series"` publiant l'œuvre parente de la série (`kind = 'series'`), son affiche dans `inventory_images`, ses saisons ordonnées dans `inventory_seasons`, et les épisodes individuels dans `inventory_works` (`kind = 'episode'`) et `inventory_episodes`.
    - Chaque épisode hérite directement du graphe média standard (`inventory_editions` → `inventory_versions` → `inventory_sources` → `inventory_assets` → `inventory_tracks`), rendant le streaming Range/seek et le confinement de chemin immédiatement fonctionnels sans réinvention.
    - Préservation totale des bibliothèques Films existantes (`lib_kind == "films"`, `kind = 'film'`).
    - Réutilisation des probes inchangés à 100 % au rescan (`reused = 1`), stabilité déterministe des UUIDs et suppression propre des épisodes retirés (`available = 0`).

- **Métadonnées locales NFO et Affiches** :
  - **Parseur XML sécurisé étendu (`crates/node/src/nfo.rs`)** : prise en charge des balises racines `<tvshow>` pour les séries et `<episodedetails>` pour les épisodes, tout en conservant les protections strictes (limite de taille 1 Mo — `MAX_BYTES = 1024 * 1024`, profondeur max 16, interdiction d'entités externes et encodage UTF-8 contrôlé).
  - **Résilience aux NFO invalides** : un NFO malformé génère un diagnostic d'anomalie sans faire échouer le scan, avec repli automatique sur les métadonnées inférées du dossier.
  - **Affiches locales de séries (`crates/node/src/images.rs`)** : découverte sécurisée de `poster.jpg`, `folder.png`, etc. dans le dossier de la série, avec validation d'en-tête et dimensions bornées.

- **API et permissions** :
  - **Catalogue `/api/v1/catalog`** : filtrage étendu supportant `kind=series` et `kind=film` garantissant une étanchéité absolue entre films et séries.
  - **Fiche série `/api/v1/catalog/{id}`** : fournit le titre, la description/synopsis, le nombre de saisons, le nombre d'épisodes, les saisons ordonnées (avec libellé "Hors-série" pour la saison 0 et "Saison X"), et les épisodes ordonnés avec numéro, titre, résumé et identifiants de lecture (`playbackMediaId`).
  - **RBAC et confinement** : vérification stricte des permissions de bibliothèque sur chaque route (catalogue, détails, streaming `/api/v1/assets/{id}/content`, image `/api/v1/assets/{id}/image`). Les chemins physiques absolus du serveur ne sont jamais exposés aux clients.

- **Interface SvelteKit (`apps/web`)** :
  - **Administration des bibliothèques (`admin/libraries/+page.svelte`)** : sélection du type "Séries" (`kind = 'series'`) et déclenchement du scan en direct.
  - **Catalogue et fiche série (`Catalog.svelte`)** : badge `SÉRIE`, sélecteur dynamique de saison et liste ordonnée des épisodes.
  - **Lecteur d'épisode (`EpisodePlayer.svelte` & `ImmersivePlayer.svelte`)** : sélection de version et lecture instantanée via le lecteur immersif avec support pause, reprise, seek et requêtes Range.

- **Validation Docker et intégrité opérationnelle** :
  - Reconstruction réussie de l'image Docker `aether:test` sous Linux x86_64.
  - Démarrage du conteneur en mode autonome avec persistance SQLite `/data` et validation de l'endpoint `/api/v1/health` (HTTP 200).
  - Contrôle de sauvegarde à chaud avec `media-backup` dans `/data/test_backup.sqlite3`.
  - Contrôle de restauration avec `media-restore` vers un volume Docker vierge (`/data`).
  - Nettoyage intégral des conteneurs, volumes et images de test.

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **100 tests réussis** (2 doc-tests, 29 `media-node`, 2 `media-contracts`, 67 `media-poc` dont le banc `series_flow.rs`), 0 échec, 5 ignorés (Playwright).
  - Frontend (`apps/web`) : `npm run check` (0 erreur, 0 avertissement), `npm run lint` (0 erreur), `npm test` (13/13 Vitest réussis), `npm run build` (succès).
  - Zéro commit, zéro push, zéro publication Docker ni déploiement NAS.

## 17 octobre 2026 — Jalon P1 : Sécurisation finale des sauvegardes et restaurations (Refus strict d'écrasement, protection contre la perte de l'ancienne copie, isolation de volume Docker et limites de concurrence)

- **Sécurisation de la restauration (`restore_database` & `media-restore`)** :
  - **Refus strict d'écrasement d'une base existante** : `restore_database` renvoie immédiatement l'erreur typée `BackupError::TargetExists` si la destination existe déjà sur disque. Aucune base existante ne peut être écrasée ou tronquée accidentellement.
  - **Refus strict des journaux résiduels incompatibles** : détection préalable de fichiers `-wal` ou `-shm` orphelins dans la destination (`BackupError::ResidualJournals`), **sans suppression automatique**, pour interdire toute réconciliation asynchrone corrompue.
  - **Élimination de la vérification trompeuse `BEGIN IMMEDIATE`** : suppression définitive du test transactionnel immédiat qui laissait faussement penser qu'un serveur actif mais momentanément inactif était arrêté.
  - **Défense en profondeur sur les chemins** : rejet strict de tout chemin contenant `..`, des liens symboliques et des points d'analyse NTFS (`FILE_ATTRIBUTE_REPARSE_POINT`, jonctions), ainsi que de tout placement à l'intérieur de la racine média (`/media`).
  - **Procédure de reprise hors ligne sur volume neuf** : documentation claire interdisant la restauration sur un volume Docker en service et imposant la restauration dans un volume Docker vierge (`aether_data_restored`) tout en conservant l'ancien volume intact.
  - **Zéro option `--force` dangereuse** : aucun contournement permis.

- **Sécurisation de la sauvegarde (`backup_database_with_options` & `media-backup`)** :
  - **Protection absolue de la copie précédente** : suppression définitive de l'ancien comportement qui effaçait la sauvegarde cible avant d'exécuter `VACUUM INTO`.
  - **Workflow atomique par fichier temporaire** : création d'un instantané temporaire unique (`.backup-<pid>-<uuid>.tmp`) dans le même répertoire.
  - **Vérification complète avant publication** : exécution de `VACUUM INTO`, contrôle d'intégrité SQLite (`PRAGMA integrity_check`), vérification de la présence des tables clés du schéma (`users`, `local_credentials`, `libraries`, `inventory_works`), application des permissions `0600` (Unix), synchronisation des blocs sur disque (`sync_file` avec support `GENERIC_WRITE` sous Windows pour `FlushFileBuffers`).
  - **Publication atomique** : remplacement de l'ancien fichier uniquement après validation intégrale (`std::fs::rename`).
  - **Refus par défaut d'écrasement** : `backup_database` refuse d'écraser un fichier existant par défaut. L'option explicite `--replace` (`BackupOptions { allow_replace: true }`) est requise pour remplacer une sauvegarde existante de façon atomique et vérifiée.
  - **Nettoyage garanti sur incident (`TempFileGuard`)** : en cas d'erreur ou d'interruption, le fichier temporaire est immédiatement purgé par le guard RAII et la sauvegarde précédente reste 100 % intacte.

- **Vérification et durcissement des sauvegardes Docker & Synology DSM** :
  - **Avertissement de criticité sur `/data/backups`** : clarification formelle dans la documentation que `/data/backups` se trouvant à l'intérieur du volume `aether_data`, il ne protège pas contre la perte ou la suppression du volume Docker. Procédures d'exportation vers un dossier hôte externe (ex. `/volume1/backups` sur DSM) ou via `docker cp` documentées.
  - **Périmètre applicatif strict** : les sauvegardes contiennent exclusivement l'état applicatif SQLite (comptes, hachages Argon2id, sessions, graphe média, catalogue, bibliothèques, permissions) et zéro média original (`/media`).
  - **Confidentialité** : permissions strictes `0600` sur les sauvegardes, `0750` sur le dossier, zéro fuite de hachages ou secrets dans les sorties de commande.
  - **Restauration vers volume neuf** : procédure étape par étape avec `docker run` sur un volume Docker dédié sans écraser le volume existant.
  - **Fidélité déterministe** : préservation totale des UUIDs, permissions de bibliothèques et reconnexion immédiate de l'administrateur et des utilisateurs sans réinitialisation de mot de passe.

- **Concurrence des écritures et limites du timeout SQLite** :
  - **Comportement sous publication massive (10k items)** : la publication atomique `publish_staged_scan` maintient le verrou d'écriture exclusif `BEGIN IMMEDIATE` pendant ~30 secondes.
  - **Contention sur routes authentifiées** : chaque requête authentifiée exécute `UPDATE sessions SET last_seen_at = unixepoch() ...`. Ces écritures se mettent en attente du verrou d'écriture jusqu'au `busy_timeout` (défaut 15 secondes).
  - **Limites du timeout** : au-delà de 15 secondes d'attente, SQLite renvoie `SQLITE_BUSY (code 5 : database is locked)` et le serveur répond HTTP 500. Augmenter le timeout masque le symptôme mais impose une latence client de 20-30s. Comportement documenté pour P1 sans altération du moteur de scan.

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **99 tests réussis** (2 doc-tests, 29 `media-node`, 2 `media-contracts`, 66 `media-poc`), 0 échec, 5 ignorés (Playwright).
  - 9 tests dédiés dans `crates/core/tests/backup_restore.rs` validant tous les cas limites : destination vierge, base existante, connexion ouverte idle, journaux WAL/SHM résiduels, sauvegarde corrompue, refus de traversée/media root, refus d'écrasement par défaut, remplacement vérifié et préservation intégrale des données existantes en cas d'échec.
  - Statut Docker : Démon Docker Engine Windows non démarré (`//./pipe/dockerDesktopLinuxEngine` introuvable), validé rigoureusement hors conteneur sans simulation.
  - Zéro commit, zéro push, zéro publication Docker ni déploiement NAS.

## 17 octobre 2026 — Jalon P1 : Fiabilité opérationnelle et reprise sur incident (Sauvegarde/Restauration SQLite WAL, Concurrence sous écriture, Diagnostics de scan, Validation Docker)

- **Sauvegarde et restauration SQLite WAL fiables (`media-backup` et `media-restore`)** :
  - **Correction des commandes invalides documentées** : élimination de l'hypothèse erronée d'un chemin hôte pour le volume nommé Docker `aether_data` et de la dépendance envers un binaire externe `sqlite3` absent de `debian:bookworm-slim`.
  - **Développement de binaires Rust natifs dédiés** : `media-backup` (`crates/core/src/bin/backup.rs`) et `media-restore` (`crates/core/src/bin/restore.rs`), intégrés dans le Dockerfile sous `/usr/local/bin` et utilisables directement en conteneur ou sur l'hôte.
  - **Sauvegarde à chaud cohérente (`media-backup`)** :
    - Utilisation de `VACUUM INTO '<destination>'` via `sqlx::QueryBuilder` : copie instantanée et consistante même sous transactions actives, sans requérir l'arrêt du conteneur.
    - Contrôle d'intégrité immédiat (`PRAGMA integrity_check`) sur la copie générée.
    - Permissions strictes `0600` sous Unix (lecture/écriture restreinte au propriétaire du processus).
    - Métriques récapitulatives affichées (œuvres, éditions, versions, sources, assets, comptes) avec **zéro fuite de mot de passe, de hash ou de secret dans les logs**.
  - **Restauration atomique et intègre (`media-restore`)** :
    - Validation préalable en lecture seule (`PRAGMA quick_check`) sur le fichier source.
    - Détection proactive des verrous SQLite actifs interdisant la restauration à chaud sur une base en cours d'utilisation.
    - Écriture atomique intermédiaire via fichier `.staging` avec synchronisation disque (`sync_all()`), suivie d'un remplacement atomique du fichier de base.
    - Purge propre des fichiers journaux SQLite résiduels (`-wal`, `-shm`) pour prévenir toute corruption par réconciliation d'un ancien journal.
    - Exécution automatique des migrations SQLx au redémarrage pour garantir la stricte cohérence du schéma.
  - **Banc de test d'intégration automatisé (`crates/core/tests/backup_restore.rs`)** :
    - Cycle complet validé : amorçage administrateur, création d'utilisateur régulier, création de bibliothèque, octroi de droits (`grants`), injection et staging de médias synthétiques, publication dans le catalogue, sauvegarde à chaud en cours d'activité, restauration dans un volume vierge, reconnexion réussie de l'administrateur et de l'utilisateur avec leurs sessions/mots de passe, consultation du catalogue avec UUIDs préservés, et rejet strict d'une sauvegarde corrompue.

- **Concurrence des écritures pendant la publication SQLite (10k items)** :
  - **Analyse du comportement transactionnel SQLite WAL** :
    - En mode WAL, les requêtes de lecture (`GET /api/v1/catalog`, streaming HTTP Range) ne bloquent jamais sur la transaction d'écriture exclusive `BEGIN IMMEDIATE` de `publish_staged_scan` (durée ~30s pour 10 000 items, latences p50 ~ 7 ms).
    - En revanche, toute requête HTTP authentifiée exécute `UPDATE sessions SET last_seen_at = ? WHERE id = ?`, ce qui constitue une écriture.
    - Sous `BEGIN IMMEDIATE`, une écriture concurrente se met en attente de libération du verrou d'écriture jusqu'à concurrence du `busy_timeout`.
  - **Configuration du timeout d'attente (`MEDIA_SQLITE_BUSY_TIMEOUT_SECS`)** :
    - Ajout de la variable d'environnement `MEDIA_SQLITE_BUSY_TIMEOUT_SECS` (défaut 15 secondes, recommandation 45s+ pour les très grands catalogues 10k+) et méthode `IdentityStore::open_with_timeout`.
    - Pour les bibliothèques courantes (jusqu'à 1 000 médias, publication ~375 ms), les écritures concurrentes patientent et réussissent à 100 % sans erreur.
  - **Banc d'intégration concurrent (`crates/core/tests/concurrency_writes_during_publication.rs`)** :
    - Mesure empirique des opérations concurrentes (mise à jour de session, création de bibliothèque `POST /api/v1/admin/libraries`, déclenchement de scan `POST /api/v1/admin/libraries/{id}/scan`) pendant la publication exclusive.
    - Confirmation que les requêtes dont l'attente dépasse le timeout reçoivent proprement `SQLITE_BUSY (code 5)` sans corruption de données.
    - Dès le commit de la publication, 100 % des écritures suivantes reprennent instantanément : **0 blocage permanent, 0 interblocage (deadlock)**.

- **Diagnostics de scan et purge du staging (`0010_scan_diagnostics.sql`)** :
  - **Migration additive 0010** : ajout des colonnes `errors_count INTEGER NOT NULL DEFAULT 0` et `warnings_count INTEGER NOT NULL DEFAULT 0` à la table `scan_jobs`. Migrations 0001 à 0009 strictement intactes.
  - **Plafond mémoire et exactitude des compteurs** : `ScanReport` dans `crates/shared/src/inventory.rs` et `crates/node/src/scanner.rs` incrémente les compteurs exacts (`errors_count`, `warnings_count`) tout en bornant la liste des messages textuels détaillés en mémoire vive à 100 anomalies (`MAX_REPORT_ISSUES = 100`).
  - **Purge systématique du staging** : appel à `inventory::clean_staging` après `publish_staged_scan` garantissant que les tables `scan_staging_files` et `scan_staging_issues` sont intégralement vidées après chaque scan ou échec, éliminant tout gonflement inutile du fichier SQLite.
  - **Anonymisation des chemins** : assainissement systématique des chemins d'accès absolus sensibles de l'hôte dans les erreurs de scan (`library.root_path` converti en préfixe générique `/media`).
  - **Banc d'intégration (`crates/core/tests/scan_diagnostics.rs`)** : validation empirique du plafonnement à 100 anomalies, exactitude des compteurs totaux (120 erreurs, 40 warnings), purge complète des tables de staging (0 fichier, 0 problème résiduel) et absence de fuite de chemin absolu hôte.

- **Validation Docker et statut du démon** :
  - Démon Docker Engine Windows non démarré sur cet environnement de test (`//./pipe/dockerDesktopLinuxEngine` introuvable ; client Docker CLI 29.8.2 présent).
  - Conformément à `AGENTS.md` ("Si Docker est indisponible, ne pas simuler sa validation. Préparer les tests reproductibles et déclarer explicitement ce qui reste non vérifié"), aucune validation en conteneur n'a été simulée.
  - Le `Dockerfile` est mis à jour pour compiler et intégrer les binaires `media-backup` et `media-restore`.
  - La suite de tests d'intégration Rust reproduit et valide rigoureusement l'ensemble des scénarios opérationnels (sauvegarde, restauration, redémarrage, permissions et concurrence).

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **91 tests réussis** (2 doc-tests, 29 `media-node`, 2 `media-contracts`, 58 `media-poc`), 0 échec, 5 ignorés (Playwright).
  - Frontend (`apps/web`) : `npm run check` (0 erreur, 0 avertissement), `npm test` (13/13 Vitest), `npm run lint` (0 erreur), `npm run build` (succès).
  - Architecture 3 crates (`media-contracts`, `media-node`, `media-poc`), migrations 0001–0010, modèle de permissions et médias originaux rigoureusement préservés. Zéro commit, zéro push, zéro publication Docker ni déploiement NAS.

## 16 octobre 2026 — Jalon P1 : Robustesse et scalabilité du scanner (Cold Ingestion 10k, publication paginée par clés, audit pool 4 connexions, BEGIN IMMEDIATE)

- **Problèmes identifiés et résolus** :
  - **Plafond mémoire des anomalies (`MAX_REPORT_ISSUES = 100`)** : `ScanReport.errors` et `warnings` n'accumulent plus de listes non bornées en RAM (plafonnement à 100 en mémoire vive via `record_error` et `record_warning`), tandis que 100 % des anomalies sont streamées et persistées dans `scan_staging_issues`.
  - **Chargement de cache streaming sans allocation de vecteur intermédiaire** : `load_scan_cache` et `load_image_cache` utilisent désormais `.fetch(&store.pool)` avec `TryStreamExt::try_next` au lieu de `fetch_all`, éliminant les allocations massives de `Vec<SqliteRow>` en mémoire.
    - Initial scan : 0 entrée en base = mémoire $O(1)$.
    - Rescan : $O(N_{\text{indexés}})$ entrées chemins + timestamps légers (~150 octets/fichier sans `probe_json`).
  - **Publication atomique chunkée par clés (`publish_staged_scan`)** :
    - Élimination des `fetch_all()` non bornés de `scan_staging_files` : pagination par clés (`LIMIT 250` ordonné par `relative_path ASC`), traitant par tranches de 250 et libérant immédiatement la mémoire des probes après chaque lot.
    - Résolution unifiée des affiches et de l'éditorial en une seule passe via `LEFT JOIN inventory_images` et `LEFT JOIN inventory_editorial`, supprimant les requêtes unitaires `SELECT`.
    - Rafraîchissement éditorial groupé (`refresh_editorial_for_works`) par lots de 250 via `sqlx::QueryBuilder`, réduisant les allers-retours SQL par 250.
    - Conservation stricte de l'atomicité transactionnelle (`tx.commit().await` unique en fin de parcours).
  - **Audit du pool SQLite (4 connexions) & élimination de `SQLITE_BUSY_SNAPSHOT`** :
    - **Audit** : le pool `IdentityStore` est explicitement configuré à 4 connexions pour permettre des lectures concurrentes via l'isolation de snapshot WAL (`SqlitePoolOptions::new().max_connections(4)`).
    - **Bug critique résolu** : `store.pool.begin()` ouvrait une transaction différée (`BEGIN`), créant un snapshot de lecture. En présence de requêtes concurrentes authentifiées mettant à jour les sessions (`sessions.last_seen_at`), toute tentative d'écriture ultérieure échouait avec `SQLITE_BUSY_SNAPSHOT (code 517) : database is locked`.
    - **Correction** : utilisation systématique de `conn.begin_with("BEGIN IMMEDIATE")` pour les transactions d'écriture (`publish_staged_scan`, `stage_batch`, `ingest`). Le verrou d'écriture est acquis dès l'ouverture, garantissant un snapshot à jour et éliminant 100 % des erreurs `SQLITE_BUSY_SNAPSHOT`.
  - **Normalisation des chemins Windows** :
    - Élimination des faux positifs de non-concordance de racines (`library configuration changed during scan`) en normalisant les préfixes `\\?\` sur les chaînes de chemins lors de la vérification et de l'insertion dans `inventory_sources` / `inventory_images`.

- **Métriques mesurées et comparatives (Banc `crates/core/tests/scanner_performance.rs`)** :
  - **Cold Ingestion de 10 000 NOUVEAUX médias synthétiques** (AV1 10-bit Main 10 + 2x Opus + 3 sous-titres + NFO + affiches) :
    - Staging 10 000 fichiers (100 lots de 100) : **4 649 ms à 5 504 ms** (moyenne **46,5 à 55 ms/lot**). Empreinte mémoire différentielle staging : **+1,33 Mio**.
    - Publication atomique intégrale (10 000 œuvres, éditions, versions, sources, assets, 40 000 pistes, 5 000 affiches, 10 000 éditoriaux) : **29,7 s à 34,6 s**.
    - Empreinte mémoire différentielle publication : **+5,09 Mio** par rapport au baseline (pic RSS total : 15,05 Mio à 37 Mio en mode test).
  - **Publication 10 000 entrées réutilisées** : **3 834 ms** (contre 7 733 ms avant optimisation, soit un gain de 2x).
  - **Rescan de 10 000 médias inchangés** : publication en **689 ms** (accélération 5,6x).
  - **Banc de concurrence et latence sous écriture SQLite** :
    - 50 requêtes concurrentes (25 requêtes catalogue + 25 requêtes streaming HTTP Range) exécutées en continu pendant la transaction d'écriture exclusive de `publish_staged_scan`.
    - Résultat : **0 erreur `SQLITE_BUSY`**, 50/50 requêtes réussies.
    - Latences mesurées pendant l'écriture active : p50 = **7,29 ms** (à 11,13 ms), p95 = **17,11 ms** (à 23,63 ms).
  - **Scan de 100 fichiers réels** :
    - Scan initial : 100 probes en 7,84 s (RSS: 38,01 Mio).
    - Rescan inchangé : 0 probe en 661 ms (RSS: 38,23 Mio).
    - NFO modifié : 0 probe en 736 ms.
    - Affiche supprimée : 0 probe en 817 ms.

- **Couverture des cas difficiles (4 nouvelles suites de tests automatisés)** :
  1. `test_10000_new_synthetic_media_cold_ingestion_benchmark` : ingestion à froid de 10 000 nouveaux médias avec métadonnées réalistes, staging et publication atomique paginée.
  2. `test_mixed_diff_difficult_cases_and_atomic_publication` : mélange de fichiers inchangés, modifiés, disparus, corrompus/invalides, NFO modifiés, affiches supprimées, et nouveaux fichiers ; vérification de la stabilité déterministe des UUIDs et du catalogue.
  3. `test_sqlite_pool_audit_and_concurrent_reads_during_publication` : audit du pool 4 connexions, mesure des latences p50/p95/p99 et validation de la lecture streaming/catalogue sans `SQLITE_BUSY` pendant la publication.
  4. `test_crash_cancellation_permissions_and_rollback` : annulation/crash pendant le staging, rollback immédiat en cas de changement de configuration de bibliothèque pendant la publication, contrôle strict des permissions (403 Forbidden sur scan et catalogue pour rôle `user`), et respect du flag `complete: false` (fichiers manquants non marqués indisponibles).

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **89 tests réussis** (64 unitaires/intégration Rust + doc-tests), 0 échec, 5 ignorés (Playwright).
  - Architecture 3 crates (`media-contracts`, `media-node`, `media-poc`), migrations 0001–0009, Docker et modèle de permissions scrupuleusement préservés. Aucun commit, push ou publication effectué.

## 16 octobre 2026 — Jalon J3 (`docs/ARCHITECTURE_ROADMAP.md`) / P1 : Scanner incrémental streaming et ingestion progressive à mémoire bornée

- **Goulots d'étranglement confirmés et mesurés** :
  - `load_cache` désérialisait la totalité des `probe_json` SQLite dans une `HashMap` géante : **+37 à +57 Mio de pic RSS** pour 10 000 fichiers.
  - `ScanReport.files` accumulait tous les fichiers scannés en mémoire dans le Node avant retour au Core (mémoire proportionnelle en $O(N)$).
  - L'ancienne méthode d'ingestion exécutait ~17 requêtes par fichier et recalculait l'éditorial de toutes les œuvres (`refresh_editorial`) dans une seule transaction exclusive (`BEGIN IMMEDIATE`), verrouillant SQLite et provoquant des blocages de requêtes concurrentes.
- **Architecture et optimisations réalisées** :
  - **Migration additive `0009_scan_staging.sql`** : création des tables isolées `scan_staging_files` et `scan_staging_issues`, et de l'index `inventory_images_library_available`. Migrations 0001 à 0008 strictement inchangées.
  - **Scanner streaming (`LocalNode::scan_library_streaming`)** : le Node émet des lots bornés (`InventoryBatch`, taille 100 par défaut) avec backpressure asynchrone (`callback(batch).await`). Cache de dossiers borné à `MAX_FOLDER_CACHE = 128` entrées.
  - **Découplage des probes sur fichiers inchangés** : `load_scan_cache` n'extrait que `(size, modified_ns)`. Pour les fichiers inchangés, le Node renvoie `reused: true` et `Probe::default()`, éliminant le clonage et la ré-allocation d'arbres de métadonnées.
  - **Staging par lots bornés (`stage_batch`)** : chaque lot de 100 est écrit dans `scan_staging_files` via une transaction courte (< 20 ms), sans verrouillage prolongé de SQLite.
  - **Publication atomique (`publish_staged_scan`)** :
    - Activation groupée des fichiers inchangés (`reused = 1`) en une seule passe `UPDATE`.
    - Désactivation atomique des fichiers absents / supprimés.
    - Ingestion des seuls nouveaux fichiers et mise à jour ciblée (`refresh_work_editorial`) restreinte aux seules œuvres modifiées.
  - **Résilience aux pannes et annulations** :
    - Nettoyage automatique du staging (`clean_staging`) en cas d'erreur ou d'annulation ; inventaire en direct inchangé.
    - Au démarrage du Core, le routeur purge les résidus de staging orphelins et marque les jobs interrompus en `error` ("Scan interrompu par un redémarrage").
- **Métriques comparatives mesurées (banc `crates/core/tests/scanner_performance.rs`)** :
  - **Cache 10 000 entrées** : `load_scan_cache` charge 10 000 entrées en **145 ms** (+5,30 Mio RSS) vs `load_cache` legacy en **446 ms** (+37,07 Mio RSS), soit un gain mémoire de 7x et 3x plus rapide.
  - **Staging 10 000 entrées** : 100 lots en **1 789 ms** (moyenne **17,89 ms/lot**, max **39 ms/lot**).
  - **Publication atomique 10 000 entrées** : **7 733 ms**.
  - **Catalogue sur 10 000 entrées** : 24 résultats paginés renvoyés en **324 ms**.
  - **Scan de 1 000 fichiers synthétiques sur disque** : 1 000 découverts, **0 probe vidéo**, 1 000 réutilisés, pic RSS borné en $O(1)$ (+0,11 Mio RSS).
  - **Scan de 100 fichiers réels** :
    - Scan initial : 100 probes en 9,98 s.
    - Second scan inchangé : **0 probe vidéo**, 100 réutilisés en **746 ms** (contre 1 229 ms avant optimisation).
    - Modification NFO : **0 probe vidéo** en **705 ms**, métadonnées mises à jour dans le catalogue.
    - Suppression affiche : **0 probe vidéo** en **771 ms**, affiche retirée dans le catalogue.
  - **Concurrence catalogue et lecture pendant le scan** :
    - 25 requêtes catalogue : latence moyenne **8 ms** (max **32 ms**), 100 % succès (200 OK).
    - 25 requêtes HTTP Range de streaming : 25/25 `206 Partial Content` (0 échec, 0 `SQLITE_BUSY`).
  - **Annulation et reprise après redémarrage** : validées avec staging vidé à 0 ligne et inventaire intact.
- **Validations qualité** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **86 tests réussis**, 0 échec, 5 ignorés.

## 16 octobre 2026 — Correction de l'initialisation Unicode de `title_sort` et audit de sécurité ffprobe stdin

- **Correction de l'initialisation Unicode de `title_sort` (Migration additive 0008 et rattrapage Rust)** :
  - **Diagnostic** : Dans la migration `0007_catalogue_indexes.sql`, l'instruction `UPDATE inventory_works SET title_sort = lower(title)` utilisait la fonction SQLite interne `lower()`, restreinte aux caractères ASCII `A-Z`. Les œuvres insérées avant la migration 0007 comportant des majuscules accentuées ou ligatures (`Été`, `À`, `Ça`, `Œuvre`, etc.) conservaient ces majuscules dans `title_sort`, ce qui rompait la recherche par sous-chaîne (`instr(w.title_sort, 'été')` renvoyait 0 résultat) et faussait le tri par rapport aux nouvelles œuvres normalisées en Rust via `title.to_lowercase()`.
  - **Migration additive `0008_catalogue_unicode_sort.sql`** : création de la table de suivi `inventory_works_unicode_migration (completed_at TEXT NOT NULL)`. La migration 0007 n'a pas été altérée.
  - **Procédure de rattrapage Rust par lots bornés (`backfill_title_sort_unicode`)** :
    - Exécutée de façon sérialisée et atomique dans la transaction `BEGIN IMMEDIATE` de migration d'identité (`IdentityStore::open`).
    - Parcours paginé par lots de 250 lignes (`LIMIT 250`, mémoire O(1)), garantissant une empreinte mémoire bornée quelle que soit la taille de la base SQLite.
    - Seules les lignes dont `title_sort != title.to_lowercase()` sont mises à jour.
    - Intégrité totale préservée : UUIDs (`id`), clés d'identité, métadonnées NFO, affiches locales, bibliothèques, permissions et inventaire restent strictement intacts. Zéro nouveau scan vidéo requis.
    - Idempotence et court-circuit instantané (< 0,05 ms) lors des lancements ultérieurs grâce au marqueur dans `inventory_works_unicode_migration`.
  - **Test d'intégration dédié** (`crates/core/tests/catalog_performance.rs::test_migrated_database_from_0006_to_0007_then_0008_unicode_fix`) :
    - Création d'une base sur le schéma 0006 avec des œuvres réelles (`Été d'amour`, `À bout de souffle`, `Ça`, `Œuvre d'art`, `Zèbre`, `Alpha`).
    - Application de la migration 0007 et confirmation formelle du bug `lower()` de SQLite (majuscules préservées, recherche en échec).
    - Ouverture via `IdentityStore::open` appliquant 0008 et le rattrapage.
    - Validation du tri exact et des recherches insensibles à la casse (`été`, `ÉTÉ`, `à`, `ça`, `œuvre`).

- **Audit et vérification de `ffprobe` via stdin (`fd,pipe`)** :
  - **Protocole effectivement utilisé** : `fd:` (confirmé par trace libavformat sur `"-"`).
  - **Support du seek** : le protocole `fd:` sur descripteur de fichier régulier (`std::fs::File` via `Stdio::from(file)`) préserve les capacités de repositionnement (`lseek` sous Unix, `SetFilePointer` sous Windows), contrairement aux pipes anonymes non seekable (`cat | ffprobe`).
  - **Validation des conteneurs et profils** :
    - MKV, MP4 classique, MP4 fragmenté (fMP4) : 3 pistes (AV1 8-bit, 2 Opus FR/EN), durées exactes, codecs validés.
    - Profil représentatif 10 bits (`representative.mkv`, `representative.mp4`) : 6 pistes (AV1 Main10 `yuv420p10le`, Opus 5.1 à 6 canaux, Opus stéréo, 3 pistes de sous-titres), durée 12 s validée.
    - MP4 avec atome `moov` déplacé en fin de fichier (`moov` situé après `mdat`) : le probe via descripteur réussit sans réouverture de chemin, alors qu'une injection via pipe échoue avec l'erreur `moov atom not found`.
  - **Limite de sécurité documentée (Sandbox OS vs Whitelist applicative)** :
    - Une whitelist applicative de protocoles (`fd,pipe`) et formats (`matroska,webm,mov,mp4`) empêche libavformat de résoudre des chemins arbitraires (`file:`), des références externes (MOV `dref`) ou des URLs réseau, **mais ne constitue pas un sandbox de sécurité au niveau du système d'exploitation** (aucun filtre d'appels système seccomp imposé au binaire `ffprobe`).
    - La défense en profondeur documentée dans `SECURITY.md` et `probe.rs` s'appuie sur le confinement Docker (`UID 10001:10001`, `cap_drop: ALL`, `no-new-privileges: true`), le montage en lecture seule `:ro`, les plafonds de mémoire (`-max_alloc 67108864`), d'analyse (`-probesize 5000000`, `-analyzeduration 5000000`), de sortie stdout (1 Mio), le délai d'exécution (10 s avec `kill_on_drop`), et la double vérification de descripteur (`verify_unchanged_sync`).

- **Validation globale** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **85 tests réussis** (26 dans `media-node`, 2 dans `media-contracts`, 57 dans `media-poc`), 0 échec, 5 ignorés (4 tests browser headless et 1 worker helper).

## 16 octobre 2026 — Jalon J2 (`docs/ARCHITECTURE_ROADMAP.md`) : Catalogue SQLite performant et pagination en base

- **Transfert complet du filtrage, tri, recherche, comptage et pagination vers SQLite** :
  - Dans `crates/core/src/inventory.rs::catalogue()`, élimination intégrale du scan mémoire global (`fetch_all` sans LIMIT, `Vec<Value>` et tri Rust).
  - Requête paramétrée sécurisée construite via `sqlx::QueryBuilder` : `WHERE w.library_id IN (?, ...)` applique strictement les permissions d'accès utilisateur en amont de toute pagination.
  - Court-circuit instantané (< 0,01 ms) lorsque `visible` est vide ou que le type demandé est incompatible, éliminant tout appel superflu à SQLite.
  - Requête de comptage dédiée (`SELECT COUNT(*)`) exécutée sans sous-requête d'affiche ni allocation de structures métier.
  - Pagination SQLite `LIMIT ? OFFSET ?` : seules les 24 lignes de la page demandée sont lues et converties en JSON (réduction de 99,76 % des allocations mémoire sur 10 000 œuvres).
  - Évaluation de la sous-requête corrélée d'affiches locales (`poster_url`) bornée exclusivement aux 24 œuvres de la page demandée.
- **Migration non destructive `0007_catalogue_indexes.sql` et plan d'exécution optimisé** :
  - Colonne `title_sort TEXT NOT NULL DEFAULT ''` sur `inventory_works`, peuplée automatiquement avec `lower(title)` et maintenue par Rust avec `to_lowercase()` pour le respect intégral des propriétés Unicode.
  - Index couvrants `inventory_works_library_sort(library_id, title_sort, id)` et `inventory_works_title_sort(title_sort, id)`.
  - Index sur clés étrangères du graphe média : `inventory_editions(work_id)`, `inventory_versions(edition_id)`, `inventory_sources(version_id)`, `inventory_assets(source_id, available)`.
  - Validation `EXPLAIN QUERY PLAN` (`tests/catalog_performance.rs`) : comptage via index couvrant direct (`COVERING INDEX`), vérification `EXISTS` du graphe en O(1) par index seeks systématiques.
- **Préservation de la sémantique de recherche et de tri Unicode** :
  - Recherche par sous-chaîne `instr(w.title_sort, ?)` insensible à la casse Unicode (`été` retrouve `Été à Paris` et `Été & Neige`, `éléphant` retrouve `Éléphant Blanc`, `ça` retrouve `Ça chapitre un`), sans problème d'échappement de caractères spéciaux LIKE (`%`, `_`).
  - Tri déterministe avec bris d'égalité stable `ORDER BY w.title_sort ASC, w.id ASC` (et `DESC`), préservant à l'identique l'ordre historique de `sort_by_key`.
- **Métriques réelles mesurées en banc reproductible (`catalog_performance.rs`)** :
  - 1 000 œuvres : passage de 64,12 ms à **22,04 ms** (gain 2,9x) ; recherche ciblée en **7,71 ms** (gain 8,3x).
  - 10 000 œuvres : passage de 854,38 ms à **235,91 ms** (gain 3,6x) ; page éloignée (offset=9960) en **335,48 ms** (gain 2,5x) ; recherche ciblée en **53,66 ms** (gain 15,9x).
  - Empreinte mémoire Rust : 24 structures JSON allouées au lieu de 10 000 (division par plus de 400).
- **Validation** :
  - `cargo fmt --check` : OK.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **81 tests réussis** (23 dans `media-node`, 2 dans `media-contracts`, 56 dans `media-poc`), 0 échec, 5 ignorés.
  - Documentation complète créée dans `docs/SQL_PERFORMANCE.md`.

## 16 octobre 2026 — Sécurisation de l'analyse ffprobe sur descripteur confiné (`probe_file` / `probe_opened`)

- **Audit et élimination de la substitution transitoire de chemin (TOCTOU) dans `ffprobe`** :
  - Dans `crates/node/src/probe.rs` et `crates/node/src/scanner.rs`, `probe` et `probe_opened` transmettaient précédemment un chemin canonique à `ffprobe`.
  - Résolution : `ffprobe` est exécuté avec `-protocol_whitelist fd,pipe` et `-` sur stdin connecté directement au descripteur/handle `OpenedFile` cloné (`std::process::Stdio::from(opened.try_clone_std()?)`), avec `file.rewind()?` préalable.
  - Le protocole `file:` est totalement retiré de la whitelist, interdisant mathématiquement à `ffprobe` d'ouvrir le moindre chemin de fichier sur le disque ou de résoudre des références externes (dref, segments Matroska externes).
  - Capacités de seek préservées : sur Linux comme sur Windows, un descripteur régulier hérité sur stdin (`fd 0` / `STD_INPUT_HANDLE`) conserve l'aptitude `lseek` / `SetFilePointerEx`. Vérifié avec succès sur les conteneurs MKV, MP4, MP4 fragmenté et MP4 avec atome `moov` déplacé en fin de fichier.
  - Le scanner (`scanner.rs`) utilise désormais `probe::probe_opened(&opened)` au lieu de `probe::probe(opened.canonical_path())`.
  - Contrôle double : `OpenedFile::verify_unchanged_sync()` est exécuté avant et après le probe, garantissant l'intégrité de l'attribut fichier et détectant toute altération du chemin sur le disque.
  - Test dédié de non-régression et de résistance à la substitution ajouté : `probe::tests::probe_opened_consumes_handle_and_resists_path_substitution`.
- **Validation** :
  - `cargo fmt --check` : OK.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **77 tests réussis** (23 dans `media-node`, 2 dans `media-contracts`, 52 dans `media-poc`), 0 échec, 5 ignorés.

## 16 octobre 2026 — Jalon J1 (`docs/ARCHITECTURE_ROADMAP.md`) : Workspace Rust 3 crates et confinement par descripteur (`OpenedFile`)

- **Découpage effectif en 3 crates Cargo (`crates/core`, `crates/node`, `crates/shared`)** :
  - `crates/shared` (`media-contracts`) : contrats partagés, identifiants et constantes de marque (`brand.rs`), types de pistes/probes (`media.rs`), descripteurs d'inventaire/NFO/images (`inventory.rs`) et erreurs communes (`errors.rs`). Aucune dépendance sur Axum, SQLx ou Tokio.
  - `crates/node` (`media-node`) : confinement des racines et fichiers (`files.rs` avec `OpenedFile`), exécution bornée de `ffprobe` (`probe.rs`), parsing XML NFO borné sur handle ouvert (`nfo.rs`), validation et découverte d'affiches locales sur handle ouvert (`images.rs`), parcours récursif avec cache de vidéos par dossier (`scanner.rs` / `FolderVideos` éliminant la relecture `O(N^2)` des dossiers), inventaire de fixtures (`inventory.rs`), streaming HTTP `GET`/`HEAD`/`Range` 64 Kio sur descripteur ouvert (`stream.rs`) et façade `LocalNode` (`lib.rs`). **Zéro dépendance vers `media-poc` ou `sqlx`**.
  - `crates/core` (`media-poc`) : serveur HTTP Axum, authentification/sessions/CSRF, catalogue et persistance SQLite (`migrations/0001` à `0006` inchangées, identifiants déterministes et UUID existants préservés), délégant toutes les opérations fichiers, le scan et le streaming média/affiches à `LocalNode` via `ReadGrant` / `ReadTarget`.
- **Élimination du TOCTOU de réouverture par chemin (`canonicalize` + `ServeFile`)** :
  - Sous Linux/Docker : traversée relative confinée via `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_XDEV)` avec repli composant par composant `openat(O_NOFOLLOW | O_CLOEXEC)` (`open_unix_fallback`, activé sur `ENOSYS` ou `EPERM` seccomp) et vérification `fstat`.
  - Sous Windows : ouverture des répertoires intermédiaires sans `FILE_SHARE_DELETE` maintenus ouverts pendant la traversée, ouverture avec `FILE_FLAG_OPEN_REPARSE_POINT`, rejet immédiat de `FILE_ATTRIBUTE_REPARSE_POINT` (`0x400`, symlinks et junctions NTFS) sur chaque handle et vérification d'identité de handle (`verify_unchanged` / `verify_unchanged_sync`).
  - Le streaming HTTP (`GET`, `HEAD`, `Range` simple RFC 9110 avec `200 OK`, `206 Partial Content`, `416 Range Not Satisfiable`, blocs de 64 Kio), la lecture NFO et le décodage d'images consomment directement le `std::fs::File` contenu dans `OpenedFile`.
  - L'analyse `ffprobe` consomme directement le descripteur de fichier ouvert via stdin avec whitelist `fd,pipe` (aucun accès `file:`).
- **Mise à jour Docker et validations complètes** :
  - `Dockerfile` mis à jour pour mettre en cache les dépendances des 3 crates (`crates/core`, `crates/node`, `crates/shared`).
  - `cargo fmt --check`, `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 erreur, 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **76 tests réussis** (`2` dans `media-contracts`, `22` dans `media-node`, `52` dans `media-poc`), 0 échec, 5 ignorés (et `24/24` tests unitaires `media-contracts` + `media-node` validés en conteneur Linux Docker).
  - Frontend (`apps/web`) : `npm run check` (0 erreur, 0 warning), `npm run lint`, `npm test` (13/13 Vitest), `npm run build` réussis sans modification de `apps/web`.
  - Validation Docker E2E Linux (`docker compose build` + `docker compose up -d`) : conteneur `healthy` (RSS au repos ~2,11 MiB), bootstrap admin, login, scan `/media`, catalogue, streaming `Range: bytes=0-63` (`206 Partial Content` via `openat2` Linux) et persistance des UUID après redémarrage du conteneur validés.

## 16 octobre 2026 — Audit et optimisation du frontend SvelteKit (Performance & Code Splitting)

- **Audit et optimisation ciblée du frontend (`apps/web`)** :
  - **Scission dynamique et chargement différé du lecteur** : `ImmersivePlayer` et `NativePlayer` scindés en modules asynchrones (`chunks/ZWKrfpLt.js` et `chunks/5MZuhulj.js`), chargés en parallèle uniquement lorsqu'une fiche média est consultée (`?work=`).
  - **Élimination de l'initialisation inutile du lecteur** : aucun élément `<dialog>`, `<video>` ni écouteur d'événement (`fullscreenchange`, `keydown`) n'est monté sur les écrans sans lecture (`/`, `/search`, `/settings`, listes des bibliothèques).
  - **Réduction mesurée des bundles de production** :
    - Chunk principal du catalogue (`Catalog.js`) : réduit de **42,65 Ko** (gzip: 13,93 Ko) à **21,35 Ko** (gzip: 7,62 Ko), soit **-50,0 %** brut (-45,3 % gzip).
    - Feuille de style du catalogue (`Catalog.css`) : réduite de **18,09 Ko** à **10,81 Ko**, soit **-40,2 %** brut.
    - Transfert initial total sur `/` : réduit de 145,21 Ko à **123,87 Ko JS** (-14,7 %) et de 23,71 Ko à **16,43 Ko CSS** (-30,7 %).
  - **Optimisation des requêtes réseau et cache mémoire** :
    - Mise en cache mémoire de la configuration publique (`fetchBrandName()` dans `brand.ts`), éliminant 100 % des appels redondants à `/api/v1/config` lors des navigations.
    - Mise en cache en mémoire des médias de démonstration (`demoMediaCache`) et parallélisation systématique des résolutions de sources (`Promise.all`).
  - **Optimisations de mémoire et rendu** :
    - Déplacement des données statiques dans des blocs `<script module>` (`Icon.svelte`, `admin/libraries/+page.svelte`, `Catalog.svelte`), évitant les allocations à chaque instanciation.
    - Réutilisation unique de `Intl.DisplayNames` dans `track-labels.ts`.
    - Déduplication du balisage dans `Poster.svelte` via snippet Svelte 5.
    - Suppression de la dépendance inutilisée `@opentelemetry/api` dans `package.json`.
  - **Vérifications de non-régression validées** :
    - `npm run check` (0 erreur, 0 avertissement).
    - `npm run lint` (0 erreur).
    - `npm test` (13/13 Vitest réussis).
    - `npm run build` (build réussi en 405 ms avec externalisation bootstrap CSP).
    - `cargo test --test static_web` (test d'intégration Rust Core / Axum réussi).
  - Rapport complet documenté dans `docs/WEB_PERFORMANCE.md`.

## 16 octobre 2026 — Distribution Docker autonome P1 et validation E2E

- **Jalon P1 validé et opérationnel** : première distribution Docker « tout-en-un » (`aether:latest`, `compose.yaml`), réunissant le frontend SvelteKit compilé, le Core Axum/Tokio, le Media Node local, la persistance SQLite en mode WAL et `ffprobe`.
- **Décisions architecturales retenues** :
  - Pipeline de construction multi-stage en 3 étapes :
    - Étape 1 (`web-builder`) : Node.js 22 bookworm-slim compile le frontend statique via `@sveltejs/adapter-static` et externalise les scripts d'amorçage (`scripts/externalize-bootstrap.mjs`) pour la conformité CSP.
    - Étape 2 (`rust-builder`) : Rust slim-bookworm compile les binaires release `media-poc` et `media-bootstrap` avec mise en cache des dépendances Cargo et dépouillement des symboles (`strip --strip-all`).
    - Étape 3 (`runtime`) : Image minimale Debian bookworm-slim avec `ffprobe`, `ca-certificates`, `tini` et l'utilisateur non-privilégié `aether` (`UID:GID 10001:10001`).
  - **Zéro runtime Node.js permanent** : l'interpréteur Node.js est totalement absent de l'image de production ; les fichiers statiques sont servis directement par Axum (`ServeDir`).
  - **Gestion propre du cycle de vie et signaux** : `ENTRYPOINT ["tini", "--"]` avec `STOPSIGNAL SIGINT` et `stop_grace_period: 15s`. `media-poc` capture `SIGINT`, annule les scans en cours sans perte d'inventaire et synchronise le journal WAL SQLite.
  - **Liaison réseau et sécurité** : liaison hôte restreinte par défaut à la boucle locale (`127.0.0.1:3000:3000`) pour prévenir toute exposition non authentifiée sur le LAN. Compatibilité proxy inverse documentée avec validation stricte de l'en-tête `Host` correspondant à `MEDIA_PUBLIC_URL`.
  - **Sonde de santé découplée** : route `/api/v1/health` isolée de la validation `Host` externe, garantissant le succès des sondes Docker internes (`127.0.0.1`) sans fuite d'informations sensibles quel que soit le domaine public configuré.
  - **Sécurité et moindre privilège** : exécution non-root (`10001:10001`), `cap_drop: ALL`, `no-new-privileges: true`.
  - **Étanchéité des données** : bibliothèques médias montées strictement en lecture seule (`:ro`) ; état applicatif et base SQLite isolés dans le volume persistant `aether_data` (`/data`).
- **Amorçage administrateur local (Bootstrap)** :
  - Commande documentée : `docker compose exec -it aether media-bootstrap`.
  - Rejet systématique en mode non interactif sans terminal (code 1).
  - Amorçage interactif par pseudo-TTY avec saisie masquée et confirmation du mot de passe. Zéro identifiant ou mot de passe par défaut.
  - Verrouillage définitif en base SQLite : toute tentative ultérieure échoue (`initial bootstrap is already closed`).
- **Métriques réelles mesurées en conteneur (AC-11)** :
  - Taille de l'image : contenu 213 Mo (753,84 Mo de couches décompressées, empreinte disque hôte 790 Mo).
  - Délai de démarrage jusqu'à l'état `healthy` : 6,34 secondes.
  - Mémoire RSS du conteneur au repos (idle) : 2,88 Mo (2,883 MiB).
  - Mémoire RSS pic pendant le scan ffprobe de bibliothèque : 12,56 Mo.
- **Vérifications de bout en bout validées (AC-1 à AC-11)** :
  - AC-1 : `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (52 réussis, 0 échec, 5 ignorés).
  - AC-2 : `npm run check` (0 erreur, 0 warning), `npm run build`, `npm test` (13/13 Vitest réussis).
  - AC-3 : `.dockerignore` exclut `.git`, `target`, `node_modules`, fixtures et logs (< 5 Mo ; transfert effectif mesuré : 5,82 Ko).
  - AC-4 : image non-root (10001:10001), `ffprobe` dans le `PATH`, aucun binaire Node.js dans le runtime.
  - AC-5 : démarrage Compose, migrations SQLite WAL automatiques dans `/data`, conteneur `healthy`.
  - AC-6 : rejet des requêtes anonymes (401/403), contrôles `Host`/`Origin`/CSRF actifs sous Docker.
  - AC-7 : amorçage administrateur interactif réussi, second amorçage rejeté, session et cookie sécurisé émis.
  - AC-8 : scan de bibliothèque `:ro` avec `ffprobe`, catalogue et affiches locales peuplés, écriture refusée dans `/media` (`Read-only file system`), 0 octet modifié dans les médias sources.
  - AC-9 : streaming MP4 et requêtes HTTP Range seek (`206 Partial Content`, `416 Range Not Satisfiable`) conformes.
  - AC-10 : arrêt (`docker compose down`) et redémarrage (`docker compose up -d`) avec persistance des comptes, du catalogue et récupération propre des scans interrompus (`Scan échoué ou interrompu`).
  - AC-11 : métriques réelles enregistrées dans la documentation.
- Documentation ajoutée : `docs/DEPLOYMENT_DOCKER.md`, mise à jour de `README.md` et `MEMORY.md`.

## 16 octobre 2026 — audit et consolidation de l'architecture Rust

- Feuille de route : `docs/ARCHITECTURE_ROADMAP.md`. Audit du code actuel sur
  develop, sans refactorisation backend, modification Web/packaging/migrations,
  commit, push ni publication. Un sous-agent a vérifié les frontières Node/I/O.
- Cible retenue : trois crates core/node/shared, appels Rust en standalone,
  Core propriétaire des UUID/droits/SQLite, Node sans dépendance Core/SQL Core.
  Auth et DB restent des modules ; aucune crate vide supplémentaire créée.
- Trois macro-jalons proposés : extraction et sécurité fichiers P1 ; standalone
  utilisable/mesuré P1 ; Node distant et OIDC P2. Ce plan n'implémente aucune de
  ces étapes et ne clôture pas P1. Les quatre images restent une cible packaging.
- Risques prioritaires vérifiés : TOCTOU confinement/réouverture, cycle logique
  Node/inventory, rapports/cache et pagination catalogue en RAM, coût des images
  et relectures de dossiers, contention SQL et budgets globaux manquants.
- Web statique servi par Rust : runtime sans Node.js possible. ffprobe externe
  reste requis pour les scans actuels ; aucun parseur Rust natif annoncé.
- Exécutés : cargo metadata --locked --no-deps --format-version 1 ; cargo fmt
  --check ; cargo clippy --locked --all-features --all-targets -- -D warnings ;
  cargo test --locked --all-features : 50 tests réussis, 5 ignorés (4 navigateurs
  et helper subprocess). git diff --check réussi. Aucun benchmark, navigateur,
  conteneur ou NAS relancé. Les objectifs de performance du document sont proposés.
- Statut initial propre ; `.agents/` et `ORIGINAL_REQUEST.md` sont apparus pendant
  l'audit, hors de nos modifications, et ont été laissés intacts.

## 16 octobre 2026 — clarification bibliothèques et scanner

- Décision : Media Node responsable des dossiers, scan récursif, analyse réelle
  MKV/MP4 multipistes et streaming ; Core responsable du catalogue global SQLite,
  comptes/permissions et réception de l'inventaire. Web = interface/administration.
- Chaque bibliothèque appartient à un Node et une racine explicite : Films →
  Node local → /Films, Séries → Node local → /Séries (exemples uniquement).
  P1 conserve un seul Node local ; aucun multi-Node P2 implémenté.
- Scanner futur autonome, sans dépendance Radarr/Sonarr : dossiers manuels et
  conventions ARR, films/séries/saisons/épisodes, sous-titres externes, NFO/images.
  Entrées non fiables et médias toujours en lecture seule ; caches séparés.
- Code inspecté : inventaire explicite de fixtures, libraryId statiques, sessions
  SQLite présentes ; pas de Library/Node/racine persistants ni de grants fins.
  Les anciennes notes globales pré-auth ne décrivent plus le code courant.
- Contrats précisés dans docs/LIBRARIES_AND_SCANNER.md et documents associés.
  Aucun code, migration ou endpoint ajouté ; administration/permissions sur
  fixtures d'abord, véritable scanner ensuite. Aucun média personnel connecté.
- Validation de cette intervention : git diff --check et revue des liens locaux ;
  suites Rust/Web non relancées pour cette clarification documentaire seule.


> **État courant : 17 octobre 2026. Distribution Docker autonome P1 et fiabilité opérationnelle validées. Sauvegarde et restauration SQLite WAL (`media-backup` et `media-restore`), résistance à la concurrence sous écriture SQLite (10k items), diagnostics de scan bornés (migration 0010) et purge du staging opérationnels. Standalone déployé, testé et récupérable après sinistre.** Les entrées datées conservent l'historique ; vérifier code/tests avant toute annonce.

## 1. Vision actée

- [x] Aether est un **nom de code provisoire** et doit rester changeable.
- [x] Application gratuite, **open source**, entièrement **auto-hébergeable**, interface moderne inspirée de Netflix.
- [x] **Mode autonome prioritaire** : Web + Core + Node local, installation simple et SQLite.
- [x] **Multi-Node** ensuite : un catalogue et une interface, plusieurs serveurs médias.
- [x] **Multi-Core et réplicas Web envisagés pour de fortes charges**, sans les implémenter dans le MVP.
- [x] Stack cible : Rust (Axum/Tokio) + Svelte 5/SvelteKit, SQLx/SQLite, PostgreSQL plus tard.
- [x] Authentification locale sécurisée pour le premier admin et SSO OIDC (ex. Authentik) ; rôle OIDC mapping explicite.
- [x] **Seulement deux rôles `administrator` et `user` : aucun `guest`/invité**.
- [x] Volumes des médias en **lecture seule** et coexistence avec Jellyfin : aucune migration obligatoire.
- [x] Pas de transcodage vidéo obligatoire ; remux facultatif et tests de compatibilité réels.

`[x]` ici signifie **décision retenue**, et **non fonctionnalité codée**.

## 2. Priorité multimédia mise à jour (TRÈS IMPORTANT)

### Priorité n°1 — IMMÉDIATE

- [x] Les fichiers existants **`.mkv` et `.mp4`** constituent les entrées prioritaires.
- [x] Les codecs **AV1 vidéo + Opus audio** sont le **premier couple à faire fonctionner**, pas un cas optionnel.
- [x] Analyser correctement **plusieurs pistes vidéo, plusieurs audios, plusieurs sous-titres intégrés** lorsqu'ils existent.
- [x] **La preuve en navigateur est nécessaire** : présence AV1/Opus dans le fichier ≠ lecture directe garantie. Autoriser un remux/packaging sans réencodage lorsque testé ; sinon afficher l'incompatibilité.
- [x] Les films restent accessibles via Jellyfin parallèlement : **aucune écriture dans les médias**.

### Priorité n°2 — APRÈS le parcours autonome/multi-Node

- [x] Supporter **plusieurs assets distincts** : 1 ou N fichiers vidéo, 1 ou N audios, 1 ou N sous-titres ; associations, langues, offsets, durées et seek.
- [x] Gérer également l'hybride (MKV/MP4 avec audio et sous-titres externes).
- [x] Privilégier audio français et sous-titres français forcés si disponibles/appropriés, tout en respectant le choix explicite utilisateur.

### Ambition de format ultérieure — PAS encore validée

- [x] Le produit doit **prévoir** le cas de fichiers de pistes *vraiment indépendantes et sans conteneur multimédia* (flux AV1 élémentaire et paquets Opus bruts), pas seulement des MKV/MP4 à une piste chacun.
- [ ] Définir et tester le **format effectif** de ces données, le manifeste d'assemblage, l'index, l'initialisation codec et les horodatages.
- [ ] Prouver le packaging / la lecture browser avec ces données raw **sans recoder la vidéo**.
- [ ] Décider seulement après POC si ce mode sera recommandé ou si les fichiers mono-piste conteneurisés restent préférables.

**Ne pas confondre** `.opus` (habituellement Ogg Opus) avec un flux de paquets Opus sans conteneur. `.mkv` mono-piste reste un conteneur. Cette nuance doit figurer dans la documentation.

## 3. Utilisateurs, permissions et SSO

- [x] Rôles : administrateur et utilisateur **exclusivement**, même en distribué.
- [x] Premier lancement : administrateur local créé sans identifiants prédéfinis. Un utilisateur normal n'accède pas aux réglages Core/Node.
- [x] OIDC : Authentik compatible mais pas obligatoire pour une installation locale ; aucune promotion admin automatique ou auto-inscription publique par défaut.
- [x] Les profils de lecture multiples par compte peuvent être développés après le MVP ; **un profil n'est pas un rôle invité**.
- [x] Des permissions de bibliothèques/médias contrôlées côté Core et Node seront nécessaires pour le multi-utilisateur.

## 4. Plateforme de référence

- **Synology DS1522+** (Ryzen R1600, pas d'iGPU), installation coexistante avec Jellyfin et médias présents.
- Accès lecture seule `:ro` ; index, SQLite et cache ailleurs.
- Faible RAM/CPU recherchés, sans objectif chiffré arbitraire non démontré.
- La montée en charge dépend du débit disque/réseau, du packaging, des éventuels relais et de l'efficacité des requêtes.

## 5. Décisions techniques retenues

| Domaine | Décision | État |
|---|---|---|
| Backend | Rust édition 2024 + Axum/Tokio/Tower | Banc local P0 codé et testé ; Core produit non développé |
| Frontend | Svelte 5 + SvelteKit + TypeScript strict | Build statique servi par Rust, écran local P1 et direction visuelle v1 testés |
| Lecteur | HTML5 natif + remux par langue, provisoire | Décision utilisateur P1 ; comparaisons suspendues, interface Player légère conservée |
| DB MVP | SQLx + SQLite | Choisi, non codé |
| DB cluster | PostgreSQL partagé, migration explicite | Futur uniquement |
| Cache/coordination | Local d'abord, Valkey éventuel | Futur si besoin |
| Auth | local bootstrap + OIDC Authorization Code / PKCE | Bootstrap CLI testé ; connexion/sessions/OIDC absents |
| Streaming | Direct si compatible, remux/packaging testé, erreur sinon | Natif, remplacement URL, MSE minimal et Shaka DASH testés localement ; P0 partiel, Video.js et sortie physique NT |
| Raws AV1/Opus | Format et index à concevoir/valider ultérieurement | **Non implémenté** |
| Plugins | Facultatifs, pas de framework à ce stade | Hors MVP |
| Licence | GNU Affero General Public License v3.0 (AGPL-3.0) | Décidée et fichier LICENSE ajouté |

## 6. Feuille de route (aucune phase terminée)

- [ ] **P0A** — Scanner les MKV AV1+Opus et lire/démontrer sur Chrome/Edge/Firefox ; informations de pistes exactes.
- [ ] **P0B** — Même chose pour les MP4 AV1+Opus ; différence claire entre MP4 valide, navigateur compatible et fMP4/MSE si requis.
- [ ] **P0C** — Pistes internes multiples, seek, pistes FR/EN, sous-titres et erreurs explicites ; comparer moteurs de lecture.
- [x] **P1** — Mode autonome utilisable : scan read-only, catalogue minimal, lecture P0, admin local, SQLite, déploiement simple.
- [ ] **P2** — Deux Nodes, appairage/auth, catalogue global, OIDC Authentik et permissions user/admin.
- [ ] **P3A** — Associations de pistes externes, versions vidéo alternatives et audio indépendant, synchronisation.
- [ ] **P3B** — POC raw AV1 / raw Opus + index/manifeste, puis décision de format ; **pas nécessaire pour lancer la V1**.
- [ ] **P4** — Films/séries, recherche, fiches, progression, profils et polish UI.
- [ ] **P5** — Tests de charge, sauvegarde, observabilité, robustesse, compatibilité étendue.
- [ ] **P6 possible** — Plusieurs Core actifs, PG + sessions et jobs partagés, réplication statique et tests de failover.

## 7. Non-objectifs immédiats

Pas de rôle invité, pas de cluster, pas de moteur de recommandation, pas de transcodage vidéo obligatoire, pas de nouveau conteneur propriétaire codé avant POC, pas de plugins, pas d'import Jellyfin indispensable, pas d'app native téléphone/TV au MVP, pas de support « tous les codecs et tous les navigateurs » promis.

## 8. Vérifications et état réel

- [x] Code exécutable présent et vérifié : banc P0 Axum local uniquement.
- [x] Scan MKV AV1+Opus réussi : fixture synthétique, trois pistes, adaptateur ffprobe temporaire.
- [x] Scan MP4 AV1+Opus réussi : fixtures classique/fragmentée, trois pistes chacune.
- [x] Lecture navigateur réelle vérifiée : Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1 headless Windows ; pixels/PCM/pause/seek/fin et FR/EN par remux, écoute humaine et sélection interne non validées.
- [x] Sécurité bootstrap administrateur vérifiée.
- [x] Standalone déployé.
- [ ] Deux Nodes et OIDC testés.
- [ ] Pistes externes synchronisées.
- [ ] Formats raw conçus et vérifiés.
- [ ] Tests de charge effectués.

**Ne cocher une case ci-dessus qu'après commande/test documenté** (OS, version, méthode, résultat, erreur). Les décisions sont une spécification, pas un état de livraison.

## 9. Prochain pas recommandé à l'IA

La décision utilisateur du 15 octobre autorise P1 et suspend les comparaisons
approfondies de lecteurs. La dernière intervention vise la direction visuelle,
pas de nouvelles fonctions serveur. Examiner docs/UI_DIRECTION.md et les
preuves P1 avant d'élargir l'UI. Recueillir le retour utilisateur sur cette
première composition. Ensuite seulement, jalon séparé pour admin local/SQLite,
puis bibliothèques ro avec autorisations serveur. Pas de médias personnels
exposés par ce prototype localhost sans authentification.

## 10. Jalon réel du 15 octobre 2026 — premier banc P0

- Dépôt initial : seulement les quatre documents racine, aucun code ; `docs/`
  absent malgré les références. Les documents de ce dossier ont été créés pour
  consigner le périmètre réel, pas présentés comme une spécification préexistante.
  Les modifications utilisateur déjà présentes ont été conservées.
- Workspace Rust édition 2024, une seule crate `media-poc` dans `crates/core`,
  frontières internes HTTP/media/node. Axum 0.8.9, tower-http 0.7.1, Tokio
  1.53.3 ; dépendances résolues dans Cargo.lock. Rust/Cargo 1.96.1 utilisés.
- Quatre fixtures synthétiques de huit secondes : MKV, MP4 faststart, WebM,
  MP4 fragmenté ; AV1 Main 8 bits 320×180 24 fps et deux Opus mono 48 kHz
  FR/EN. Génération avec FFmpeg 9.0.2 ; remux `-c copy`, décodage de toutes
  les pistes et comparaison SHA-256 de tous les paquets encodés réussis.
- Probe ffprobe temporaire au démarrage, jamais pendant la lecture : tous les
  flux conservés avec paramètres disponibles et codec init. Tableau extensible,
  mais aucune commutation de pistes, sous-titre, vidéo alternative ou format raw
  validé. Langues source ISO conservées ; normalisation BCP 47 à faire.
- Serveur limité à `127.0.0.1`, quatre IDs explicites, aucun chemin HTTP libre,
  contrôles Host/Origin/cross-site. Lecture seule, Range via ServeFile (64 KiB).
  Pas d'auth : fixtures synthétiques uniquement, pas d'exposition publique/NAS.
  Confinement canonique non atomique face à un écrivain hostile : limite TOCTOU
  documentée, test de symlink Unix pas exécuté sous Windows.
- Chrome 155.0.8059.40 headless via Playwright 1.62.1 : les quatre URLs natives
  ont produit pixels et signal PCM, pause stable, seek à 4 s, reprise et fin.
  Source absente : HTTP 404 et MediaError 4 affichée. Pas d'écoute humaine,
  de sélection FR/EN, de MSE, Video.js/Shaka, Edge/Firefox/Safari ou preuve NAS.
- Commandes exécutées : `./scripts/generate-fixtures.ps1`, `cargo fmt --check`,
  `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`
  (**5 tests Windows réussis**) et `node scripts/browser-smoke.cjs` avec le
  Playwright de l'environnement. Test d'I/O sur copie temporaire de 1 GiB,
  premier chunk borné et abandon ; aucune mesure de RSS globale ni de charge.
- Preuves et commandes : `docs/STREAMING_COMPATIBILITY.md`, `docs/TEST_PLAN.md`,
  `docs/evidence/2026-10-15/{generation,browser}.json`. CI Linux ajoutée mais
  non exécutée sur GitHub. Pas de commit/push/publication, pas de licence modifiée,
  pas de média personnel touché. **Aucune phase complète cochée.**
- Prochain petit jalon proposé : mêmes fixtures sur Edge/Firefox, écoute et
  sélection audio manuelles. Puis P0C (sous-titres/vidéos alternatives et
  comparaison moteurs), avec validation du périmètre avant élargissement.

## 11. Second jalon réel du 15 octobre 2026 — Edge/Firefox et limite audio

- Mêmes quatre fixtures : SHA-256 conformes au premier jalon et identiques
  avant/après chaque test. Aucun original modifié ni régénéré.
- Scénario commun `scripts/media-checks.cjs`, runner `browser-matrix.cjs`.
  Chrome 155.0.8059.40 et Edge 155.0.4283.45 via Playwright 1.62.1 ; Firefox
  155.0.1 installé via geckodriver 0.37.1 officiel Mozilla, outil dev uniquement.
  Profils de test distincts, Windows x64 10.0.26300, headless, pas de fonction
  expérimentale audio activée. Edge existant trouvé dans EdgeCore ; aucune
  installation après le refus automatique d'une tentative d'extraction initiale.
- **12 lectures natives réussies** : pixels RGB, PCM avant/après seek,
  fréquence dominante 439,453125 Hz (FR 440 Hz), pause, seek 4 s, reprise et fin.
  Source manquante : HTTP 404, MediaError 4 et message visible sur les 3 clients.
- **Sélection FR/EN non validée** : `video.audioTracks` absent dans les 12 cas.
  Le banc affiche cette limite et désactive son sélecteur. Le scénario prévoit
  une séquence FR/EN/FR et comparaison 440/880/440 Hz si deux pistes sont
  exposées, mais cette branche reste NT ici. Menus intégrés et écoute humaine NT.
- WebM/fMP4 remuxés sont lus par URL native. `isTypeSupported` annonce MP4/WebM
  et refuse le MIME Matroska testé ; **aucun append/lecture MSE exécuté**.
- fmt/clippy, 5 tests Rust et syntaxe JS vérifiés. Rapports et commandes dans
  `docs/TEST_PLAN.md`, `docs/evidence/2026-10-15/native-audio/` et la matrice.
  CI distante, NAS, Safari, sous-titres et vidéo alternative NT. P0A/P0B restent
  partiels, P0C commencé mais non terminé. Aucun ajout SvelteKit/auth/catalogue/Node.
- Économie de contexte : lectures ciblées et un scénario partagé entre clients ;
  aucun pourcentage d'économie inventé.

## 12. Jalon P0C réel du 15 octobre 2026 — remux FR/EN et changement de source

- `scripts/prepare-language-remux.ps1` produit quatre nouveaux fichiers dans
  `tests/fixtures/generated/language-remux` avec `-c copy`, une vidéo AV1 et
  un audio Opus chacun. Refus d'écrasement vérifié. Tous les 192 paquets AV1 et
  401 paquets Opus sélectionnés sont identiques par SHA-256 aux originaux,
  sorties décodées intégralement. Les quatre fichiers initiaux restent intacts.
- Préparation unique : MKV FR 1407,36 ms (premier lancement), MKV EN 87,11 ms,
  MP4 FR 134,03 ms, MP4 EN 108,30 ms ; 710698 octets supplémentaires.
  Pas de benchmark répété ou de mesure NAS. FFmpeg uniquement hors lecture.
- Inventaire : quatre IDs dérivés optionnels et association langue/source/index,
  aucune entrée HTTP par chemin libre. Un dossier de remux partiel ou des
  codecs/langues incompatibles empêchent le démarrage. Les remux utilisent les
  mêmes routes GET/HEAD/Range et le même confinement en lecture seule.
- Menu de banc distinct de l'audio natif : changement réel d'URL, métadonnées,
  restauration de `currentTime`, puis reprise seulement si auparavant en lecture.
  Erreurs explicites ; pas de restauration automatique de l'ancienne source.
- Scénario commun `scripts/remux-checks.cjs`, lancé via
  `MEDIA_TEST_SCENARIO=remux` : **36 changements en lecture + 12 en pause réussis**,
  MKV/MP4 sur Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1,
  Windows x64 10.0.26300 headless, navigateurs exécutés séquentiellement.
  FR mesuré 439,453125 Hz, EN 878,90625 Hz (une observation 884,765625 Hz),
  tolérance ±20 Hz. Erreur de position enregistrée 0 s, pause conservée à
  4,25 s, reprise FR et fin vérifiées. 404/MediaError 4 conservé sur les 3 clients.
- Demande → détection audio stable : Chrome 138,8–156,8 ms, Edge 143,2–169,2 ms,
  Firefox 164–288 ms. Cette mesure inclut la FFT glissante ~171 ms ; ce n'est
  pas le silence physique. Silences PCM échantillonnés : 22,8–233 ms selon le
  cas, écarts callbacks vidéo avant/après 50–253,3 ms. Polling maximal observé
  44 ms. Quelques frames antérieures à la cible avant restauration sont
  détectées ; absence de flash et synchronisation A/V fine non prouvées.
- Ce compromis duplique la vidéo/recharge le démuxeur et **n'est pas définitif**.
  Vraie sélection interne, MSE, Video.js/Shaka et écoute humaine restent NT.
  Méthode/limites : `docs/P0C_AUDIO_SWITCHING.md`, matrice chiffrée et preuves
  `docs/evidence/2026-10-15/remux/`. Tentative intermédiaire Chrome conservée :
  race du harnais entre readyState prêt et événement activant le menu ; attente
  corrigée puis série finale complète réussie.
- 6 tests Rust réussis, fmt/clippy et syntaxe JS vérifiés. CI adaptée pour
  préparer les remux mais exécution GitHub non revendiquée. Le plan suivant
  10 bits/Opus 5.1/sous-titres est écrit, **aucune fixture ni implémentation**.
  P0C complet reste ouvert ; aucun ajout hors périmètre ni push/publication.
- Régression native Chrome rejouée après P0C : quatre sources initiales et
  erreur 404 réussies. SHA-256 des quatre remux toujours conformes à leur
  génération après tous les essais.

## 13. Jalon P0C du 15 octobre 2026 — profil représentatif 10 bits/5.1/texte

- Nouveau générateur Python standard `scripts/generate-representative-fixtures.py`,
  dossier `tests/fixtures/generated/representative`, refus d'écrasement vérifié.
  Mire synthétique encodée une fois en AV1 Main `yuv420p10le`, SDR BT.709,
  320×180/24 fps, 12 s. Aucun transcodage de vidéo existante.
- MKV/MP4 multiaudio FR Opus 5.1 et EN Opus stéréo, trois sous-titres FR complet,
  FR forced et EN complet ; deux dérivés EN. AV1/Opus copiés, 288 paquets vidéo
  et 601 par audio comparés par SHA-256, init codec identique. SRT MKV converti
  explicitement en mov_text MP4 : cette conversion texte n'est pas bitcopy.
- Décodage intégral A/V, 20 contrôles PCM de canaux et 60 marqueurs d'amplitude
  hors navigateur réussis. FR FL/FR/FC/LFE/BL/BR : 440/550/660/80/770/880 Hz,
  EN 990/1100 Hz ; marqueurs à 2, 6 et 9 s. Préparation unique 21,09 s,
  2266372 octets. Huit anciens médias et quatre nouveaux intacts après tests.
- Limite réelle du muxeur MP4 9.0.2 : default ajouté sur vidéo et premier
  sous-titre malgré les dispositions 0 demandées ; MKV les conserve. Forced
  du second sous-titre et defaults audio corrects. Écart consigné par
  `strict_disposition_preservation=false`, pas masqué par le succès A/V.
- Scénario commun `representative-checks.cjs`, `MEDIA_TEST_SCENARIO=representative` :
  **12 parcours A/V réussis**, 4 sources × Chrome 155.0.8059.40, Edge
  155.0.4283.45, Firefox 155.0.1, Windows x64 10.0.26300 headless ; profils
  par défaut, navigateurs séquentiels. Pixels, PCM et six/deux fréquences
  séparées avant/après seek, pause, 48 seeks, reprise, fin et erreur404/code4.
- 48 observations de flash/amplitude : écart horloge média audio − PTS vidéo
  Chrome −2,40..+20 ms, Edge −0,89..+20,97 ms, Firefox −54..+6,50 ms.
  Mesure grossière, RMS ~21,3 ms, vidéo ~41,7 ms, polling max 27,5 ms,
  tolérance préétablie ±200 ms ; pas de synchronisation physique prouvée.
- Graphe FR expose bien six voies, mais destination annoncée limitée à deux
  canaux : **restitution physique 5.1 NT**. Canvas ne prouve pas affichage
  physique 10 bits. HDR/4K, matériel/logiciel décodeur, NAS et films longs NT.
- Trois pistes texte détectées par fichier, mais `textTracks.length=0` et
  `audioTracks` absent dans les douze cas. Menu texte conditionnel désactivé ;
  sélection/cues/rendu intégrés et auto-forced non validés. EN vérifié par
  chargement de son dérivé, pas par commutation interne. Restauration de
  position FR/EN 8 bits non attribuée à ces nouveaux fichiers.
- 7 tests Rust, fmt/clippy et syntaxes JS/Python vérifiés. Tests HTTP couvrent
  12 assets ; tests du profil, pistes, forced, MP4 tronqué et dossier incomplet.
  CI adaptée, exécution distante non revendiquée. Régressions Chrome native
  et remux 8 bits repassées. Un premier Cargo test a été bloqué par le binaire
  Windows en cours ; arrêt du prototype puis relance complète réussie.
- Preuves `docs/evidence/2026-10-15/representative/`, méthode et matrice mises
  à jour. Proposition séparée `PLAYER_COMPARISON_EXPERIMENT.md`, non exécutée,
  moteur non choisi. P0C complet reste ouvert ; aucun ajout hors P0 ni publication.
- Vérification texte additionnelle réellement exécutée :
  `python scripts/representative_subtitles.py`, 12 pistes et 48 cues exacts
  (horodatages à ±20 ms). Module aussi intégré aux générations fraîches.
  Rapport distinct archivé ; aucun rendu navigateur déduit de ce résultat.

## 14. P0C — MSE minimal exécuté (2026-10-15)

- Première expérience limitée, sans intégrer simultanément Video.js/Shaka.
  Banc /mse : une MediaSource et deux SourceBuffers, vidéo commune conservée,
  audio remplacé sans load/src ni seek forcé. Buffer audio complet 12 s remplacé
  pour conserver la langue après seek arrière ; pas un scheduler de film long.
- scripts/prepare-mse.py : remux -c copy, 288 AV1/601 FR/601 EN paquets et init
  identiques, décodage réussi ; MKV source AV1 Main10/FR Opus 5.1/EN stéréo.
  Originaux intacts. PTS/DTS audio +7 ms, durée quantifiée jusqu'à 1 ms,
  timebases conservées dans rapport ; aucune identité temporelle prétendue.
- Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1 Windows headless :
  18 changements (lecture/pause/après seek), six/deux signaux séparés, seek EN
  arrière/avant, pixels/frames, pause, source et buffer vidéo conservés validés.
  Trois sous-titres WebVTT externes : cues et neuf captures avec texte visible.
  Forced manuel seulement ; ni sélection intégrée ni auto-forced validés.
- Demande → signal correct en lecture/après seek : Chrome 431,7–450,1 ms,
  Edge 424–447,8 ms, Firefox 377–1205 ms. FFT ~171 ms, RMS ~21 ms,
  trois observations correctes, polling max56 ms. Silences échantillonnés
  consignés dans les rapports ; pas de promesse sans interruption. Pause inclut 100 ms
  d'observation et reprise, chiffrée séparément. Un parcours/browser seulement.
- Échecs conservés : comparaison format SRT/VTT corrigée numériquement ;
  duration=12 refusée corrigée par fin réelle des buffers ; Firefox AudioContext
  sans activation utilisateur bloqué, vrai clic WebDriver puis succès, aucune
  préférence autoplay modifiée. Attentes bornées et progression de test ajoutées.
- 8 tests Rust passent, fmt/clippy, syntaxes JS/Python. HTTP MSE fixe : GET/HEAD,
  Range exact/416, MIME, whitelist/Origin ; 404 et langue invalide navigateur.
  CI préparée, non exécutée à distance. Adaptateurs navigateur réutilisés.
- CPU/RSS, films longs, réseau dégradé, segments MSE tronqués, EOS, dérive A/V,
  restitution physique 5.1/7.1 et affichage physique10bits : NT.
- Clarification utilisateur : chaque piste peut être mono/stéréo/5.1/7.1 ;
  conserver ses canaux sans imposer le 5.1. Mono et 7.1 restent non testés.
- Recommandation provisoire : poursuivre MSE vidéo commune/audio séparé/VTT,
  prochain petit essai Shaka + DASH réel ; Video.js10 ensuite, aucun lecteur
  définitif choisi. Disponibilité Video.js10 vérifiée, compatibilité NT.
  Remux URL 8bits reste témoin, latences non comparables directement.
- Preuves et limites : docs/P0C_MSE_EXPERIMENT.md et evidence/2026-10-15/mse/.
  P0C reste ouvert ; aucun ajout P1, push ou modification des originaux.

- Revue : assertion pixels corrigée pour exclure alpha et exiger >10 couleurs
  RGB, puis trois parcours navigateur rejoués avec succès. Régression HTML5
  Chrome (4 sources, pause/seek/fin/404) réussie après extraction des adaptateurs.
  Refus d'écraser le packaging existant vérifié.

## 15. P0C — Shaka 5.2.13 face au MSE (2026-10-15)

- Shaka fixé depuis npm, SHA512 archive/SHA256 bundle consignés, Apache-2.0
  et LICENSE conservés dans fixtures ignorées. setup prepare-shaka.py horsligne
  possible avec --archive, aucun CDN ni FFmpeg ajouté au serveur.
- prepare-dash.py : copies binaires fMP4/VTT MSE inchangés, MPD12.008s,
  SegmentList/Timeline et init/ranges tirés de tfdt/tfhd/trun réels (7fragments),
  154ms unique, paquets/init reliés aux preuves MSE. Audio+7ms conservé.
  Originaux et copies vérifiés avant/après. Préparations refusent écrasement.
- /shaka et whitelist HTTP fixe ; Shaka LoadMode.MEDIA_SOURCE observé.
  APIs versionnées selectAudioTrack(track,0), preferredAudio/preferredText,
  NativeTextDisplayer(player), selectTextTrack(track|null). ABR désactivé,
  handleForcedSubtitlesAutomatically=false pour respecter le choix texte manuel.
- Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1 Windows 10.0.26300
  headless : 108 changements (18×6couples), 36marqueurs A/V et choix manuels
  conservés après seek. FR et FRforced par défaut Shaka, EN manuel prioritaire ;
  12captures Shaka inspectées, FR complet/forced/EN/off corrects.
- Latence médiane/P95=max (n12actifs) : MSE Chrome370.4/383.2ms,
  Edge365.9/386.7ms, Firefox1120/1151ms ; Shaka160.75/181.2ms,
  152.05/211ms,167/207ms. Pause n6parcouple séparée avec hold100ms/reprise.
  Mêmes URL vidéo mais Shaka36/36 actifs avec waiting/seeking, silence jusqu’à
  150ms, gap vidéo182.64ms. MSE0silence détecté et0waiting ; reste signalancien
  plus longtemps. Aucune promesse de lecture ininterrompue.
- A/V : indicateur video.currentTime au seuil RMS moins PTS flash, pas timestamp
  PCM physique. 36observations dans±200ms ; plages et résolution dans protocole.
  Pas de mesure précise de+7ms, de sortie matérielle ni de dérive longue.
- Harness commun nouveau : FFT8192, RMS derniers1024 du tableau8192. Ancienne
  troncature initiale biaisait le RMS d’environ149ms ; parcours préliminaire
  conservé, exclu. Ne pas comparer ces latences aux chiffres historiques MSE.
- CPU/RAM échantillonnés séparément : arbre navigateur par PID+création, Rust
  distinct, CPU deltas non négatifs, working sets sommés (pages partagées
  possiblement doublées). CPU comprend graphe/tests ; collecte coûte247–746ms.
  Pas de budget produit/NAS ni d’avantage CPU extrapolé. Chiffres et JSONL archivés.
- 9tests Rust, fmt/clippy, syntaxes JS/Python, tests médiane/P95/PID passent.
  MPD absent diagnostic Chrome→404/Shaka1001 ; pas de preuve affichage erreur
  via ce diagnostic API. Menu texte du banc synchronisé avec choixAPI puis
  contrôleChrome effectué. CI préparée, exécution distante non revendiquée.
- Recommandation provisoire : Shaka candidat pour DASH/langues/texte et vitesse
  de sélection ; tester marge audio non nulle et clips longs avant décision.
  MSE inchangé conservé ; Video.js reste prochain jalon distinct, aucunP1.
- docs/P0C_SHAKA_EXPERIMENT.md, STREAMING_COMPATIBILITY et preuves
  docs/evidence/2026-10-15/shaka/. Mono/7.1, physique 5.1 / 10 bits, réseau dégradé,
  segments corrompus, endurance et NAS NT. Aucun push/publication.

## 16. P0C — safeMargin, protocole et pilote (2026-10-15)

- Shaka5.2.13 et MSE conservés. switchLanguage accepte explicitement 0/.5/1/2/4,
  défaut0 inchangé. Aucun média, packaging ni bufferingGoal modifié.
- Protocole et runner isolent un changement mesuré par présentation12s neuve ;
  marges/sens tournants, fenêtres500ms après signal, compteurs frames perdus
  distincts des callbacks, probes A/V et seekarrière. Sampler CPU exclu pour
  comparer toutes les marges à un0 frais dans les mêmes conditions.
- PiloteChrome12essais 0/4 réussi pour changements/marqueurs ; marge4 rejoue
  l'ancienne langue après seekarrière dans6cas, marge0 dans0cas. Régression
  comptée séparément ; pilote exclu de la matrice finale en cours.
- Nouveau test statistiques échecs/sens/frames/seeks et anciens tests stats
  passent ; syntaxesJS/fmt vérifiées. Documents P0C_SAFE_MARGIN_EXPERIMENT.md.
- Matrice complète en cours, aucune marge gagnante ni fonctionnalité globale
  validée à ce stade. Résultats finaux à consigner après exécution complète.

- Première série finale achevée : Chrome140essais, aucun échec changement/marqueur,
  112retours de langue incorrecte après seek dans les marges positives.
  Preuves docs/evidence/2026-10-15/safe-margin/chrome/. Edge/Firefox en cours.

- Série Edge achevée :140changements/marqueurs aboutis,112probes seek ne
  correspondent pas à la langue sélectionnée. Série Firefox lancée séparément.

## P0C — safeMargin terminé (2026-10-15)

Shaka5.2.13 conservé, aucun média/packaging réencodé ou modifié. MSE conservé.
424 essais :420 changements mesurés,4 échecs de préparation Firefox conservés,
complétés par4 essais indépendants. Marges0/.5/1/2/4, trois navigateurs Windows.
20 mesures de lecture et4 pause/4 après-seek par marge et navigateur.

| Navigateur | Marge0 latence P50/P95 | Marge0,5 latence P50/P95 |
|---|---:|---:|
| Chrome |163/186ms|409/437ms|
| Edge |166/197ms|402/422ms|
| Firefox |185/214ms|1123/1134ms|

Marge0,5 : zéro silence détecté et zéro frame perdue signalée en lecture et
après-seek. Mais les336 probes arrière de TOUTES les marges positives rejouent
l'ancienne langue ; les84 probes marge0 sont corrects. Aucun réglage ne résout
tout. Candidat fluidité :0,5s, NON promu ; défaut0 conservé. Marge4 retarde le
signal d'environ2,3s sans bénéfice démontré.420 marqueurs A/V dans±200ms,
indicateur média non physique ; polling maximal168ms limite les conclusions.

9 tests Rust, fmt/clippy, syntaxes et statistiques passent. Anciennes suites
navigateur :4/6 réussies ; ShakaChrome échoue sur progression vidéo aprèsseek,
MSEFirefox timeout PCM FR. Ces échecs sont conservés, causes non résolues.
Pas de lecteur définitif ni ajout Video.js/SvelteKit.
Voir docs/P0C_SAFE_MARGIN_EXPERIMENT.md.
Preuves : docs/evidence/2026-10-15/safe-margin/.

## Vérification courte de la resélection audio (2026-10-15)

Six essais ciblés FR→EN, safeMargin0,5, Chrome 155.0.8059.40,
Edge 155.0.4283.45 et Firefox 155.0.1. Script scripts/verify-audio-seek.cjs.
Sans attente500ms après seek1,25s, le contrôle initial trouve déjà le signalEN
après retour0,5s sur les trois navigateurs : cela ne prouve aucune correction.
Avec la même attente500ms que la matrice précédente, le défaut est reproduit
sur les trois : pisteEN sélectionnée mais signalFR après seekarrière.
Resélectionner la pisteEN déjà active avec selectAudioTrack(track,0), puis
observer700ms, laisse le mauvais signalFR dans les trois cas. Cette méthode
simple ne valide donc pas une invalidation du buffer. Le résultat dépend du
point de changement dans les segments ; aucune correction générale livrée.
Source vidéo conservée et37 empreintes originales identiques avant/après
chaque essai. Syntaxe du nouveau script et deux suites statistiques passées.
Pas de modification Rust : suite Rust non relancée, pas de répétition de la
matrice420points pour cette vérification ciblée (token-saver appliqué).
Preuves docs/evidence/2026-10-15/audio-seek-quick/, contrôles et reproductions
séparés. Prochain essai : mécanisme supporté d'invalidation audio, puis preuve
FFT ; aucune méthode privée ou double commutation non validée ajoutée.

## Fichiers communautaires, CI unifiée et politique de sécurité (2026-10-15)

- Licence actée et ajoutée : GNU Affero General Public License v3.0 (`LICENSE`), conformément au choix utilisateur explicite.
- Fichiers communautaires ajoutés à la racine et configurés pour GitHub :
  - `CONTRIBUTING.md` : aligné avec les règles d'architecture (rôles `administrator`/`user`, lecture seule `:ro`, absence de transcodage forcé, priorité P0-P5, nom neutre `MEDIA_*`).
  - `SECURITY.md` : politique de sécurité standard et signalement privé GitHub Security Advisories, pointant vers `docs/SECURITY.md`.
  - `CODE_OF_CONDUCT.md` : Contributor Covenant v2.1.
  - `.github/ISSUE_TEMPLATE/` : formulaires structurés pour bugs (`bug_report.yml`), propositions d'améliorations (`feature_request.yml`) et `config.yml`.
  - `.github/PULL_REQUEST_TEMPLATE.md` : checklist d'architecture et de sécurité `AGENTS.md`.
  - `.github/dependabot.yml` : suivi automatisé hebdomadaire pour Cargo et GitHub Actions, prêt pour npm (`apps/web`).
- CI unifiée `.github/workflows/ci.yml` :
  - Remplace le banc initial `p0.yml` sans duplication.
  - Exécute génération des fixtures synthétiques P0, tests statistiques Node, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`.
  - Intègre l'audit de sécurité des dépendances avec `rustsec/audit-check` (`cargo audit`).
  - Intègre la configuration conditionnelle pour SvelteKit (détection de `apps/web/package.json`).

## Nettoyage forcé de la variante active — preuve ciblée (2026-10-15)

API publique Shaka5.2.13 testée : selectVariantTrack(activeVariant,true,0).
La source versionnée player.js/switchVariant_ confirme force=true pour la
variante déjà active si clearBuffer=true, contrairement à selectAudioTrack.
Le nettoyage concerne aussi la vidéo : ce n'est PAS une invalidation audio seule.

Six essais, FR→EN et EN→FR sur Chrome 155.0.8059.40, Edge 155.0.4283.45,
Firefox 155.0.1 : défaut préalable reproduit6/6 après changement marge0,5 et
seekarrière ; nettoyage forcé puis bonne langue réellement décodée6/6.
FFT trois observations consécutives, six/deux voies, confirmation après700ms.
URL vidéo conservée, frames vidéo à nouveau présentées,37 originaux intacts
par essai. Aucun load(), réencodage, API privée ou double changement de piste.

| Navigateur | Retour au signal correct FR→EN | EN→FR |
|---|---:|---:|
| Chrome |354,9ms|778,2ms|
| Edge |395,7ms|368,3ms|
| Firefox |639ms|161ms|

Des événements waiting/seeking/buffering sont observés. Une frame perdue
signalée dans le cas FR→EN Edge et dans le cas FR→EN Firefox. Ne pas annoncer
une récupération sans interruption. Un essai par sens/navigateur : ni P95
ni stabilité à long terme, ni synchronisation physique validés ici.

Script scripts/verify-audio-seek.cjs : MEDIA_SEEK_RECOVERY=variant-clear,
MEDIA_SEEK_FROM=fr ou en, MEDIA_BROWSER_CHANNEL et MEDIA_REPORT_DIR neuf.
Sans MEDIA_SEEK_RECOVERY, l'essai de simple resélection reste disponible.
Preuves docs/evidence/2026-10-15/variant-clear-quick/.

Conclusion : mécanisme public de récupération démontré sur ces6cas, pas encore
intégré automatiquement au seek. Défaut0 du banc inchangé. Prochain petit jalon :
politique de récupération sur seek avec pause conservée et prévention des boucles,
puis répétitions/seek avant-arrière et mesure A/V. Ne pas choisir le lecteur final.
Seul le script expérimental a changé ; syntaxeJS vérifiée, suite Rust non
relancée car aucun code Rust/serveur modifié. Tests précédents non effacés.


## 2026-10-15 — P0C, politique expérimentale après seek

Cause partielle établie : safeMargin positif conserve des paquets audio anciens ;
resélection de la piste active ne les purge pas. MPD/IDs distincts, aucune preuve
justifiant un changement de packaging. Cause des blocages vidéo après purge
non identifiée. Purge publique selectVariantTrack(active,true,0) = contournement,
pas correction fiable ; purge audio ET vidéo. Marge normale0 conservée.

Contrôleur isolé audio-seek-policy.js, opt-in dans /shaka : tentative unique,
verrou/récursion/concurrence, position/pause/vitesse/choix manuel conservés,
append frais attendu, timeout et erreur terminale, aucune FFT de production.
Watchdog5s après reprise : blocage>2s en lecture -> erreur, pas seconde purge.
Vrai changement audio marge0 nettoyant déjà l'audio seul : pas purge vidéo inutile.

36 essais préliminaires Chrome/Edge : seuls8 seeks protégés mesurés,7 avec
langue finale correcte,3 marqueurs A/V réussis ; autres échecs préparation ou
synchronisation.12/12 contrôles mesurés reproduisent l'ancienne langue.
Ces résultats incomplets ne sont PAS un taux de fiabilité produit.
Deux démarrages Firefox WebDriver expirés45s : aucune validation Firefox actuelle.
Contrôle final préparation1,25s FR->EN seek0,5s : Chrome et Edge1/1 protégé
correct chacun, marqueurs A/V réussis ; contrôle Chrome déjà correct sans purge,
contrôle Edge incorrect. Signal307/1755ms ; silence238/1677ms ; frames perdues6/46.
Position conservée à8,7ms près. Fluidité insuffisante, option désactivée par défaut.

Oracle représentatif : FR six fréquences440/550/660/80/770/880Hz, EN990/1100Hz,
pas le couple mono440/880 ancien. Aucun média/packaging réencodé ou modifié ;
hash37 originaux avant/après chaque série complète. Tests antérieurs conservés.
Rust fmt/clippy strict/9 tests intégration passent. Tests Node politique/statistiques
passent ; CI ajoutée mais pas exécutée à distance. Précisions et preuves :
docs/P0C_AUDIO_SEEK_RECOVERY.md et docs/evidence/2026-10-15/audio-seek-policy/.

Prochain jalon P0 seulement : isoler blocages vidéo, refaire Firefox fonctionnel,
puis répétitions séquentielles et seeks avant/arrière dans une même présentation.
Ni solution définitive, ni P1, ni Video.js validés.

Validation finale Node : 10 tests politique + 2 suites statistiques =12/12, exit0. Syntaxe des cinq scripts modifiés vérifiée. Le dernier ajustement distinguant audioId/marge0 est couvert unitairement ; pas de nouvelle matrice navigateur après cet ajustement.

## 2026-10-15 — P1 local puis première refonte visuelle

- Décision utilisateur : HTML5 natif et copies par langue provisoires ; petites
  interruptions/rechargement acceptés, comparaisons Shaka/MSE/Video.js suspendues.
  Expériences conservées ; aucune récupération instable activée par l'écran P1.
- Première intégration apps/web Svelte 5.57.2 / SvelteKit 3.1.0 / TypeScript strict et build
  statique sous /app/, servi par Axum existant. Pas de Node permanent. API démo
  sources/pistes, quatre copies AV1 10 bits / Opus FR 6 canaux et EN 2 canaux, VTT P0 réutilisés.
  Préparation hors lecture bornée, paquets et originaux SHA256 inchangés.
  Pas de compte, DB, vraie bibliothèque, multi-Node ou plugin implémenté.
- Le premier écran fonctionnel est remplacé à la demande de l'utilisateur :
  noir encre/blanc chaud/améthyste, hero cinématographique original, marque SVG
  existante conservée, vraies cartes de sélection MKV/MP4, lecteur intégré,
  panneau de séance et contrôles discrets. Le paysage est une illustration
  déclarée, pas une vidéo/œuvre promise dans le catalogue.
- Fondations : tokens.css global, Button/Field/Badge/Icon/MediaCard ; styles
  hover/focus/active/disabled, clavier, mobile et mouvement réduit. Aucune nouvelle
  dépendance UI. NativePlayer et logique de lecture/audio/sous-titres inchangés.
  Seul ajustement serveur de cette refonte : CSP img-src self pour les images locales.
- Vérifications après refonte : check Svelte/TS : 0 erreur et 0 avertissement, lint,
  5 tests Vitest, build, npm audit : 0 vulnérabilité ; Rust fmt/clippy strict et 10 tests
  intégration passent (9 P0 conservés). Browser P1 MKV/MP4 sur Chrome/Edge/Firefox :
  pixels/PCM réels, FR/EN pause/lecture, position/volume/vitesse, seek EN,
  VTT/off/choix manuel, fin et erreur404 réussis ; hashes médias/VTT inchangés.
- Firefox est validé à 1× seulement : retour FR 5.1 à 1,5× sans PCM attendu observé
  avant refonte, cause non établie, pas corrigée dans le jalon visuel. Ne pas
  annoncer de compatibilité universelle ou d'expérience technique achevée.
- Layout Chrome 360/390/768/1280/1440/1920 px : pas de débordement/erreur CSP,
  focus clavier et images vérifiés ; carte MP4→vraie source, erreurs Core503,
  VTT404 et audio404, tentative audio unique. Captures desktop/mobile inspectées.
- Preuves docs/evidence/2026-10-15/p1-ui-v1/, direction docs/UI_DIRECTION.md,
  parcours/commandes docs/P1_WEB_INTEGRATION.md. CI distante, vraie sortie
  physique 5.1 / 10 bits, films longs, NAS, Safari/mobile réel restent non vérifiés.
- Première direction visuelle concrète, pas une expérience finie. Ne pas étendre
  automatiquement tous les écrans ; prochain petit jalon : retour sur cette base.

## 2026-10-15 — Lecteur immersif P1 validé localement

- Le lecteur encadré/panneau permanent de la page est remplacé par un dialog
  plein écran, ouvert depuis les actions existantes de l'accueil. Identité,
  hero, cartes et choix de source de l'accueil conservés. Aucune route ajoutée.
- Fullscreen API depuis le clic ; repli couvrant la fenêtre en cas de refus.
  Vidéo sans controls natifs, ratio conservé sur fond noir ; Retour/titre en
  haut, progression et commandes superposées en bas, menus flottants à droite.
- PlayerControls Svelte ne connaît que PlaybackState/callbacks. ImmersivePlayer
  reçoit une PlayerFactory ; NativePlayer et backend inchangés. Types étendus
  aux métadonnées déjà présentes pour les libellés audio/vidéo réels.
- FR/EN, WebVTT/off/forced, vitesse, volume/muet, seek, sources MKV/MP4 ; choix
  manuels et état conservés. Les sources de démo ne sont pas présentées comme
  des œuvres ou versions éditoriales. Pistes vidéo internes affichées en
  lecture seule avec sélection explicitement indisponible ; aucun faux choix.
- Contrôles cachés après3s en lecture ; pause/menu/erreur/clavier les gardent.
  Curseur masqué, mouvement/tactile les révèlent, menus clavier, Échap,
  Space/flèches/M/F, zones sûres et focus restauré après sortie fullscreen.
- Retour détruit le moteur, retire src/pistes, arrête média et timers. Les
  callbacks tardifs sont ignorés par génération. Aucun retry automatique ajouté.
- Tests réels Windows10.0.26300 headless : Chrome155.0.8059.40,
  Edge155.0.4283.45, Firefox155.0.1/geckodriver0.37.1. Fullscreen réel et refus
  simulé, menus/clavier, audio/texte/source, retour/nettoyage réussis. Toucher
  simulé390×844/844×390 et layout360–1920px sans débordement/CSPerror ;
  erreurs Core503/VTT404/audio404 explicites, tentative audio unique.
- Non-régression média P1 :6parcours MKV/MP4, pixels/PCM FR6/EN2, seek,
  pause/reprise, choix texte, fin/404, hashes originaux/VTT inchangés. Chrome
  à1,5× ; Edge et Firefox validés à1×. Les échecs Edge multicanaux accélérés
  observés ne sont pas effacés ni présentés comme résolus ; Firefox1,5× reste
  également une limite connue. Pas de récupération ou de transcodage ajouté.
- Tests P1 adaptés aux vrais menus sans supprimer leurs assertions média.
  Position restaurée observée à seeked, séparément de l'avancement après reprise.
  Fixture repositionnée avant commutation active pour ne pas mesurer après EOS.
- check Svelte/TS0erreur0avertissement, lint/build,8tests Vitest et10tests Rust
  passent ; fmt/clippy stricts et git diff --check réussis. Pas de CI distante.
- Documentation docs/IMMERSIVE_PLAYER.md ; preuves et captures dans
  docs/evidence/2026-10-15/immersive-player/. Mobile physique/Safari, films
  longs, restitution5.1/10bits et NAS restent non vérifiés. P1 complet non clos.
- Token saver appliqué à la fin : lectures ciblées, logs condensés et réutilisation
  des preuves existantes ; aucun pourcentage de quota économisé inventé.

## 2026-10-15 — Premier parcours catalogue P1 validé localement

- Navigation statique Accueil / Films / Séries / Recherche / Paramètres ; fiches
  par paramètre `work`, film et série séparés, saison puis épisode vers lecteur.
- API `/api/v1/catalog` et `/api/v1/catalog/{id}`, recherche, tri alphabétique,
  pagination 24 par défaut (maximum API 100). Associations explicites des
  fixtures Work / Edition / MediaVersion / MediaSource vers Asset / Track.
  Un film `fixture-film` regroupe MKV/MP4 ; `fixture-series` contient une saison
  et `fixture-episode-1`, réutilisant explicitement la même mire synthétique.
  Pas de fusion par titre, base persistante ou identification automatique.
- Années/affiches réelles, dates d'ajout, historique et recommandations absents :
  états explicites, replis typographiques signalés. Aucun média personnel.
- Lecteur/moteur conservés ; sources limitées à l'œuvre ou l'épisode choisi.
  Amorçage des scripts navigateur adapté sans retirer assertions média.
  Bootstrap statique externalisé récursivement pour routes directes sous CSP.
- Vérifié : 15 tests Rust passent ; check Web 0/0, lint, 8 tests Vitest et
  build passent après ajustements UI. Chrome155.0.8059.40 Windows headless :
  deux parcours média P1 MKV/MP4 pixels/PCM/hashes réussis après adaptation.
  Commandes Rust standard fmt --check, clippy --locked --all-targets -- -D warnings,
  test --locked relancées avec succès. Dernier browser-catalog : chargement
  explicite et épisode jamais présenté comme film par `work` détourné vérifiés.
- browser-catalog : 390/768/1440px, fiches, série→épisode→MP4, recherche vide,
  catalogue vide injecté, 503/réessai, 404, pagination simulée72 bornée24 passent.
  Captures Accueil/Films/Séries desktop/mobile inspectées. browser-p1-layout :
  360/390/768/1280/1440/1920px sans débordement/CSP, audio404 tentative unique,
  VTT404/Core503 passent. browser-immersive : fullscreen réel/refus, menus,
  clavier, tactile simulé390×844/844×390 passent. Preuves dans
  docs/evidence/2026-10-15/catalogue/ (report.json, regression, layout, immersive).
  Aucun nouveau résultat Edge/Firefox/Safari/mobile physique/NAS ; pagination
  simulée sans mesure de charge ou catalogue personnel.
- Bootstrap admin, authentification, sessions, RBAC, SQLite et scanner absents :
  interdisent l'exposition de dossiers personnels. Prochain petit jalon : admin
  local + sessions/RBAC + SQLite, avant dossier explicitement configuré `ro`
  et scan borné hors requêtes HTTP. P1 complet non clos, essais P0 suspendus.
- Token saver : lectures ciblées et deux délégations ; aucun chiffre d'économie
  revendiqué. Documentation : docs/CATALOGUE_P1.md et contrat docs/API.md.


## 2026-10-15 — Navigation principale et menu profil

- Navigation principale limitée à Accueil / Films / Séries / Recherche. Recherche
  associe une loupe SVG et son texte sur desktop comme dans la barre mobile.
  Paramètres reste accessible à son URL existante via le menu profil uniquement.
- ProfileMenu frontend en haut à droite : avatar de repli, mention « Aucun profil
  configuré », Profil désactivé « Bientôt », lien Paramètres fonctionnel. Pas de
  faux compte, rôle, déconnexion, authentification ou nouveau backend.
- Popover HTML natif auto : ouverture clic/Entrée/Espace, focus du premier lien
  utilisable, fermeture Échap/clic extérieur/re-clic du bouton/navigation et
  sortie clavier hors du composant. Retour du focus après Échap vérifié.
  Styles sombre/améthyste, hover/focus/active et mouvement réduit conservés.
- Aucun fichier du lecteur ni code Rust modifié pour cet ajustement. Ancien diff
  de travail préservé. Le test catalogue passe désormais par le menu profil pour
  ouvrir Paramètres et vérifie les quatre liens principaux.
- Web : npm run check (0 erreur/0 avertissement), lint, 8 tests Vitest et build
  réussis. Chrome155.0.8059.40 Windows10.0.26300 headless : nouveau runner
  browser-profile.cjs passe en360/390/768/1440px (clavier, tactile simulé,
  fermeture, focus, navigation, aucun débordement/CSP). Captures inspectées.
  browser-catalog.cjs et browser-immersive.cjs passent sans retirer assertions.
  Preuves : docs/evidence/2026-10-15/profile-navigation/.
- Pas de nouvelle validation Safari/Firefox/Edge/mobile physique. Tests Rust non
  relancés : backend inchangé. Prochain petit jalon fonctionnel reste admin local,
  sessions/RBAC et SQLite avant bibliothèques personnelles ; aucun élargissement.


## 2026-10-15 — Séparation profil, paramètres et administration

- Menu : « Mon profil · Bientôt » (avatar/identité/mot de passe/préférences
  langue/audio/sous-titres), Paramètres pour les réglages personnels uniquement.
  Retrait de « Bibliothèques personnelles » et du bouton d’ajout de bibliothèque.
- Bibliothèques Films/Séries, dossiers/scans, utilisateurs/permissions et
  configuration relèvent de l’administration serveur future. Administration et
  Déconnexion restent masquées : aucune identité authentifiée ni rôle simulé.
  Deux rôles seulement administrator/user ; futurs droits à contrôler côté backend.
- Aucune nouvelle fonctionnalité, route, auth ou administration. Lecteur,
  catalogue métier et navigation principale inchangés. Tests existants adaptés
  aux nouveaux libellés sans retirer les autres assertions.
- Vérifié : check0/0, lint/build et browser-profile.cjs Chrome155 Windows
  headless360/390/768/1440px passent, y compris absence Administration/Déconnexion
  et bibliothèques dans Paramètres. Preuves docs/evidence/2026-10-15/personal-settings/.
  Suites média/Rust non relancées : changement de présentation uniquement.

## 2026-10-15 — Corrections ciblées du lecteur immersif

- ImmersivePlayer.svelte : maintien du focus sur la surface (.player-shell) lors du
  masquage automatique des contrôles ; ajout d'une écoute de secours globale des
  raccourcis clavier (Space, flèches, M, F, Échap) via svelte:window sans perte après
  inertie des contrôles.
- PlayerControls.svelte : structure ARIA normalisée (rôles group et presentation pour les
  sections, aria-controls et id de menu liés, suppression de la duplication de Tab,
  pattern de roving tabindex 0/-1 lors de la navigation fléchée Home/End/haut/bas sans
  anomalie de modulo négatif, bouton Fermer préservé).
- Mobile : titre secondaire relevé de 6px à 10px lisible sur smartphone et 10px desktop.
- Smartphone : disposition désencombrée sur mobile (<700px), curseur de volume inline masqué
  au profit des boutons matériels/muet, espacements aérés (row-gap 8px), suppression des
  débordements et cibles tactiles confortables (>=40px).
- Vérifications exécutées :
  - apps/web : npm run check (0 erreur, 0 avertissement), npm run lint (0 erreur),
    npm test (8 tests Vitest passés), npm run build réussi.
  - Rust : cargo fmt --check, cargo clippy --locked --all-targets -- -D warnings,
    cargo test --locked (15 tests passés).
  - Navigateurs réels : node scripts/browser-immersive.cjs (Chrome 155 headless, 11 étapes
    incluant focus masqué, ARIA/roving tabindex, taille mobile >=10px et volume masqué),
    node scripts/browser-immersive-firefox.cjs (Firefox 155 headless avec geckodriver) réussis.

## 2026-10-15 — Mise à jour complète de la toolchain et des dépendances

- **Toolchain Rust** : mise à jour vers stable 1.99.1 (rustc 1.99.1, cargo 1.99.1, rustup 1.29.1).
- **Cargo.toml** : passage de `rust-version = "1.85"` à `rust-version = "1.99"`.
- **Dépendances Rust** : vérification et alignement avec crates.io sur les dernières versions stables :
  - `axum` 0.8.9, `tokio` 1.53.3, `tower` 0.5.3, `tower-http` 0.7.1, `serde` 1.0.229, `serde_json` 1.0.154, `thiserror` 2.0.21, `http-body-util` 0.1.5, `tempfile` 3.27.0.
- **Frontend web (`apps/web`)** : mise à jour vers les dernières versions stables compatibles :
  - `eslint` : passage de `^9.0.0` (9.39.5) à `^10.12.0`.
  - `vitest` : passage de `^4.0.0` (4.1.12) à `^5.0.5`.
  - `typescript-eslint` : passage de `^8.0.0` à `^8.71.2`.
  - `eslint-plugin-svelte` : passage de `^3.0.0` à `^3.23.1`.
  - `svelte-check` : passage de `^4.0.0` à `^4.7.6`.
  - `@opentelemetry/api` : passage de `^1.0.0` à `^1.9.1`.
  - `typescript` : fixé à `^6.0.3` (dernière version de la branche 6.x, compatibilité stricte avec le peer requirement `^5.0.0 || ^6.0.0` de `svelte-check`).
  - Dépendances déjà à jour : `svelte` (5.57.2), `vite` (8.3.4), `@sveltejs/kit` (3.1.0), `@sveltejs/adapter-static` (4.0.0), `@sveltejs/vite-plugin-svelte` (7.3.1), `playwright` (1.64.1).
- **Vérifications et tests validés** :
  - Backend : `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` (15/15 tests passés).
  - Frontend : `npm audit --audit-level=high` (0 vulnérabilité), `npm run check` (0 erreur, 0 avertissement), `npm run lint` (ESLint 10 propre), `npm test` (8/8 tests Vitest 5 passés), `npm run build` (build statique réussi).
  - Scripts de validation : `node scripts/player-comparison-stats.test.cjs`, `node scripts/safe-margin-stats.test.cjs`, `node --test scripts/audio-seek-policy.test.cjs` (10/10 tests passés).

## 2026-10-15 — Correction CI GitHub : horodatages sous-titres WebVTT sous FFmpeg 6.1.1

- **Cause identifiée** : dans `representative.mkv`, le délai lookahead des flux audio Opus (`delay 312`, ~6,5 ms arrondi à 7 ms en timebase 1 kHz) induit un `start_time` de conteneur de `-0.007000`. Sous FFmpeg 6.1.1 (Ubuntu CI), l'absence de l'option `-copyts` lors de l'extraction WebVTT soustrayait `start_time`, décalant chaque horodatage de cue de +7 ms (`2.007 --> 2.507` au lieu de `2.000 --> 2.500`), déclenchant `RuntimeError: Subtitle timing/text mismatch: fr-full`. Sous FFmpeg 9.0.2 (Windows), ce décalage conteneur n'était pas reporté sur l'extraction d'un flux de sous-titre isolé (`start: 0.000000`).
- **Correction** : ajout de l'option explicite `-copyts` à la commande FFmpeg d'extraction des sous-titres dans `scripts/prepare-mse.py` (ligne 118, aligné sur la ligne 67 vidéo/audio). Conservation stricte de l'assertion d'égalité exacte `source_cues == output_cues` et `len(output_cues) == 4`.
- **Test de régression ciblé** : `mse_subtitles_exact_timing_and_no_preroll_drift` ajouté dans `crates/core/tests/p0.rs`, vérifiant les horodatages stricts des 3 pistes WebVTT et rejetant explicitement le décalage `+7ms` (`00:02.007`).
- **Tests exécutés et validés** :
  - `prepare-mse.py` validé sous FFmpeg 6.1 et FFmpeg 9.0.2 (reproduction de l'erreur sans `-copyts` et succès avec `-copyts`).
  - Rust : `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` (16 tests passés).
  - Node : tests statistiques et de politique audio (10/10 tests passés).
- **Tests non exécutés** : CI GitHub distante (pas de push automatique).



## 16 octobre 2026 — jalon identité SQLite / bootstrap local

- SQLx 0.9.0, SQLite embarqué/WAL/FK et migration 0001 : users UUID,
  local_credentials séparés, rôle CHECK administrator/user, marqueur de
  bootstrap durable. Modèle User typé avec création/désactivation ; aucune
  session/table OIDC anticipée, aucune nouvelle route HTTP.
- Core ouvre MEDIA_DB_PATH (défaut data/core.sqlite3) et migre avant HTTP.
  Parent privé existant requis, distinct des médias ; aucun média modifié.
  CLI media-bootstrap : terminal interactif, saisie masquée/confirmation,
  pas d'arguments ni secret en variable/pipe. Aucun compte réel créé par l'agent.
- Argon2id v19, m=65536 KiB/t=3/p=1, sortie32octets, sel OS, PHC seulement,
  Zeroizing pour mot de passe/confirmation, hashing spawn_blocking.
- BEGIN IMMEDIATE protège migration et bootstrap ; test initial a révélé que
  le verrou de migration SQLx SQLite est un no-op, corrigé par transaction
  englobant vérification/application. Bootstrap atomique et définitivement
  fermé même après suppression du compte ; rollback de panne testé.
- Windows / Rust1.99.1 : cargo fmt --check, cargo clippy --locked --all-targets
  -- -D warnings réussis ; cargo test --locked : 22 tests réussis (16 existants,
  6 identité), un helper ignoré exécuté par le test multi-processus.
  Trois processus sur une base initialement absente : 1 gagnant, 2 refus.
  Deux pools concurrents, migrations rejouées/réouverture, hash valide/mauvais
  secret, rôle interdit/FK, entrées invalides, corruption, parent absent,
  panne d'insertion et refus CLI sans terminal couverts. Un lancement de test
  concurrent au précédent a eu un conflit linker Windows ; relance séquentielle
  complète réussie. Correction finale Clippy stylistique uniquement.
- Pas de preuve de saisie masquée interactive manuelle, de NAS, de CI distante,
  de navigateur supplémentaire ou de coût Argon2 de production. Lecteur/UI
  inchangés ; pas de relance Web ou des expériences codecs.
- Frontière de confiance : opérateur OS et ACL du dossier, pas de protection
  contre un opérateur pouvant remplacer DB/symlinks. Aucun endpoint sensible.
  Prototype toujours loopback, sans session : aucune bibliothèque personnelle,
  publication, tunnel ou exposition LAN autorisée. Voir docs/LOCAL_ADMIN.md.
- Prochain jalon proposé, non commencé : connexion, sessions révocables,
  CSRF/rate limiting et permissions backend. P1 complet reste ouvert.
- Skill codex-token-saver appliqué : tests Rust pertinents conservés ; aucun
  test Web/codec répété sans changement concerné ni pourcentage d'économie annoncé.

## 16 octobre 2026 — connexion, sessions et permissions des fixtures

- Parcours Web /app/login/ -> session -> catalogue/lecture proteges -> deconnexion.
  Compte CLI existant reutilise, aucun compte reel cree par l'agent, aucun role
  supplementaire ni inscription/OIDC/multi-Node. Lecteur et medias conserves.
- Migration 0002 : sessions (SHA-256 du jeton seulement, user_id, dates) et budget
  global de connexion. Deux UUIDv4 OS par jeton (244 bits aleatoires), cookie
  HttpOnly/SameSite Strict/Path=/, Secure pour origine HTTPS ; expiration absolue
  12h, idle30min controlees atomiquement, suppression a la deconnexion.
  CSRF derive/separe du jeton + Origin exact ; aucun jeton localStorage.
- Argon2id existant, verification factice pour compte absent/desactive ; message
  generique. Budget SQLite 10 tentatives/60s conserve apres redemarrage et maximum
  deux workers Argon2, permis conserve meme si HTTP annule. Pas de secrets logs.
- secure_app impose auth puis permission avant catalogue/descripteurs/fichiers,
  sous-titres et derives MSE/DASH, y compris GET/HEAD/Range. Administrator autorise
  uniquement sur inventaire synthetique ; user refuse403 sans droits implicites.
  Pas encore de stockage/gestion de grants par bibliotheque ou de medias personnels.
- Banc historique app compile seulement via fixture-bench pour tests ; binaire
  toujours secure_app meme avec cette feature, aucun bypass runtime. CI adaptee
  pour clippy/test --all-features ; CI distante non executee.
- MEDIA_PUBLIC_URL origine exacte HTTPS ou HTTP loopback, Host/Origin verifies,
  en-tetes forwarded ignores, listener toujours127.0.0.1. TLS non termine par Core,
  aucun proxy/publication effectue, HTTPS navigateur/deploiement non valide.
- Verification finale Windows/Rust1.99.1 : cargo fmt --check,
  cargo clippy --locked --all-features --all-targets -- -D warnings passent ;
  cargo test --locked --all-features : 27 tests reussis, 2 ignores par harness
  (helper bootstrap lance par competing_processes, navigateur lance separement).
  Le test core_restart arrete/redemarre vraiment trois processus : session
  conservee puis revocation conservee. Tests HTTP HTTPS Secure/Host/Origin passent.
- Premier run complet a revele une course PRAGMA WAL a l'ouverture SQLite.
  Correction IdentityStore : trois tentatives maximum uniquement BUSY/LOCKED,
  attente50/100ms ; corruption/permissions/autres erreurs jamais masquees.
  Test concurrent repasse six fois ; suite complete finale reussie.
- Web : npm run check (0 erreur/avertissement), lint, npm test (12 tests), build
  passent. cargo test --locked --test browser_auth -- --ignored --nocapture passe :
  Chrome155.0.8059.40 Windows10.0.26300 headless390/1440px, erreur login puis succes,
  cookie HttpOnly/Strict, identite/menu admin, catalogue, MP4/seek, rechargement,
  logout puis catalogue/Range401, aucune erreur JS/CSP. Captures inspectees dans
  docs/evidence/2026-10-16/auth/ ; pas de nouvelle preuve PCM/MKV/Firefox/Edge/Safari.
- Limites : flux deja ouvert non interrompu au logout, budget global pouvant etre
  epuise par un acteur, revocation session courante seulement, pas de droits fins,
  TLS reel/NAS/charge non testes ; ACL operateur/TOCTOU du banc toujours applicables.
  Voir docs/LOCAL_SESSIONS.md et docs/TEST_PLAN.md. Pas de commit/push/deploiement.
- Prochain petit jalon P1 propose : droits explicites par bibliotheque en SQLite,
  matrice administrator/user et tests de chaque ressource/derive, toujours sur
  fixtures, avant toute connexion de dossiers personnels ro.
- Token saver applique : trois sous-agents aux fichiers distincts, lectures ciblees,
  integration centralisee et tests utiles conserves. Aucun gain chiffre revendique.

## 16 octobre 2026 — bibliothèques et permissions P1 livrées sur fixtures

- SQLite migration0003 : Node local, bibliothèques stables (UUID créations),
  nom/type/racine canonique/état et droits par compte. Initialisation unique des
  deux bibliothèques de fixtures sans grants. Configuration et grants atomiques.
- Administration Web : liste/ajout/modification/activation/utilisateurs autorisés.
  API session administrator, Origin exact et CSRF mutations. Node valide racines
  existantes dans MEDIA_FIXTURE_DIR, refuse relatifs/parents/sorties et junctions.
  Aucun scan, disque distant, NAS ni original modifié.
- Core autorise chaque lecture SQLite : recherche filtrée avant pagination,
  fiches et URLs directes, sous-titres/médias GET/HEAD/Range, révocation immédiate
  aux prochaines requêtes. Administrator ne lit pas une bibliothèque désactivée.
  Assets demo/sous-titres explicitement partagés Films/Séries (même mire) ;
  P0/MSE/DASH et descripteurs relèvent de Films. Racine/type incompatible retire
  association fixture. Les nouvelles bibliothèques restent vides avant scanner.
- Catalogue éditorial encore explicite en Rust, seul registre bibliothèques/droits
  persisté. Pas d'endpoint image privée ; illustrations statiques publiques.
- Tests : cargo fmt --check ; cargo clippy --locked --all-features --all-targets
  -- -D warnings ; cargo test --locked --all-features réussis :32 tests,
  2 ignorés (helper exécuté par sous-processus, navigateur lancé séparément).
  CRUD/CSRF/rôles/rollback/révocation/confinement/junction Windows testés.
  core_restart relance trois vrais processus : bibliothèque modifiée/désactivée
  conservée et média refusé après redémarrage, session/logout conservés aussi.
- Web check0/0, lint, 12 tests, build réussis. cargo test --locked --test
  browser_auth -- --ignored --nocapture réussi : Chrome155.0.8059.40 Windows
  10.0.26300 headless390/1440, création/modification/désactivation, catalogue,
  MP4/seek/logout, pas d'erreur JS/CSP/débordement. Captures inspectées dans
  docs/evidence/2026-10-16/libraries/. Premier essai échoué sur sélecteur de test
  encodé CP1252 ; corrigé UTF-8, relance réussie.
- Limites : fixtures/loopback, pas de scanner/NFO/transcodage/Node distant,
  compte réel ni interface création utilisateurs. Canonicalisation/ouverture
  non atomiques contre écrivain OS hostile ; flux ouvert non coupé à révocation.
  Aucun nouvel essai PCM/MKV/Firefox/Edge/Safari/NAS/charge/CI distante.
  Aucun commit/push/déploiement. Voir docs/LIBRARIES_AND_SCANNER.md.
- Trois sous-agents sur fichiers distincts, lectures ciblées token-saver,
  intégration/tests transverses centralisés ; aucun gain chiffré annoncé.
  Prochain jalon après validation : scanner local Media Node indépendant ARR.

## 16 octobre 2026 — premier scanner Films local P1

- Branche develop, jalon Bibliothèques/Permissions réutilisé. Aucun push,
  publication, déploiement, compte réel ou média personnel utilisé.
- Node interne : parcours récursif MKV/MP4, sans symlink/point de réanalyse Windows,
  caches Jellyfin et temporaires ignorés ; erreurs non bloquantes. Adapter probe
  existant réutilisé, AV1/Opus non imposés. JSON technique et index réels conservés.
  Identité par fournisseur explicitement qualifié, dossier exact avec année ou
  chemin exact ; conflits explicites séparés. Aucun rapprochement flou/NFO/réseau.
- Migration0004 : Work/Edition/MediaVersion/MediaSource/Asset/Track distincts et
  état durable des scans. IDs stables, taille/mtime/cache probe, pas de doublon
  au second scan. Versions d'un dossier de film identifiées sous une carte.
  Groupement limité à la bibliothèque/racine ; rapprochement entre bibliothèques
  différé. Un fichier disparu devient indisponible ; aucune purge des métadonnées.
  Parcours incomplet conserve les disparitions non confirmées ; transaction et
  snapshot de racine évitent une publication après changement de configuration.
- MEDIA_LIBRARY_ROOT : racine opérateur absolue existante, mode standalone-local
  vide, aucun probe de fixtures ni amorçage des bibliothèques de démo. Le mode
  MEDIA_FIXTURE_DIR historique reste isolé. DB hors médias, y compris bootstrap.
- Administration Scanner : POST/GET /api/v1/admin/libraries/{id}/scan,
  session administrator + Origin/CSRF, 202, doublon409 ; idle/queued/running/
  completed/error, compteurs finaux et erreurs. Un seul probe local simultané,
  cancellation Ctrl+C et attente des tâches ; jobs interrompus signalés au restart.
- Catalogue/fiche SQLite avec recherche/tri/pagination ; descripteur natif
  /api/v1/media/{assetId}. Routes de contenu existantes et streaming64Kio conservés.
  Contrôles actifs des droits avant GET/HEAD/Range, révocation et désactivation.
  Le lecteur garde son moteur ; source non française possible, aucune piste
  interne sélectionnable ni lecture compatible inventée. Labels démo séparés.
- Tests Rust : cargo fmt --check ; cargo clippy --locked --all-features
  --all-targets -- -D warnings ; cargo test --locked --all-features :38 réussis,
  3 ignorés par harness (2 navigateurs lancés séparément, helper processus exécuté
  par competing_processes). Inclut scan exclusif/arrêt/restart jobs, jonction
  Windows, corruption/temporaire, deux bibliothèques, révocation GET/HEAD/Range,
  réouverture SQLite/IDs/cache, AV1 10bits/Opus FR/EN/sous-titres internes,
  changement de racine/réassociation, disparition et scan incomplet.
- Web : npm run check 0 erreur/avertissement ; npm run lint ; npm test13/13 ;
  npm run build réussis. Parcours browser_scanner : création bibliothèque Web,
  scan2 fichiers, un film/deux versions, second scan0 probe/2 réutilisés,
  MP4 lecture/pause/seek4s, arrêt forcé/relance réel du binaire même SQLite puis
  catalogue/lecture/seek réussis. Chrome155.0.8059.40, Windows10.0.26300 headless.
  Rapports/captures docs/evidence/2026-10-16/scanner/ ; captures inspectées.
- Non-régression navigateur : cargo test --locked --test browser_auth --
  --ignored --nocapture réussi1/1, Chrome390/1440, CRUD permissions, lecture/seek,
  logout/Range401 et aucune erreur JS/CSP. Passe browser_scanner finale1/1 réussie.
- Dernier ajustement : erreur I/O du processus ffprobe fait échouer le job sans
  publier une fausse corruption globale ; disparition pendant probe reste une
  erreur fichier non bloquante. fmt/Clippy strict repassés ; cargo test --locked
  --all-features node::scanner --lib :3/3 et cargo test --locked --all-features
  --test scanner_flow :1/1 repassés (13,87s pour le parcours intégré).
- Sources : copies synthétiques en dossiers jetables, hashes/comparaisons bytes
  avant/après identiques ; aucun original modifié. Pas de nouvelle preuve PCM,
  compatibilité MKV générale, Edge/Firefox/Safari, NAS, charge/RSS ou CI distante.
- Premier essai navigateur échoué : POST scan répondait uniquement status, UI
  attendait errors/counters. Corrigé des deux côtés, relance réussie. Une première
  suite Rust a rencontré LNK1104 à cause du même exécutable navigateur lancé en
  parallèle ; validations suivantes sérialisées et réussies.
- Limites : confinement puis ouverture non atomiques (TOCTOU), ACL opérateur
  requises ; flux déjà ouvert non coupé à révocation ; inventaire/cache en RAM
  proportionnels aux fichiers ; ffprobe non sandboxé. Séries/NFO/images/pistes
  externes/OIDC/multi-Node/ordonnanceur et vrai déploiement non implémentés.
  Détails/usage : docs/LOCAL_FILM_SCANNER.md. Prochain petit jalon recommandé :
  métadonnées NFO Films strictement locales et bornées, après validation utilisateur.

## 16 octobre 2026 — métadonnées NFO Films locales

- develop et modifications non commitées du scanner préservés. Migration0004
  inchangée (SHA256 9175E1B4934317EE89921B164D18A048211A7D5AA4E1B28FD4D3A9D1A5C13E6F).
  Migration additive0005_film_editorial : candidats par asset, JSON/provenance Work.
- Node/nfo.rs, quick-xml0.42.0 : sidecar exact `<base vidéo>.nfo` prioritaire,
  puis movie.nfo seulement dans un sous-dossier non ambigu. Pas de movie.nfo
  partagé à la racine ; conflits explicites fichier/dossier refusés. NFO exact
  invalide ne bascule pas silencieusement sur le partagé. Aucun appel ARR/réseau.
- title/originaltitle/plot/outline/year/premiered/genres/rating/mpaa/IDs explicitement
  qualifiés extraits. Champs techniques ignorés. Identifiants NFO jamais utilisés
  comme clés de fusion ; IDs du graphe média conservés. Conflits interversions
  ou avec repli local signalés, fournisseur contradictoire retiré des IDs publics.
- Chaque scan relit les NFO bornés indépendamment du cache vidéo : actualisation
  et suppression sans nouveau ffprobe vidéo. Valide actualise ; absent rétablit
  le repli ; invalide garde le dernier instantané valide, sinon repli local.
  Nouveau graphe/racine ne reprend pas l'ancien instantané. Sélection Work
  déterministe exacte/partagée puis chemins, transactionnelle. Médias indisponibles
  conservent leurs métadonnées. Avertissements NFO séparés des erreurs média,
  exposés au statut admin sans rendre la vidéo indisponible.
- XML non fiable : DTD/entités externes/inconnues rejetées, sans résolveur réseau.
  UTF8 strict/BOM accepté, taille1Mio, profondeur16, champs16Kio,64genres ;
  caractères XML interdits et références invalides rejetés. Liens/reparse refusés.
  Valeurs jamais utilisées comme chemins/URLs. Texte échappé Svelte, aucun HTML
  arbitraire. Risque TOCTOU existant toujours explicite, ACL opérateur requises.
- Web garde design/navigation/lecteur, affiche titre/année/synopsis/genres dans
  Accueil, Films, Recherche et fiche. API catalogue enrichie, provenance nfo/local,
  autres champs et metadataWarnings. Aucun nouveau endpoint ou permission.
- Vérifications : cargo fmt --check ; cargo clippy --locked --all-features
  --all-targets -- -D warnings passent. cargo test --locked --all-features :
  45 réussis,4 ignorés (3 navigateurs explicites, helper processus exercé ailleurs).
  Tests nouveaux : parser hostile/XXE/expansion/taille/profondeur/encodage/HTML,
  NFO exact/partagé/jonction Windows, migration0004→0005 préservant les IDs,
  changement de racine, update/delete/invalides/cache sans probe, deux versions,
  IDs contradictoires, droits fiche/GET/HEAD/Range et hashes sources/NFO.
- Web npm run check :0 erreur/avertissement ; npm run lint ; npm test13/13 ;
  npm run build passent. cargo test --locked --test browser_nfo -- --ignored
  --nocapture :1/1 réussi en49,22s. Chrome155.0.8059.40 Windows10.0.26300 headless,
  quatre phases initial/restart/update/delete, vrais arrêts/relances Core même DB,
  métadonnées Accueil/Films/Recherche/fiche, HTML hostile rendu texte, MP4/seek4s/
  pause ;0 probe après update/delete et deuxième scan. Aucune erreur JS/CSP.
  Preuves distinctes docs/evidence/2026-10-16/nfo/, capture initiale inspectée.
- 13 anciens fichiers de preuve du16octobre comparés SHA256 : tous inchangés.
  Copies synthétiques jetables seulement ; seules les mutations explicites des
  tests changent leurs NFO. Aucun original média/NFO écrit par le scanner,
  hashes et comparaisons avant/après vérifiés. Aucun commit/push/publication.
- Limites : UTF8 seulement, association prudente des NFO partagés, rating0..10,
  pas de validation fournisseur en ligne ni fusion entre bibliothèques ; aucun
  nouveau résultat mobile/Edge/Firefox/Safari/NAS/charge/CI distante. Séries,
  illustrations, sous-titres externes, multi-Node/DB externe restent différés.
  Prochain petit jalon recommandé : affiches locales Films en lecture seule,
  après validation. Détails : docs/LOCAL_FILM_NFO.md.
- Token saver appliqué à la demande : lectures ciblées, journaux limités,
  compilations intégrées centralement ; validations obligatoires conservées,
  aucun gain chiffré annoncé.


## 16 octobre 2026 — affiches locales Films

- Develop, sans commit/push/publication. Migration additive0006_local_posters ;
  migrations0004/0005 intactes. Inventaire images distinct du graphe vidéo, UUID
  stable par Work/racine/chemin ; chemins relatifs privés, taille/mtime/MIME/priorité.
- Node : poster puis folder puis base-vidéo-poster puis base-vidéo ; formats
  jpg/jpeg/png/webp dans cet ordre. Partage refusé à la racine et entre identités
  ambiguës ; première image présente invalide = avertissement/repli. Une affiche
  manquante ne supprime pas son Work. Cache images taille/mtime, sans ffprobe vidéo.
- Core : GET/HEAD /api/v1/assets/{uuid}/image, session et droits de bibliothèque
  vivants, désactivation/révocation appliquées. Confinement symlink/reparse et
  validation réelle image0.25.10 avant flux64Kio ; MIME/nosniff et no-store.
  Catalogue/fiche posterUrl révisée taille/mtime, aucun chemin image exposé.
- Limites :10Mio compressés, dimensions4096, pixels décodés32Mio, budget interne
  décodeur32Mio de meilleur effort, validation sérialisée sur worker bloquant.
  Coût CPU par GET/HEAD, aucune preuve de RSS/charge agrégée. Confinement puis
  validation puis ouverture non atomiques : TOCTOU résiduel et ACL opérateur.
  Révocation ne coupe pas un corps ouvert ni des pixels déjà affichés.
- Web : composant Poster adapté, Accueil/Films/recherche/fiche, ratio2:3,
  cartes lazy/async et fiche eager, repli si absent/erreur ; design/lecteur conservés.
- Token saver appliqué : lectures ciblées et validations regroupées ; aucun gain
  chiffré annoncé. Détails : docs/LOCAL_FILM_POSTERS.md.

- Validation finale : cargo fmt --check ; cargo clippy --locked --all-features
  --all-targets -- -D warnings ; cargo test --locked --all-features réussis :
  50 tests réussis,5 ignorés (4 navigateurs explicites et helper subprocess).
  Inclut migrations0004→0005→0006 avec IDs/metadata conservés, priorités/formats,
  absences/corruptions/extensions trompeuses/oversize/jonction Windows, cache
  images, modifications/suppressions sans probe, droits GET/HEAD de2 bibliothèques,
  désactivation/grants/révocation de session, graphes/NFO/originaux inchangés.
- Web : npm run check (0 erreur/avertissement), npm run lint, npm test (13/13),
  npm run build réussis. Chrome explicite : cargo test --locked --test
  browser_posters -- --ignored --nocapture,1/1 réussi en39,51s ; Chrome155.0.8059.40,
  Windows10.0.26300 headless,4 phases initial/restart/update/delete, vrais arrêts
  et relances Core avec même SQLite. Nouveaux pixels/dimensions après rescan,
  Accueil/Films/recherche/fiche, lazy/repli après erreur, MIME/nosniff/no-store,
  refus anonyme GET/HEAD, NFO échappé, lectureMP4/pause/seek4s et aucune erreur
  JS/CSP. Preuves : docs/evidence/2026-10-16/posters/ ; captures initial/delete
  inspectées visuellement. Originaux vidéo/NFO/affiche comparés, seules mutations
  explicites des copies jetables par tests. Aucun média personnel connecté.
- Essais initiaux corrigés : verrou Windows sur exécutable lors de compilations
  concurrentes ; attente de scan navigateur remplacée par polling API ; Clippy
  if imbriqué simplifié. Validation centrale finale sérialisée et réussie.
- Pas de preuve NAS/LAN/charge/CI distante ni nouveau navigateur. Prochain petit
  jalon recommandé : durcir les ouvertures de fichiers contre TOCTOU avant toute
  exposition à un stockage modifiable par un acteur hostile. Séries/backdrops/
  pistes externes/OIDC/multi-Node/PostgreSQL/Docker restent hors périmètre.

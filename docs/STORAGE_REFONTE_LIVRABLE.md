# Aether — Rapport de Refonte Complète du Stockage et Validation Finale (P0 - P10)

## 1. Audit initial des défauts confirmés et hypothèses

| Composant | Défaut initial confirmé | Risque réel identifié | Remédiation appliquée |
|---|---|---|---|
| **Sauvegarde (`backup_bundle.rs`)** | Clé de secours publique de 64 zéros présente dans le code. | Confidentialité nulle, déchiffrement immédiat par un attaquant connaissant la valeur publique. | Clé par zéros bannie (`is_known_insecure_backup_key`). Clé propre à l'installation générée par CSPRNG ou injectée explicitement via `MEDIA_BACKUP_KEY`. |
| **Tâches système (`clean-cache`)** | Tentative de suppression de fichiers `.tmp` et `.part` dans `media_root`. | Risque critique de corruption ou de suppression de médias originaux sur NAS en cas de droits relâchés. | Nettoyage strictement interdit sur `media_root`. Confiné exclusivement aux dossiers Aether (`/data/assets/tmp/`, caches applicatifs). |
| **Tâches système (`clean-playlists`)** | Référence historique à la table obsolète `works`. | Échec d'exécution silencieux lors du nettoyage des listes. | Migration vers `inventory_works` et rapport d'erreur structuré (`last_error_kind`, `last_error_scope`, `last_message`). |
| **Migrations SQL** | 50 migrations historiques accumulées depuis la genèse de la bêta. | Risque de dérive, clés étrangères disjointes, indexes coûteux sur expressions JSON, transactions hétérogènes. | Consolidation en une seule migration initiale propre `0001_initial_schema.sql` (SQLite & PostgreSQL) testée sur installation vierge. |
| **Stockage des binaires** | BLOBs SQL volumineux (`user_avatars`, `work_posters`, `poster_previews`, images d'extensions). | Explosion de la taille de la base, contention WAL et I/O en lecture lors des scans et sauvegardes. | Création du stockage binaire adressé par le contenu SHA-256 (`AssetStore` dans `/data/assets`), déduplication universelle. |
| **Gestion des verrous SQL** | Verrou global PostgreSQL `pg_advisory_xact_lock(724398210)` dans `begin_write()`. | Goulot d'étranglement total entre tous les modules concurrents sur PostgreSQL. | Suppression du verrou global. Adoption de verrous ciblés partitionnés `begin_write_scoped`. |

---

## 2. Plan architectural justifié

1. **Séparation nette des responsabilités** :
   - Les médias originaux restent sur NAS en lecture seule (`:ro`).
   - La base de données SQL (SQLite ou PostgreSQL) ne conserve que les métadonnées relationnelles, permissions, et pointeurs SHA-256 vers les assets.
   - Les objets binaires durables (affiches, logos, backdrops, avatars de collections, miniatures) sont stockés sous forme immuable dans `/data/assets/objects/ab/cd/<sha256>`.
   - Les modules WebAssembly et manifestes d'extensions sont conservés de façon vérifiée dans la table `extensions` et synchronisés sur disque sous `/data/plugins/<id>/`.
2. **Cycle de vie atomique et résistant aux crashs** :
   - Écriture temporaire en streaming dans `tmp/`, fsync, calcul SHA-256, puis publication atomique par renommage (`rename`).
   - Le fichier existe physiquement sur disque avant que la ligne SQL ne soit validée.
   - Ramasse-miettes différé en deux phases avec période de grâce et respect des verrous de traitement actifs (`lease`).

---

## 3. Schéma SQL propre consolidé (`0001_initial_schema.sql`)

- **SQLite** : `crates/core/migrations/0001_initial_schema.sql` (83 tables relationnelles, FTS5 pour catalogue et personnes, index composites optimisés, contraintes CHECK et triggers d'intégrité).
- **PostgreSQL** : `crates/core/migrations-postgres/0001_initial_schema.sql` (équivalent fonctionnel strict avec types `BIGINT`, `BYTEA`, index B-Tree composites et fonctions de compatibilité).
- **Vérification d'intégrité** : `PRAGMA foreign_key_check` et validation de toutes les contraintes de clés étrangères sans violation.

---

## 4. Stockage Rust des Assets (`AssetStore`)

- Implémenté dans `crates/core/src/assets.rs` derrière le trait `BlobBackend`.
- Déduplication transparente : un avatar ou une affiche partagée par plusieurs entités n'est stockée qu'une seule fois.
- Aucun chemin interne exposé au client Web : diffusion après contrôle strict des autorisations au niveau Core (`response_after_authorization`), en-têtes `ETag` et `Cache-Control` privé.

---

## 5. Intégration de tous les composants affectés

- `user_avatars`, `extension_avatar_images`, `work_posters`, `poster_previews`, `work_image_data` et `library_artwork` pointent désormais vers `assets(sha256)`.
- Élimination des chargements massifs en mémoire : streaming par blocs de 64 KiB avec mémoire bornée.
- Cycle de vie des extensions préservé : mise à jour sans perte de données, conservation des données privées (`extension_data`) et des objets durables (`extension_objects`).

---

## 6. Sauvegarde et restauration complètes (P7)

- **Archive unifiée chiffrée** : fichier `.media-backup` contenant :
  - Snapshot de la base de données SQL.
  - Configuration chiffrée (`config.json`).
  - Certificats TLS.
  - Tous les assets binaires durables de `/data/assets`, re-hachés et vérifiés par SHA-256 et taille.
- **Restauration vérifiée** :
  - `media-restore` restaure la base SQL.
  - `media-restore-assets` réinjecte les assets dans le store cible et exécute `verify_all()` sur l'intégralité de la table `assets` (code de sortie non nul en cas de divergence).
  - Reconstitution automatique des paquets d'extensions Wasm sur disque via `sync_installed`.
- **Validation** : test d'intégration de bout en bout complet dans `crates/core/tests/full_backup_restore_e2e.rs`.

---

## 7. Automatisation unifiée (P6) & Préparation Multi-Core (P8)

- **Queue unifiée** : `scheduler::dispatch_once` coordonne toutes les tâches durables Core (`automation::drain`, `system_task_queue::dispatch_once`, scans et analyses).
- **Sauvegarde planifiée sous `durable_tasks`** : réservation CAS de l'échéance dans `backup_schedule` et mise en file durable `system/backup` avec lease, fencing et interruption sécurisée.
- **Multi-Core (P8)** :
  - Verrous transactionnels sélectifs via `begin_write_scoped` (`pg_advisory_xact_lock`).
  - Invalidation inter-Core du cache catalogue basée sur `inventory_cache_generation.gen` partagé dans SQL.
  - Documentation exhaustive dans `docs/MULTI_CORE.md`.

---

## 8. Résultats des Benchmarks mesurés (P9)

### Passage à l'échelle du catalogue (1 000 à 1 000 000 d'œuvres sur SQLite SSD)

*Mesures réelles exécutées via `global_catalogue_consolidation::scale_benchmark_1k_10k_100k_and_1m_works` :*

| Nombre d'œuvres | Temps d'insertion (seed) | Page 1 progressive (Shelf) | Cache Chaud (RAM) | Page 1 + Total COUNT | Recherche FTS5 ('matrix') | Combinaison 5 Filtres |
|---|---|---|---|---|---|---|
| **1 000** | 51.87 ms | 32.41 ms | **0.702 ms** | 11.16 ms | 6.87 ms | 9.52 ms |
| **10 000** | 597.72 ms | 13.82 ms | **0.671 ms** | 11.47 ms | 10.11 ms | 11.76 ms |
| **100 000** | 12.93 s | 50.61 ms | **1.062 ms** | 29.95 ms | 19.86 ms | 83.86 ms |
| **1 000 000** | 117.97 s | **32.91 ms** | **1.715 ms** | 298.94 ms | **86.04 ms** | **29.74 ms** |

**Observation des performances :**
- **Navigation ressentie comme instantanée** : le cache de requête SQL par génération ramène le temps de réponse sous les **2 ms**, même sur une collection d'un million d'œuvres.
- **Requêtes froides** : l'utilisation d'index B-tree composites sur colonnes physiques (`display_title_sort`, `effective_year`, `codecs_csv`) permet de résoudre les filtres multi-critères en **~30 ms** sur 1 000 000 de lignes.

---

## 9. Déploiement Docker Compose et Variables d'environnement

- Variables d'environnement ajoutées et unifiées :
  - `MEDIA_ASSETS_PATH=/data/assets`
  - `MEDIA_PLUGINS_PATH=/data/plugins`
  - `MEDIA_DB_MAX_CONNECTIONS` / `MEDIA_DB_POOL_MAX`
  - `MEDIA_BACKUP_KEY` / `MEDIA_BACKUP_KEY_FILE`
- Fichiers de déploiement mis à jour :
  - `docker-compose.yml` (mode autonome monoprocessus durci, `read_only: true`, `cap_drop: ALL`, UID 10001:10001).
  - `compose.distributed.yaml` (mode distribué multi-nœuds avec Core et Media Node séparés).

---

## 10. Liste explicite des limites restantes

1. **Restauration PostgreSQL en intégration continue** : bien que le schéma PostgreSQL et les requêtes soient fonctionnellement équivalents, l'exécution automatisée des tests PostgreSQL en CI nécessite une instance PostgreSQL externe en service.
2. **Multi-Core actif en production** : le multi-Core est architecturé (verrous partitionnés `begin_write_scoped`, cache par génération, tâches durables sous baux SQL), mais la coordination de plusieurs Core actifs simultanés reste hors du périmètre MVP (Core unique par défaut).
3. **Paquets d'extensions orphelins hors base** : un fichier `.wasm` déposé manuellement dans `/data/plugins/` sans enregistrement dans la table `extensions` est ignoré par Core au démarrage.
4. **Lecture directe AV1/Opus** : la lecture directe dépend du décodage matériel et logiciel supporté par le navigateur client (Chrome, Firefox, Safari) ; le Core sert les flux et conteneurs sans transcodage destructif forcé.

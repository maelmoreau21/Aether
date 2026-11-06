# Migrations de schéma

Aether n'utilise pas `sqlx::migrate!`. Le schéma est appliqué par
`initialize_fresh_schema` (`crates/core/src/database.rs`) à **chaque démarrage**,
dans une seule transaction (`BEGIN IMMEDIATE` sur SQLite, verrou consultatif sur
PostgreSQL). Une erreur annule toute la transaction : la base reste dans son état
précédent. `schema_version` (une ligne, `CHECK(version=1)`) reste le garde-fou
d'acceptation et n'a pas été modifié.

## Ordre réel d'application

1. **Base vide** : la section du moteur de `migrations/schema.sql` (entre
   `AETHER_SCHEMA_<MOTEUR>_BEGIN/END`) est exécutée. Journal : `baseline`.
2. **Base existante** (`users` présent, `schema_version = 1`) : `schema.sql` n'est
   **jamais** rejoué. Seuls s'exécutent :
   - SQLite : `CREATE TABLE IF NOT EXISTS` des tables `watch_party*` ;
   - PostgreSQL : colonnes `size` élargies en `bigint`, mêmes tables `watch_party*`,
     puis **colonnes d'horodatage `integer` élargies en `bigint`** (voir écarts) ;
   - journal : `compat:watch_parties`.
3. Dans tous les cas, ensuite :
   `compat:task_constraints` (Rust, `database/task_schema.rs` : catégorie/largeur
   `trickplay`, marqueurs `recap`/`preview`, reconstruction SQLite de la table), puis
   les fichiers idempotents `IDEMPOTENT_STEPS`, le seed Rust des groupes de rôles,
   `IDEMPOTENT_STEPS_AFTER_GROUPS`, enfin les étapes `VERSIONED_STEPS` (vide aujourd'hui), puis l'étape Rust unique
   `compat:translations_backfill` (JSON `translations` → `work_translations`, voir
   [METADATA_LOCALIZATION.md](METADATA_LOCALIZATION.md)). Elle n'est inscrite au journal
   qu'avec ses écritures, dans la même transaction : un échec la rejoue au démarrage suivant.

Conséquence : **un objet ajouté à `schema.sql` n'atteint pas les bases existantes**.
Il doit aussi être créé par un fichier idempotent ou une étape versionnée. Le test
`*_legacy_databases_upgrade_without_losing_data` échoue si ce n'est pas le cas.

## Journal `schema_migrations`

Créé au premier démarrage avec ce code, sans toucher à l'historique : aucune
migration existante n'est renommée ni rejouée.

| colonne | rôle |
|---|---|
| `name` | identifiant stable (nom du fichier sans `.sql`, `baseline`, `compat:…`) |
| `kind` | `baseline`, `idempotent`, `compat`, `versioned` |
| `checksum` | SHA-256 du texte, fins de ligne normalisées en LF |
| `applied_at` / `updated_at` | première inscription / dernier changement de somme |

- `idempotent` : rejoué à chaque démarrage (c'est ainsi qu'une correction compatible
  atteint les anciennes bases). Une somme modifiée est **enregistrée, pas rejetée**.
  Une ligne inchangée n'écrit rien.
- `versioned` (futur) : exécuté une seule fois. Modifier le texte d'une étape déjà
  appliquée est une erreur explicite ; écrire une nouvelle étape. Les noms ne doivent
  jamais être renommés après publication.
- Les bases créées avant le journal n'ont pas de ligne `baseline` : leur création
  initiale est inconnue.

## Écarts constatés

Mesurés en comparant une installation neuve à des bases créées depuis quatre
révisions historiques de `schema.sql` (`1be8ad5`, `38ca514`, `7ee0404`, `9f10420`,
fixtures dans `crates/core/tests/fixtures/schema_history`), puis mises à niveau.

| # | Écart | État |
|---|---|---|
| 1 | Tables `avatar_packs`, `avatar_images`, `avatar_policy` et index `avatar_images_*`, `extension_audit_retention` absents des bases créées avant leur ajout à `schema.sql` : aucun fichier ne les créait. | **Corrigé** : `migrations/baseline_gaps.sql`. |
| 2 | PostgreSQL : 29 colonnes d'horodatage (`users.created_at`, `sessions.*`, `api_keys.*`, …) restaient `integer` (32 bits, dépassement en janvier 2038) sur les bases anciennes ; seul `size` était élargi. | **Corrigé** : élargissement sans perte vers `bigint`. |
| 3 | SQLite : `rebuild_sqlite_table` copiait avec `SELECT *` (par position). Une table dont les colonnes avaient été ajoutées par `ALTER TABLE` décalait silencieusement les valeurs. | **Corrigé** : copie par nom de colonne ; refus explicite si une colonne actuelle de la base n'existe plus dans le schéma. |
| 4 | PostgreSQL n'a **aucun trigger** ; SQLite en a 29 (incrément de `inventory_cache_generation`, colonnes dérivées de `inventory_works`, index FTS). Côté PostgreSQL, des mises à jour explicites existent au moins pour une partie (par ex. `consolidation.rs` pour `has_available_asset`) ; leur couverture complète n'a pas été vérifiée. Le cache de catalogue est invalidé sur PostgreSQL par `groups.rs`, `libraries.rs`, `user_media_policy.rs`, pas par les écritures sur `inventory_works`/affiches. | **Non corrigé, non évalué fonctionnellement.** À traiter à part. |
| 5 | `inventory_works_fts` (FTS5) n'existe que sur SQLite. | Différence de conception, attendue. |
| 6 | SQLite : les tables créées par fichier idempotent ne sont pas `STRICT` et déclarent `BIGINT` là où `schema.sql` écrit `INTEGER` (même affinité). | Toléré, normalisé dans le test. |

Hors écarts d'objets, la parité SQLite/PostgreSQL (tables, colonnes, index, vues) est
vérifiée par `sqlite_and_postgres_expose_the_same_tables_columns_and_indexes`.

## Reconstruction de tables SQLite

`durable_tasks`, `manual_timeline_markers` et `automatic_timeline_markers` sont
reconstruites quand leurs contraintes sont anciennes. La reconstruction conserve
lignes, `rowid`, index et triggers de la table, copie par nom de colonne, puis
valide les clés étrangères de la table reconstruite. Les clés étrangères sont
suspendues autour de la transaction et rétablies même en cas d'échec (testé).

## Tests

```powershell
cargo test -p media-poc --lib database
cargo test -p media-poc --test schema_upgrade
# PostgreSQL 18 jetable :
$env:MEDIA_TEST_POSTGRES_URL = 'postgres://postgres:…@localhost:5432/media_test'
cargo test -p media-poc --test schema_upgrade -- --ignored
```

`schema_upgrade.rs` : pour chaque révision et chaque moteur, installe l'ancien schéma,
insère des données représentatives (accents, CJK, tâches liées, marqueur, taille
2 147 483 647 octets), contrôle `integrity_check`/`foreign_key_check`, compte les lignes
de **toutes** les tables avant/après, compare le contenu des tables remplies, compare la
structure à une installation neuve, relance le démarrage deux fois, et (SQLite) vérifie
que la copie faite avant la mise à niveau reste intacte. Un test sabote une étape en
cours de route : tout est annulé, le journal aussi, et le redémarrage après réparation
réussit.

## Procédure de récupération

Aucun retour arrière SQL n'existe : les mises à niveau sont additives sauf
l'élargissement de types (`integer`→`bigint`) et la reconstruction de tables, qui ne se
défont pas par script. La seule récupération est la **copie faite avant**.

1. Avant de démarrer une version qui change le schéma : arrêter le Core, puis copier
   la base (SQLite : le fichier `.db` avec ses `-wal`/`-shm` éventuels ; PostgreSQL :
   `pg_dump -Fc`) ou créer une sauvegarde complète (`docs/BACKUP_RECOVERY.md`).
2. Démarrer. En cas d'erreur le démarrage s'arrête et la transaction est annulée :
   la base est inchangée. Lire le message (il nomme l'étape), corriger, redémarrer.
3. Si la base a été mise à niveau mais l'application est inutilisable : arrêter le
   Core, restaurer la copie de l'étape 1 **avec la version précédente d'Aether**
   et signaler l'échec.
4. Ne pas supprimer `schema_migrations` pour « réinitialiser » : les étapes idempotentes
   se rejouent seules, les étapes versionnées perdraient leur protection contre les
   modifications.

## Métadonnées multilingues à venir

Aujourd'hui les titres sont dans `inventory_works.title` et les métadonnées éditoriales
en JSON (`editorial_json`, `fallback_json`). Un ajout multilingue sera additif :
nouvelle table clef `(work_id, locale)` créée par une **étape versionnée**
(`VersionedStep`, un script par moteur), sans modifier `title` ni les colonnes
existantes. Les tests vérifient déjà que des titres accentués/CJK traversent la mise à
niveau intacts et que les étapes versionnées s'appliquent une fois, rejettent une
modification et s'annulent avec leur journal en cas d'échec.

## Version bêta : changements destructifs

Le projet est en bêta et une rupture de schéma est envisageable. Rien dans cette
branche n'en dépend : la mise à niveau reste non destructive. Si une rupture est
décidée, la consolidation propre est de replier les fichiers idempotents dans
`schema.sql`, de relever `schema_version` et de refuser explicitement les bases plus
anciennes — décision à prendre séparément, avec la copie de sauvegarde ci-dessus.

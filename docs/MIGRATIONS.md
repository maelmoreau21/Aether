# Schéma de base de données et migrations (`crates/core/migrations`)

Le schéma initial v1.0 est figé dans deux migrations de référence, une par moteur, et installé sur une base vide par `initialize_fresh_schema` / `upgrade_schema_with_steps` (`crates/core/src/database.rs`) :

| Fichier | Rôle |
|---|---|
| `crates/core/migrations/0001_initial_v1.sql` | schéma initial v1.0 figé pour SQLite (`crates/core/schema/sqlite.sql`) |
| `crates/core/migrations/postgres/0001_initial_v1.sql` | schéma initial v1.0 figé pour PostgreSQL (`crates/core/schema/postgres.sql`) |
| `crates/core/schema/derived.sql` | vues (et index de performance) partagés par les deux moteurs, sans aucune donnée : réappliqués à chaque démarrage |
| `crates/core/schema/seed.sql` | lignes présentes dans toute base neuve (réglages singleton, tâches système, déclencheurs par défaut), identique pour les deux moteurs |

## Démarrage

Tout se passe dans une transaction (`BEGIN IMMEDIATE` sur SQLite, verrou consultatif
sur PostgreSQL) ; une erreur annule l'ensemble.

1. **Base vide** : le fichier du moteur, le seed Rust des groupes de rôles
   (`groups::initialize_role_groups`), puis `seed.sql`.
2. **Base qui contient déjà ce schéma** (`users` présent, `schema_version = 1`) : seul
   `derived.sql` est réexécuté ; les tables et les données ne sont jamais réécrites.
3. **Toute autre base** (tables sans `users`, `schema_version` absent ou différent) :
   refus explicite, « choisir une base neuve ». Aucune mise à niveau en place.

## Objets dérivés (`derived.sql`)

Les vues et les index ne portent aucune donnée : `derived.sql` les supprime et les recrée
à chaque démarrage (après le schéma sur une base neuve, après la vérification de version
sur une base existante). Une correction de performance y atteint donc aussi les bases déjà
en service, sans les recréer. Chaque instruction doit être idempotente
(`DROP VIEW IF EXISTS`, `CREATE INDEX IF NOT EXISTS`) et valide sur SQLite comme sur
PostgreSQL. Jamais de table, de colonne ni de donnée dans ce fichier.

Exemple : `effective_library_permissions` déclare ses CTE `NOT MATERIALIZED` ; sans cela,
chaque requête calculait les droits de tous les comptes avant de filtrer sur le sien
(18 ms par requête avec 10 000 comptes sous PostgreSQL, contre 0,3 ms).

Sous PostgreSQL, chaque connexion du pool fixe `plan_cache_mode = force_generic_plan`
(`IdentityStore::open_postgres`) : les requêtes préparées sont planifiées une fois par
connexion au lieu d'être replanifiées à chaque appel. Le banc
`crates/core/tests/postgres_catalogue_bench.rs` (ignoré par défaut) mesure accueil, fiches
et lecteur sur 30 000 films, 1 000 séries et 10 000 comptes.

## Modifier le schéma (v1.0+)

Les fichiers `0001_initial_v1.sql` sont **figés** pour la sortie v1.0 : toute évolution ultérieure du schéma doit ajouter une étape `MigrationStep` séquentielle (`v1 -> v2 -> ...`) dans `crates/core/src/database.rs` (testée en SQLite et PostgreSQL via `upgrade_schema_with_steps` sans perte de données et de manière idempotente), et relever `CURRENT_SCHEMA_VERSION`.

## Tests

```bash
python scripts/test-fresh-schema.py        # le schéma SQLite s'installe et ses contraintes tiennent
cargo test -p aether-core --lib database   # base neuve, réouverture sans réécriture, ancien schéma refusé
```

Les tests PostgreSQL (`tests/admin_postgres.rs`) utilisent `MEDIA_TEST_POSTGRES_URL`
(voir [POSTGRESQL.md](POSTGRESQL.md)).

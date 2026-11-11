# Schéma de base de données

Aether n'a **pas de chaîne de migrations**. Le schéma courant est décrit une seule fois,
par moteur, et appliqué à une base vide par `initialize_fresh_schema`
(`crates/core/src/database.rs`) :

| Fichier | Rôle |
|---|---|
| `crates/core/schema/sqlite.sql` | tables, index, vues et trigger SQLite |
| `crates/core/schema/postgres.sql` | les mêmes objets pour PostgreSQL (colonnes `BIGINT`, pas de trigger : la disponibilité des fichiers est maintenue en Rust) |
| `crates/core/schema/seed.sql` | lignes présentes dans toute base neuve (réglages singleton, tâches système, déclencheurs par défaut), identique pour les deux moteurs |

## Démarrage

Tout se passe dans une transaction (`BEGIN IMMEDIATE` sur SQLite, verrou consultatif
sur PostgreSQL) ; une erreur annule l'ensemble.

1. **Base vide** : le fichier du moteur, le seed Rust des groupes de rôles
   (`groups::initialize_role_groups`), puis `seed.sql`.
2. **Base qui contient déjà ce schéma** (`users` présent, `schema_version = 1`) : rien
   n'est exécuté, les données ne sont jamais réécrites.
3. **Toute autre base** (tables sans `users`, `schema_version` absent ou différent) :
   refus explicite, « choisir une base neuve ». Aucune mise à niveau en place.

## Modifier le schéma

Pendant la bêta, on **édite directement** `sqlite.sql` et `postgres.sql` (et `seed.sql`
si une ligne initiale change), en gardant les deux moteurs alignés. Les bases créées
avant le changement sont recréées : sauvegarde, nouvelle base, restauration des données
utiles par les outils `backup` / `restore`. Il n'y a ni `ALTER TABLE` de rattrapage, ni
journal, ni étape rejouée au démarrage.

Après la version 1.0, un changement de schéma incompatible devra relever
`SCHEMA_VERSION` (constante de `initialize_fresh_schema` et `CHECK` de la table
`schema_version`) et fournir une procédure de reprise explicite ; elle sera décrite ici.

## Tests

```bash
python scripts/test-fresh-schema.py        # le schéma SQLite s'installe et ses contraintes tiennent
cargo test -p aether-core --lib database   # base neuve, réouverture sans réécriture, ancien schéma refusé
```

Les tests PostgreSQL (`tests/admin_postgres.rs`) utilisent `MEDIA_TEST_POSTGRES_URL`
(voir [POSTGRESQL.md](POSTGRESQL.md)).

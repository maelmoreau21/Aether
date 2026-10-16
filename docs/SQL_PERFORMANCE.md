# SQL_PERFORMANCE.md — Performance et pagination du catalogue SQLite (Jalon J2)

## 1. Contexte et diagnostic du goulot d'étranglement

Dans la version initiale, la fonction `crates/core/src/inventory.rs::catalogue()` effectuait une récupération globale en base :
1. Une requête SQL sélectionnait **l'intégralité** des œuvres de la table `inventory_works` ayant au moins un média disponible, sans aucun filtre sur les bibliothèques autorisées de l'utilisateur ni clause `LIMIT`.
2. Pour chaque œuvre du catalogue, la sous-requête corrélée calculant l'URL de l'affiche (`poster_url`) était évaluée par SQLite, même pour des œuvres situées sur des pages non consultées ou dans des bibliothèques auxquelles l'utilisateur n'avait pas accès.
3. Toutes les lignes étaient matérialisées en mémoire Rust sous forme de `Vec<SqliteRow>`, puis désérialisées en structures JSON (`serde_json::Value`).
4. Le filtrage des permissions (`visible.contains(...)`), le filtre de type (`kind == "film"`), la recherche par sous-chaîne (`title.to_lowercase().contains(...)`), le tri alphabétique et la pagination (`skip(offset).take(limit)`) étaient effectués intégralement en mémoire vive dans Rust.

### Conséquences
- **Consommation mémoire linéaire O(N)** : la mémoire vive consommée par chaque requête catalogue était proportionnelle à la taille totale du catalogue indexé.
- **CPU gaspillé** : calculs d'affiches, tris et allocations de milliers de structures JSON jetées immédiatement après pagination.
- **Absence d'index relationnels** : les jointures du graphe de médias (`inventory_editions`, `inventory_versions`, `inventory_sources`, `inventory_assets`) ne disposaient pas d'index sur leurs clés étrangères, provoquant des parcours séquentiels complets lors de l'évaluation de la clause `EXISTS`.

---

## 2. Architecture de la solution

### 2.1 Migration non destructive `0007_catalogue_indexes.sql`

La migration ajoute les colonnes et index nécessaires sans détruire la moindre donnée ni rompre la compatibilité des schémas antérieurs :

```sql
-- Catalogue performance: Unicode sort/search column and relational graph indexes.
ALTER TABLE inventory_works ADD COLUMN title_sort TEXT NOT NULL DEFAULT '';
UPDATE inventory_works SET title_sort = lower(title);

CREATE INDEX inventory_works_library_sort ON inventory_works(library_id, title_sort, id);
CREATE INDEX inventory_works_title_sort ON inventory_works(title_sort, id);

CREATE INDEX inventory_editions_work ON inventory_editions(work_id);
CREATE INDEX inventory_versions_edition ON inventory_versions(edition_id);
CREATE INDEX inventory_sources_version ON inventory_sources(version_id);
CREATE INDEX inventory_assets_source_available ON inventory_assets(source_id, available);
```

### 2.2 Gestion de la sémantique Unicode et limites SQLite documentées

- **Limites intrinsèques de SQLite sans extension ICU** :
  - Les fonctions intégrées `lower()` et l'opérateur `LIKE` de SQLite ne gèrent la casse que pour les caractères ASCII (`a-z` / `A-Z`).
  - Les caractères accentués et non-ASCII (`É`, `è`, `à`, `ç`, etc.) ne sont pas transformés par `lower()` et sont sensibles à la casse dans `LIKE`.
  - La collation `NOCASE` de SQLite compare uniquement les caractères ASCII sans tenir compte de la casse.
- **Préservation stricte de la sémantique existante** :
  - La colonne `title_sort` est alimentée lors de l'ingest initial et des rafraîchissements NFO via la méthode Rust `title.to_lowercase()`, qui applique la table complète des propriétés Unicode DerivedCoreProperties.
  - La recherche en base utilise `instr(w.title_sort, ?) > 0` avec le paramètre de recherche nettoyé et passé en bas de casse Unicode dans Rust.
  - L'usage de `instr()` plutôt que `LIKE` garantit que les caractères spéciaux (comme `%`, `_`, `'`, `&`) sont recherchés littéralement comme sous-chaînes, reproduisant exactement le comportement de `str::contains()` en Rust.
  - L'ordre de tri `ORDER BY w.title_sort ASC, w.id ASC` (ou `DESC`) s'appuie sur la représentation UTF-8 normalisée en bas de casse et ajoute `w.id` pour garantir un bris d'égalité déterministe et une pagination stable d'une page à l'autre.

### 2.3 Migration additive `0008_catalogue_unicode_sort.sql` et rattrapage Unicode Rust

Lors de l'application de la migration `0007_catalogue_indexes.sql`, l'instruction `UPDATE inventory_works SET title_sort = lower(title);` a été exécutée par le moteur SQLite. Or, sans extension ICU, la fonction intégrée `lower()` de SQLite ne convertit que les caractères ASCII `A-Z` vers `a-z`, laissant intactes en majuscules les lettres accentuées et ligatures (`É`, `À`, `Ç`, `Œ`, etc.). Les nouvelles insertions et mises à jour utilisent en revanche `title.to_lowercase()` dans Rust.

Pour corriger les bases de données existantes sans toucher à la migration 0007 (déjà déployée) et sans nécessiter de rescan des médias :
1. **Migration additive `0008_catalogue_unicode_sort.sql`** :
   ```sql
   CREATE TABLE IF NOT EXISTS inventory_works_unicode_migration (
       completed_at TEXT NOT NULL
   );
   ```
2. **Procédure de rattrapage Rust par lots bornés (`backfill_title_sort_unicode`)** :
   - Exécutée de manière unique et sérialisée au démarrage dans la transaction `BEGIN IMMEDIATE` de migration d'identité (`IdentityStore::open`).
   - Traitement par lots bornés de 250 lignes (`LIMIT 250`, mémoire O(1)) évitant tout pic mémoire sur de grandes bases.
   - Seules les lignes où `title_sort != title.to_lowercase()` sont mises à jour.
   - Préservation stricte et intégrale de l'ensemble des UUIDs, métadonnées éditoriales NFO, affiches locales, bibliothèques et relations du graphe média.
   - Horodatage d'achèvement consigné dans `inventory_works_unicode_migration` : les démarrages ultérieurs court-circuitent instantanément (< 0,05 ms).
   - Validé sur base migrée de 0006 vers 0007 puis 0008 dans `tests/catalog_performance.rs::test_migrated_database_from_0006_to_0007_then_0008_unicode_fix`.

### 2.4 Requêtes paramétrées avec `sqlx::QueryBuilder`

Le filtrage des permissions, le comptage et la pagination sont délégués à SQLite :
1. **Élimination immédiate des états vides** : si `visible` est vide ou si `kind` ne correspond à aucun média géré, la fonction renvoie immédiatement `total = 0` et `items = []` sans solliciter SQLite.
2. **Requête de comptage dédiée (`COUNT(*)`)** :
   ```sql
   SELECT COUNT(*) FROM inventory_works w
   WHERE w.library_id IN (?, ?, ...)
     [AND instr(w.title_sort, ?) > 0]
     AND EXISTS (
       SELECT 1 FROM inventory_editions e
       JOIN inventory_versions v ON v.edition_id=e.id
       JOIN inventory_sources s ON s.version_id=v.id
       JOIN inventory_assets a ON a.source_id=s.id
       JOIN libraries l ON l.id=s.library_id
       WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local' AND l.enabled=1
     )
   ```
   Cette requête ne calcule aucune URL d'affiche et n'alloue aucune structure d'œuvre.
3. **Requête de données bornée (`LIMIT ? OFFSET ?`)** :
   La sous-requête d'affiche (`poster_url`) n'est exécutée que pour le nombre restreint de lignes demandées (par ex. 24 éléments par page).
4. **Application stricte des permissions avant pagination** : la clause `w.library_id IN (...)` et la condition `l.enabled=1` sont évaluées dans le `WHERE` en amont du `LIMIT / OFFSET`. Un utilisateur ne peut en aucun cas paginer au travers d'œuvres interdites.

---

## 3. Plan d'exécution validé (`EXPLAIN QUERY PLAN`)

L'exécution du plan vérifié sous SQLite (`tests/catalog_performance.rs::explain_query_plan_verifies_index_usage`) démontre l'utilisation systématique des index :

```
EXPLAIN QUERY PLAN (comptage):
SEARCH w USING COVERING INDEX inventory_works_library_sort (library_id=?)
CORRELATED SCALAR SUBQUERY 1
SEARCH e USING INDEX inventory_editions_work (work_id=?)
SEARCH v USING INDEX inventory_versions_edition (edition_id=?)
SEARCH s USING INDEX inventory_sources_version (version_id=?)
SEARCH l USING INDEX sqlite_autoindex_libraries_1 (id=?)
SEARCH a USING COVERING INDEX inventory_assets_source_available (source_id=? AND available=?)

EXPLAIN QUERY PLAN (données paginées):
SEARCH w USING INDEX inventory_works_library_sort (library_id=?)
CORRELATED SCALAR SUBQUERY 2
SEARCH e USING INDEX inventory_editions_work (work_id=?)
SEARCH v USING INDEX inventory_versions_edition (edition_id=?)
SEARCH s USING INDEX inventory_sources_version (version_id=?)
SEARCH l USING INDEX sqlite_autoindex_libraries_1 (id=?)
SEARCH a USING COVERING INDEX inventory_assets_source_available (source_id=? AND available=?)
CORRELATED SCALAR SUBQUERY 1
SEARCH i USING INDEX inventory_images_work (work_id=? AND available=?)
SEARCH il USING INDEX sqlite_autoindex_libraries_1 (id=?)
```

- **Recherche couvrante** : pour le comptage, `inventory_works_library_sort` est utilisé comme index couvrant (`COVERING INDEX`), évitant même la lecture de la table `inventory_works`.
- **Traversée du graphe O(1) par œuvre** : chaque table du graphe (`inventory_editions`, `inventory_versions`, `inventory_sources`, `inventory_assets`) est atteinte via un index seek direct.
- **Sous-requête d'affiche bornée** : `inventory_images_work` n'est invoqué que 24 fois par page au lieu de N fois sur tout le catalogue.

---

## 4. Mesures et comparaison Avant / Après

Mesures effectuées dans le même environnement sur un banc synthétique reproductible (`cargo test --test catalog_performance -- --nocapture`) :

| Scénario | Implémentation Legacy (en mémoire) | Nouvelle implémentation (SQLite SQL) | Gain / Facteur d'amélioration |
|---|---|---|---|
| **1 000 œuvres — Page 1 (24 items)** | 64,12 ms (1 000 rows en RAM) | **22,04 ms** (24 rows) | **~2,9x plus rapide** |
| **1 000 œuvres — Page éloignée (offset=960)** | 64,12 ms (1 000 rows en RAM) | **28,51 ms** (24 rows) | **~2,2x plus rapide** |
| **1 000 œuvres — Recherche ciblée** | 64,12 ms (1 000 rows en RAM) | **7,71 ms** (total: 83) | **8,3x plus rapide** |
| **10 000 œuvres — Page 1 (24 items)** | 854,38 ms (10 000 rows en RAM) | **235,91 ms** (24 rows) | **3,6x plus rapide** |
| **10 000 œuvres — Page éloignée (offset=9960)** | 854,38 ms (10 000 rows en RAM) | **335,48 ms** (24 rows) | **2,5x plus rapide** |
| **10 000 œuvres — Recherche ciblée** | 854,38 ms (10 000 rows en RAM) | **53,66 ms** (total: 833) | **15,9x plus rapide** |
| **Objets matérialisés en RAM (10 000 œuvres)** | 10 000 structures JSON complètes | **24 structures JSON** | **-99,76 % d'allocations** |
| **Requête utilisateur non autorisé / sans droit** | Requête SQL complète exécutée | **0 requête SQL** (< 0,01 ms) | Instantané |

---

## 5. Matrice de non-régression fonctionnelle et sécurité

La suite de tests automatisée valide l'étanchéité et la conformité :
1. **Contrat d'API JSON inchangé** : champs (`id`, `libraryId`, `kind`, `title`, `year`, `description`, `plot`, `genres`, `originalTitle`, `outline`, `premiered`, `rating`, `mpaa`, `externalIds`, `metadataProvenance`, `metadataWarnings`, `synthetic`, `posterUrl`, `mode`, `notice`) strictement identiques.
2. **Filtrage multi-utilisateurs et rôles** :
   - Administrateur : visibilité sur l'ensemble des bibliothèques actives.
   - Utilisateur restreint : visibilité limitée aux bibliothèques attribuées (test vérifié : 400 œuvres visibles sur 500).
   - Utilisateur sans droit ou bibliothèque désactivée : 0 œuvre retournée, zéro fuite d'identifiants.
3. **Recherche Unicode** :
   - Recherche `été` retrouve `Été à Paris` et `Été & Neige`.
   - Recherche `éléphant` retrouve `Éléphant Blanc`.
   - Recherche `ça` retrouve `Ça chapitre un`.
4. **Tri Unicode identique** :
   - L'ordre `title-asc` et `title-desc` produit par SQLite correspond au bit près au tri historique Rust `sort_by_key`.
5. **Gestion des pages hors limites** :
   - `offset` supérieur au nombre total d'œuvres renvoie `items: []` avec `total` exact, sans erreur SQL.

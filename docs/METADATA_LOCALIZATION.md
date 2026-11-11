# Métadonnées traduites par compte — 6 novembre 2026

Deux comptes (par exemple un français et un anglais) consultent la même œuvre,
avec le même identifiant, et reçoivent chacun les titres, résumés et slogans de
leur langue. Ce document décrit le modèle, les règles de repli, l'API, la
migration et les limites. Les fournisseurs TMDb et TVDB sont dans le dépôt
Aether-Plugins (voir « Ordre de livraison »).

## Ce qui est invariant, ce qui est traduit

| Invariant (reste sur `inventory_works`) | Traduit (nouvelles tables) |
|---|---|
| identifiants (`external_ids_json`, identifiants NFO), année, date de sortie, durée, langue originale, classification, notes, crédits (noms), nature (`film`/`series`/`episode`) | titre, titre de tri, résumé, slogan — pour les œuvres, les épisodes (ce sont des `inventory_works`) et les saisons |

Les genres, rôles de l'équipe et noms de personnes ne sont pas traduits (voir limites).

## Stockage

`work_translations(work_id, locale, language, source, priority, title, sort_title,
overview, tagline, provider_locale, updated_at)` et `season_translations`
(même forme, clé `season_id`). Clé primaire `(entité, locale, source)` : une
correction manuelle, un NFO et un fournisseur coexistent pour la même langue et
sont classés à la lecture. Un rafraîchissement du fournisseur remplace **ses**
lignes et jamais celles des autres sources.

- `locale` : étiquette canonique (`fr`, `fr-FR`, `fr-CA`, `en-GB`, `zh-Hant-TW`) ou
  `und` (neutre, valable pour toutes les langues). Les codes à trois lettres
  courants (`fra`, `eng`) sont ramenés à deux lettres. Les variantes/extensions
  sont refusées pour ne pas fragmenter les données.
- `language` : langue de base, colonne indexée qui borne la lecture.
- `source` : `manual`, `nfo`, `legacy` ou `provider:<id>` ; `priority` 100 / 80 / 40 / 50.
- Tables créées par `migrations/metadata_translations.sql` (idempotent, SQLite et
  PostgreSQL), suppression en cascade avec l'œuvre ou la saison.

Les données écrites avant ces tables restent lisibles sans réécriture : le
chargeur en déduit des lignes virtuelles depuis `editorial_json` et `manual_json`.

| Donnée existante | Ligne déduite |
|---|---|
| `editorial_json` avec `provenance = "nfo"` | `nfo`, langue de la bibliothèque |
| `editorial_json`/`manual_json` avec `provenance = "identify:<id>"` ou `"extension:<id>"` | `provider:<id>`, langue de la bibliothèque |
| `manual_json` sans provenance fournisseur (correction humaine, `provenance = "extension"` pour l'API existante) | `manual`, **neutre** (`und`) |

Une identification copie son résultat dans `manual_json` : elle n'est donc pas
traitée comme une décision humaine (sinon elle imposerait sa langue à tous).

## Règles de repli (stables)

Chaîne d'un compte : paramètre `?locale=` de la requête, puis `metadataLocale`,
puis `locale` (langue de l'interface) du compte, puis `metadataFallbackLocales`
(4 au plus), puis la langue par défaut du serveur. Au plus 6 entrées, sans doublon.

Pour chaque ligne, on cherche la première entrée de la chaîne de même langue :

1. exacte (`fr-CA` pour `fr-CA`), 2. langue seule (`fr`), 3. région voisine
(`en-GB` est servi par `en-US` avant de quitter l'anglais ; la région « par défaut »
de la langue — `fr-FR`, `en-US`, `es-ES`, `pt-PT`, `de-DE`… — passe d'abord, puis
l'ordre alphabétique).

Classement : correction manuelle de la locale exacte → correction manuelle neutre →
pour chaque entrée de la chaîne, **priorité de la source** (NFO avant fournisseur) puis
précision de la région → lignes neutres non manuelles. Chaque champ (titre, résumé,
slogan) est choisi séparément : un résumé absent en français est complété par
l'anglais sans effacer le titre français. Une chaîne vide est « absent ». Aucune
traduction trouvée : le titre et le résumé stockés ne changent pas.

La réponse indique la langue réellement servie :
`"localization": {"requested": "en-GB", "title": "en-US", "overview": null}`.

## API

- `GET /api/v1/catalog` et `GET /api/v1/catalog/{id}` : le compte authentifié reçoit sa
  langue ; `?locale=fr-CA` la remplace pour la requête (400 `invalid_locale` si
  invalide). Les épisodes et saisons d'une série sont traduits en un passage.
- `PUT /api/v1/preferences` : champs `metadataLocale` et `metadataFallbackLocales`.
- `PUT /api/v1/admin/works/{work}/metadata` (édition manuelle) : champ facultatif `locale`. Sans lui, la
  correction reste neutre (comportement historique). Avec lui, titre/slogan/résumé
  sont enregistrés pour cette langue seulement ; les textes neutres existants sont
  conservés. Réinitialiser supprime aussi les lignes manuelles.
- Recommandations : mêmes règles.
- Le champ `translations` de la réponse est maintenant `{}` (le serveur choisit la
  langue, le client n'a plus à le faire) ; `localization` le remplace.

## Caches

- **Pages du catalogue (Core)** : la clé contient la chaîne de langues, le compte,
  les bibliothèques visibles et les filtres ; deux langues ne partagent jamais une
  entrée. Toute écriture de traduction incrémente `inventory_cache_generation`
  (PostgreSQL n'a pas de déclencheurs : l'incrément est explicite).
- **Navigateur** : les caches de descripteurs et de pages sont vidés quand le
  changement de langue est enregistré côté serveur (évènement `media-locale-saved`).

## Fournisseurs et SDK

`media-extension-sdk` ajoute, sans changer `API_VERSION` ni retirer de champ :
`DetailsRequest.languages`, `StructuredMetadata.locale`,
`SeasonMetadata.translations`, `EpisodeMetadata.translations`, la constante
`LOCALIZATION_CONTRACT = 2`. Les champs absents sont ignorés des deux côtés : un
fournisseur du premier contrat répond dans une seule langue (stockée comme langue
de la bibliothèque), un Core du premier contrat ignore les nouveaux champs.
Seul risque : un auteur de plugin qui construit ces structures par littéral doit
ajouter `..Default::default()` — erreur de compilation, jamais silencieuse.

Le Core envoie à `metadata.details` jusqu'à 6 langues : celle de la bibliothèque
puis celles que lisent les comptes actifs (poids 3 pour le premier choix, 1 pour
un repli).

## Migration

Non destructive et identique sur SQLite et PostgreSQL :
1. `metadata_translations.sql` crée les tables (idempotent, relancé à chaque démarrage) ;
2. une étape Rust unique `compat:translations_backfill` (journalisée dans
   `schema_migrations`, même transaction) copie le bloc JSON `translations`
   d'`editorial_json`/`manual_json` vers `work_translations` (source `legacy`,
   `ON CONFLICT DO NOTHING`). Le JSON d'origine n'est ni modifié ni supprimé.

Retour arrière : une version précédente ignore les tables ; elle relit le JSON
conservé. Les traductions écrites après la mise à niveau ne lui sont pas visibles.

## Recherche et tri

* **Recherche** : une œuvre est trouvée par son titre de la langue de la bibliothèque, par **chacune de
  ses traductions** et par son **titre original**, sans être dupliquée. Les titres repliés (minuscules)
  sont dans `work_search_titles(work_id, title_search)`, reconstruits pour une œuvre à chaque écriture
  de traduction ou consolidation des métadonnées et remplis une fois pour les bases existantes
  (étape journalisée `compat:search_titles_backfill`). Le prédicat existant (FTS5 trigramme sur SQLite,
  `instr` sur PostgreSQL) est conservé ; la table n'est lue que par `IN (SELECT work_id …)`.
* **Tri** : les tris par titre utilisent la meilleure traduction dans la première langue de la chaîne du
  compte (même ordre de priorité que l'affichage : correction manuelle, puis priorité de la source) et
  retombent sur le titre de la bibliothèque. L'expression n'est employée que si au moins une traduction
  existe dans cette langue (index `work_translations_by_language`) ; sinon la colonne indexée habituelle
  est utilisée, sans coût.
* Limites : pas de repli des accents (comme avant), tri par `sort_title` ou titre en minuscules.

## Préférences de langue

`PUT /api/v1/preferences` accepte aussi `artworkLocale` (langue des illustrations) et
`subtitleLanguage` ; une valeur `null` supprime une préférence (retour à son défaut). Voir
[I18N.md](I18N.md) pour la séparation interface / métadonnées / illustrations / audio / sous-titres.

## Illustrations

Déjà en place et non dupliqué : `work_image_entries.language`, le tri par langues
de l'enrichissement d'images et la rotation d'affiches par compte (langue de
l'interface ou originale). Une variante n'est stockée qu'une fois, étiquetée par
sa langue ; le choix est fait à l'affichage selon le compte. TMDb ne propose pas de
régions pour les images (langue ISO 639-1 seule) ; TVDB fournit un code de langue
sur 3 lettres.

## Limites connues

- Genres, noms de rôles et biographies ne sont pas traduits.
- Une œuvre déjà identifiée n'a de traduction supplémentaire qu'après un
  rafraîchissement avec une version du fournisseur qui supporte le contrat 2.
- Une correction manuelle sans `locale` s'applique à toutes les langues, y compris
  quand l'administrateur pensait corriger le titre français.
- Épisodes TMDb : une requête saison par langue supplémentaire (borné).
- Validé sur SQLite et PostgreSQL 18 (Docker). Non validé : fournisseurs réels,
  NAS, multi-Core réel (y compris la table de recherche). L'interface (Paramètres → Langues) propose `metadataLocale`.

## Tests

```powershell
cargo test --locked -p aether-core --lib metadata_locale translations::
cargo test --locked -p media-extension-sdk
cargo test --locked -p aether-core --lib preferences::
$env:MEDIA_TEST_POSTGRES_URL = 'postgres://…'
cargo test --locked --all-features -p aether-core --lib translations::store_tests::postgres -- --ignored
cargo test --locked --all-features -p aether-core --test schema_upgrade -- --ignored
```

Scénarios couverts sur les deux moteurs : deux comptes simultanés sur le même
identifiant, `fr-FR`/`fr-CA`/`fr-BE`/`en-GB`, langue absente, repli par champ,
épisodes et saisons, NFO, identification, correction neutre puis par langue puis
suppression, cache après modification, redémarrage et reprise unique du JSON.

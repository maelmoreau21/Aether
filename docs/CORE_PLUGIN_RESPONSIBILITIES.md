# Responsabilités Core / extensions — 7 novembre 2026

Audit des frontières entre Aether Core et les extensions Wasm d'`Aether-Plugins`
(branches `develop`), centré sur Demandes, Signalements, Poster Rotator, Intro
Skipper, le stockage privé, les routes, les permissions, le SDK et le cycle de vie.
Objectif : clarifier qui fait quoi, **sans** déplacer vers Wasm ce qui est mieux natif.

Méthode : graphe AST du code Rust des deux dépôts (graphify, 343 fichiers,
5 959 nœuds) pour repérer les modules, puis lecture ciblée. Chaque duplication
ci-dessous cite le code ; rien n'est déduit des seuls noms.

## Principe retenu

Une fonction va dans une extension Wasm **seulement** si au moins un critère tient :

1. elle dépend d'un tiers (API externe, clé, quota fournisseur) ;
2. sa politique doit varier par éditeur ou être remplaçable sans redéployer le Core ;
3. c'est un calcul lourd mais pur, isolable sans accès aux données (empreintes).

Tout le reste — identité, droits, données transactionnelles, validation de données
dont le Core est propriétaire — reste natif. Une invocation Wasm coûte un processus
worker (≈ 10,8 Mo de working set mesuré, ≈ 100 ms en debug, voir
`docs/evidence/2026-10-20/extensions/worker-measurements.json`) : elle doit
apporter quelque chose que le Core ne fait pas déjà.

## Matrice de responsabilités

| Domaine | Core générique (hôte) | Métier natif du Core | Logique propre au plugin | Verdict |
|---|---|---|---|---|
| Exécution Wasm | worker jetable, imports limités à 3 fonctions, 16 Mio mémoire, fuel, délais, admission 4 workers dont 1 interactif (`extensions/runtime.rs`, `extensions/worker.rs`) | — | handler `media_run` | Sain ; lancement dédupliqué par cette PR |
| Paquets, signature, catalogue | Ed25519, SHA-256, manifeste décrit = manifeste signé, compatibilité, health check avant mise à jour, rollback (`package.rs`, `repositories.rs`, `updates.rs`) | — | publication signée (`Aether-Plugins/scripts`) | Sain |
| Permissions | liste fermée (`package.rs:30`), opération → permission (`adapters.rs:16`), mise à jour refusée si permissions/hôtes s'élargissent (`install.rs`, `change_version_with_key`) | — | déclaration dans le manifeste | Sain ; table dispersée en 4 endroits (R5) |
| Stockage privé | espaces par extension épinglés à la clé de l'éditeur, quotas, scopes bibliothèque/œuvre/utilisateur (`data.rs`, `objects.rs`) | — | clés/valeurs, objets | Sain |
| Demandes | auth/CSRF, droits bibliothèque, broker worker | tables `media_requests`, `acquisition_*`, connexions Radarr/Sonarr/Lidarr, files durables, recherche distante, plan de surveillance (`acquisition/`) | `requests.validate`, `requests.plan` (choix du service, preuve single/enregistrement) | **Duplication partielle** (D4) |
| Signalements | auth/CSRF, droits bibliothèque, broker worker | tables `media_reports*`, workflow, commentaires, import historique (`reports/mod.rs`) | `reports.validate`, `reports.classify` | **Duplication complète** (D3) |
| Rotation des affiches | — | rotation native par bibliothèque et par compte (`extensions/artwork_rotation.rs`, `library_options.poster_rotation_enabled`) | `artwork.rotation.policy` (renvoie la config telle quelle) | **Plugin mort** (D5) |
| Intro Skipper | stockage des marqueurs manuels/automatiques, tâches durables, cache fournisseur (`automation.rs`, `timeline_markers.rs`) | empreintes Chromaprint calculées par le Node (`node/src/tasks.rs:136`) | appariement d'empreintes, consensus par saison, classification des chapitres (`Aether-Plugins-IntroSkipper`) | **Séparation correcte**, aucun doublon |
| Fournisseurs de métadonnées / sous-titres | broker HTTP (allowlist, SSRF, 10 req, 256 Kio), limites de débit, adaptateurs génériques | fusion des métadonnées, traductions, images | appels TMDb/TVDB/IMDb/OMDb/fanart/MusicBrainz/Discogs/LrcLib/OpenSubtitles | Bon usage de Wasm (critère 1) |
| Avatars | collections d'images, galerie | avatars natifs | — | Natif ; plugin masqué du catalogue |

## Duplications confirmées

| # | Duplication | Preuve | Effet | Traitement |
|---|---|---|---|---|
| D1 | Lancement du worker et cadrage des trames copiés 3 fois (résolution du binaire, environnement vidé, `kill_on_drop`, fenêtre Windows masquée, longueur bornée avant allocation) | `applications.rs` (invoke), `extensions/run.rs` (invoke_worker_context), `extensions/updates.rs` (verify_candidate) | Toute correction d'isolement devait être faite trois fois | **Corrigé dans cette PR** |
| D2 | Liste des fonctions natives (`avatars`, `poster-rotator`) codée deux fois | `extensions/repositories.rs` (catalogue, install) | Un ajout oublié à un endroit laisserait installer un doublon d'une fonction native | **Corrigé dans cette PR** |
| D3 | Validation des signalements écrite deux fois avec des règles **divergentes** | Core `reports/mod.rs` `valid()` ; plugin `Aether-Plugins-Reports/src/lib.rs` `validate()` | Description : Core 3–8 000 octets, plugin 1–8 192. Instant : Core ≤ 7 jours, plugin ≤ 1 an. `engine`/`errorCode` : Core accepte espace et `+` (≤ 128), plugin les refuse (≤ 256) → un signalement `engine: "native player"` passe le Core puis échoue en `invalid_report` sans cause. Un worker par création pour une validation déjà faite. `reports.classify` n'est appelé nulle part par le Core. | Feuille de route R1 |
| D4 | Choix du service et bornes de sélection des demandes écrits deux fois | Core `acquisition/http.rs` `choose()` et `acquisition/mod.rs` `valid()` ; plugin `Aether-Plugins-Requests/src/lib.rs` `validate()` | Saisons : Core ≤ 200, plugin ≤ 1 000. Épisodes : Core ≤ 2 000, plugin ≤ 1 000 → une demande de 1 500 épisodes passe le Core puis échoue au plugin. La preuve single/enregistrement (`requests.plan`) est, elle, une vraie politique plugin. | Feuille de route R2 |
| D5 | Poster Rotator publié comme plugin (3 versions, clé dédiée) alors que la rotation est native | `repositories.rs` refuse son installation (`feature_is_native`) ; aucune occurrence de `artwork.rotation.policy` dans `crates/` | Paquets signés maintenus sans aucun appelant | Feuille de route R3 |
| D6 | Deux formes de manifeste côté plugins | `media_extension_sdk::Manifest` (contrat signé) et `aether_plugin_api::PluginManifest` (Aether-Plugins) | Risque de divergence entre `--manifest` d'outillage et le manifeste signé | Feuille de route R6 |

Non retenu comme duplication : `aether-plugin-api` réexporte le SDK et ajoute des
aides propres aux fournisseurs (langues d'images, configuration) ; c'est une
couche plugin légitime. Les deux tables de limites (`subtitle_provider_limits`,
`automation_provider_limits`) ont des clés différentes ; leur fusion n'apporterait
rien sans migration (non proposée).

## Routes et tables spécifiques

- `applications.rs` relie en dur `requests → media-requests` et `reports → media-reports`.
  Rendre ce lien générique (« toute extension déclarant la permission ») élargirait
  silencieusement les privilèges : un autre éditeur pourrait servir les Signalements.
  Une généralisation exige une liaison explicite choisie par l'administrateur (R4).
- `GET /api/v1/extensions/features` renvoie un objet fixe `{requests, reports}` ;
  une liste des capacités actives serait additive et compatible (R4).
- `media_reports*`, `media_requests`, `acquisition_*` sont des **tables métier du
  Core**, pas des tables de plugin : elles restent où elles sont. Aucune suppression,
  aucune migration destructive.
- `application_scope.sql` cible des identifiants en dur ; migration historique
  idempotente, à conserver telle quelle.

## Stockage, quotas et droits imposés

Déclaré **et** imposé :

- `storage.private` vérifié à chaque requête de stockage et d'objets (`data.rs:20`, `data.rs:77`) ;
  un module qui importe le stockage sans la permission est refusé à l'usage.
- Quotas : 100 000 entrées / 64 Mio (`data.rs:13`), 64 objets / 128 Mio, objet ≤ 16 Mio,
  segments ≤ 16 Kio, 128 opérations broker par invocation (`run.rs`), HTTP 10 requêtes /
  256 Kio / 6 s, mémoire 16 Mio, fuel 2×10⁸, 65 s par exécution, 8 s pour Demandes/Signalements.
- Espace de données épinglé à l'éditeur et à sa clé (`data.rs:361`) : une réinstallation
  par un autre éditeur ne relit pas les anciennes données.
- Une mise à jour qui ajoute une permission ou un hôte HTTP est refusée
  (`extension_update_requires_authorization`, avec `newPermissions` et `newHttpHosts`) tant
  que l'administrateur ne l'a pas autorisée nommément (`approvedAccess` de l'installation) :
  pas d'extension de privilèges silencieuse. L'administration affiche les nouveaux accès et
  « Mettre à jour tout » n'en accorde jamais.
- Révocation surveillée toutes les 30 ms pendant un run (`worker_permitted`) ; génération
  revérifiée après Demandes/Signalements.

## Plugin désactivé, absent ou défaillant

- Admission par extension (une exécution à la fois) et créneau interactif réservé :
  une extension lente ne bloque ni les autres ni l'interface.
- Délais bornés et `kill_on_drop` : un worker bloqué est tué.
- Plugin absent/désactivé : `features` renvoie `false`, les routes Demandes/Signalements
  renvoient `503 extension_disabled`, la file d'acquisition s'arrête proprement
  (`acquisition/worker.rs`) ; le reste du serveur n'est pas affecté.
- Couplage à noter : Signalements et Demandes, entièrement natifs côté données, sont
  **indisponibles** sans leur paquet, alors que celui-ci ne fait (D3) qu'une validation
  redondante. Voir R1/R2.

## Migrations, désinstallation, réinstallation, retour arrière

- Mise à jour : instantané transactionnel (`extension_update_*`) des données, objets,
  espaces et grants ; retour arrière qui restaure le tout et laisse l'extension désactivée.
- Renommage de clés déclaré par `storageMigration` et exigé si des données privées existent.
- Rétrogradation refusée s'il existe des données privées ; garde version + SHA-256.
- Désinstallation : « conserver » purge seulement le cache et les envois incomplets ;
  « purger » supprime données, objets, espaces et historique de runs.
- Les tables métier (signalements, demandes) ne dépendent pas du cycle de vie du paquet.

## Cette Pull Request (première séparation sûre)

1. Nouveau module `extensions/worker.rs` : le Core possède un seul lanceur de worker
   et un seul cadrage borné. `applications.rs`, `run.rs` et `updates.rs` l'utilisent ;
   chaque appelant garde ses propres codes d'erreur, à l'identique.
2. `NATIVE_FEATURES` unique dans `repositories.rs` pour le masquage et le refus
   d'installation des fonctions natives.

Aucun changement de contrat SDK, de schéma SQL, de manifeste, de paquet signé ni de
réponse HTTP. Retour arrière : revert du commit.

## Feuille de route (hors de cette PR)

| # | Changement | Compatibilité et garde-fous |
|---|---|---|
| R1 | Signalements natifs : la validation du Core devient la seule source ; l'appel `reports.validate` devient facultatif (utilisé si le paquet est installé, comme politique supplémentaire), et la fonction s'active par un réglage du Core. Aligner d'abord les règles D3 en documentant la plus stricte. | Paquet 0.1.0 toujours accepté ; tables inchangées ; aucune donnée migrée. Gain : un worker en moins par signalement. |
| R2 | Demandes : le Core garde la validation structurelle ; le plugin garde `requests.plan` (politique de service, preuve single). Aligner les bornes D4 dans une nouvelle version signée du plugin, puis retirer l'appel `requests.validate` redondant. | Le Core accepte encore l'ancienne version du paquet pendant au moins une version mineure. |
| R3 | Poster Rotator : marquer le paquet obsolète dans le catalogue (`deprecated`), cesser d'en publier ; garder les versions signées existantes et la clé publique pour l'historique. | Le Core le refuse déjà ; rien à migrer. |
| R4 | Liaison capacité → extension choisie par l'administrateur, `features` en liste de capacités. | Valeur par défaut = liaison actuelle ; jamais d'élargissement implicite. Contrat HTTP additif. |
| R5 | Table unique permission ↔ opérations ↔ portée par défaut, partagée par `package.rs`, `adapters.rs`, `applications.rs`, `install.rs`. | Interne au Core ; tests de non-régression des refus existants. |
| R6 | Générer le manifeste d'outillage d'`aether-plugin-api` depuis le `Manifest` du SDK, avec test de parité. | Côté Aether-Plugins ; contrat SDK inchangé. |
| R7 | Pré-compilation/mise en cache du module Wasm par génération dans le worker, si les mesures montrent que la compilation domine. | Mesure d'abord ; pas d'état partagé entre extensions. |

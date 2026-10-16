# Architecture Rust — audit et feuille de route

Audit du 16 octobre 2026, branche `develop`, dépôt `maelmoreau21/Aether`.
Le code du répertoire de travail prime sur les jalons historiques. Le statut
initial était propre : aucun changement local non commité signalé par Git.
Ce document fixe une cible ; il ne déclare ni refactorisation ni P1 achevé.
Périmètre : lecture du backend, manifests, migrations, contrats Web et documentation ;
aucune modification de code, migration, Web ou packaging.

## 1. État réel et décisions prioritaires

| Domaine | Présent dans le code | Limite structurante |
| --- | --- | --- |
| Workspace | Une crate `media-poc`, dans `crates/core`, Rust 2024, MSRV déclarée 1.99 | `crates/shared` n'a pas de manifeste ; `brand.rs` est inclus par `#[path]` |
| Identité | SQLx/SQLite, bootstrap CLI, Argon2id, sessions, CSRF, deux rôles et permissions de bibliothèque | `IdentityStore` porte aussi le pool du catalogue, des bibliothèques et des jobs |
| Catalogue | Films persistés, Work/Edition/Version/Source/Asset/Track, NFO et affiches locales | DTO partiellement en `serde_json::Value`, catalogue de fixtures toujours mêlé aux handlers |
| Node local | Découverte MKV/MP4, cache de probe, NFO/images bornés, confinement, streaming Range | Résolution SQL et ouverture des médias se croisent entre Core et Node |
| Jobs | États SQLite, exclusivité par bibliothèque, un scan actif, interruption au redémarrage | Rapport et caches complets en RAM ; pas de protocole distant |
| Web | SvelteKit statique, `adapter-static`, `prerender`, servi par Rust sous `/app/` | Chemin de build relatif au répertoire courant, politique d'origine locale |
| Analyse média | `media.rs::probe`, processus `ffprobe` borné, hors lecture | Aucun parseur MKV/MP4 Rust natif livré ; pas de transcodage serveur |

Versions déclarées examinées : Axum 0.8.9, SQLx 0.9.0 avec SQLite bundled,
image 0.25.10, quick-xml 0.42.0 ; versions Web dans `apps/web/package.json`.
Ce relevé ne prétend pas établir les dernières versions disponibles ni leur sécurité.

Décisions retenues :

1. Conserver le tout-en-un comme référence : un serveur Rust, un Core, un Node local,
   SQLite et fichiers Web statiques. Aucun RPC réseau en mode autonome.
2. Extraire seulement deux crates : contrats partagés et Node. Authentification,
   base et catalogue restent des modules du Core. Pas de crates `auth`, `database`
   ou `media` supplémentaires tant qu'un consommateur indépendant ne les exige pas.
3. Retirer toute dépendance du Node envers `IdentityStore` et les routes Core.
   Le Core décide des droits et des identités éditoriales ; le Node observe et sert ses assets.
4. Préserver intégralement les migrations 0001–0006, UUID, relations et JSON techniques.
   Une extraction de fichiers Rust ne doit déclencher aucune réindexation destructive.
5. Mesurer puis borner catalogue, scan et images avant d'augmenter le parallélisme.
   La priorité AV1/Opus MKV/MP4 et les limites navigateur restent inchangées.

## 2. Frontières et workspace cible

```text
Cargo.toml                    members = core, node, shared
crates/shared/                package media-contracts, lib media_contracts
  src/{lib,brand,ids,media,inventory,errors}.rs
crates/node/                  package media-node, lib media_node
  src/lib.rs                  LocalNode ; aucune connaissance des utilisateurs
  src/{files,scanner,probe,nfo,images,stream,inventory}.rs
  src/bin/media-node.rs        ajouté au jalon distant seulement
crates/core/                  package actuel conservé pendant la transition
  migrations/                 fichiers existants inchangés, propriété du Core
  src/lib.rs                  assemblage des routes et services
  src/{auth,identity,permissions,libraries,catalog,scans}.rs
  src/db/{mod,identity,catalog,inventory,jobs}.rs
  src/playback.rs              résolution autorisée et descripteurs publics
  src/fixture_bench/           expériences et associations synthétiques
  src/main.rs                 hôte standalone : Core + LocalNode + statiques
  src/bootstrap.rs            bootstrap utilisant la même base
  src/bin/media-core.rs        hôte Core seul, au jalon distant
apps/web/                     inchangé par cet audit
```

Les noms de packages/binaires restent génériques ; les quatre noms d'images
imposés sont des noms de distribution. Ne pas renommer immédiatement `media-poc`
et casser les scripts, tests et travaux Docker en cours. La crate Core peut
conserver une dépendance Node pour composer le standalone : son binaire Core seul
ne doit ni initialiser le Node local, ni demander de montage média ou `ffprobe`.
Une feature Cargo dédiée au Node local n'est utile que pour alléger ce binaire.

Dépendances autorisées : `core → node → shared` et `core → shared`.
`shared` contient Serde, types et validation élémentaire, sans SQLx, Axum,
I/O disque ou processus. `node` ne dépend jamais de `core` ; pas de pool SQL Core
dans son API. Les tests Node doivent pouvoir compiler et tourner seuls.
Le Web reste client de `/api/v1`, sans dépendance aux structures SQL.

### Refactorisations ciblées

| Source actuelle | Destination / changement |
| --- | --- |
| `node/scanner.rs`, `node/nfo.rs`, `node/images.rs` | Crate Node, modules déjà sans SQL/HTTP ; séparer contexte de dossier et observations techniques |
| `media.rs` | `node::probe`, interface de probe et adaptateur ffprobe temporaire ; DTO techniques dans shared |
| `node.rs` | Scinder inventaire/validation de racines, handlers et banc synthétique ; abandonner les appels vers `crate::inventory` |
| `inventory.rs` | SQL/ingestion dans `core::db::inventory`, DTO dans shared, streaming dans Node, descripteurs dans playback |
| `identity.rs` | Conserver identité/bootstrap ; déplacer l'ouverture du pool et les migrations dans `core::db` ; renommer le store général sans changement SQL |
| `catalog.rs` | Handlers et DTO typés ; isoler les associations synthétiques, conserver chemins/champs publics |
| `scans.rs` | Routes admin/CSRF, orchestration et jobs durables Core ; exécution appelée par contrat Node |
| `permissions.rs` | Garder autorité Core ; remplacer progressivement l'analyse de chaînes d'URL par résolution typée de ressources |
| `lib.rs`, `main.rs` | Assemblage explicite des modes, configuration des statiques et arrêt propre |

`inventory.rs` (~39 Ko dont tests), `auth.rs` (~23 Ko), `node.rs` et scanner
(~20–21 Ko chacun) sont des points de vigilance, pas des preuves suffisantes de
mauvais découpage par leur seule taille. Le mélange SQL/HTTP/fichiers de
`inventory.rs` justifie sa séparation en premier.

Cycle logique observé : `node::content → inventory::content → node::scanner`,
et ingestion `inventory → node::{scanner,nfo,images}`. Aucun cycle Cargo ne peut
actuellement apparaître entre crates puisqu'il n'y en a qu'une ; déplacer les
modules sans changer ce sens de dépendance créerait le problème.

## 3. Contrats internes et persistance

### Contrat unique local puis distant

Commencer par des appels Rust typés et un canal borné d'observations. Le futur
transport HTTPS sérialise les mêmes messages, sans imposer HTTP au standalone.
API interne distincte proposée : `/internal/v1/...`, indisponible publiquement.
REST/JSON suffit pour contrôle et lots ; pas de bus, gRPC ou queue externe requis.

| Message | Champs / invariants à stabiliser |
| --- | --- |
| Core → Node `ScanRequest` | `scan_id`, `node_id`, `library_id`, `config_revision`, curseur et budgets ; racine enregistrée sur le Node, jamais chemin venant du lecteur |
| Node → Core `ScanStarted` | Identités et révision acceptées ; refus d'une configuration inconnue |
| Node → Core `InventoryBatch` | `scan_id`, séquence, observations bornées : clé physique locale, empreinte, pistes, NFO/image et diagnostics |
| Core → Node `BatchAck` | Dernière séquence durable et correspondances clé locale ↔ UUID Core ; aucun UUID recréé au rejeu |
| Node → Core `ScanFinished` | Dernière séquence, compteurs, couverture complète/incomplète et erreurs ; publication seulement après validation Core |
| Node → Core `ScanFailed` | Code typé, possibilité de reprise, révision ; aucune disparition globale déduite d'une panne |
| Core → Node `ReadGrant` | Asset/image, bibliothèque, Node destinataire, version, méthodes, expiration, session de lecture et identifiant de capacité |

`NodeId`, `LibraryId`, `WorkId`, `EditionId`, `MediaVersionId`, `SourceId`,
`AssetId`, `TrackId`, `ScanId` deviennent des newtypes transparents Serde.
Conserver les chaînes et UUID existants, y compris les identifiants historiques
`local` et `fixture-*` dans leurs contextes ; ne pas imposer UUID à tous les IDs.
DTO distincts pour observation physique, décision éditoriale et réponse Web.
Les suggestions d'identité du scanner ne fusionnent pas automatiquement deux Work.

Pistes : conserver JSON probe brut, index de flux, codec/type et champs absents.
Ajouter progressivement des vues typées : langue normalisée et langue brute,
timebase rationnelle, timestamps entiers, décalages, dispositions, canaux,
initialisation et pre-skip. Ne pas réécrire les JSON historiques ni convertir
les horodatages en flottants. Les associations externes/raw restent P3.
Erreurs bornées avec code stable, détail admin et motif public sans chemin disque.

### Propriété des données

- Core : comptes, credentials, sessions, grants, bibliothèques, registre des Nodes,
  graphe catalogue, UUID et état de publication des scans. SQLx SQLite concret,
  encapsulé dans des modules de requêtes, sans abstraction multi-SGBD générale.
- Node : racines locales autorisées, correspondance asset ↔ fichier, cache technique,
  futures préparations. En P1, adaptateur local alimenté par le Core ; en P2,
  inventaire opérationnel durable propre au Node, sans accès au SQLite Core.
- Le chemin relatif et la racine sont des informations internes confidentielles,
  jamais une entrée du streaming public. La racine distante est validée sur son
  Node ; Core ne la canonicalise pas sur son propre disque.
- Garder les UUID existants, unicités et `root_snapshot`. Les IDs de piste sont
  conservés par `(asset_id, stream_index)` tant que la piste existe. Le système
  actuel ne garantit pas la reconnaissance d'un renommage : ne pas la promettre.
- Préserver WAL, clés étrangères, migrations embarquées et verrou de migration
  `BEGIN IMMEDIATE` ; pool actuel de quatre connexions, attente busy de 15 s à mesurer.
  Le nom `IdentityStore` ne doit plus masquer cette base applicative unique.
- Pour remplacer le rapport RAM : staging durable par scan et lots idempotents,
  transaction courte de publication, ancienne génération lisible jusqu'à commit.
  Vérifier révision/racine avant publication ; aucun « absent » global après scan
  incomplet. Les erreurs individuelles peuvent déjà rendre leur asset indisponible.
  Toute nouvelle table nécessite une migration additive ultérieure, hors audit.
- PostgreSQL 18 externe reste une option future : SQL spécifique, tests et outil
  de transfert vérifié requis. Changer l'URL ne transfère aucune donnée. Pas de
  SQLite partagé entre Core actifs, ni base sur montage réseau supposé sûr.

## 4. Sécurité et lecture

Le Core authentifie et autorise avant catalogue, descripteur, GET/HEAD, Range,
image et sous-titre. Seuls `administrator` et `user` existent ; aucun privilège
implicite via profil ou futur OIDC. Conserver session hachée/révocable, CSRF,
rate limiting et bootstrap CLI sans identifiants prédéfinis.

Le Node n'accepte que des références inventoriées. Il ouvre en lecture seule,
depuis une racine opérateur autorisée, refuse liens/reparse points et chemins
sortants. **Priorité de durcissement : ouvrir relativement à un handle de racine
avec politique anti-liens, puis vérifier et streamer ce même handle.** Les appels
actuels `confined_file` puis `ServeFile` rouvrent le chemin : TOCTOU résiduel.
Le montage `:ro` empêche l'écriture par l'application, pas la substitution par
un autre processus hôte. Tester la primitive sur Linux et Windows avant choix.

En local, un objet de lecture autorisé non constructible par un handler non
autorisé remplace la capacité réseau. En distant, capacité authentifiée limitée
à l'asset/Node et de courte durée, jamais cookie utilisateur transmis au Node.
Préférer une signature vérifiable avec clé publique Core et rotation ; ne pas
inventer de cryptographie. Prévoir révocation par contrôle Core/lease courte,
refus après expiration et politique explicite de partition réseau. Aujourd'hui,
la révocation bloque les nouvelles requêtes, pas un corps déjà ouvert.

Appairage P2 : action admin, code à usage unique expirant, identité et secret/clé
par Node, TLS, renouvellement/révocation, protection contre rejeu. Ne jamais
faire confiance au `node_id` d'un lot sans authentifier son émetteur. Les URLs
Node sont configurées et validées, pas des cibles arbitraires fournies au lecteur
(SSRF). Données direct Node quand joignable ; relais Core en dernier recours.
La stratégie CORS/CSP/origine et capacités de lecture doit être testée avant
séparation Web/Node sur plusieurs origines ; conserver même origine pour P1.

Le probe externe reste borné (10 s, JSON 1 Mio, protocoles fichier et formats
limités), sans sandbox complet. Rust natif est la cible, pas une raison pour
remplacer aveuglément le probe : comparer tous les champs/fixtures AV1+Opus,
seek et erreurs avant substitution. Aucun transcodage vidéo obligatoire.

## 5. Cinq principaux risques techniques

| Priorité | Risque constaté | Action et preuve attendue |
| --- | --- | --- |
| 1 | Réouverture des chemins après confinement ; analyseurs de fichiers non fiables | Streaming du handle vérifié, tests de substitution/liens, budgets parseurs et ACL explicites |
| 2 | Cycle logique Core/Node, DTO JSON et `local` codé dans les SQL | Extraction dans le sens unique défini ; Node compilable seul ; contrats de sérialisation et tests des permissions inchangés |
| 3 | RAM O(N) : cache SQL `fetch_all`, rapports et clones de probes ; catalogue tout chargé puis filtré/trié/paginé en Rust | Lots/staging, cache borné ; filtre droits/recherche/tri/LIMIT en SQL avec parité Unicode ; mesurer RSS et allocations |
| 4 | I/O/CPU : relecture de dossiers par vidéo potentiellement quadratique ; affiches redécodées à chaque GET/HEAD sous un sémaphore unique | Contexte de dossier calculé une fois ; cache dérivé borné hors médias ; conserver contrôle de droits et invalidation fiable |
| 5 | Contention SQLite et saturation : nombreuses requêtes par fichier dans une transaction globale, requêtes imbriquées des fiches, file de scans et connexions sans budget global | Compter les requêtes, `EXPLAIN QUERY PLAN`, publication courte, admission bornée et tests de lenteur/annulation ; mesurer latence auth pendant scan |

Les buffers de streaming de 64 Kio ne représentent pas la RSS par spectateur.
Les images limitent entrée à 10 Mio, dimensions à 4096 et pixels décodés à 32 Mio ;
le budget décodeur est de meilleur effort. Argon2 tourne déjà en `spawn_blocking`
avec deux permis conservés dans le worker ; les images utilisent aussi un worker
borné. Garder ces protections, mesurer leur cumul avec probe et SQLite. Déplacer
hors des workers async les futurs traitements CPU ; ne pas paralléliser sans budget.

## 6. Mesures et critères de performance proposés

Aucun résultat de charge ni objectif RAM absolu n'est établi par cet audit.
Protocole reproductible : build release verrouillé, OS/CPU/RAM/disque et versions
consignés, sources synthétiques légales, mêmes données avant/après, trois passes
à froid et à chaud. RSS/arbre de processus inclut `ffprobe` ; mémoire cgroup en
conteneur. Séparer temps serveur et démarrage/décodage navigateur.

| Scénario | Mesures | Critère d'acceptation proposé, à valider sur matériel cible |
| --- | --- | --- |
| Catalogue 1k/10k/100k œuvres, deux utilisateurs aux grants distincts | p50/p95/p99, allocations, lignes SQL lues, plans, RSS | Page bornée à 100, droits dans SQL, aucun DTO construit pour tout le catalogue ; p95 et pic RSS sans régression >10 % sur même banc |
| Scan 1k/10k fichiers, dossiers dispersés puis denses | fichiers/s, probes lancés/réutilisés, CPU, RSS, I/O, temps de verrou | Deuxième scan inchangé : zéro probe vidéo ; queue bornée ; IDs conservés ; après lots, mémoire de travail ne croît plus avec tous les probes |
| Vidéos 1/10 puis 100 clients si matériel suffisant, Range/seek/slow clients | débit, latence premier octet/seek, RSS, descripteurs, erreurs, CPU | Pas de lecture intégrale ; 206/416/HEAD corrects ; buffers/connexions bornés ; annulation libère les ressources ; budget de concurrence documenté |
| Affiches valides/hostiles, 1/10/50 requêtes | p95, décodages, attente worker, RSS, invalidation | Une seule validation active conservée initialement ; attente bornée ; aucune lecture sans droit ; changement/retrait détecté |
| Scan + navigation + connexion + arrêt forcé | latence API, SQLITE_BUSY, WAL, durée de publication, reprise | Aucun job fantôme après restart, aucune disparition sur parcours incomplet, permissions toujours actives, pas d'erreur 5xx sous la charge nominale choisie |

Le seuil relatif de 10 % est un garde-fou proposé, pas un résultat. Fixer ensuite
des budgets absolus à partir de la baseline sur machine de développement et
DS1522+ ou matériel équivalent. Ne pas extrapoler Windows local vers NAS.
Benchmarks des parseurs/projections en Rust ; tests HTTP avec vrais corps et
annulations ; navigateur pour AV1/Opus, audio, pause/seek et erreurs. Une mire
courte ou un manifeste DASH ne prouve ni débit long métrage ni Direct Play MKV.

## 7. Quatre images, sans Node.js au runtime

| Image cible | Contenu / mode |
| --- | --- |
| `aether` | Build Web statique + serveur standalone Rust, Core/SQLite et LocalNode dans le même processus ; médias ro et données/cache séparés |
| `aether-web` | Fichiers statiques servis par serveur HTTP léger ; aucun serveur SvelteKit/Node.js permanent |
| `aether-core` | Binaire Core/API, DB et bootstrap ; sans scan de disques distants, sans montage média obligatoire |
| `aether-node` | Binaire Node et inventaire opérationnel local ; scans/probe/stream, sans base utilisateurs |

La faisabilité sans Node.js runtime est étayée par `vite.config.ts`, le
`prerender` et `lib.rs::public_routes` utilisant `ServeDir`. Node.js sert au build
et aux tests seulement. Le serveur Rust peut servir les fichiers construits
sur disque ; les embarquer dans le binaire n'est pas nécessaire.
**Un processus serveur Rust ne signifie pas zéro sous-processus** : `ffprobe`
reste nécessaire aux scans actuels. Ne pas le retirer des images qui scannent
avant remplacement validé ; pas de FFmpeg CLI requis pendant la lecture.

Points à transmettre à l'IA packaging : chemin absolu/configuré du build Web,
health/readiness, user non-root, volumes distincts, démarrage/arrêt des jobs,
SIGTERM en plus de Ctrl+C et accès réseau explicite. Le `main.rs` actuel écoute
sur loopback exclusivement ; le publier dans un conteneur ne suffit pas à le
rendre joignable. Autoriser une écoute réseau exige un contrat d'origine/proxy
et de cookies cohérent, sans supprimer les vérifications Host/Origin.
Les quatre noms d'images sont la cible ; les binaires séparés ne sont pas livrés.
Cet audit n'a ni construit ni validé une image, et ne modifie pas ce packaging.

## 8. Plan en trois macro-jalons maximum

### J1 — Consolider les frontières sans changer le produit (P1)

Extraire shared/Node, découper inventaire SQL/lecture, isoler les expériences,
garder API/Web/migrations identiques. Ajouter tests de contrats et ouvrir le
même handle vérifié jusqu'au streaming. Pas de Node réseau.

Réussite : `cargo metadata` montre le graphe acyclique ; tests Node sans Core ;
fmt, Clippy strict et tests Rust passent ; migrations et IDs comparés avant/après
avec réouverture de la même SQLite ; parcours Film/NFO/affiche/lecture et refus
GET/HEAD/Range inchangés. Tests de substitution Linux/Windows et scan incomplet.

### J2 — Fermer un standalone utilisable et mesuré (P1 / V1 locale)

Appliquer pagination SQL, scan en lots après baseline, bornes d'admission,
annulation et cache images selon mesures. Achever installation/bootstrap,
gestion des comptes et récupération locale, configuration bibliothèques/grants,
diagnostic des erreurs, sauvegarde/restauration minimale vérifiée et arrêt propre.
Préserver le lecteur et rendre explicitement les incompatibilités AV1/Opus.
Coordonner le packaging avec l'autre IA, sans réécrire son travail.

Réussite : installation vide sans compte par défaut, administrateur puis user
avec grants distincts, film scanné et relu après restart, interdictions vérifiées,
originaux inchangés, backup restauré avec IDs/grants conservés, mesures §6 publiées.
Image tout-en-un sans Node.js runtime testée sur Linux ; lecture pause/seek/Range
et erreur codec observés sur clients ciblés. Décider explicitement si V1 est
Films uniquement : les séries réelles ne sont pas livrées par les fixtures.
Une V1 annoncée Films/Séries exige scanner et parcours épisodes/saisons testés.

### J3 — Distribuer le Node et ajouter OIDC (P2)

Ajouter binaires Core seul/Node, transport des contrats, inventaire Node durable,
appairage/révocation et lecture directe autorisée. Puis OIDC Code+PKCE, sessions
Core et mapping admin explicite, sans supprimer la récupération locale.
Un Core unique et SQLite restent la référence ; PostgreSQL n'est pas prérequis.

Réussite : deux machines, un Core et deux Nodes ; redémarrage et perte réseau,
rejeu de lots sans doublon, refus de mauvais Node/grant expiré, rotation/révocation,
aucune fuite de bibliothèque, chemins ou credential utilisateur ; streaming direct
et relais restreint démontrés. Authentik valide issuer/audience/signature/state/
nonce/PKCE et l'absence d'élévation implicite. Chaque parcours garde une preuve propre.

Après ces jalons : P3 pistes externes puis raw après POC, P4 progression/reprise,
profils et enrichissement séries, P5 ordonnanceur avancé/télémétrie/charge étendue.
La sauvegarde minimale et les mesures de sécurité/performance nécessaires à V1
ne sont pas repoussées à P5. Multi-Core, Kubernetes, plugins et recommandations
restent hors MVP.

## 9. Tâches adaptées à une IA moins coûteuse

- Extraire mécaniquement les DTO/brand et ajuster les imports après validation
  des contrats ; vérifier JSON compatible et graphe Cargo, sans renommer les IDs.
- Déplacer les tests et expériences sous le module dédié, avec preuve que le
  binaire normal n'active jamais le constructeur sans authentification.
- Ajouter des tests de non-régression DTO/UUID/restart avec fixtures existantes,
  sans toucher aux migrations ni inventer des capacités du lecteur.
- Préparer générateur de catalogue synthétique et harnais de mesure §6 ; produire
  CSV bruts et paramètres, sans interpréter des résultats absents.
- Relever les plans SQL et requêtes par endpoint, puis proposer un changement
  limité de pagination ; conserver la sémantique Unicode et les permissions.

Garder les décisions de sécurité des handles, du protocole de capacités, de
publication transactionnelle et de migration des données sous revue principale.
Pas de délégation simultanée des mêmes fichiers avec l'IA packaging.

## 10. Vérification de cet audit

Inspection des sources, manifests, six migrations et tests présents ;
`cargo metadata --locked --no-deps --format-version 1` exécuté avec succès.
Résultats de la suite Rust et vérification du diff consignés dans `MEMORY.md`
avec cette décision : fmt et Clippy strict réussis, suite Rust 50 réussis / 5
ignorés (quatre navigateurs et un helper subprocess), `git diff --check` réussi.
Deux éléments non suivis (`.agents/`, `ORIGINAL_REQUEST.md`) sont apparus pendant
l'audit et ont été laissés intacts. Les preuves navigateur historiques ne sont pas présentées
comme de nouveaux essais ; aucun benchmark, NAS ou déploiement exécuté ici.

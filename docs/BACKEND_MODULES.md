# Modules internes du backend — 4 novembre 2026

Les crates et leurs dépendances restent inchangées : Core (`media-poc`) dépend de
Node, des contrats et du SDK ; Node dépend des contrats, jamais de Core/SQLx.
Les features Core restent `fixture-bench` et `query-metrics` ; Node n'en déclare
pas. Les routes, JSON, migrations, protocoles, configurations et formats médias
ne sont pas modifiés par ces extractions.

Les trois fichiers initiaux sont remplacés par 25 fichiers Rust : 19 modules
d'implémentation, trois façades et trois fichiers de tests. Les petits tests
privés du descripteur restent aussi colocalisés dans `inventory/playback.rs`.

Ordre retenu : inventaire (bénéfice fort, risque transactionnel maîtrisé en
conservant la publication intacte), puis Nodes et serveur Node (bénéfice fort,
risque plus élevé d'autorisation/transport). Les lots sont appliqués et validés
successivement. Les extractions d'identité du scanner et de worker de sessions
restent les suites les plus autonomes ; les moteurs médias et leases demandent
davantage de précautions. Le packaging n'a pas un bénéfice de découpage comparable.

## Inventaire Core

`inventory.rs` (5 560 lignes initialement) devient `inventory/mod.rs`, façade
compatible pour les 41 noms publics ou visibles dans la crate.

| Module privé | Responsabilité |
|---|---|
| `scan` | Caches incrémentaux, ingestion historique, staging idempotent, backfill Unicode |
| `publication` | Publication atomique Films/Séries, sources, pistes et instantané d'automatisation |
| `editorial` | Réconciliation NFO/métadonnées, consolidation et diagnostics |
| `catalogue` | Résolution des bibliothèques de ressources, filtres, pagination, fiches et éditions |
| `catalogue::cache` | Cache borné existant, génération, budget et invalidation |
| `playback` | Descripteurs, sélection des sources/pistes, identifiants audio et chapitres validés |
| `resources` | Livraison locale/distante, préparations, audio, sous-titres et images |

`scan` et `publication` importent explicitement `editorial`. `resources` utilise
les helpers de `playback`, sans dépendance inverse. La résolution d'une
bibliothèque reste dans `catalogue` : l'autorisation du relais Node n'a pas à
dépendre des handlers de livraison. Cette résolution ne remplace pas les
contrôles de session, de bibliothèque et de politique parentale des appelants.
Le verrou du cache est visible uniquement par son module catalogue parent.

La transaction de publication possède toujours validation de configuration,
indisponibilité des absents, graphe Films/Séries, éditorial/consolidation,
instantanés de sous-titres/audio, outbox, planification et nettoyage du staging.
Les helpers éditoriaux reçoivent la même transaction ; `trim_memory` reste après
le commit. La fonction de publication reste longue : son découpage transactionnel
plus fin et la duplication d'upserts éditions/versions sont volontairement reportés.

## Gestion des Nodes Core

`nodes.rs` (3 628 lignes) devient une façade conservant ses 29 noms publics ou
visibles dans la crate, son état d'authentification et son assemblage de routes.

| Module privé | Responsabilité |
|---|---|
| `registry` | Enregistrements, projections administrateur, télémétrie et sélection en cache chaud |
| `transport` | Validation des URL, authentification signée et transport d'appairage/rotation |
| `channel` | Canaux sortants, heartbeats et rendez-vous bornés des flux de relais |
| `scans` | Dispatch, fencing des tentatives, callbacks et synchronisation d'inventaire |
| `management` | Administration, enrollment, reconnexion, rotation et révocation |
| `playback` | Capacités directes, autorisation vivante et relais HTTP borné |

Entre implémentations, `registry` est la base ; `transport` l'utilise, puis
`channel`. `scans`, `management` et `playback` importent leurs dépendances précises.
Registre en mémoire et projections SQL restent ensemble pour éviter une
dépendance réciproque entre types, télémétrie et requêtes administrateur.
Les deux statiques de transport existantes restent uniques et locales au processus.
Les gardes de relais restent capturées par les corps HTTP jusqu'à leur destruction.

## Serveur Media Node

Le découpage de `server.rs` (3 664 lignes) comporte `state` (configuration,
identité et inventaire persistants), `control_transport` (signatures et HTTP de
contrôle), `scans` (admission/annulation/priorité lecture), `playback` (médias,
préparation et durée de vie des corps), `outbound` (canal Core et uplinks),
`routes` (assemblage HTTP et cycle de vie). La façade conserve les 12 noms
publics existants, dont `send_signed_http_request` et les gardes de flux utilisés
par Core.

Les dépendances des implémentations suivent `state`, puis `control_transport`,
`scans`, `playback`, `outbound` ; `routes` assemble les handlers. Les imports
entre frères sont explicites, sans glob. Deux champs d'état auparavant privés
(`identity_changes`, `preparations`) et le DTO de requête `StreamQuery` deviennent
`pub(super)` pour les consommateurs internes ; leur visibilité externe reste
fermée. Le correctif HEAD des uplinks de chapitres, déjà présent avant ce lot,
est conservé. Admission, ordre des verrous, détention des slots après annulation,
persistance d'identité et framing/signature des réponses restent inchangés.

## Audit des autres domaines

| Domaine examiné | Décision / frontière utile pour un prochain lot |
|---|---|
| `auth` (~1 925 lignes, nombreux tests) | Worker borné de rafraîchissement des sessions isolable ; conserver ensemble vérification de session, cookies/CSRF et throttle |
| `extensions` (~2 778) | Validation pure, lifecycle et invocation/broker à distinguer ; sous-modules existants, grants et verrou de mutations à respecter |
| `automation` (~1 911) | Cache fournisseur autonome ; admission/outbox et application des résultats partagent des invariants de lease et d'atomicité |
| `parties` (~3 395) | Protocole, rooms, REST et WebSockets identifiables ; mutations SQL/room/broadcast fortement entrelacées |
| `scanner` (~2 037) | Parsing d'identité Films/Séries isolable ; orchestration, annulation et contexte de dossier à préserver |
| `mp4_audio` (~1 835) | Parse/index, fenêtres et corps virtuel identifiables ; invariants communs de tables, timebases, pré-roll et bornes sensibles |
| `packaging` (~1 247, dont ~692 de tests) | Production cohérente cache/admission/processus ; pas de découpage massif justifié |

`oidc`, avatars d'extensions, `assets`, `product`, `libraries` et `consolidation`
ont aussi été recensés ; leur taille seule ne justifie pas une extraction.
Observation statique à instruire séparément : `WatchPartyHub::get_or_load_room`
charge SQL après libération du read lock puis insère sans revérifier l'entrée ;
deux chargements concurrents pourraient créer deux rooms. Non reproduit, non corrigé.
La publication conserve aussi son traitement existant de l'erreur ignorée de
`schedule_library` ; aucune correction fonctionnelle n'est mêlée aux déplacements.

## Vérifications locales

Environnement : Windows, Rust 1.99.1, FFmpeg 9.0.2. Cargo exécuté avec `-j 1`
après un échec initial `LNK1102` avant modification (mémoire du linker).

| Lot | Référence | Après extraction |
|---|---|---|
| Inventaire | 8 unitaires ; intégrations catalogue/scans/NFO/images/séries/consolidation/publication | 8 unitaires et 18 intégrations réussies |
| Nodes | 5 unitaires ; enrollment/relais, deux Nodes, versions, panne/reprise et chapitres réels TCP | 13 unitaires inventaire/Nodes et 6 intégrations réussis |
| Serveur Node | 9 unitaires serveur et 3 contrats de transport réussis | 9 unitaires, 3 contrats transport et 7 intégrations Core finales réussis |

Les 21 intégrations de référence ont réussi. Le benchmark catalogue à un million
d'œuvres et celui à 100 Nodes simulés sont exclus des commandes ciblées, sans
modifier leurs tests. Le contrat TLS avec certificats éphémères n'est pas exécuté.
Les tests/conditions/assertions déplacés sont conservés.

Targets d'intégration exécutés : `catalog_performance`, `scanner_flow`,
`series_flow`, `nfo_flow`, `posters_flow`, `concurrency_writes_during_publication`,
`global_catalogue_consolidation`, `distributed_multi_node`,
`node_enrollment_and_relay`, `version_and_nodes`, `multi_node_scale_and_failover`
et `chapter_images_remote`. Filtres unitaires : `inventory::`,
`nodes::transport_policy_tests` et `server::stream_signature_tests` ; transport
Node : `internal_transport`. Le formatage global et `git diff --check` passent.
Aucune régression détectée dans ces parcours. Les erreurs d'import du harnais
extrait ont été corrigées ; aucune assertion n'a été modifiée pour faire passer
les tests. La suite générale de tests workspace n'a pas été exécutée.

Une comparaison syntaxique locale des éléments Rust, y compris tests, constantes
et littéraux SQL, confirme l'identité des 200 éléments : 60 d'inventaire, 65
Nodes et 75 serveur. Elle normalise les fins de ligne et les virgules finales
facultatives des paramètres ajoutées par rustfmt. Elle exclut les imports,
enveloppes de modules et visibilités,
revus séparément ; un contrôle des réexports vérifie aussi les noms et visibilités
de façade. Aucun wrapper, crate, verrou, allocation ou clone n'est ajouté aux
chemins d'exécution. Les copies initiales et journaux locaux sont sous
`target/refactor-baseline/` et `target/refactor-*.log` (non versionnés).

Mesures ponctuelles du banc existant `catalog_performance`, avant/après le lot
inventaire, profil test non optimisé, même machine, `-j 1`, un thread de tests :

| Catalogue 10 000 œuvres | Avant | Après inventaire |
|---|---:|---:|
| Page initiale, 24 éléments | 1,16 ms | 0,568 ms |
| Page profonde (offset 9 960) | 38,38 ms | 18,17 ms |
| Recherche | 27,58 ms | 23,27 ms |

Échantillons uniques, sensibles au cache/bruit : aucun gain de performance n'est
revendiqué. Pas de mesure globale de streaming, RSS ou contention, ni preuve
NAS, PostgreSQL, Linux/ARM ou décodage navigateur apportée par ce lot.
`cargo fmt --all --check` et
`cargo clippy --locked -j 1 --workspace --all-targets --all-features -- -D warnings`
réussissent sur l'état final. Aucun commit, push, build/publication Docker ou
déploiement n'est effectué. Les modifications préexistantes du worktree sont
conservées ; les autres domaines de l'audit restent hors de ces trois lots.

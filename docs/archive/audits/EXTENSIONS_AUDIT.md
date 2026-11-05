# API d'extensions — audit initial du 20 octobre 2026

Statut : étape 1, inspection statique du checkout local. Aucun moteur, endpoint,
SDK ou mécanisme d'isolation livré par cet audit. Les modifications préexistantes
du catalogue, du lecteur, du Node et des tests ont été conservées.

## Périmètre et critères

Identifier les composants réutilisables, les manques et les décisions nécessaires
avant les contrats versionnés. Respecter l'ordre des quinze sections demandées
le 20 octobre : audit, contrats, manifeste/permissions/isolation, extension
synthétique, tâches/médias, SDK, administration/catalogue, Trickplay, Intro Skipper.
La lecture AV1/Opus reste prioritaire et indépendante des extensions.

Critères de cet audit : références au code actuel, distinction entre mécanismes
existants et exigences futures, absence de second RBAC ou planificateur proposé,
premier parcours testable explicite. Aucun résultat historique de tests ne vaut
validation de ce checkout.

## Réutilisation et écarts

| Domaine | Point d'appui actuel | Écart à traiter |
| --- | --- | --- |
| Sessions et administration | `crates/core/src/auth.rs` : `AuthState::authenticate`, `require_csrf`; rôles existants | Une extension doit avoir une identité propre, sans session administrateur héritée. |
| Bibliothèques | `crates/core/src/permissions.rs` : `require_media`, vue `effective_library_permissions` | Ajouter les capacités explicitement accordées à l'extension aux contrôles existants. Le raccourci administrateur ne doit jamais lui donner accès à toutes les bibliothèques. |
| Catalogue et pistes | `crates/core/src/inventory.rs`, `crates/shared/src/ids.rs`, `inventory.rs`, `media.rs` | DTO minimisés sans chemins, pagination, droits par ressource, provenance des enrichissements et distinction œuvre/édition/version/source/asset/piste. |
| Accès physique | `crates/node/src/files.rs` : `OpenedFile::open_in_root`, `verify_unchanged` | Résolution depuis l'inventaire et opérations bornées pour extensions; ne pas exporter les chemins ni assimiler un descripteur confiné à une sandbox. |
| Core/Node | `crates/shared/src/protocol.rs`, `crates/core/src/nodes.rs`, `node.rs` | Le protocole interne versionné et ses jetons de lecture ne sont pas un protocole d'extensions. Ajouter des opérations dédiées, avec contrôle vivant des droits et révocation. |
| Tâches | `crates/core/src/scans.rs` : `ScanJobs`, sémaphore, annulation; `scheduler.rs` et migration `0021_scheduled_tasks.sql` | Planification persistante spécialisée scans; pas de registre générique de tâches d'extensions, priorité, reprise par checkpoint ou quotas CPU/RAM par extension. Étendre progressivement ces mécanismes, sans créer une file parallèle. |
| Préférences et lecteur | `crates/core/src/preferences.rs`, `apps/web/src/lib/player/types.ts` : interface `Player`, état et `seek` | Pas de contrat public de segments, Trickplay, corrections manuelles ou contributions déclaratives. Garder les choix utilisateur et le moteur vidéo derrière l'interface existante. |
| Administration | `apps/web/src/lib/admin/AdminCapabilities.svelte` | Présentation du parcours futur seulement; aucun gestionnaire installé. Ajouter les écrans après le parcours synthétique. |
| Dépendances | Workspace Core/Node/Shared et manifestes Cargo inspectés | Aucun runtime d'extensions identifié. Les dépendances transitives `wasi`/`wasm-bindgen` du lockfile ne prouvent pas un hôte Wasm. |

## Capacités absentes

Manifeste et validation typée, paquet signé et chaîne de confiance, installation
atomique, registre/versionnement, migrations et rollback, identité d'extension,
runtime isolé, quotas, broker réseau/secrets, stockage privé, livraison durable
des événements, SDK Rust et contrat OpenAPI d'extensions restent à construire.
Les identifiants d'événements, déduplication, délais, reprise et ordre de livraison
doivent être définis avant de promettre des hooks fiables.

Les contrats doivent couvrir les quatre extensions de référence sans les
implémenter ensemble : ressources dérivées et manifestes pour Trickplay;
segments et corrections pour Intro Skipper; propositions avec provenance et
priorités pour les métadonnées; droits séparés pour statistiques personnelles
et agrégées. Aucun calcul lourd dans les requêtes vidéo.

## Arbitrages pour l'étape des contrats

- « Indépendante du Core » signifie contrat versionné séparément, hébergé par
  les mécanismes existants; aucun service permanent supplémentaire imposé.
- La compatibilité descendante exige une matrice de versions et des tests;
  elle ne peut être une garantie illimitée pour toutes les versions futures.
- Les capacités d'écriture/suppression des médias évoquées dans l'ancien
  `EXTENSIONS.md` sont exclues de cette demande : les originaux restent en lecture seule.
- La livraison doit suivre le nouvel ordre : le catalogue et son interface ne
  sont plus des prérequis du premier test synthétique, contrairement à la fin
  du document du 19 octobre.
- Wasm/WASI et processus isolé restent candidats. Comparer arrêt effectif,
  boucle CPU, allocations, blocage d'appel hôte, accès disque/réseau et crash;
  mesurer démarrage/RSS/CPU sur les OS ciblés avant le choix. Un processus seul
  ou Wasm seul ne constitue pas une preuve d'isolation complète.
- Les permissions d'extension sont une restriction supplémentaire appliquée
  par les contrôles Core/Node existants, avec consentement par bibliothèque;
  aucun rôle supplémentaire ni RBAC concurrent.

## Prochain jalon proposé : contrats uniquement

Produire un manifeste versionné, des types Rust/JSON et un document OpenAPI
cohérents pour un sous-ensemble synthétique : identité/version, compatibilité,
`metadata.read`, bibliothèque accordée, entrée bornée et proposition de résultat.
Définir erreurs, quotas, révocation, provenance et versions avant le runtime.
Ne pas annoncer ces routes disponibles avant raccordement et tests HTTP.

Puis éprouver un paquet synthétique local signé par une clé de test, autorisé
sur une bibliothèque, retournant une proposition sans modifier les médias.
Les tests du futur parcours devront refuser signature/digest invalides,
archives piégées, version incompatible, absence de consentement, accès à une
autre bibliothèque, appel après révocation et dépassements de quotas; vérifier
l'arrêt pendant traitement et le fonctionnement du Core sans extension.

## Validation et limites de cet audit

Inspection de `git status --short`, `git diff --stat`, des documents de référence,
des manifestes Cargo et des sources ciblées. Le test existant
`crates/core/tests/scheduler.rs` couvre notamment administration/CSRF,
persistance et dispatch de scans; il n'a pas été relancé pour cet audit documentaire.
Aucun test runtime, navigateur, multi-Node, SQLite/PostgreSQL ni mesure de
ressources exécuté. Aucun choix de version de runtime ou de nouvelle dépendance.

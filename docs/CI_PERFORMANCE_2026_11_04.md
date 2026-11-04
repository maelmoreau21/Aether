# Optimisation CI/CD — audit du 4 novembre 2026

État initial : `develop` et `origin/develop` au commit `743cc613e89f761eae1f9d6763c2f50560233b41`, worktree propre. Branche de travail : `perf/ci-build-once`. Aucune modification métier ni du profil release ; aucune publication expérimentale autorisée ou lancée.

## Mesures initiales

Données publiques de GitHub Actions, jobs et étapes conservés dans [les preuves JSON](evidence/2026-11-04/ci-performance/). Les journaux ont été consultés via le connecteur GitHub. La CLI `gh` locale n'est pas authentifiée ; l'API publique fournit les horodatages. Le temps de workflow ci-dessous va de `run_started_at` à la fin du dernier job, sans assimiler `updated_at` à la fin des calculs.

| Run develop | État | Temps réel | Somme des jobs | Estimation arrondie par job |
|---|---|---:|---:|---:|
| [41903062442](https://github.com/maelmoreau21/Aether/actions/runs/41903062442) | succès | 20 min 44 s | 78 min 50 s | 88 min |
| [41819674651](https://github.com/maelmoreau21/Aether/actions/runs/41819674651) | succès | 21 min 57 s | 72 min 07 s | 81 min |
| [41906877646](https://github.com/maelmoreau21/Aether/actions/runs/41906877646) | succès, HEAD initial | 21 min 32 s | 79 min 38 s | 90 min |
| [41897534982](https://github.com/maelmoreau21/Aether/actions/runs/41897534982) | échec formatage ; smoke AMD64 annulé | 35 min 31 s | 80 min 46 s | 87 min |

La somme des jobs représente l'occupation observée des runners. L'arrondi à la minute par job est une estimation technique, pas une facture : visibilité publique, plan, gratuité, tarifs et crédits ne sont pas déduits. Attentes de démarrage et orchestration appartiennent au temps réel, pas à la somme des jobs. Le dernier run était en cours au début de l'audit et a été relevé une fois terminé.

Run détaillé `41903062442` :

| Mesure | Avant | Après | Gain |
|---|---:|---:|---:|
| Rust, maximum des 4 jobs | 10 min 55 s | À mesurer | Non calculable |
| Rust, commandes des partitions 0/1/2 | 271 / 424 / 456 s | À mesurer | Non calculable |
| Docker AMD64, build Bake CI | 462 s | À mesurer | Non calculable |
| Docker ARM64, build Bake CI | 448 s | À mesurer | Non calculable |
| Navigateur, job complet | 319 s | À mesurer | Non calculable |
| Frontend, job complet | 84 s | À mesurer | Non calculable |
| GHCR, matrice de publication après CI | 439 / 515 s | À mesurer | Non calculable |
| GHCR, manifests seuls | 37 s | À mesurer | Non calculable |
| CI/CD complet | 1 244 s | À mesurer | Non calculable |
| Occupation cumulée runners | 4 730 s | À mesurer | Non calculable |

Le chemin critique passe par Rust `checks` (655 s), le gate agrégé, le calcul/tag bêta puis la reconstruction ARM64 de publication (515 s), enfin les manifests. La phase GHCR commence après toute la CI, ce qui additionne ses builds aux validations. Le solve Bake de publication prend 372 s en AMD64 et 434 s en ARM64. Les builds CI et publication n'ont pas la même version embarquée : le cache ne peut éviter cette compilation finale. Le run `41819674651` montre des builds CI très chauds (Bake 59/36 s), mais une publication toujours longue. Aucun de ces runs n'établit un cold build intégral.

## Audit des caches et tests

- Cargo : restauration effective d'environ 485 Mo, en 6–8 s, même restore-key entre les partitions. Les dépendances sont réutilisées ; les crates workspace sont recompilées, ce qui est attendu avec rust-cache. Seul `checks` sauvegarde le cache commun. Le navigateur a son cache distinct et ses features propres : aucun partage forcé incompatible.
- Fixtures : hits exacts, environ 1,4 Mo pour Rust et 3,2 Mo pour le navigateur. Les fixtures Rust restaurées restent validées. Les deux jeux sont volontairement distincts.
- FFmpeg : hit exact d'environ 25,7 Mo incluant OS, architecture, distribution et script. Les bibliothèques runtime et le vrai filtre `lavfi` sont toujours contrôlés. L'action composite évite déjà apt quand les binaires sont exécutables : aucun changement inutile.
- npm : cache présent ; `npm ci` reconstruit normalement `node_modules` (31–41 s ici). Il n'est pas remplacé par un cache de dépendances installées non prouvé.
- BuildKit : étapes FFmpeg/OS/Web et dépendances souvent `CACHED`, scopes AMD64/ARM64 séparés. Les deux anciens scopes restent lisibles pour ne pas provoquer une reconstruction froide artificielle ; seule la validation écrit désormais `ci-images-*`.
- Nettoyage Rust : suppression récursive systématique des SDK préinstallés, coût 105/69/68/164 s selon les quatre runners. Ce coût dépasse largement celui des transferts Cargo.
- Les 62 cibles d'intégration sont découvertes par Cargo ; aucune exclusion ajoutée. Les poids anciens sous-estiment notamment `extensions` (117,33 s, contre poids 51). Mesures de chaque cible : [integration-seconds.json](evidence/2026-11-04/ci-performance/integration-seconds.json). Les suites restent séquentielles dans chaque partition et les tests internes conservent leurs règles d'isolation.
- Les plus longues suites observées sont `extensions` (117 s), `scanner_performance` (104 s), `global_catalogue_consolidation` (97 s). Leurs attentes sont principalement du travail réel et du polling borné (50 ms scanner, 1 ms worker), pas des pauses longues à supprimer arbitrairement. Aucun changement de fixtures SQLite ni de synchronisation métier sans mesure détaillée.
- Les contrats PostgreSQL/OIDC/deux Nodes prennent 126 s dans `checks`, dont 65,6 s OIDC avec PostgreSQL. Ils ne dupliquent pas simplement SQLite : conserver le changement de backend. Les déplacer sur un cinquième runner ajouterait compilation/cache/service ; aucun gain net de minutes démontré. Ils restent obligatoires et séquentiels.
- Le run en échec montre formatage après les préparatifs coûteux et un smoke Docker bloqué/annulé. L'audit ne conclut pas à un test instable sur ce seul cas ; aucune erreur n'est ignorée et aucun délai de test n'est augmenté.

## Changements proposés et compromis

| Problème / cause | Modification | Effet attendu | Risque et preuve requise |
|---|---|---|---|
| Deux compilations release par architecture, version différente | Calculer la version avant le build, transporter les trois images déjà smoke-testées dans une seule archive par architecture ; publication par `docker load` puis push | Supprimer entièrement le second solve et le second linkage | Mesurer save/gzip/upload/download/load ; conserver seulement si le transfert est nettement inférieur aux 372/434 s remplacées |
| Nettoyage lent, systématique | Vérifier l'espace avant restauration Cargo ; supprimer uniquement les SDK si moins de 20 Go sont disponibles | Éviter les 68–164 s lorsque le disque suffit | Seuil conservateur ; rapport `df`, aucune suppression du workspace ou des caches |
| Poids obsolètes | Répartir les mêmes cibles selon les durées du run détaillé | Charges estimées 348/347/347 s (ancien plan réévalué : 241/382/419 s) | Simulation de planning, pas gain observé ; compile/runner variance exclus |
| Modifications de tests invalident l'image de production | `COPY` précis des sources, migrations, expériences servies et plugins embarqués ; état local exclu du contexte | Conserver la couche Rust sur un changement de test seulement | Audit des `include_str!`/`include_bytes!`, build Docker obligatoire |
| CLI de packaging jamais copiée au runtime | Retirer `media-extension-pack` des deux compilations Docker et du strip | Éviter un linkage inutile ; sept binaires runtime conservés | Aucun outil supprimé des images : ce CLI n'y était pas livré |
| Frontend Docker et CI divergent | Aligner le builder éphémère sur Node 24 | Même version majeure pour build et validation | Build Web et smoke complets |
| Release main publie indépendamment de CI | Réutiliser la CI complète dans Release, puis promouvoir ses archives | Toutes les gates portent sur le commit et les images publiés | CI ne se déclenche plus séparément sur push main ; les PR main/develop gardent les noms de checks existants |
| Tag bêta créé avant échec de publication | Poser le tag après publication réussie | Permettre une reprise sans marquer prématurément la version comme terminée | Sérialiser tout le parcours de version/publication develop |
| Mesures incomplètes | Artefacts Cargo `--timings`, collecteur API et workflow manuel de comparaison CGU | Décisions futures fondées sur compilation/CPU/RAM/taille | Aucun benchmark coûteux automatique et aucune modification du profil de production |

Deux partitions donneraient une charge de tests estimée de 521/521 s ; quatre, 261/261/260/260 s. Trois restent le compromis retenu : le job de contrôles reste une limite et un runner supplémentaire ajoute compilation et préparatifs. Aucun benchmark existant obligatoire n'est déplacé ni désactivé.

## Publication des images exactes

1. Un seul solve Bake natif par architecture construit standalone/Core/Node en partageant Rust/Web/FFmpeg. Le profil release reste opt-level 3, ThinLTO, CGU 1, panic abort, strip. La révision OCI est posée sur les images finales ; elle ne devient pas un argument de compilation Rust supplémentaire, afin de préserver le cache sur les changements PR frontend/tests sans nouvelle version embarquée.
2. Les trois images passent les contrôles existants : non-root, santé, version, vraie extraction AV1 de chapitre, FFmpeg 9.0.2, taille Node. Les checks de lecture AV1/Opus/Range et multi-Node restent dans la CI complète.
3. `docker save` regroupe les trois tags en partageant leurs couches ; gzip niveau 1 puis artifact sans recompression, rétention un jour. Le manifeste lie SHA-256 de l'archive, SHA du commit, architecture, source, version et IDs des images du rapport natif. Le cycle save/load est exercé aussi sur les PR ; aucun secret de registre ne leur est accordé.
4. Après succès de toutes les gates, chaque runner de publication télécharge uniquement l'archive de son architecture depuis le même run, vérifie son identité, la charge et compare les IDs/labels. Il ne contient plus de commande de build.
5. Les tags techniques `sha-<commit>-<arch>` sont poussés seulement à ce stade. Les manifests distants doivent correspondre aux IDs exacts testés (digest de config avec le stockage classique, digest du manifest avec containerd).
6. Les six références sont prévalidées avant tout tag final ; chaque index doit contenir exactement AMD64+ARM64 avec leurs digests attendus. Les tags develop/latest/version/série sont mis à jour par index complet. Les digests des index sont archivés pour le changelog de release.

Un registre ne fournit pas de transaction globale sur tous les tags et dépôts : chaque tag final référence un index complet avec les deux architectures, mais une panne entre deux tags peut laisser des alias à des générations différentes. Aucune architecture unique n'est publiée comme tag final. Les publications sont sérialisées ; les mises à jour de PR annulent leurs validations obsolètes. Les pushes de confiance sont sérialisés depuis l'allocation de version ; leur temps d'attente de file doit être mesuré séparément.

Les permissions d'exécution des validations restent `contents: read`, sans `packages: write`. Le job appelant la CI réutilisable depuis Release déclare le plafond contents/packages write déjà nécessaire au parcours release, car la définition CI contient aussi des jobs protégés de tag/publication ; ces jobs sont conditionnés à develop et ne s'exécutent pas dans cet appel main. Aucun `pull_request_target`, `workflow_run` privilégié ou téléchargement d'artefacts depuis un run fourni par une PR n'est ajouté. Protections de branche inchangées. Le ruleset public `main-protection` (25174698) a été lu : PR obligatoire, suppression et force-push interdits, aucun bypass. Les anciennes API de protection de main/develop renvoient 403 au connecteur ; les autres exigences éventuelles ne sont donc pas prétendues exhaustivement connues. Après expiration de l'archive, relancer la validation complète, pas publier une reconstruction non vérifiée.

## Rust : décisions conservatrices et protocole de mesure

Pas de gain de production supposé : CGU 1, ThinLTO, opt-level 3 et jobs Cargo 2 dans Docker sont conservés. Le runner local Docker a 8 CPU/~7,5 Gio RAM et ne représente pas les runners natifs GitHub ; aucun comparatif local n'est présenté comme une preuve GitHub. Le workflow `benchmark-rust.yml`, manuel et sans publication, compare CGU 1/4/8 × jobs 2/4 sur une architecture native choisie, avec mêmes sources et lockfile, builds isolés, `/usr/bin/time -v`, taille/SHA des sept exécutables et `cargo --timings`. Il mesure séparément un build chaud sans changement. Son coût de six builds est volontairement opt-in.

Avant d'adopter CGU 4/8 ou davantage de jobs : répéter sur AMD64 et ARM64, contrôler RAM maximale, taille, démarrage réel du serveur et débit/latence représentatifs (scan/catalogue/Range/permissions) sur les mêmes fixtures, puis CI complète. `--version` n'est pas une mesure de démarrage serveur. Le workflow de comparaison ne prouve pas ces performances runtime.

Les builds dev/test omettent déjà les symboles mais gardent assertions et overflow checks. L'incrémental release est désactivé par défaut ; transférer son état ne réduit pas le ThinLTO final des sept exécutables. `sccache` ne supprime pas ce linkage, et le cache Cargo restaure déjà les dépendances : aucune couche supplémentaire sans mesure. Le linker par défaut de la toolchain Linux récente doit être relevé avant d'ajouter mold/lld ; changer RUSTFLAGS invaliderait tous les caches. Aucune adoption dans ce lot. Références : [profils Cargo](https://doc.rust-lang.org/cargo/reference/profiles.html), [inspection des manifests Docker](https://docs.docker.com/reference/cli/docker/buildx/imagetools/inspect/), [restrictions des caches Actions](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

## Validation et mesures après changement

Essai supplémentaire réellement exécuté : registre Docker 3 jetable lié à `127.0.0.1`, six références synthétiques, cycle archive/load et trois index multiarchitecture produits par le vrai script de promotion, puis digests des index relus. Aucun accès en écriture à GHCR. Cet essai a révélé que Docker 29/containerd peut utiliser le digest du manifest comme ID local ; la vérification accepte désormais uniquement l'un des deux identifiants immuables (config ou bytes exacts du manifest), avec contrat de régression. Les fixtures synthétiques ne constituent aucune preuve d'exécution ARM64 du serveur Aether ni une mesure du transfert des vraies images.

Vérifications locales initiales : Actionlint 1.7.12, contrats release (15), archives (3), manifests (6), partitions (4), plans Bake AMD64/ARM64 et couverture Cargo. Aucun résultat de publication du nouveau parcours n'est encore disponible. Le tableau sera complété avec les vrais résultats PR ; un run PR sans publication ne sera pas comparé comme un CI/CD complet.

Collecte reproductible :

```sh
python scripts/ci-performance-report.py --run RUN_ID --output target/ci-performance.json
python scripts/ci-integration-shard.py --index 0 --count 3 --list
```

Comparer les commits et les cas séparément : cold complet ; warm sans changement ; changement Rust sans dépendances ; changement frontend ; changement Cargo.lock/npm lockfile. Le premier build après modification du Dockerfile peut être froid au niveau des dépendances. L'absence de mesures pour un cas reste explicite. La détection de changements pour sauter des jobs est différée : aucune économie supplémentaire n'est revendiquée et la CI complète reste systématique avant publication. Les retours frontend et formatage restent indépendants des builds Docker ; Clippy/unitaires ne sont plus précédés d'un nettoyage inconditionnel.

PR de revue : [#36](https://github.com/maelmoreau21/Aether/pull/36), créée en brouillon. Premier run PR : [41911338866](https://github.com/maelmoreau21/Aether/actions/runs/41911338866). Résultat en attente au moment de cette entrée.

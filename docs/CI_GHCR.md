# CI et publication GHCR

## Publication mutualisée et partition Rust allégée — 29 octobre 2026

Diagnostic du [run 40931283481](https://github.com/maelmoreau21/Aether/actions/runs/40931283481) :
la publication standalone recompile les dépendances en 4 min 56 s puis les
binaires en 9 min 01 s, bien qu'un job Images ait déjà construit les runtimes.
`MEDIA_VERSION` était déclaré avant la précompilation : chaque nouvelle version
invalidait cette couche. Les trois publications utilisaient aussi trois runners
compilant chacun les mêmes neuf binaires, avec une seule tâche Cargo.

La publication réutilise maintenant le plan Bake partagé par la validation,
avec deux tâches Cargo, trois jeux de métadonnées/tags et un seul export de cache
`publish-images`. Elle importe aussi le cache `ci-images`. Les arguments de
version/branche sont déclarés après la précompilation des dépendances ; les deux
commandes Cargo montent les caches registry/git. La compilation finale reste
nécessaire lorsque la version embarquée ou les sources changent. Les profils
release et la limite locale par défaut d'une tâche Cargo restent conservés.

Les trois images sont toujours vérifiées : téléchargement par digest après
push, labels source/révision, commande version, FFmpeg/ffprobe 9.0.2 et taille
Node inférieure à 200 Mo. Le mode sans push utilise désormais le tag SHA réellement
chargé. Les trois fichiers digest sont regroupés dans `digest-images`, compatible
avec le téléchargement `digest-*` de la release. La publication reste sérialisée,
interdite aux PR et conditionnée aux mêmes contrôles sur develop.

Rust conserve ses trois partitions d'intégration ; une quatrième entrée `checks`
exécute en parallèle format, Clippy, unitaires, ABI et contrats PostgreSQL/OIDC/
multi-Node. PostgreSQL ne démarre que pour cette entrée. Le contrôle agrégé
`Rust (Format, Clippy, Tests)` exige toujours la réussite de toute la matrice.
Les commandes supplémentaires utilisent les mêmes features que les unitaires,
pour éviter les recompilations dues à l'alternance des features. Aucun test
obligatoire n'est retiré. Le quatrième runner peut augmenter les minutes cumulées
et le gain dépend du nombre de runners disponibles.

Validation locale : actionlint 1.7.12 sur les trois workflows, 14 tests de politique
de release, 3 tests de couverture des partitions, plans `docker buildx bake --print`
avec métadonnées synthétiques (version vide, bêta et stable), vérification des
gates de publication et syntaxe Bash réussis ; `git diff --check` réussi.
Daemon Docker indisponible : aucune construction/exécution de conteneur ni
publication réelle dans ce jalon. Une première reconstruction de cache est
attendue ; aucun gain de temps après modification n'est encore mesuré sur GitHub.

## Réduction des compilations et des runs obsolètes — 29 octobre 2026

Les profils Cargo dev/test omettent les symboles de débogage dans ce workflow
(`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`). Les assertions et
contrôles de débordement restent actifs. L'objectif est de réduire le temps
de liaison, le disque et les transferts de cache ; un premier run peut devoir
reconstruire les artefacts avec ce réglage.

Films et Séries sont exécutés par une seule commande Cargo ciblant leurs deux
tests pour mutualiser la compilation. `MEDIA_SCANNER_REPORT_DIR` réserve le
dossier Films, sans changer le dossier Séries ni les validations de rapports.
Les nouveaux commits annulent les jobs de validation obsolètes de la même ref,
y compris sur les branches ; les jobs de publication ne sont pas interrompus.
Les trois images Docker utilisent désormais un seul job Buildx Bake pour
partager les étapes Rust/Web/FFmpeg, avec deux tâches Cargo de compilation
et un seul export de cache des étapes communes. Les anciens caches restent
lisibles. Les trois images sont chargées et le contrôle du runtime Node reste
obligatoire. Le plan Bake a été validé avec `docker buildx bake --print` ;
une construction réelle n'a pas été exécutée (daemon Docker indisponible).
Aucun contrôle obligatoire
n'est retiré. Actionlint 1.7.12, parsing YAML, vérification des dossiers et
`git diff --check` passent localement ; le gain réel et l'exécution Linux sont
à mesurer au prochain run GitHub.

## Parcours playlists — 29 octobre 2026

Le lecteur réduit et le panneau ouvert possèdent tous deux des commandes de
navigation audio. Le test cible maintenant les commandes du panneau. Le test
`product_tests::mixed_playlists` passe sous Windows avec
`MEDIA_TEST_PLAYLIST_BROWSER=1` et Chrome 156 ; la CI Linux reste à confirmer.

## Parcours Séries — 29 octobre 2026

Le test attendait une classe `loaded` supprimée de la couverture. Il vérifie
maintenant le chargement réel de l'image et suit les panneaux, sélecteurs et
menus actuels. Le scénario complet passe localement sous Windows avec Chrome
156.0.8078.48, y compris lecture, permissions et changement de compte. La
relance du job GitHub Linux reste nécessaire.

## Sélection des versions dans les parcours navigateur — 29 octobre 2026

Le journal GitHub montre que les deux scans Films aboutissent, puis que le
sélecteur de version expire : il se trouve désormais dans le panneau replié
« Versions et informations techniques ». Les scénarios scanner, NFO et
affiches ouvrent ce panneau avant de sélectionner une version. Une relance du
job Linux reste nécessaire pour confirmer l'ensemble du parcours en CI. Le
scénario scanner ouvre aussi le panneau de suivi avant les contrôles vu/non vu.
Après reconstruction Web, `cargo test --locked -p media-poc --test
browser_scanner -- --ignored --nocapture` réussit sous Windows avec Chrome
156.0.8078.48, en MP4 puis en MKV après redémarrage. NFO et affiches ne sont
pas relancés ici.

## Parcours administrateur navigateur — 27 octobre 2026

Le run CI 40466113909 échouait avant le scan Films : l'administration ouvre
la vue d'ensemble par défaut et le test cherchait le bouton d'ajout dans la
section Bibliothèques masquée. Les scénarios Films, Séries, NFO et affiches
ouvrent désormais explicitement cette section ; le scénario distribué ouvre
la section Nodes. Les assertions et délais de lecture restent inchangés.
Après reconstruction du frontend, les parcours Films et Séries passent avec
Chrome 156.0.8078.48 sous Windows, sur fixtures synthétiques et SQLite
temporaire. Les scénarios NFO et affiches passent aussi sur les quatre phases
initiale, redémarrage, mise à jour et suppression. Ils attendent une vraie
progression avant le seek et l'achèvement du scan demandé. Le scénario distribué
et la CI Linux après correction ne sont pas encore validés ici.

## Réduction de l'attente CI — 22 octobre 2026

Le run `39827517263` a pris 22 min 25 s. Les logs montrent 13 min 27 s
pour l'étape d'intégration Rust ; dans le job navigateur, 7 min 06 s pour
FFmpeg et 10 min 08 s pour Chrome/dépendances. FFmpeg était pourtant restauré
du cache : les installations apt ont téléchargé 15,5 Mio puis 22,8 Mio à
environ 38 Kio/s. Ce sont des mesures du run précédent, pas un benchmark après.

Changements :

- Les cibles d'intégration Cargo sont découvertes via `cargo metadata` et
  réparties de façon déterministe sur trois runners isolés. Les 44 cibles
  actuelles sont réparties en 15/15/14 ; aucune liste manuelle à maintenir.
  Chaque cible reste séquentielle (`--test-threads=1`) ; les scénarios de
  concurrence conservent leurs tâches et assertions. Les cibles ignorées le
  restent, comme auparavant, et les parcours navigateur gardent leur job dédié.
- Format, Clippy strict, unitaires, ABI Wasm et contrats PostgreSQL restent
  obligatoires, exécutés une fois dans la partition 0. Le cache Cargo est
  partagé (`shared-key: rust`), avec sauvegarde par cette partition uniquement.
- Le contrôle `Rust (Format, Clippy, Tests)` garde son nom et agrège la réussite
  des trois partitions. Un échec, une annulation ou une partition ignorée le
  fait échouer ; la publication dépend toujours de ce contrôle et des autres
  validations. `fail-fast: false` laisse les autres partitions finir leurs tests.
- Chrome préinstallé est lancé et vérifié avant toute installation. Seul un
  échec déclenche l'installateur Playwright existant, puis une nouvelle
  vérification obligatoire. Aucun remplacement par Chromium ou faux rapport.
- FFmpeg mis en cache est exécuté avant d'installer quoi que ce soit. S'il est
  fonctionnel, aucune opération apt. Si les bibliothèques partagées manquent,
  seules les bibliothèques de codecs sont installées ; les outils de compilation
  sont réservés à un cache manquant. Versions 9.0.2 et test lavfi toujours exigés.
- Les jobs Rust/navigateur ont un plafond de 35 minutes, le contrôle agrégé de
  5 minutes. Cela borne un vrai blocage ; cela ne répare pas un check GitHub
  incohérent dont les logs et le workflow sont déjà terminés.

Le gain dépend du cache, du téléchargement apt et du nombre de runners
disponibles. Trois runners peuvent consommer davantage de minutes cumulées et
compiler des dépendances communes en parallèle. Un démarrage sans cache peut
donc rester long ; aucune durée après modification n'est garantie.

Vérifications locales :

```powershell
python scripts/test-ci-integration-shard.py
python scripts/ci-integration-shard.py --index 0 --count 3 --list
python scripts/ci-integration-shard.py --index 1 --count 3 --list
python scripts/ci-integration-shard.py --index 2 --count 3 --list
node scripts/ci-ensure-chrome.cjs
docker run --rm -v "${PWD}:/repo:ro" -w /repo rhysd/actionlint:1.7.12
```

Trois tests de répartition passent (couverture exhaustive sans doublon,
découverte des seuls tests du workspace et rejet des paramètres invalides).
Chrome Windows local réutilisé avec succès ; actionlint et ShellCheck des
scripts FFmpeg passent. Invocation réelle multi-package `cargo test --locked
--workspace --all-features --test contracts_test --test version_and_nodes -j 1
-- --test-threads=1` : 5 tests réussis. Le script de partition a aussi exécuté
réellement la cible `version_and_nodes` avec succès (2 tests), au-delà du plan.
Les chemins apt Linux, les trois runners GitHub et le
gain de temps distant nécessitent encore un nouveau run après push autorisé.
Pas de tests fonctionnels supprimés, de publication déclenchée ou de média modifié.

## Non-régression audio segmenté — 22 octobre 2026

Le job navigateur génère/valide désormais aussi `dual-opus.mp4` (AV1 10 bits
1080p, deux Opus 5.1) puis exécute `browser-dual-opus.cjs` avec
`MEDIA_TEST_SEGMENTED=1`. Son rapport `passed` est obligatoire, purgé avant test,
vérifié au même titre que Films/Séries et archivé dans le même artefact. La clé
du cache navigateur inclut le générateur ; la CI Rust garde son cache distinct.
Le benchmark Docker/Nginx de 336 Mio reste une commande locale documentée,
pas un benchmark NAS ni un nouveau service permanent de CI.

## Dossier des rapports navigateur Films — 22 octobre 2026

Le script local écrit désormais dans `target/browser-scanner` par défaut, alors
que le workflow vérifie et archive `docs/evidence/2026-10-16/scanner`. Cette
divergence provoquait un `FileNotFoundError` malgré la fin du parcours navigateur.
L'étape Films fixe maintenant `MEDIA_REPORT_DIR` au dossier attendu par la CI ;
le test Rust transmet déjà cette variable au script navigateur. Le nettoyage,
les assertions `passed: true`, les captures obligatoires et l'upload restent stricts.

Vérifié localement : parsing YAML et concordance du dossier de génération avec
le nettoyage, les deux rapports, les trois captures et l'archivage ; actionlint
1.7.12 et `git diff --check`. Pas de nouvelle exécution GitHub Linux ni de push.

## Assertion de langue du scanner — 22 octobre 2026

Le journal GitHub fourni confirme les 12 tests P0 réussis. L’échec suivant était
une ancienne assertion de `scanner_flow` attribuant `fr` à l’original MKV bilingue.
Le contrat documenté depuis le 21 octobre expose `und` pour cet original : sa
première piste n’est pas une garantie de sélection par le navigateur.

Le test vérifie désormais `internal-default`/`und`, les sources explicites FR/EN
avec leurs index et canaux, et `en` pour l’original MP4 mono-audio. Les tags bruts
`fra`/`eng` restent vérifiés. Aucun code de scan ou de lecture n’a été modifié.
Commande ciblée : `cargo test --locked -p media-poc --all-features --test scanner_flow
-j 1 -- --test-threads=1` : 1 réussi Windows, ainsi que fmt/diff checks.
La suite GitHub complète reste à relancer ; aucun push effectué.

## Cache des fixtures P0 et profil AV1 — 22 octobre 2026

Les jobs Rust et navigateur utilisaient la même clé alors que seul Rust préparait
MSE/DASH. Un cache créé par le navigateur pouvait donc faire sauter la préparation
Rust et provoquer des 404 et fichiers VTT absents. Les clés sont maintenant
distinctes (`fixtures-p0-full-v2` / `fixtures-p1-browser-v2`), ce qui écarte aussi
les anciennes entrées incomplètes. Le job Rust vérifie les fichiers non vides et
les rapports MSE/DASH `passed`/`originals_unchanged`, même après restauration.

Le test du profil AV1 accepte les deux représentations du profil 0/Main, dont
la chaîne `"0"` observée avec le build CI. Codec AV1, format 10 bits, couleurs et
canaux restent contrôlés ; aucune assertion de lecture n'est désactivée.

Validation locale : `python scripts/validate-prepared-fixtures.py`, rejet d'un
répertoire incomplet, parsing YAML et distinction des clés, `cargo fmt --all --check`
et `cargo test --locked -p media-poc --all-features --test p0 -j 1 -- --test-threads=1`
(11 réussis sous Windows). Le douzième test Linux sur les symlinks n'est pas
compilé sous Windows. La CI Linux complète reste à relancer ; aucun push effectué.

## Correctif FFmpeg des fixtures — 21 octobre 2026

Le profil partagé désactivait `libavdevice`, qui fournit l'entrée synthétique
`lavfi`. Le profil `fixtures` réactive désormais `--enable-avdevice` et seulement
`--enable-indev=lavfi` ; le profil Docker `runtime` conserve les périphériques
désactivés. L'action setup-ffmpeg exécute un court test `testsrc2` + `sine` après
installation, y compris après restauration du cache. La clé binaire inclut déjà
le script de construction ; les deux clés des fixtures l'incluent désormais aussi.

Les anciennes preuves navigateur sont retirées immédiatement après checkout.
L'upload des artefacts reste effectué après échec si des fichiers existent, mais
est ignoré quand une panne de setup/fixtures n'a produit aucune preuve. Le contrôle
des rapports `passed: true` et des captures sur un parcours réussi reste strict,
ainsi que `if-no-files-found: error` lorsque l'upload est effectivement lancé.
L'avertissement Node `punycode` ne provoque pas l'échec de génération.

Validation locale ciblée : actionlint 1.7.12 sur les workflows, `bash -n` sur le
script de construction, test court lavfi avec le FFmpeg Windows installé et
`git diff --check`, tous réussis. Le test Windows ne prouve pas la reconstruction
du profil Linux. Docker Desktop Linux est arrêté : aucune compilation Linux ni
nouvelle exécution GitHub n'a été réalisée. Aucun push ni publication.

## Contrat

`ci.yml` traite les push/PR vers `main` et `develop`, ainsi que les lancements
manuels. La publication dépend de tous les jobs : actionlint, Rust (format,
Clippy workspace/all-features/all-targets avec `-D warnings`, tests), RustSec,
SvelteKit (audit, check, lint, tests et build), Chrome Films/Séries et construction
des trois images `linux/amd64`. Aucune publication sur PR.

Le seul lancement manuel est celui de **CI**, sur la branche choisie dans GitHub.
Il refait tous les contrôles. `publish.yml` ne possède que `workflow_call` et
construit exactement `github.sha`, sans paramètre permettant de remplacer la
branche à étiqueter. Les permissions ordinaires sont `contents: read` ; seul le
job de publication reçoit aussi `packages: write`. Aucun PAT supplémentaire
n'est nécessaire au workflow.

| Cible Docker | Package | develop | main |
|---|---|---|---|
| `runtime` | `ghcr.io/maelmoreau21/aether` | `develop` | `main`, `latest` |
| `runtime-core` | `ghcr.io/maelmoreau21/aether-core` | `develop` | `main`, `latest` |
| `runtime-node` | `ghcr.io/maelmoreau21/aether-node` | `develop` | `main`, `latest` |

Chaque image reçoit aussi `sha-<SHA complet>`, les labels OCI source/révision,
et un digest de contenu. Un tag SHA est une convention du workflow, pas une
garantie d'immutabilité imposée par GHCR : pour figer une installation, utiliser
`image@sha256:...`. Les trois publications sont indépendantes ; un échec partiel
peut laisser certains packages publiés. Ne pas utiliser ce run comme livraison
complète tant que les trois jobs ne sont pas réussis.

Avant publication, le Node est vérifié avec `ffprobe` et `ffmpeg` 9.0.2 fonctionnels et absence de
bibliothèques partagées `libav*.so`, et taille inférieure
à 200 millions d'octets. Le runtime minimal du Dockerfile est conservé. Cette
limite détecte une régression de taille ; elle ne mesure pas la RAM du Node.
Après publication, chaque job télécharge son image par digest, contrôle les
labels OCI et interroge l'API Packages pour vérifier le dépôt associé.

## Incident du 18 octobre 2026

Au commit `1deb03f2d38774f8d7dcad0a9ece18fb5a6adf13`, les runs
[CI](https://github.com/maelmoreau21/Aether/actions/runs/39106868858) et
[GHCR](https://github.com/maelmoreau21/Aether/actions/runs/39106868272)
ont `conclusion=failure`, zéro job et zéro check-run retournés par l'API.
Les API accessibles n'exposent pas de message de validation supplémentaire.

Reproduction locale avec actionlint 1.7.12 :

```text
ci.yml:248:11: error while parsing reusable workflow "./.github/workflows/publish.yml": yaml: line 70: could not find expected ':'
publish.yml:70:0: could not parse as YAML: could not find expected ':'
```

Le heredoc shell commence avec des lignes non indentées au niveau YAML ; elles
terminent le bloc `run: |`. Le parseur essaie donc de lire les tags d'images
comme des clés YAML. L'appel réutilisable propage cette erreur dans la CI.
Les tags sont maintenant produits par `docker/metadata-action`.
Le lancement manuel historique pouvait également étiqueter le SHA courant comme
une autre branche sans passer les tests : ce paramètre a été supprimé.

Le contrôle complet a révélé deux blocages supplémentaires : le test navigateur
Films utilisait les anciens labels/compteurs de l'administration et `cargo audit`
refusait `rsa 0.9.10` (`RUSTSEC-2023-0071`), apporté par jsonwebtoken/RustCrypto.
Les sélecteurs/assertions suivent désormais les éléments réellement affichés.
Le backend JWT passe à AWS-LC, déjà présent via TLS, en conservant les validations
de signatures/claims et sans exception RustSec. Cela ne constitue pas une preuve
d'exploitation de l'alerte RSA dans Aether, qui vérifie des signatures publiques.

## Réglages GitHub à vérifier par un administrateur

- Actions autorisées : checkout, toolchain/cache Rust, setup-node, actions Docker,
  upload-artifact et conteneur actionlint. Les restrictions du dépôt ou du compte
  peuvent refuser une action avant exécution.
- Autoriser l'écriture Packages au `GITHUB_TOKEN` du job de publication.
- Pour un package préexistant : donner au dépôt Aether l'accès Actions en écriture
  et vérifier son association au dépôt. Les labels OCI seuls ne prouvent pas
  l'association effective côté GitHub.
- Choisir explicitement la visibilité souhaitée. Un package privé demande une
  authentification de lecture ; le workflow ne le rend pas public.

Références : [registre Container GitHub](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)
et [permissions des workflows réutilisables](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations).

## Installation après publication validée

Ces commandes modifient une installation et sont à exécuter uniquement après
autorisation de son opérateur. Conserver auparavant les digests actuels et une
sauvegarde vérifiée. Ne jamais supprimer les volumes pour une mise à jour.
Les exemples utilisent la configuration existante et ses médias `:ro`.

```powershell
# Standalone, après avoir choisi un tag SHA dont le run complet a réussi.
$env:AETHER_TAG = 'sha-<SHA complet validé>'
docker compose -f docker-compose.yml pull
docker compose -f docker-compose.yml up -d --no-build

# Alternative distribuée : un Core et un Node polyvalent.
docker compose -f compose.distributed.yaml pull
docker compose -f compose.distributed.yaml up -d --no-build
```

Pour un package privé, authentifier Docker avec un identifiant de lecture Packages
avant `pull`, sans écrire le secret dans l'historique des commandes.

Retour arrière de ce jalon (aucune migration SQL ajoutée) : rétablir `AETHER_TAG`
sur le tag SHA précédent conservé, puis refaire `pull` et `up -d --no-build`
pour le seul mode installé. Pour une garantie stricte, fixer les références par
digest dans un override Compose. Garder les mêmes volumes et secrets ; aucune
commande `down -v`, suppression de données ou restauration automatique.

## Limite de validation actuelle

Commandes exécutées et résultats sur les sources finales :

```powershell
cargo fmt --check
cargo clippy --locked --workspace --all-features --all-targets -- -D warnings
cargo test --locked --workspace --all-features -j 1 -- --test-threads=1
cargo audit
cargo audit --no-fetch
npm --prefix apps/web run check
npm --prefix apps/web run lint
npm --prefix apps/web test
npm --prefix apps/web run build
npm --prefix apps/web audit --audit-level=high
```

Rust : 129 tests réussis, zéro échec, 7 ignorés explicites (bancs navigateur,
certificats et helper de sous-processus). Les parcours Films/Séries et TLS ont
été exécutés séparément : 2 tests Chromium réussis et 4 tests TLS réussis.
Web : 35 tests, check 0 erreur/avertissement, lint/build/audit réussis.
Actionlint 1.7.12 passe via binaire Windows et via le conteneur Linux utilisé
par la CI. Les deux `docker compose config --quiet` passent avec secret
d'appairage jetable ; aucun fichier de configuration contenant un secret réel
n'a été affiché. Trois images reconstruites ; matrice ffprobe finale 8/8.
Les commandes Docker/TLS détaillées restent dans [P2 Docker](P2_DOCKER_VALIDATION.md),
avec les variables `MEDIA_TEST_NODE_IMAGE`, `MEDIA_TEST_CORE_IMAGE` et
`MEDIA_TEST_STANDALONE_IMAGE` permettant de cibler les tags locaux
`aether-node:p2-final-qa`, `aether-core:p2-final-qa`, `aether:p2-final-qa`
sans remplacer les tags historiques.

La première passe parallèle a dépassé le délai d'un scan 1000, et une
recompilation parallèle a manqué de mémoire virtuelle Windows (erreur 1455).
La suite complète finale ci-dessus passe avec compilation/cas séquentiels,
sans modifier les timeouts ou assertions de concurrence.

Preuves locales de ce jalon : [Docker et mesures](evidence/2026-10-18/p2-ci/docker.json),
[Chrome distribué](evidence/2026-10-18/p2-ci/browser-distributed.json),
[Chrome standalone](evidence/2026-10-18/p2-ci/browser-standalone.json),
[OIDC synthétique dans les deux modes](evidence/2026-10-18/p2-ci/oidc.json).
Les 86 contrôles Docker passent avec deux Nodes et le transport interne HTTPS ;
Chrome 155.0.8059.40 est exécuté sous Windows 10.0.26300, headless.

Sur les courtes fixtures de ce banc, les pics mémoire cgroup échantillonnés des
Nodes en lecture directe sont 10,47 et 8,48 Mio, avec respectivement 35,4 % et
36,3 % d'un cœur en moyenne, instrumentation comprise. Le Core atteint 16,05 Mio
en direct et 18,48 Mio en relais. Le standalone atteint 81,80 Mio pendant la
phase lecture de ce run. Ces valeurs incluent caches/enfants et allocations
conservées ; elles ne sont pas une mesure RSS du seul binaire ni une promesse de
capacité. Les échantillons de latence, scans et réseau sont conservés séparément
dans le JSON. Hôte partagé avec les vérifications Rust : aucune comparaison de
performance avant/après ni conclusion NAS/deux machines physiques.

Les fichiers locaux corrigés ne sont ni commités ni poussés. Aucun nouveau run
GitHub ni aucune publication n'a été lancé. La CLI `gh` n'est pas authentifiée ;
le connecteur permet les lectures de runs mais refuse les endpoints administratifs
Actions/Packages. Les tentatives de lecture des trois manifests `:develop` ont
échoué à l'obtention du jeton anonyme GHCR (`403 Forbidden`) : cela ne distingue
pas un package absent d'un package privé. Leur présence/téléchargement et les
réglages administratifs restent donc non confirmés.

Le jalon CI ne clôture pas P2 : PostgreSQL 18, l'extension du contrat OIDC et
Traefik restent à implémenter et valider dans l'ordre demandé.

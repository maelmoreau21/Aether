# CI et publication GHCR

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

# Images AMD64 et ARM64 — contrôles du 3 novembre 2026

Le plan construit les trois cibles `runtime`, `runtime-core` et `runtime-node`
séparément sur `linux/amd64` et `linux/arm64`. Les jobs GitHub utilisent des
runners natifs (`ubuntu-latest` et `ubuntu-24.04-arm`) ; la machine locale est
Windows avec Docker Desktop Linux x86_64. Une exécution ARM locale relève de
l'émulation et ne constitue pas une validation sur un NAS ARM.

## Contrats et garde de publication

- `docker-bake.ci.hcl` prend `TARGET_ARCH` et sépare les scopes de cache GHA
  par architecture. Une résolution Bake partage les étapes entre les trois images.
- CI et publication utilisent le checkout du SHA exact et `source: .`.
- La publication construit et charge localement une architecture, vérifie les
  trois images, puis pousse uniquement les références immuables de cette
  architecture lorsque `inputs.push` est vrai.
- Le job final dépend du succès des deux architectures. Le script
  `publish-multiarch-manifests.py` exige les six digests, le même SHA et les mêmes
  tags finaux avant de publier les trois index. Sans `--publish`, il ne fait
  qu'afficher le plan.
- Le smoke vérifie architecture, UID 10001, versions, healthcheck et démarrage
  isolé. Node et standalone doivent extraire une vraie image JPEG depuis une
  fixture AV1 montée en lecture seule. Aucun média ou volume d'installation
  réelle n'est monté ; les conteneurs du smoke n'ont pas de réseau.

Le profil FFmpeg 9.0.2 active explicitement `libdav1d`, `mjpeg` et les filtres
nécessaires aux images de chapitres et planches Trickplay. Les bibliothèques
dynamiques `libav*.so` ne sont pas requises ; la dépendance CPU `libdav1d6` est
installée dans Node et standalone. Le Core seul ne contient pas FFmpeg.

## Vérifications effectivement exécutées

| Contrôle | Résultat |
|---|---|
| `python scripts/test-release-policy.py` | 15 tests réussis après reprise |
| `python scripts/test-docker-multiarch.py` | 4 tests de plan et refus réussis après reprise |
| `docker buildx bake -f docker-bake.ci.hcl --print` | Trois cibles AMD64 résolues localement |
| Probe AMD64 FFmpeg compilé | Rapport préexistant du même lot : AV1 vers JPEG 320×180, 6 485 octets ; planche Trickplay 9 803 octets, réseau coupé |
| Trois images AMD64 finales | Construction et smoke réussis ; détails ci-dessous |
| Trois images ARM64 finales | Construction lancée sous émulation locale ; résultat encore en attente |
| CI native ARM et matériel ARM physique | Non vérifiés pendant ce lot |

Le rapport du probe est `target/mission-probe-smoke/report.json`. Ses durées
ponctuelles (1,001 s et 1,230 s) concernent uniquement cette étape AMD64 ; elles
ne constituent ni une mesure de lecture ni un comparatif de performance.

## Reprise des constructions interrompues

Les premières tentatives AMD64 et probe ARM ont été interrompues par une perte
du moteur Docker (`rpc error: Unavailable ... EOF`). Le moteur était arrêté à
la reprise ; cet échec ne constitue pas une erreur de compilation prouvée.
Docker Desktop 29.9.2 a été redémarré. Environnement observé : 8 CPU et
8 082 644 992 octets de mémoire dans la VM Linux x86_64.
Les images de construction résolues annoncent Rust 1.99.1 et Node.js 22.23.4.
Le stage probe contient FFmpeg/ffprobe liés à `libdav1d.so.6`, `libm` et `libc` ;
les bibliothèques `libav*.so` ne sont pas des dépendances dynamiques de ce binaire.

La reprise sérialise les architectures et fixe `CARGO_BUILD_JOBS=1`. Les caches
GHA sont désactivés dans les commandes locales, sans modifier les réglages CI.
Les caches de registre et certaines couches sont chauds ; aucune comparaison
froid/chaud ou accélération n'est revendiquée. Le contexte local comprend les
changements graphiques concurrents présents au lancement, qui sont préservés.

Commande locale AMD64 (aucun push) :

```powershell
$env:TARGET_ARCH='amd64'
docker buildx bake -f docker-bake.ci.hcl --load `
  --set '*.cache-to=' --set '*.cache-from=' `
  --set '*.args.CARGO_BUILD_JOBS=1' `
  --set 'runtime.tags=media-mission:standalone-amd64' `
  --set 'runtime-core.tags=media-mission:core-amd64' `
  --set 'runtime-node.tags=media-mission:node-amd64' --progress plain
```

Journaux : `target/mission-images-amd64-retry.log`, puis métadonnées de durée
dans `target/mission-images-amd64-build.json`.

## Reprise ARM64 du 4 novembre 2026

Le journal ARM du 3 novembre s'arrête pendant la compilation Rust et FFmpeg,
sans résultat final. Après redémarrage de Docker Desktop le 4 novembre, aucune
image finale ARM64 n'est présente. La reprise conserve les trois artefacts AMD64
et utilise la même commande avec `TARGET_ARCH=arm64`, les trois tags ARM64 et
`CARGO_BUILD_JOBS=1`. Journal distinct : `target/mission-images-arm64-retry.log`.
Les couches terminées sont réutilisées ; les étapes interrompues sont recompilées.
La durée de cette reprise ne doit pas être additionnée ou comparée au build AMD64
comme une mesure de performance contrôlée.

## Trois runtimes AMD64 vérifiés

Construction terminée à 21:30:30 UTC : **1 459,26 secondes** (24 min 19 s),
avec un seul job Rust. La reconstruction des dépendances a duré 8 min 56 s ;
l'édition de liens LTO des binaires a représenté une partie importante du reste.
La VM disposait encore d'environ 6 Go et son swap était inutilisé pendant cette
phase. Cette durée défavorable est conservée, sans comparaison avec les anciens
builds dont les conditions de cache et de parallélisme différaient.

| Image locale | Taille `docker inspect.Size` (octets, non compressée) | ID |
|---|---:|---|
| `media-mission:standalone-amd64` | 352 076 867 | `sha256:b7920e5e3f6ae523e6e08cbae89123c95245a21dabe3d6ecb77ee4fccc3201b2` |
| `media-mission:core-amd64` | 302 713 251 | `sha256:c5d3343b7219dc5a196efe136cf7bc460af0c3ed251e7278c9c9921e3090b60d` |
| `media-mission:node-amd64` | 182 804 610 | `sha256:5fcef0c5766a61b447a6ade02d84e950179a21b6ae466627f17e04067b6407a9` |

Le smoke a réellement démarré les trois services dans des conteneurs jetables,
sans réseau et sans volumes utilisateur, et confirmé UID 10001, healthchecks et
version `0.1.0-beta.0-dev`. Node reste sous le seuil existant de 200 000 000 octets.
Les deux runtimes média exécutent FFmpeg et ffprobe 9.0.2 et produisent une vraie
image AV1 de 320×180, 6 485 octets, depuis la fixture montée en lecture seule.
Rapport : `target/mission-images-smoke-amd64/report.json` ; journal :
`target/mission-images-smoke-amd64.log`. Les temps ponctuels de healthcheck et de
génération ne prouvent ni la lecture vidéo/audio ni une capacité de production.

Les sources sont le HEAD local `525b61f` **plus le worktree présent au lancement**,
y compris les modifications graphiques concurrentes. Aucun checkout immuable du
diff n'a été créé : les IDs d'images et le journal identifient les artefacts, mais
ces images ne sont pas présentées comme une reconstruction du seul commit propre.
Les seuls ajustements de style/tests ultérieurs ne justifient pas une nouvelle
reconstruction coûteuse. Une CI sur un checkout neuf devra valider le futur SHA
intégré avant publication.

La construction ARM reprend le même plan avec `TARGET_ARCH=arm64`, des tags
locaux suffixés `arm64`, et un job Rust. Elle s'exécute sur l'hôte x86_64 sous
émulation ; journaux et métadonnées : `target/mission-images-arm64.log` et
`target/mission-images-arm64-build.json`. Un lancement ne constitue pas encore
une preuve de compilation ou d'exécution réussie des trois images.

Retour arrière : revenir au plan monoarchitecture précédent ou conserver les
images déjà déployées. Les modifications de ce lot ne déploient, ne remplacent
et ne publient aucune image automatiquement pendant les tests locaux.

## Fin de reprise du 4 novembre : ARM64 non validé

La reprise a réutilisé les couches disponibles et construit le frontend sous
émulation (138,4 s). Les compilations Rust et FFmpeg étaient encore en cours
lors de l'arrêt de Docker Desktop. Le dernier journal d'objets FFmpeg est à
1 138,1 s d'étape : ce nombre n'est ni une durée totale de build ni un succès.
Le journal hôte montre une fermeture des services à 07:00:41 heure de Paris ;
la cause de cette fermeture n'est pas établie ici, et aucun OOM n'est affirmé.
À la vérification finale, le pipe Docker Linux est absent et aucun processus
Docker ni client de build ne subsiste. **Aucune des trois images ARM64 finales
ni leur smoke n'est validée.**

Journal préservé : `target/mission-images-arm64-retry.log`. Il n'existe pas de
rapport final `mission-images-arm64-build.json` réussi. Les trois images AMD64
et leurs preuves précédentes restent distinctes. Pas de troisième relance
aveugle après deux interruptions ; prochaine action Docker : rétablir un moteur
stable puis reprendre les trois ARM64 et leur smoke, avant toute publication.
Les sources applicatives ont continué d'évoluer après l'envoi du contexte de
build : ces images ne prouvent pas l'intégration des derniers lots de code.

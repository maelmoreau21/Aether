# Lecture MP4 AV1/Opus sur NAS — 22 octobre 2026

## État et cause connue

Incident Avatar sur Synology **non déclaré résolu** : ni le NAS ni le fichier
personnel ne sont accessibles ici. Le relevé fourni confirme MP4 AV1 10 bits
1080p et deux Opus 5.1, mais ne contient pas les indices absolus FFmpeg. Il est
impossible de désigner honnêtement FR ou EN comme piste défectueuse à partir
de ce seul relevé. Les tests ci-dessous utilisent des fichiers synthétiques.

L'ancien chemin imposait la copie complète de la vidéo en fMP4, puis de l'audio
en Ogg avant son chargement. Un seul worker refusait toute préparation concurrente.
La fermeture annulait l'attente du navigateur ; elle ne garantissait pas que le
reverse proxy interrompe immédiatement son travail amont. Une autre sélection
pouvait donc recevoir `preparation_busy`. C'est un mécanisme de panne possible,
pas la preuve du motif observé sur le NAS. Les erreurs FFmpeg étaient envoyées
vers `Stdio::null()`, empêchant le diagnostic d'un rejet de piste.

## Parcours corrigé

- Le MP4 original fournit sa vidéo et son index de seek par GET/HEAD/Range,
  avec les mêmes autorisations. Aucun fichier vidéo intégral n'est copié pour
  changer de langue. Les liens Node directs existants sont conservés.
- L'audio sélectionné est copié en courtes fenêtres fMP4 pour MSE Opus, via
  `/prepared/audio-{index}-chunk-{n}`. Un `<audio>` et le `<video>` existant
  conservent la coordination pause/seek/volume/vitesse. Aucun WebAudio de production.
- Une fenêtre couvre 10 s ; FFmpeg lit au plus 12 s de sortie, avec une seconde
  de preroll avant les fenêtres suivantes. L'input seek utilise le descripteur
  confiné `fd:` ; un output `-ss 0` retire l'audio négatif provenant de l'image
  clé précédente avant rebasing. Sans cela, le contenu pouvait être décalé de
  plus d'une seconde malgré des horloges HTML5 apparemment synchrones.
- Pas d'encodeur audio/vidéo, de filtre, de `-ac` ou de downmix serveur. Les paquets
  Opus sont copiés ; les tests vérifient les hachages, 5.1 et 7.1 et le placement
  du contenu des fenêtres. Le décodage de sortie physique multicanal reste client.
- Seulement la fenêtre courante et la suivante sont demandées. Les anciens
  buffers navigateur sont retirés ; une fenêtre évincée par le navigateur est
  redemandée au seek. Le changement de langue garde la vidéo, la position et
  le plein écran, après validation transactionnelle du nouveau candidat audio.
- Un worker, deux attentes maximum de 5 s pour les fenêtres, coalescence des
  requêtes identiques ; durée FFmpeg par fenêtre plafonnée à 20 s, sortie 16 Mio.
  Les limites générales ne sont pas augmentées. Le cache reste à 64 entrées,
  avec éviction LRU des seules fenêtres non utilisées par une réponse active.
  Des leases gardent les fichiers présents jusqu'à la fin du streaming HTTP.
- Les seeks obsolètes et la fermeture annulent les fetchs. Un proxy peut laisser
  finir une fenêtre déjà commencée ; aucune garantie d'arrêt distant immédiat
  n'est annoncée. Ce travail est désormais court et borné, au lieu d'une copie
  de l'épisode entier. Un cache rempli de préparations historiques ne se vide
  pas automatiquement : seules les nouvelles fenêtres sont évincées.
- Sans MSE Opus, repli sur Ogg existant, avec erreur explicite si illisible.
  Le MP4 vidéo reste original, mais la copie audio complète de ce repli peut
  toujours retarder la lecture. MKV conserve le parcours préparé historique.

## Diagnostic du véritable fichier

Les erreurs affichent maintenant la langue et l'ID `internal-N`, au premier
chargement comme au changement. Refaire séparément FR puis EN après une fermeture
du lecteur permet d'identifier la piste qui échoue, sans deviner ses indices.

Sur le NAS, après installation d'une image contenant ces modifications :

```sh
docker logs --since 10m aether-core 2>&1
docker exec aether-core sh -c 'command -v ffmpeg; command -v ffprobe'
```

Événements JSON utilisables dans `docker logs`, également sur un Node distant :

| Événement | Informations |
| --- | --- |
| `prepared_http_start` | asset, Node, composant, méthode HEAD/GET, présence de Range |
| `prepared_audio_track` | indice absolu, langue réelle du probe, codec et canaux |
| `preparation_requested` | tentative unique, composant/indice/fenêtre, octets source |
| `preparation_waiting`, `preparation_cache_hit` | attente derrière le worker ou réutilisation |
| `preparation_ready` | taille copiée, durée en ms |
| `preparation_ffmpeg_error` | code, composant, durée, fin de stderr FFmpeg |
| `preparation_cancelled_or_failed` | arrêt avant succès, durée de la tentative |
| `prepared_http_error`, `prepared_remote_response` | code ou HTTP amont et temps Core |
| `node_prepared_start`, `node_prepared_response` | méthode, composant, HTTP et durée sur Node |

Stderr est drainé sans bloquer le processus et sa mémoire est limitée à 16 Kio.
Il reste côté serveur ; le navigateur reçoit le code technique et l'identité de
piste, jamais le contenu brut des diagnostics. Les logs structurés n'incluent
pas de jeton, URL signée ou chemin de bibliothèque. La console navigateur
indique aussi fenêtre, taille et durée du fetch `[media audio]`.

Pour conclure sur Avatar, il reste à obtenir ces lignes lors de l'échec et de
l'attente, puis à observer FR/EN/FR, seek et reprise sur le vrai NAS. Un échec
`unsupported_preparation`, un worker occupé, un FFmpeg absent et un problème
HTTP du proxy doivent rester des diagnostics distincts.

## Reproduction locale et Docker

Depuis la racine, FFmpeg/ffprobe disponibles et dépendances Web installées :

```powershell
python scripts/generate-dual-opus-fixture.py
# Nouveau nom : jamais écraser un média existant.
ffmpeg -v error -nostdin -n -stream_loop -1 -i tests/fixtures/generated/dual-opus/dual-opus.mp4 -t 3500 -map 0 -c copy tests/fixtures/generated/dual-opus/dual-opus-long.mp4
$env:MEDIA_TEST_SEGMENTED='1'
$env:MEDIA_REPORT_DIR='target/segmented-audio'
node scripts/browser-dual-opus.cjs
```

Pour l'essai Docker avec le **profil runtime exact** (encodeurs audio/vidéo
désactivés), des conteneurs de test et Nginx avec buffering actif :

```powershell
docker build --target probe-runtime --tag media-preparation-runtime:test .
docker pull nginx:stable-alpine
$env:MEDIA_TEST_SEGMENTED='1'
$env:MEDIA_TEST_DOCKER='1'
$env:MEDIA_TEST_PROXY='1'
$env:MEDIA_TEST_FIXTURE=(Resolve-Path tests/fixtures/generated/dual-opus/dual-opus-long.mp4).Path
$env:MEDIA_REPORT_DIR='target/segmented-audio-docker-proxy-long'
node scripts/browser-dual-opus.cjs
```

Les remuxers utilisent 2 CPU maximum et 256 Mio, sans réseau, utilisateur 10001,
capabilities retirées, médias montés `ro`. Nginx utilise 64 Mio et un port publié
uniquement sur localhost, avec `proxy_read_timeout 10s`. Les conteneurs temporaires
sont supprimés. La limite mémoire n'est pas une mesure de RSS. Le script conserve
l'identité de l'image Nginx effectivement testée dans son rapport.

## Résultats et limites

Vérifications exécutées : Web 81 tests, check 0 erreur/avertissement, lint/build ;
Node 44 tests réussis/2 ignorés puis les trois tests packaging (dont dual-5.1
explicitement activé, coalescence et copie 7.1) et les trois tests streaming
réussis. Core `browser_scanner`, `scanner_flow` et `distributed_multi_node`
réussis, avec nouvelles fenêtres HEAD/Range et relais distant. Fmt, Clippy
workspace/all-targets strict et actionlint réussis ; suite Rust workspace
générale et nouvelle CI GitHub Linux non exécutées.

Windows 10.0.26300, Chrome 155.0.8059.85 et Edge 155.0.4283.62 headless ;
fixtures 1080p AV1 Main 10 bits à **12 fps**, deux Opus 5.1 FR/EN. Le gros MP4
fait 352 256 545 octets (336 Mio), dure 3500 s et a son index en fin de fichier.
Il ne reproduit pas le 23,976 fps ni les horodatages particuliers d'Avatar.

Dernier scénario Docker + Nginx + gros MP4 : première image **529 ms**, lecture
avec audio prête **1470 ms**. Réouverture EN **430 ms**, FR **410 ms**. Onze
fenêtres demandées sur tout le scénario, **5 004 087 octets** préparés, au lieu
d'une copie vidéo de 336 Mio. Annulation pendant un fetch retardé reçu par le
serveur : nettoyage navigateur **2,1 ms**, aucun élément audio résiduel, fermeture
HTTP amont observée dans cet essai. Ce sont des observations ponctuelles, pas
une borne garantie. [Rapport Docker/Nginx](evidence/2026-10-22/nas-preparation/docker-proxy-long.json).

Les tests observent un vrai signal PCM FR/EN distinct, des images décodées,
FR/EN/FR répété sans rechargement vidéo, le passage d'une frontière de fenêtres,
pause/reprise, seeks 8/900/1800/3480/2 s, rejet HTTP 404 du candidat avec ancien
audio conservé et fermeture pendant un fetch retardé. L'analyse WebAudio sert
uniquement à mesurer les fixtures dans les tests. Les mesures de première image
utilisent `requestVideoFrameCallback`, séparément de la lecture audio prête.

Le cache de préparation est vide au départ de chaque scénario ; les caches
du système hôte ne sont pas vidés. Les mesures ne sont pas des chiffres NAS,
ni un engagement de temps de démarrage. Les rapports de cette intervention sont
dans `evidence/2026-10-22/nas-preparation/`.

Le parcours Core/SQLite réel a aussi été exécuté sous Windows, en MP4 puis MKV
après redémarrage. Scanner/permissions/Range et deux Nodes locaux sont testés
séparément, y compris les nouvelles fenêtres signées distantes. L'application
Core complète sous Docker et le NAS physique n'ont pas été validés dans ce lot :
le scénario Docker teste le vrai FFmpeg runtime et Nginx avec le lecteur de
production, via un serveur HTTP de fixtures isolé. Pas de preuve sur la sortie
physique 5.1/7.1, les périphériques, le proxy Synology/TLS ou une séance complète.

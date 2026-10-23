# Streaming : mesures du 23 octobre 2026

La cible de 50 ms est atteinte sur les petites fixtures chaudes dans le banc
du moteur natif, sans établir 50 ms du clic dans toute l'application sur NAS.
Le gain démontré porte surtout sur la continuité pendant une préparation audio :
la présentation précédente continue pendant l'attente HTTP du candidat.

## Mesure et reproductibilité

`scripts/browser-streaming-performance.cjs` transpile les modules TypeScript de
production et les utilise dans Chrome installé, headless, Windows
10.0.26300, Chrome 155.0.8059.86, FFmpeg 9.0.2. Son serveur HTTP local de fixture
sert les originaux par Range et prépare seulement les fenêtres audio demandées,
avec `-c:a copy`, sans encodeur, filtre ou modification des originaux.
Ce serveur JavaScript constitue un instrument de test, pas le Media Node Rust.

Le chronomètre commence dans le gestionnaire d'un vrai clic navigateur sur
le bouton du banc. Il exclut la latence de dispatch Playwright. Ce bouton ne
représente pas le catalogue Svelte, les permissions Core ou un trajet réseau NAS.
`browser-scanner.cjs` mesure séparément le clic réel `.hero-cta`, l'ouverture du
dialogue et sa première frame, dans le parcours Core/SQLite/Svelte.

- Première image : premier `requestVideoFrameCallback` après le clic. Il observe
  une frame envoyée au compositeur, avec son timestamp média ; ce n'est pas une
  mesure photon/écran. Le callback peut être retardé d'une synchronisation écran.
  [Définition MDN](https://developer.mozilla.org/en-US/docs/Web/API/HTMLVideoElement/requestVideoFrameCallback).
- Premier audio : PCM décodé dans un graphe WebAudio réservé au test, FFT sans
  lissage, seuil -80 dB, polling nominal de 8 ms. La fréquence de la nouvelle
  piste et son identité doivent correspondre lors du switch. Le graphe de test
  peut modifier la sortie multicanal ; aucune preuve de haut-parleurs 5.1.
- A/V : horloges HTML5 à moins de 150 ms au moment du signal. Cela ne prouve
  pas seul la synchronisation du contenu d'un fichier arbitraire ; les fixtures
  à marqueurs et tests de paquets restent nécessaires.
- Switch : 600 ms de lecture stable avant chaque clic, exclus du chronomètre.
  Observation poursuivie 180 ms après disponibilité du candidat. Le trou vidéo
  utilise les callbacks de frames ; le silence observe **tous** les éléments
  audio, car le candidat en chargement précède la piste audible dans le DOM.
- Réouvertures : premier passage avec contexte navigateur neuf et cache de
  préparation vide ; suivants avec contexte et fenêtres explicitement réutilisés.
  Le cache disque de Windows n'est pas purgé : aucun passage n'est annoncé
  « disque froid ». Le hachage SHA-256 est calculé **après** les mesures.
- Médianes et p95 : échantillons bruts conservés, p95 par rang supérieur.
  Dix reprises chaudes sont un échantillon réduit, pas un engagement de service.

Le mode `MEDIA_PERFORMANCE_BASELINE=1` injecte `native.ts` depuis une révision Git
sans changer les fichiers du checkout. Les autres modules sont partagés avec le
candidat, dont la correction de suivi des fenêtres du candidat audio. Il s'agit
d'isoler la modification de commutation du lecteur, pas de comparer deux builds
complets de Core/Node. Les bancs sont alternés avant/après par scénario.
La référence est `HEAD` par défaut, remplaçable par `MEDIA_PERFORMANCE_BASE_REF`.
La référence utilisée ici est `362a5ddf9ea4cfa57ea522c7d47a560683059730`.
Les prochaines exécutions enregistrent le commit résolu et les SHA-256 des
modules injectés ; une modification des sources pendant le banc provoque un
échec. Les premiers rapports archivés précèdent cet ajout de provenance.

## Petites fixtures AV1 10 bits, Opus 5.1 FR et stéréo EN

Fixtures légales existantes : **320 × 180, environ 24 images/s**, MP4 766 638
octets, MKV 762 049 octets, environ 12 secondes. Les descripteurs explicitent vidéo originale et audio fMP4 MSE
par fenêtres. Un essai séparé de l'original entier laisse le navigateur choisir
sa piste par défaut : aucune langue n'est déduite de la seule métadonnée.

Résultats après modification, moteur isolé, dix reprises chaudes par ligne.
Valeurs médiane / p95, en millisecondes :

| Source | Première frame | Premier PCM | Lecture prête |
| --- | ---: | ---: | ---: |
| MP4 original, piste navigateur | 28,3 / 29,2 | 29,0 / 34,5 | 23,2 / 28,3 |
| MP4 original + Opus segmenté sélectionné | 27,6 / 29,0 | 44,3 / 46,0 | 33,6 / 39,8 |
| MKV original, piste navigateur | 27,9 / 29,3 | 30,4 / 44,6 | 25,1 / 32,0 |
| MKV original + Opus segmenté sélectionné | 38,6 / 42,1 | 58,4 / 71,7 | 49,8 / 60,3 |

Première ouverture avec contexte neuf et préparation vide : MP4 segmenté
26,7 ms frame / 131,6 ms PCM / 116,6 ms prêt ; MKV segmenté
26,2 / 147,9 / 144,9 ms. Afficher la première frame n'implique donc pas que
la lecture avec son sélectionné ait déjà commencé. Aucune accélération nouvelle
du démarrage n'est attribuée à cette modification : le chemin chaud était déjà
rapide dans ce banc.

Onze seeks par source segmentée, positions 1 et 6 s, fenêtres disponibles :
MP4 28,0 / 45,5 ms jusqu'à frame, 37,0 / 46,2 ms jusqu'au PCM ;
MKV 42,6 / 70,3 ms jusqu'à frame, 44,5 / 81,0 ms jusqu'au PCM.
Les 22 switches locaux par conteneur avaient un nouveau PCM médian de
38,2 ms MP4 et 50,0 ms MKV, avec maxima 116,3 et 124,9 ms : la cible
100 ms n'est pas garantie, notamment au premier candidat non préparé.

## Attente audio contrôlée : gain de continuité démontré

Même MP4, mêmes pistes et même serveur ; chaque réponse de fenêtre audio attend
**300 ms explicitement injectées**. Six ouvertures, douze switches par version.
Ce scénario simule une attente réseau/préparation et ne prétend pas mesurer un
vrai réseau distant. Le coût de FFmpeg et le premier candidat restent inclus.

| Indicateur switch | Avant médiane / p95 | Après médiane / p95 |
| --- | ---: | ---: |
| Plus grand trou vidéo observé | 343,3 / 522,1 ms | 66,5 / 67,0 ms |
| Silence PCM observé | 346,1 / 512,0 ms | 9,2 / 20,3 ms |
| Clic → nouveau PCM de la langue demandée | 351,7 / 518,6 ms | 359,8 / 554,2 ms |

La nouvelle langue n'arrive pas plus vite que le réseau. L'amélioration est
la continuité de l'ancienne présentation pendant l'attente. Les événements
pause/playing sont archivés, la source vidéo reste identique et son compteur
`loadstart` n'augmente pas pendant les switches.

## Cache de fichiers préparés côté Rust

Audit du chemin existant : Core résout les droits et le descripteur ; le Node
sert le fichier original depuis un handle confiné par blocs de 64 Kio. Les
MP4/MKV compatibles gardent leur vidéo originale et ses index de seek ; la
sélection explicite de langue utilise les fenêtres Opus copiées. Les tests
comparatifs original natif / original avec audio MSE conservent ce compromis :
le natif est rapide mais n'offre pas de choix interne fiable sur Chrome, tandis
que les fenêtres permettent de choisir la piste sans recopier la vidéo.

Le repli vidéo préparé, lorsqu'un vrai chargement natif échoue, attend encore
la fin du remux complet. Le repli audio Ogg sans MSE peut également copier
toute la piste. Ils restent des coûts ouverts sur les gros fichiers et clients
incompatibles ; aucun remux vidéo progressif nouveau n'est annoncé ici.
Une fenêtre audio est préparée jusqu'à 12 secondes, puis livrée ; ce n'est pas
un envoi HTTP des paquets FFmpeg au fur et à mesure de leur production.

Un microbenchmark du véritable cache Node compare le verrou conservé pendant
l'ouverture avec la réservation d'une lease avant l'ouverture hors verrou.
Huit sessions, 512 requêtes de 64 Kio suivies d'annulation, trois alternances :
médiane 255,879 → 101,665 ms pour le lot, soit 60,3 % de réduction.
Mesure Windows/debug de lecture locale, sans TCP ni navigateur ; ce chiffre
n'est pas une latence première image.
[Échantillons bruts](evidence/2026-10-23/streaming-performance/node-cache.json).

## Gros fichier et sessions simultanées

Le MP4 synthétique existant fait 352 256 545 octets (336 Mio), dure
3 500,01 s et contient deux Opus 5.1 avec AV1 10 bits **1 920 × 1 080,
environ 12 images/s**. Les positions
900 s, 1 800 s et 3 480 s sont réellement demandées.

Le banc a révélé un vrai échec intermittent : réouverture après seek puis
`Réserve de lecture insuffisante après 8 secondes`. La vidéo était à 0 s,
`seeking=false`, `readyState=4`, mais `buffered` restait à
3 447,587–3 500,013 s, ou vide ; l'audio avait bien son buffer 0–10 s.
Une frame vidéo à zéro avait déjà été reçue. Après suppression d'une fuite
des graphes WebAudio de test, l'original direct et le chemin segmenté échouaient
encore au sixième lancement, après un seek à 900 s.

Le correctif de `startup-buffer.ts` accepte l'état natif `HAVE_ENOUGH_DATA`
(`readyState=4`) lorsque le média ne cherche pas. Les états `HAVE_FUTURE_DATA`
conservent le contrôle de réserve ; le délai maximal n'est pas augmenté.
[Sémantique des états de disponibilité](https://developer.mozilla.org/en-US/docs/Web/API/HTMLMediaElement/readyState).
Le même banc, huit ouvertures et huit seeks **par stratégie**, puis seize
switches segmentés, termine sans erreur après ce changement.

Sur ce passage final : sept reprises chaudes segmentées, frame médiane / p95
122,7 / 137,2 ms, PCM 126,0 / 135,5 ms ; huit seeks segmentés, frame
60,3 / 92,3 ms, PCM 105,0 / 234,4 ms. Certaines fenêtres étaient nouvelles,
d'autres réutilisées lors du deuxième cycle de positions. La cible de reprise
<100 ms et la cible de PCM <100 ms ne sont pas atteintes de façon constante.
Les graphes de test retirés du DOM sont déconnectés ; cette correction du banc
est distincte du correctif de démarrage de production.
Ce fichier n'est ni un remux de plusieurs dizaines de Go ni un débit NAS réel.

Quatre sessions simultanées sur la petite fixture MP4, contextes navigateur neufs
avec fenêtres serveur explicitement chaudes : première frame 43,5–82,9 ms,
premier PCM 74,9–122,2 ms ; 29–30 frames par session pendant l'observation,
aucune frame perdue rapportée ni erreur. Cette courte observation ne constitue
pas un test de saturation longue durée.

Le RSS et le CPU exposés par ce banc concernent exclusivement son processus HTTP
JavaScript, qui transpile aussi le lecteur : ils ne sont pas le coût Rust ou NAS
par session. Le serveur lit par blocs de 64 Kio ; les originaux ne sont jamais
chargés dans un Buffer complet et aucune copie vidéo entière n'est préparée.
La mémoire navigateur et le coût du décodage AV1 sur matériel NAS ne sont pas
mesurés ici.

## Parcours réel Svelte/Core/SQLite/Node local

`browser_scanner` exécuté avec le build statique de production, des bibliothèques
jetables et un vrai redémarrage du Core : MP4 puis MKV, décodage pixels/PCM,
FR/EN/FR, pause/seek, erreurs de langue, progression, sous-titres et repli après
échec réseau de l'original passent. Les deux JSON `core-report*.json` archivent
les clics dans le vrai catalogue. Ce parcours utilise le binaire Rust debug.

| Clic → | MP4 premier lancement | MKV premier lancement après redémarrage |
| --- | ---: | ---: |
| Dialogue ouvert | 15,5 ms | 24,4 ms |
| Première frame | 295,5 ms | 277,0 ms |
| Événement vidéo playing | 626,7 ms | 617,5 ms |

Réouvertures suivantes : MP4 frame 57,6–73,7 ms ; MKV frame 56,6–84,4 ms.
Le dernier lancement MKV avec échec réseau forcé et repli vidéo préparé atteint
127,9 ms. Un passage précédent avait obtenu 144–194 ms au premier lancement et
44–67 ms en réouverture ; il reste archivé dans `core-earlier-report*.json`.
Cette variabilité et ces petits échantillons ne définissent pas un p95 de
l'application, ni une amélioration démontrée de son premier lancement.
Le son de ce parcours est contrôlé par PCM dans le scénario, mais son instant
initial exact n'est pas instrumenté ; `playing` ne le remplace pas.
Les 50 ms ne sont donc pas une garantie du parcours complet, même local.

Le premier contrôle avait fermé le dialogue dès la création du candidat audio
(son attribut dataset était déjà présent), avant sa validation et la sauvegarde
de préférence. Le contrôle attend maintenant la piste unique en lecture et la
préférence persistée et la sélection validée par Svelte avant de tester son repli.
Le second échec est archivé dans `core-before-cancel-guard-failed.json`.
Le lecteur vérifie aussi l'annulation après chaque promesse `play()` ; un ancien
lecteur détruit et son timeout ne peuvent plus repauser l'élément vidéo réutilisé.
Le dialogue ignore une sélection audio terminée après sa fermeture/réouverture.

## Commandes

Depuis la racine du dépôt, PowerShell, avec Chrome/FFmpeg/ffprobe et les fixtures
déjà générées :

```powershell
$env:MEDIA_PERFORMANCE_ROUNDS='11'
$env:MEDIA_REPORT_DIR='target/streaming-performance/reproduce-mp4'
node scripts/browser-streaming-performance.cjs

$env:MEDIA_TEST_FIXTURE='tests/fixtures/generated/representative/representative.mkv'
$env:MEDIA_REPORT_DIR='target/streaming-performance/reproduce-mkv'
node scripts/browser-streaming-performance.cjs
```

Comparaison du délai contrôlé (exécuter avant puis après, aucun fichier source
réécrit) :

```powershell
$env:MEDIA_TEST_FIXTURE='tests/fixtures/generated/representative/representative.mp4'
$env:MEDIA_PERFORMANCE_ROUNDS='6'
$env:MEDIA_PERFORMANCE_AUDIO_DELAY_MS='300'
$env:MEDIA_PERFORMANCE_BASELINE='1'
$env:MEDIA_PERFORMANCE_BASE_REF='362a5ddf9ea4cfa57ea522c7d47a560683059730'
$env:MEDIA_REPORT_DIR='target/streaming-performance/reproduce-before'
node scripts/browser-streaming-performance.cjs
$env:MEDIA_PERFORMANCE_BASELINE='0'
$env:MEDIA_REPORT_DIR='target/streaming-performance/reproduce-after'
node scripts/browser-streaming-performance.cjs
```

Remettre `MEDIA_PERFORMANCE_AUDIO_DELAY_MS=0` pour les performances naturelles.
`MEDIA_PERFORMANCE_CONCURRENT=1` ajoute les quatre sessions après les essais
sériels. Le script sort en erreur si une stratégie échoue et archive le détail.
Les chiffres ci-dessus proviennent des JSON associés, pas des temps de tests
Cargo. Droits, annulation, Range et nœuds déconnectés restent couverts par les
tests de production ciblés ; ce banc ne les remplace pas.

Preuves : [rapports JSON](evidence/2026-10-23/streaming-performance/).
Les rapports `*-mp4-final` et `*-mkv-final` servent aux latences de démarrage,
seek et nouveau signal, ainsi qu'aux quatre sessions. Leurs premiers oracles de
silence et trou vidéo ont été corrigés ensuite ; **ne pas utiliser ces deux
champs**. Les comparaisons de continuité utilisent uniquement
`before-delayed-v3.json` et `after-delayed-v2.json`, avec observation de toutes
les pistes et stabilisation explicite. Les passages longs échoués et réussis
sont conservés séparément.

## Vérifications exécutées

```powershell
npm test --prefix apps/web
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo test --locked -p media-node --lib -j 1 -- --test-threads=1
cargo test --locked -p media-node --lib -j 1 copies_dual_51_opus_mp4_without_changing_packets_or_channels -- --ignored --nocapture --test-threads=1
cargo test --locked -p media-node -j 1 benchmark_preparation_cache_open_concurrency -- --ignored --nocapture --test-threads=1
cargo test --locked -p media-poc --test scanner_flow --test distributed_multi_node --test node_enrollment_and_relay -j 1 -- --test-threads=1
$env:MEDIA_REPORT_DIR='target/streaming-performance/core-scanner-final'
cargo test --locked -p media-poc --test browser_scanner -j 1 -- --ignored --nocapture --test-threads=1
cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings
cargo fmt --all --check
git diff --check
```

Web : 136 tests réussis, Svelte/TypeScript zéro erreur/avertissement, lint et
build statique réussis. Une erreur d'une fenêtre candidate après son chargement
mais avant son engagement reste locale ; validation synchrone juste avant
destruction de l'ancienne piste, y compris après résolution concurrente du buffer.
Node : 49 tests réussis, trois ignorés dans la suite générale, dont le benchmark
et le contrôle des deux Opus 5.1 exécutés séparément avec succès. Core : cinq
tests d'intégration réussis, puis
le scénario navigateur avant/après redémarrage réussi. Clippy strict et format
réussis. La suite Rust complète du workspace et les clients Edge/Firefox/Safari
n'ont pas été exécutés pour cette mission. Aucun déploiement ni média personnel
utilisé ; CPU/RSS Rust par session et mesures NAS restent à effectuer.

Confirmation du lecteur final après le dernier correctif de transaction du
candidat : [final-current-mp4.json](evidence/2026-10-23/streaming-performance/final-current-mp4.json).
Les deux stratégies terminent trois ouvertures et trois seeks chacune, plus
six switches segmentés, sans erreur. Ce court passage de non-régression porte
les SHA-256 des sept modules effectivement injectés et du média, et complète
les 136 tests Web. Il ne remplace pas les comparaisons historiques à dix
reprises : deux reprises chaudes segmentées ont une première frame maximale
de 40,8 ms ; six switches ont un nouveau PCM médian de 57,4 ms mais un maximum
de 226,3 ms. Les limites de 50 et 100 ms restent donc conditionnelles.

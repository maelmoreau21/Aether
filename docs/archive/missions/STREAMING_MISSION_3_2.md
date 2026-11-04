# Mission 3.2 — protocole Release et concurrence

État : harnais prêt, résultats Release à produire. Les rapports debug historiques
restent inchangés. Aucun succès Release/16 sessions/p99 UI n'est présumé.

L'oracle v2 distingue `firstRawPcmMs` (PCM de l'élément sélectionné, audible,
sans condition de lecture vidéo, position ou dérive), `firstAnyPcmMs` (peut
appartenir à l'ancienne piste pendant un switch), et `firstAudioMs` (ancien PCM
aligné à 150 ms). `initialPcm.signedDriftSeconds` conserve la différence des
horloges au premier PCM brut, même si elle dépasse 150 ms ou si la vidéo est
encore en pause. Les échantillons de la première seconde restent dans le brut.
Le polling PCM est de 8 ms, ses écarts réels sont enregistrés. Ces horloges et
rVFC ne mesurent ni haut-parleur physique ni photons de l'écran.

`openingOutcomes` compte tous les essais, y compris les timeouts et les ouvertures
partielles. Les quantiles tous essais exigent au moins 100 essais; un rang qui
atteint les observations manquantes est `unobserved-failure-or-deadline`, jamais
une durée fabriquée. `observedOnly` reste explicitement conditionnel. Les erreurs
d'une action ultérieure ne suppriment pas les premières observations d'ouverture.
Les quantiles froids restent indisponibles avec un seul essai froid.

Après création et gel du snapshot Release (binaires + Web + manifeste), commandes
PowerShell depuis la racine. Les trois commandes ci-dessous permettent respectivement 100, 104 et
112 essais chauds avec comptes ordinaires distincts par session concurrente :

```powershell
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-release --build-profile release --sessions 4 --rounds 25 --skip-actions --out target/streaming-release-4
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-release --build-profile release --sessions 8 --rounds 13 --skip-actions --out target/streaming-release-8
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-release --build-profile release --sessions 16 --rounds 7 --skip-actions --out target/streaming-release-16
```

Lots fonctionnels séparés (les échecs sont conservés, sans retry caché) :

```powershell
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-release --build-profile release --sessions 1 --rounds 1 --prepared-reopen --close-during-preparation --out target/streaming-release-reopen-close
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-release --build-profile release --sessions 1 --rounds 1 --skip-actions --robustness --fault-mode node-crash --out target/streaming-release-crash
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-release --build-profile release --sessions 1 --rounds 1 --skip-actions --robustness --fault-mode network-cut --out target/streaming-release-network
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-release --build-profile release --sessions 1 --rounds 1 --skip-actions --truncate-after-scan-bytes 65536 --out target/streaming-release-corrupt
```

La coupure réseau détruit les vrais sockets d'un proxy TCP transparent et refuse
les connexions jusqu'à restauration, en laissant Node actif. Le crash arrête le
vrai processus Node. Le scénario de fermeture attend la première requête média
réelle puis ferme le dialogue sans attendre la préparation et vérifie l'absence
d'audio résiduel et la réouverture. Il enregistre l'état exact avant fermeture :
une préparation déjà achevée ne prouve pas une annulation en vol.

La corruption concerne exclusivement une **copie privée** de la fixture, tronquée
après le scan réel. Aucun hardlink n'est tronqué, aucun média original modifié.
Le lot corrompu doit produire un échec explicite; `passed:false` est conservé et
n'est pas à lui seul un échec de robustesse. Examiner erreurs/état UI et mémoire.

Les Job Objects Windows mesurent CPU Rust et descendants FFmpeg, RSS échantillonné
et I/O logique; les fichiers de cache sont comptés séparément. L'instrumentation
GlobalAlloc Rust du composant est une expérience distincte, jamais une estimation
des allocations globales du processus ou du navigateur. Le snapshot Release est
identifié par SHA, la valeur `--build-profile` n'est pas une attestation du build.

Docker Linux répond sur cette machine. Inspection en conteneurs temporaires
`--rm --read-only --network none`, sans montage de dépôt ni compilation : image
`media-ci-stable:20261021` fournit Cargo1.99.1, Node26.12.1 et Chrome sous `/opt/google`.
`bash -lc` réinitialise PATH : utiliser `bash -c`. Aucun FFmpeg accessible dans
PATH des images inspectées `media-ci-stable`, `media-ci-ffmpeg:9.0.2` ou
`media-preparation-runtime:test`; une image nommée FFmpeg ne prouve pas son runtime.
Le collecteur système actuel refuse Linux plutôt que de produire de faux chiffres
Win32. Aucun test Linux du moteur, NAS ou Node physique distant n'est encore prouvé.

Validation ciblée du harnais :

```powershell
node --test scripts/streaming-benchmark-oracle.test.cjs
```

Deux contrôles : le PCM brut est enregistré malgré vidéo en pause/dérive initiale
1 s; avec 98 observations et deux timeouts sur100 essais, p95=95 ms et p99 reste
explicitement non observé. Ce sont des tests de l'oracle, pas des mesures moteur.

Export de diagnostic exclusif (aucun timing froid réutilisable) :

```powershell
node scripts/streaming-system-benchmark.cjs --binary-dir target/streaming-system-artifacts/candidate-native-v1 --sessions 1 --rounds 1 --export-native-input target/audio-concurrency-native-input-full --out target/native-input-export-full-3-2
```

Exécuté avec succès : douze réponses Core/Node authentifiées exactes, pistesMP4
2/3 et fenêtres0/1/2/90/91/180, sans remux par le harnais. SHA de chaque fichier,
route sans jeton, taille et empreintes des binaires dans `provenance.json`.
Ce mode sort avant le navigateur et toutes les cellules de mesure; les requêtes
d'export chauffent explicitement les index/caches.

Validation de l'oracle complétée : quatre tests passent, dont coupure et restauration
de vrais sockets TCP echo. Ces checks n'affirment pas la robustesse du lecteur.

Linux : FFmpeg9.0.2 est finalement présent dans
`target/ffmpeg-linux-9.0.2/bin` du dépôt (montéRO dans le conteneur), pas dans PATH
des images. Le premier `cargo --offline` n'a pas pu résoudre `multer` dans l'ancien
cache. Compilation en ligne dans un cache dédié désormais en cours sur une copie
figée de crates/Cargo : `target/linux-mission-3-2-snapshot`. Les médias restent
montésRO, sans copier le film1.76Go. Logs bruts dans
`target/linux-mission-3-2-evidence/`; aucun résultat Linux réussi anticipé.

La configuration16comptes respecte le vrai budget d'authentification : au plus
9logins puis pause asynchrone61s, enregistrée dans `authSetupPacing`, hors chronomètre
clic. Aucun spoofingIP ni neutralisation de la protection. Les scénarios avec
fermeture signalent `closedBeforePresentationReady`; si la préparation finit trop
vite pour être interrompue, ils ne déclarent pas une annulation réussie.

Linux effectué : suite complète sur snapshot antérieur au correctif guard,
66réussites/3échecs/4ignorés. Les trois échecs génèrent leurs fixtures avec des
encodeurs/filtres absents du FFmpeg9local (`--disable-encoders --disable-filters`),
preuve `linux-ffmpeg-buildconf.log`; ils ne sont pas effacés du rapport.
Sur snapshot avec guard bloquant,25tests distincts passent :10Native,6vidéo,
6confinement,3Range/stream. Le filtre `stream::` a répété6tests vidéo dans le log
brut; ils ne comptent pas comme6nouvelles preuves. Le microbenchmark natif est
ignoré. La barrière annulation et les admissions4/8/16 passent sous LinuxDocker.
Le tout dernier plafond64admissions froides nécessite un dernier retestNative
sur snapshot séparé; son état final sera enregistré dans les preuves.

Clôture Linux : dernier Native11réussites/0échec/1microbenchmark ignoré, avec
plafond64, refus65e, exemptioncachechaud et barrièreannulation. Les autres modules
ciblés vidéo6/confinement6/stream3 avaient réussi sur snapshotguard précédent.
Synthèse courte : `docs/evidence/2026-10-24/streaming-mission-3-2/linux-summary.json`;
logs complets et SHA des snapshots conservés. Conteneurs temporaires terminés.
Aucun CargoWindows lancé par cet agent. Le runtime FFmpeg minimal laisse trois
échecs de génération dans le lot complet antérieur; aucun «tous les tests Linux
passent» n'est revendiqué. La campagne de 100 ouvertures UI est suspendue pour économiser le quota;
aucun p99 UI Release ne peut être annoncé à ce stade.

## Smoke Release final — 24 octobre 2026

Un seul lot à4sessions est terminé avec succès :4ouvertures froides applicatives,
4chaudes,16switches et32seeks, aucune erreur. Aucun lot8/16/100 n'a été lancé.
Les artefacts Core/Node Release et Web ont été copiés puis vérifiés inchangés;
les sources ayant changé après compilation dans d'autres chats, les SHA des
artefacts font foi et n'attestent pas un HEAD exact.

Sur les4ouvertures chaudes, médiane/max : première image2251,9/2388,9ms,
PCM brut2375,3/2408,7ms. Switch PCM aligné215,5/331,7ms (n8), seek image
232,1/342,1ms (n16), seek PCM aligné293,8/448,4ms. Sur4ouvertures froides
applicatives, image médiane2667,7ms, PCM brut2472,6ms. Les objectifs50/100ms
ne sont donc pas atteints dans ce lot concurrent. Aucun p99UI n'est estimé.
Les horloges proches au premier PCM ne prouvent pas la synchronie physique
entre premier affichage et premier son.

Pour chaque cellule complète (4ouvertures+8switches+16seeks), Node : CPU total
671,9ms froid/843,8ms chaud, RSS maximal échantillonné21,46/21,76Mio,
0processusFFmpeg et0octet de nouveau cache de préparation. Ces compteurs
n'incluent pas Chrome et l'I/O logique n'est pas une mesure d'écriture physique.
Pas de comparaison de gain avec les anciens lots debug sous conditions distinctes.

Preuves : `release-smoke4.json`, `release-smoke4-summary.json`,
`release-artifacts.json` dans le dossier de preuves Mission3.2. CompilationLinux
de l'exemple d'allocations réussie (`cargo check`,4m31), aucune mesure d'allocation
exécutée sous Linux. Permissions de bibliothèques/identités réelles conservées.
Les processus Rust/FFmpeg du smoke ont été arrêtés par le harnais; aucun NAS,
Node physique distant, reprise préparée ou scénario de panne supplémentaire
n'a été validé par ce petit smoke. Aucun push ni déploiement.

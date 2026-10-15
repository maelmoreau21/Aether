# P0C — Shaka face au MSE minimal, 15 octobre 2026

**Résultat : Shaka change la langue plus vite, avec davantage d'interruptions
observées. Aucun lecteur définitif choisi. Video.js non développé.**

## Périmètre et reproductibilité

Windows x64 10.0.26300, Chrome 155.0.8059.40, Edge 155.0.4283.45,
Firefox 155.0.1 ; headless, profils neufs, aucune préférence codec/autoplay
expérimentale. Playwright 1.62.1 / geckodriver 0.37.1.

Shaka **5.2.13**, fixé depuis npm, archive vérifiée SHA512 et bundle SHA256
consignés dans [la provenance](evidence/2026-10-15/shaka/shaka-provenance.json).
Apache-2.0 et LICENSE conservés dans les fixtures ignorées par Git ; licence du
projet inchangée. Les sources de cette version ont été consultées pour
`attach`, `load`, `getAudioTracks`, `selectAudioTrack(track, safeMargin)`,
`getTextTracks`, `selectTextTrack(track|null)`, `preferredAudio`, `preferredText`
et `NativeTextDisplayer(player)` :
[source officielle versionnée](https://github.com/shaka-project/shaka-player/blob/v5.2.13/lib/player.js).
Pas d'ancienne API `selectAudioLanguage`, pas de CDN à la lecture.

À partir des fixtures représentatives et MSE existantes :

```powershell
python scripts/prepare-dash.py
python scripts/prepare-shaka.py
cargo run --locked -p media-poc
```

Les deux préparations refusent l'écrasement. Ouvrir `/shaka`, cliquer
Préparer Shaka puis lecture ; `/mse` reste le témoin inchangé.
Les scripts n'ajoutent aucune dépendance FFmpeg au serveur. Le nouveau
packaging n'exécute même pas de muxeur : **copies binaires des mêmes fMP4**
AV1 Main 10 bits SDR BT.709, FR Opus 5.1 et EN Opus stéréo. Aucun réencodage.

Le MPD statique 12,008 s contient un Period, une représentation vidéo et deux
adaptations audio FR/main et EN/alternate. SegmentList, SegmentTimeline,
Initialization et byte ranges proviennent des boîtes tfdt/tfhd/trun réellement
présentes : sept fragments par flux. Les timestamps audio +7 ms déjà mesurés
restent inchangés, sans correction cachée. Les empreintes des fichiers complets
relient ces copies aux preuves précédentes de tous les paquets et codec init.
WebVTT FR complet, FR forced et EN : mêmes octets que MSE, Role forced-subtitle
explicite dans le MPD. Préparation DASH unique : 154 ms, pas un benchmark NAS.

## Protocole commun exécuté

Six sessions séquentielles, MSE puis Shaka pour chaque navigateur. Trois tours
de six changements par session : EN/FR en lecture, EN/FR en pause, EN/FR après
seek. **108 changements**, dont 72 en lecture/après seek et 36 en pause.
Chaque tour vérifie aussi EN après seek arrière. Aucun rechargement de page
ou de source pendant une session ; la même URL blob reste en place.
Le chemin Shaka observé est bien `LoadMode.MEDIA_SOURCE`, pas une URL native.

Même graphe Web Audio pour les deux moteurs : FFT8192, six voies séparées,
FR 440/550/660/80/770/880 Hz, EN 990/1100 Hz avec quatre voies silencieuses.
Trois observations consécutives correctes exigées. RMS sur les **1024 derniers
échantillons d'un tableau temporel complet de 8192** ; fenêtre courte ~21,3 ms,
FFT ~170,7 ms à 48 kHz. Polling visé 10 ms, maximum réel jusqu'à 101 ms.

La fenêtre RMS a été corrigée pendant la mise au point : passer un tableau
1024 à un analyser de taille8192 tronquait le début de sa fenêtre, introduisant
un biais d'environ149 ms. Le [parcours préliminaire](evidence/2026-10-15/shaka/preliminary-old-rms-window.json)
est conservé, **exclu des statistiques finales**. Les anciennes latences du
jalon MSE précédent ne sont donc pas comparées directement aux nouvelles.

Pause : conservation vérifiée pendant100 ms, puis reprise pour observer le PCM.
Le délai demande→signal inclut cette pause volontaire ; reprise→signal est
également conservé séparément. Les silences volontaires sont exclus des
statistiques des changements en lecture.

À chaque changement en lecture : frames présentées et PTS en progression,
événements waiting/seeking/buffering, gaps callbacks vidéo, silence RMS total
et durée de la séquence silencieuse maximale sont enregistrés. Deux frames
en progression ne prouvent pas l'absence d'un gel intermédiaire.

Après les changements EN et FR : seek vers les flashs connus à6 et9 s,
observation conjointe de la vidéo blanche et du marqueur d'amplitude audio.
**36 observations**, tolérance préétablie ±200 ms. Le temps dit audio est
`video.currentTime` au polling du RMS, pas un timestamp intrinsèque du PCM.
Il s'agit d'un indicateur de cohérence de l'horloge média, pas d'une mesure de
la sortie physique ni d'une validation précise des +7 ms. Les seeks répétés
et ce clip court ne valident pas la dérive d'un film long.

## Préférences, sous-titres et erreurs

Shaka : FR choisi par `preferredAudio:[{language:'fr'}]`, FR forced par
`preferredText:[{language:'fr',forced:true}]`. Signal FR réellement vérifié,
cue FR FORCE CUE2 et capture par navigateur. Ensuite choix manuel EN texte :
il reste sélectionné pendant les18 changements et les seeks de chaque moteur.
Les choix audio EN restent effectifs après seek malgré la préférence FR initiale.
Cela teste la session courante, pas une persistance de préférences utilisateur.

`accessibility.handleForcedSubtitlesAutomatically=false` est explicite pour
empêcher un changement audio d'écraser le choix texte manuel. ABR désactivé,
une seule vidéo ; `selectAudioTrack(track,0)` efface immédiatement le buffer
audio. Ce compromis vitesse/coupures n'est pas présenté comme une configuration
optimale. Un autre safeMargin n'a pas été comparé ici.

FR complet, FR forced, EN puis off : texte exact des cues à4,25 s vérifié,
captures conservées. Douze captures Shaka inspectées : vidéo visible, forced
par défaut, FR complet, EN, absence de texte en off sur les trois navigateurs.
Rendu natif des **WebVTT externes via Shaka**, pas sélection de sous-titres
intégrés au MKV/MP4. Le menu du banc a été corrigé pour refléter les sélections
effectuées par API ; contrôle Chrome supplémentaire effectué.

Pas d'erreur de décodage dans les six parcours finaux. Le diagnostic Chrome
avec un MPD absent renvoie réellement404 et Shaka1001/category1 :
[preuve](evidence/2026-10-15/shaka/ui-error-chrome.json). Ce chargement diagnostic
appelle directement l'API et ne valide pas l'affichage d'erreur du bouton.
Les **9 tests Rust** passent, dont nouvelles routes DASH/Range exact/MIME,
whitelist et refus cross-site. `cargo fmt --check`, clippy avec `-D warnings`,
syntaxes JS/Python et tests des statistiques médiane/P95/cumul CPU passés.
CI préparée pour le MPD et les statistiques, non exécutée à distance.

## Limites et recommandation provisoire

Le MSE minimal charge les pistes complètes en mémoire avant les changements ;
Shaka demande des plages de fragments au serveur et gère leur scheduling.
Les octets encodés sont identiques, les politiques de buffers et les requêtes
ne le sont pas : c'est une comparaison des deux implémentations, pas un test
isolant exclusivement le coût des bibliothèques. Ordre fixe, seulement trois
tours, réseau loopback et instrumentation coûteuse limitent les conclusions.

**Shaka est le candidat à poursuivre pour la gestion DASH/audio/langues/texte** :
il remplace plusieurs mécanismes du POC et réduit le temps jusqu'au signal
sélectionné. Il n'améliore pas la continuité avec safeMargin=0 : silences et
gaps vidéo subsistent, waiting/seeking sont fréquents. Le MSE reste un témoin
utile, et n'est pas supprimé. Avant toute décision : comparer une marge audio
non nulle, tester des clips longs et réseau dégradé, puis Video.js dans son
jalon distinct. Aucun lecteur définitif ni passage àP1.

Sortie physique5.1, mono/7.1, affichage physique10bits, NAS, matériel du décodeur,
réseaux distants, segments corrompus et endurance restent NT. Chaque piste
conserve son propre nombre de canaux ; aucun format multicanal n'est imposé.

## Rejouer les mesures

Serveur actif, Playwright disponible pour Node et geckodriver local :

```powershell
$env:MEDIA_BROWSER_CHANNEL='chrome'
Remove-Item Env:MEDIA_BROWSER_EXECUTABLE -ErrorAction SilentlyContinue
$env:MEDIA_PLAYER_ENGINE='mse' # puis 'shaka', même protocole
$env:MEDIA_REPORT_DIR='artifacts/nouvel-essai-mse-chrome'
$env:MEDIA_SAMPLE_RESOURCES='1'
$env:MEDIA_SERVER_PID=(Get-Process media-poc | Where-Object Path -EQ (Join-Path (Get-Location) 'target\debug\media-poc.exe')).Id
node scripts/browser-player-comparison.cjs
```

Rejouer chaque moteur avec un dossier neuf pour Edge et Firefox :
`MEDIA_BROWSER_CHANNEL=msedge` et chemin `MEDIA_BROWSER_EXECUTABLE` vers Edge,
puis `firefox` et chemin Firefox. Les chemins locaux de référence figurent
dans [le jalon MSE](P0C_MSE_EXPERIMENT.md). Les ressources Windows sont
facultatives : supprimer MEDIA_SAMPLE_RESOURCES pour un parcours sans sampler.
`node scripts/summarize-player-comparison.cjs <dossier-parent-des-six-rapports>`
produit les statistiques. `node scripts/player-comparison-stats.test.cjs`
vérifie notamment le cumul CPU lors de disparition/réutilisation des PID.

[Rapports, ressources brutes, captures, MPD et packaging](evidence/2026-10-15/shaka/).

## Mesures finales

Les tableaux ci-dessous sont issus du [résumé JSON](evidence/2026-10-15/shaka/summary.json).
P95 par rang supérieur ; n=12 en lecture/après seek par couple moteur/navigateur,
donc **P95=max avec cet effectif**. Statistiques descriptives, pas une estimation
robuste du comportement rare en production.

| Navigateur | Moteur | Latence médiane | P95 / maximum | Pause→signal médiane / max | Silence total médian / max en lecture | Gap vidéo max | Waiting en lecture |
|---|---|---:|---:|---:|---:|---:|---:|
| Chrome | mse | 370,4 ms | 383,2 ms | 485,5 / 520,4 ms | 0 / 0 ms | 66,7 ms | 0/12 |
| Chrome | shaka | 160,7 ms | 181,2 ms | 175,7 / 192 ms | 30,9 / 100,3 ms | 116,4 ms | 12/12 |
| Edge | mse | 365,9 ms | 386,7 ms | 476,5 / 513,8 ms | 0 / 0 ms | 66,9 ms | 0/12 |
| Edge | shaka | 152 ms | 211 ms | 178,1 / 191,5 ms | 30,1 / 110 ms | 166,5 ms | 12/12 |
| Firefox | mse | 1120 ms | 1151 ms | 1232,5 / 1241 ms | 0 / 0 ms | 108,6 ms | 0/12 |
| Firefox | shaka | 167 ms | 207 ms | 178 / 184 ms | 72 / 150 ms | 182,6 ms | 12/12 |

Les36 changements Shaka en lecture/après seek génèrent chacun waiting et seeking,
contre zéro pour le MSE. Les frames et PTS continuent néanmoins de progresser.
Le MSE conserve plus longtemps l’ancien signal ; zéro silence détecté ne signifie
ni zéro délai ni zéro microcoupure sous la résolution du polling.

| Navigateur | MSE : écart horloge média A/V min..max | Shaka : min..max |
|---|---:|---:|
| Chrome | 10,7..28,6 ms | 10,7..21,3 ms |
| Edge | 10,7..21,3 ms | 8..20,9 ms |
| Firefox | -63,3..0 ms | -29,3..10,7 ms |

Les36 observations restent dans ±200ms ; pas de désynchronisation persistante
détectée dans ce protocole court. Aucune preuve de synchronisation physique.

### CPU et mémoire mesurés séparément

Somme des working sets résidents des processus identifiés, pas mémoire privée :
les pages partagées peuvent être comptées plusieurs fois. CPU calculé par deltas
non négatifs pour chaque PID+date de création ; les processus disparaissant
ne font pas soustraire du CPU. Les processus nés et morts entre deux samples
peuvent être manqués ; le premier cumul d’un nouveau processus est exclu.
CPU en % d’un processeur logique (100%=un cœur logique occupé, huit disponibles).
Les valeurs en % de la machine et les identités sont dans le JSON.

| Navigateur | Moteur | CPU navigateur moyen | Working set navigateur médian / pic | CPU Rust moyen | Working set Rust pic |
|---|---|---:|---:|---:|---:|
| Chrome | mse | 84,8 % | 776,7 / 828,9 MiB | 0 % | 9,9 MiB |
| Chrome | shaka | 96,4 % | 781,1 / 815,2 MiB | 4,2 % | 9,7 MiB |
| Edge | mse | 67,8 % | 624,4 / 676,9 MiB | 0 % | 9,6 MiB |
| Edge | shaka | 93,1 % | 660,8 / 695,3 MiB | 3,3 % | 9,7 MiB |
| Firefox | mse | 75,7 % | 927,9 / 1090,4 MiB | 0 % | 9,7 MiB |
| Firefox | shaka | 105,7 % | 929,1 / 1056 MiB | 4,6 % | 9,7 MiB |

Mesure chaude après préparation et initialisation, environ8,5–26,6s selon le
parcours : pas le démarrage/probe Rust, pas la préparation DASH. Le MSE a déjà
téléchargé son clip complet avant cette fenêtre ; Shaka continue ses requêtes
Range pendant les changements. CPU Rust0 signifie aucun incrément observé
à la résolution du compteur, pas coût nul.

Le graphe FFT/RMS, le polling, les callbacks et WebDriver font partie du CPU
navigateur. Le sampler CIM/Get-Process cible500ms mais coûte environ247–746ms
par collecte sur ces runs ; son propre CPU et sa durée figurent dans les traces,
sans inclure tout le coût des services Windows sollicités. Les processus
navigateur de fond sont inclus. Ce sont des mesures du **banc instrumenté**,
pas des budgets RAM du produit ni une preuve d’efficacité CPU supérieure.
Aucun classement de performance CPU n’est justifié par ces petits parcours.

Suite exécutée : [comparaison safeMargin](P0C_SAFE_MARGIN_EXPERIMENT.md).
Les marges positives améliorent les coupures mesurées mais régressent au seek ;
aucune configuration générale validée. Les échecs de réexécution sont conservés.

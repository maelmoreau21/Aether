# Lecteur — seeks et démarrage, 30 octobre 2026

Les essais utilisent Chrome 156 sous Windows et les modules TypeScript de
production. Le serveur local HTTP et les fenêtres préparées par FFmpeg sont
synthétiques : ils ne représentent pas Core, Media Node ou NAS. Les sources
sont figées avant chaque essai et leurs empreintes sont dans les JSON. Les
mesures d'un seul poste avec de petits échantillons ne démontrent pas un gain
général ; elles ne remplacent pas la reproduction du problème utilisateur.
Le candidat mesuré ici précède une dernière correction audio ; les empreintes
archivées permettent de l'identifier et ces mesures ne sont pas attribuées au
code final.

Sur le repli vidéo AV1 progressif et audio Opus externe, trois seeks isolés
puis six seeks espacés de 100 ms (1, 30, 3, 28, 4, 25 s) ont abouti avant et
après. Avant : cible finale 25,028 s, écart A/V 3,6 ms, série 819 ms. Après :
25,003 s, écart 2 ms, série 701 ms. Une observation par version. Le callback
vidéo est installé après `play()` : le champ `firstFrameMs` ne mesure pas
nécessairement la première frame du parcours. Ce test n'a pas reproduit le
blocage de dix secondes signalé sur le matériel de l'utilisateur.

Le même scénario final a été rejoué avec la vidéo MP4 originale servie avec
Range et une piste Opus externe, plus proche du format indiqué par l'utilisateur.
Le candidat atteint 25,068 s, audio à 25,091 s, en 656 ms pour la série ; PCM
non nul et aucune erreur. Le premier essai HEAD atteint aussi la cible en 664 ms,
mais l'échantillon PCM initial reste nul : le test est déclaré échoué. Une seule
répétition de contrôle HEAD réussit, avec PCM non nul et 667 ms. La cause de
l'échantillon nul initial n'est pas établie ; aucun gain de vitesse ni correction
de cet épisode intermittent n'est déduit de ces essais. Ces tests sont toujours
sur un serveur synthétique local, sans véritable Core/Node ni film personnel.

Rapports supplémentaires : `target/player-rapid-final-20261030/report.json`,
`target/player-rapid-direct-before-20261030/report.json`,
`target/player-rapid-direct-before-repeat-20261030/report.json` et
`target/player-rapid-direct-final-20261030/report.json`. Le drapeau
`MEDIA_PLAYER_DIRECT=1` sélectionne l'original MP4 dans le banc.

Sur une fixture MP4 de 3 500 s, deux réouvertures chaudes et trois seeks par
version réussissent. Pour l'audio segmenté, les p95 par rang supérieur sont les
maxima de ces petits échantillons : frame chaude 127,9 → 135,3 ms, PCM chaud
130,8 → 134,0 ms, frame après seek 63,3 → 75,4 ms et PCM après seek
171,4 → 493,5 ms. Le dernier résultat s'est dégradé dans cet essai ; sa cause
et sa reproductibilité ne sont pas établies. Les caches disque de Windows ne
sont pas purgés. Aucune conclusion NAS, réseau distant, sortie sonore physique,
longue durée ou percentile robuste n'en découle.

Rapports bruts locaux : `target/player-rapid-before-20261030/report.json`,
`target/player-rapid-after-20261030/report.json`,
`target/player-startup-before-20261030/report.json` et
`target/player-startup-after-20261030/report.json`.

```powershell
$env:MEDIA_PLAYER_REF='HEAD' # retirer pour prendre les sources de travail
$env:MEDIA_PLAYER_REPORT_DIR='target/player-rapid-before-20261030'
node scripts/browser-external-progressive-poc.cjs
$env:MEDIA_PERFORMANCE_PLAYER_REF='HEAD' # retirer pour le candidat
$env:MEDIA_PERFORMANCE_ROUNDS='3'
$env:MEDIA_TEST_FIXTURE='tests/fixtures/generated/dual-opus/dual-opus-long.mp4'
$env:MEDIA_PERFORMANCE_SEEK_TARGETS='2,1800,3480'
$env:MEDIA_REPORT_DIR='target/player-startup-before-20261030'
node scripts/browser-streaming-performance.cjs
```

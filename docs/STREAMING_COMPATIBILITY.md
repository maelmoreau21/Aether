# Compatibilité observée — P0 natif et limite de sélection audio

## AV1 10 bits et deux Opus 5.1 — 22 octobre 2026

Chrome 155.0.8059.85 et Edge 155.0.4283.62 sous Windows, headless : MP4
synthétique 1080p AV1 Main 10 bits, deux Opus 5.1 FR/EN, lecture décodée et
FR/EN/FR répété sans rechargement vidéo. Plein écran, position, pause/seek/reprise,
échec audio et réouverture vérifiés. La sonde `codecs="av01"` incomplète rejetait
à tort les sources préparées ; correction et commandes dans
[AV1_OPUS_AUDIO_FIX.md](AV1_OPUS_AUDIO_FIX.md).
Le parcours Core/SQLite/Chrome MP4/MKV et les préférences serveur passent aussi.
Pas de preuve sur le fichier Avatar ni la sortie physique 5.1 ou le NAS.

## Scanner Films P1 — 16 octobre 2026

Chrome155.0.8059.40 / Windows10.0.26300 headless : MP4 AV1 10bits + Opus FR
synthétique copié dans une bibliothèque locale, découvert/indexé puis lu dans
le lecteur natif existant. Progression temporelle, pause et seek4s vérifiés,
ainsi que catalogue et nouvelle lecture après arrêt/relance réel du Core avec
la même SQLite. Aucun probe au deuxième scan des deux fichiers inchangés.
Rapports et captures : [evidence/2026-10-16/scanner/](evidence/2026-10-16/scanner/).
Ce parcours n'ajoute pas de preuve PCM/audio physique, de compatibilité MKV
générale, de commutation des pistes internes ou de résultat Edge/Firefox/Safari.
Les fichiers MKV multipistes sont inspectés par les tests Node/SQLite séparés.

Le jalon **10 bits/5.1/sous-titres** figure en fin de document. Les premières
sections restent les preuves des fixtures 8 bits/mono.

Observation du **15 octobre 2026**. NT = non testé. Pas de compatibilité extrapolée
à partir des codecs, d'un MIME, de `canPlayType` ou de la présence d'un manifeste.

Clients effectivement exécutés : **Chrome 155.0.8059.40**, **Microsoft Edge
155.0.4283.45** et **Mozilla Firefox 155.0.1**, tous headless, Windows x64
build 10.0.26300. Chrome/Edge via Playwright 1.62.1 ; Firefox installé, non
modifié, via geckodriver 0.37.1 et WebDriver. Profils de test séparés ; aucune
activation expérimentale de `audioTracks`. Edge a été trouvé dans EdgeCore :
aucune installation n'a été nécessaire. Lecture native `HTMLVideoElement`
via HTTP local Axum, **sans append MSE, Video.js, Shaka ou transcodage**.

Même vidéo synthétique dans tous les cas : AV1 Main, `av01.0.00M.08`, 8 bits,
YUV420p, 320×180, 24 fps, 8 secondes ; deux pistes Opus mono 48 kHz FR/EN.
FFmpeg/ffprobe 9.0.2. Les remuxes conservent chaque paquet vidéo et audio
(SHA-256 comparés par piste), et les quatre fichiers sont décodables hors navigateur.

| Source réelle | Chrome natif headless | Pause / seek 4 s / reprise / fin | Audio PCM / pixels | Edge | Firefox | Safari |
| --- | --- | --- | --- | --- | --- | --- |
| MKV AV1 + 2 Opus | Réussi | Réussi | RMS 0,08848 / 256 valeurs (premier essai Chrome) | Réussi | Réussi | NT |
| MP4 faststart AV1 + 2 Opus | Réussi | Réussi | RMS 0,08815 / 256 valeurs (premier essai Chrome) | Réussi | Réussi | NT |
| WebM remux `-c copy` | Réussi | Réussi | RMS 0,08848 / 256 valeurs (premier essai Chrome) | Réussi | Réussi | NT |
| MP4 fragmenté remux `-c copy` | Réussi en URL native | Réussi | RMS 0,08854 / 256 valeurs (premier essai Chrome) | Réussi en URL native | Réussi en URL native | NT |

« Réussi » pour Edge/Firefox inclut pixels décodés (plus de 3 900 couleurs RGB
distinctes), PCM non nul avant/après seek, pause stable, seek à 4 s, reprise et
fin de lecture. Le second passage Chrome utilise le même scénario. La fréquence
dominante mesurée est **439,453125 Hz** sur les douze lectures, cohérente avec
la piste FR 440 Hz. Le rapport `passed` concerne uniquement ce parcours natif ;
la sélection audio a un résultat séparé, **indisponible**, jamais compté comme réussi.

Les pixels sont lus via canvas après lecture ; le signal PCM via Web Audio.
Les compteurs étaient respectivement 87, 86, 86 et 85 frames décodées avant
la fin, après reprise. Ce sont des contrôles automatisés de décodage, pas une
écoute humaine ni une observation sur écran physique. Le script attend ensuite
la fin réelle et archive les événements et les réponses réseau.

La source inexistante renvoie HTTP 404 et produit `MediaError.code=4`,
`MEDIA_ELEMENT_ERROR: Format error` sur Chrome/Edge et `404: Not Found` sur
Firefox, affichés par la page. Les fichiers tronqués
et invalides sont rejetés par le probe Rust/ffprobe dans les tests automatisés.

**Interprétation limitée :** aucun échec direct MKV/MP4 n'a été observé sur ces
fixtures et ces trois clients. Les expériences de remux démontrent une alternative
possible ici, pas la résolution d'un échec sur un autre navigateur. Un MP4
fragmenté lu par URL n'est pas une preuve MSE/DASH/HLS. La synchronisation
fine audio/vidéo n'est pas mesurée par le seul signal RMS.

## Sélection FR/EN — limite observée, pas un succès

Sur **Chrome, Edge et Firefox**, pour les quatre sources, `video.audioTracks`
est absent avec les réglages par défaut. Le sélecteur du banc reste désactivé
et explique cette limite. Les deux pistes existent dans le probe, mais cela
ne prouve pas qu'elles sont sélectionnables par la page. La séquence de test
FR → EN → FR (440 → 880 → 440 Hz) est prévue dans le scénario si une API expose
deux pistes ; **cette branche n'a été exécutée sur aucun des clients testés**.
Le changement **interne au même fichier** vers EN et le retour FR restent donc **non validés**.
Les menus internes des contrôles natifs sont à inspecter manuellement ; ils
ne sont pas déclarés dépourvus de sélection sur la seule absence de l'API JS.

## Native, remux et MSE

- MKV/MP4 d'origine : URL native HTTP. Aucun échec direct observé sur ce jeu
  précis ; aucune conclusion pour tous les MKV/MP4 ou tous les profils codecs.
- WebM et MP4 fragmenté : remux préparés **hors lecture**, paquets inchangés,
  puis URL native. Ils ne démontrent pas une commutation audio ou MSE.
- `MediaSource.isTypeSupported` est interrogé pour chaque MIME avec les codecs
  `av01.0.00M.08,opus`. Sur les trois clients : false pour `video/x-matroska`,
  true pour `video/mp4` et `video/webm`. C'est une capacité **annoncée**, pas
  une preuve d'append, décodage ou sélection audio MSE. **Lecture MSE : NT.**

Restent NT : écoute humaine, commutation FR/EN interne, sous-titres, vidéos alternatives,
HDR/10 bits/4K, longs métrages et réseau distant, contraintes NAS, autres OS,
Safari, Video.js 10/Shaka et packaging MSE. Aucun choix de moteur n'est figé.
**P0A/P0B non clôturés ; P0C commencé, pas terminé.**
Procédure humaine précise : [vérifications manuelles](MANUAL_BROWSER_CHECKS.md).

## P0C — remux FR/EN et changement de source, 15 octobre 2026

Quatre remux `-c copy` : MKV → MKV et MP4 → MP4 faststart, chacun AV1 et
un Opus FR/EN. Chaque paquet vidéo/audio est identique au flux source sélectionné,
les sorties se décodent et les originaux conservent leurs SHA-256 initiaux.
Pas de FFmpeg pendant la lecture ni de MSE ; changement réel d'URL avec seek.

Windows x64 10.0.26300, headless, mêmes Chrome 155.0.8059.40, Edge
155.0.4283.45 et Firefox 155.0.1, essais séquentiels en loopback sans limitation
réseau. **36 changements en lecture et 12 en pause réussis** : six EN/FR par
conteneur/client, puis deux en pause à 4,25 s. Erreur de restauration enregistrée :
**0 s**, pause préservée, reprise et fin atteintes. FR mesuré à 439,453125 Hz,
EN à 878,90625 Hz (une observation Firefox MP4 à 884,765625 Hz, tolérance ±20 Hz).

Millisecondes ci-dessous : min–max de six changements par ligne, un passage local.

| Navigateur / source | Demande → audio stable | Demande → callback vidéo restaurée | Écart callbacks avant/après | Silence PCM échantillonné cumulé par changement |
| --- | --- | --- | --- | --- |
| Chrome MKV | 138,8–151,4 | 41,3–81,9 | 75,0–116,7 | 25,5–51,0 |
| Chrome MP4 | 142,2–156,8 | 48,6–81,6 | 50,0–100,0 | 22,8–52,5 |
| Edge MKV | 149,4–167,9 | 53,0–113,8 | 82,8–133,5 | 37,5–88,8 |
| Edge MP4 | 143,2–169,2 | 48,8–97,6 | 66,5–133,3 | 28,3–65,8 |
| Firefox MKV | 165,0–288,0 | 76,6–210,6 | 100,4–253,3 | 62,0–233,0 |
| Firefox MP4 | 164,0–227,0 | 76,9–145,4 | 89,0–186,3 | 49,0–147,0 |

La détection audio inclut une FFT glissante ~171 ms et trois observations
correctes ; le silence est mesuré séparément par RMS sur ~21 ms. Polling visé
10 ms, plus grand pas observé 44 ms. Les callbacks suivent le compositeur
après seek, pas un écran physique ; l'écart inclut la cadence normale (maxima
des fenêtres de référence avant changement : 40,6–83,3 ms). Ce n'est donc
**ni une garantie de latence ni une commutation déclarée transparente**.

Callbacks à une position antérieure à la cible avant restauration : Chrome
MP4 1, Firefox MKV 5, Firefox MP4 6 ; zéro ailleurs. L'absence de flash d'image
au rechargement n'est pas prouvée ; perception physique et synchronisation A/V
fine restent à vérifier humainement. Les tonalités continues ne suffisent pas.

Préparation mesurée une fois : MKV FR 1 407,36 ms (premier lancement), MKV EN
87,11 ms, MP4 FR 134,03 ms, MP4 EN 108,30 ms ; 710 698 octets supplémentaires.
Pas une mesure NAS ou de remux à la volée. La vidéo est dupliquée et le
démuxeur rechargé ; aucune sélection interne comparable n'a été mesurée avec
l'API par défaut absente. Video.js/Shaka/MSE restent NT et le choix ouvert.

Preuves : [préparation](evidence/2026-10-15/remux/preparation.json),
[Chrome](evidence/2026-10-15/remux/chrome.json),
[Edge](evidence/2026-10-15/remux/msedge.json),
[Firefox](evidence/2026-10-15/remux/firefox.json),
[résumé calculé](evidence/2026-10-15/remux/summary.json).
Une [tentative intermédiaire Chrome](evidence/2026-10-15/remux/attempt-1-chrome.json)
a échoué avant l'essai MP4 : `readyState` prêt avant le traitement de l'événement
activant le menu. Le harnais attend désormais aussi le contrôle activé ; la
série finale ci-dessus a ensuite réussi.

[Méthode et comparaison détaillées](P0C_AUDIO_SWITCHING.md).
Le [plan initial 10 bits/5.1](NEXT_MEDIA_TESTS.md) a ensuite donné lieu au
[jalon représentatif distinct](P0C_REPRESENTATIVE_MEDIA.md). **P0C complet reste ouvert** : sélection
interne, sous-titres, autres profils, moteurs et écoute humaine. Ce résultat
valide le compromis du changement de source par remux préparé seulement.

Preuves conservées : [rapport navigateur](evidence/2026-10-15/browser.json),
[génération, SHA-256 et probes](evidence/2026-10-15/generation.json).
Les captures locales sont dans `artifacts/browser/` (ignoré par Git).
Nouveau scénario partagé et preuves par navigateur :
[Chrome](evidence/2026-10-15/native-audio/chrome.json),
[Edge](evidence/2026-10-15/native-audio/msedge.json),
[Firefox](evidence/2026-10-15/native-audio/firefox.json).
Leurs captures sont dans `artifacts/browser-{chrome,msedge,firefox}/`.
Les SHA-256 des quatre fichiers sont identiques avant/après chaque scénario
et correspondent à ceux du premier jalon ; aucune fixture n'a été régénérée.
Voir [le plan de tests](TEST_PLAN.md) pour les commandes effectivement exécutées.

Sources techniques consultées pour construire le banc :
[Axum](https://docs.rs/axum/0.8.9/axum/),
[ServeFile](https://docs.rs/tower-http/0.7.1/tower_http/services/struct.ServeFile.html),
[ffprobe](https://ffmpeg.org/ffprobe.html),
[muxeurs FFmpeg](https://ffmpeg.org/ffmpeg-formats.html).
[navigateurs Playwright](https://playwright.dev/docs/browsers),
[API audioTracks](https://developer.mozilla.org/en-US/docs/Web/API/HTMLMediaElement/audioTracks),
[protocoles distants Firefox](https://firefox-source-docs.mozilla.org/remote/).
Ces documentations expliquent les outils ; elles ne remplacent pas les mesures ci-dessus.

## P0C — AV1 Main 10 bits / Opus 5.1 / sous-titres, 15 octobre 2026

Source synthétique 12 s, 320×180/24 fps, AV1 Main `yuv420p10le`, SDR BT.709 ;
FR Opus 5.1 et EN stéréo 48 kHz. Sous-titres MKV SubRip : FR complet,
FR forced, EN complet. MP4 : conversion explicite SRT→mov_text, **copie des
paquets AV1/Opus et init codec**. Deux dérivés EN complètent le jeu. Huit médias
antérieurs inchangés. Décodage hors navigateur et 20 contrôles PCM de canaux /
60 marqueurs audio réussis.

Les textes et horodatages intégrés ont aussi été extraits et comparés :
**12 pistes et 48 cues corrects**, tolérance 20 ms. Cette vérification hors
navigateur ne change pas le résultat négatif d'exposition/rendu ci-dessous.

Windows x64 10.0.26300, profils headless par défaut. Quatre fichiers par client :
MKV/MP4 multiaudio FR par défaut et leurs dérivés EN. **12 parcours A/V réussis**,
48 observations de marqueurs, pause, seeks avant/arrière, reprise, fin et
erreurs 404/MediaError 4 vérifiés. Canaux mesurés avant et après seek.

| Client | MKV/MP4 10 bits | FR PCM / EN PCM | Écart d'observation A/V, ms* | Audio interne | Sous-titres intégrés natifs |
| --- | --- | --- | --- | --- | --- |
| Chrome 155.0.8059.40 | Décodés | 6 / 2 vérifiés | −2,40 à +20,00 | API absente | 0 piste exposée |
| Edge 155.0.4283.45 | Décodés | 6 / 2 vérifiés | −0,89 à +20,97 | API absente | 0 piste exposée |
| Firefox 155.0.1 | Décodés | 6 / 2 vérifiés | −54,00 à +6,50 | API absente | 0 piste exposée |

*`horloge média au seuil audio − PTS de la frame blanche`, 16 observations par
client incluant EN. Polling max observé 27,5 ms, RMS ~21,3 ms, cadence vidéo
~41,7 ms. Tolérance grossière fixée avant test ±200 ms ; temps mural entre
callbacks consigné aussi. **Pas de preuve de synchronisation physique**.
Erreur de seek échantillonnée maximale 17,21 ms (tolérance 150 ms).

FR : fréquences 440/550/660/80/770/880 Hz par voie ; EN : 990/1100 Hz.
La destination Web Audio annonce **2 canaux maximum** : aucune restitution
physique 5.1 revendiquée. Aucun affichage physique 10 bits, HDR, 4K ou NAS validé.

Trois sous-titres détectés par ffprobe, mais `textTracks.length=0` et
`audioTracks` absent dans les douze cas. Aucun rendu/sélection de sous-titres
intégrés ou auto-forced validé ; sélecteur désactivé et branche cues/sélection NT.
Les menus internes restent à inspecter humainement. EN est vérifié par son
fichier dérivé : la restauration FR/EN 8 bits précédente n'est pas attribuée
automatiquement à ces variantes.

Écart de muxage réel : le MP4 expose `default` sur vidéo et premier sous-titre
malgré les dispositions `0` demandées ; MKV les conserve. `forced` sur le
deuxième sous-titre et defaults audio restent corrects. Le rapport conserve
`strict_disposition_preservation=false`, séparément de la réussite A/V.

Preuves : [préparation](evidence/2026-10-15/representative/preparation.json),
[Chrome](evidence/2026-10-15/representative/chrome.json),
[Edge](evidence/2026-10-15/representative/msedge.json),
[Firefox](evidence/2026-10-15/representative/firefox.json),
[résumé et contrôle après seek](evidence/2026-10-15/representative/summary.json).
[Vérification des textes/horodatages](evidence/2026-10-15/representative/subtitle-verification.json).
[Méthode détaillée](P0C_REPRESENTATIVE_MEDIA.md).

La [comparaison MSE/Video.js/Shaka](PLAYER_COMPARISON_EXPERIMENT.md) est une
expérience séparée dont la première étape MSE est décrite ci-dessous, sans moteur choisi. **P0C reste ouvert**
pour sélection interne, sous-titres et preuves physiques.

## P0C — première expérience MSE réellement exécutée, 2026-10-15

La proposition précédente a maintenant une première étape exécutée : MSE
minimal, pistes fMP4 séparées, aucun manifeste DASH ni moteur tiers intégré.
AV1 Main10 + FR Opus5.1 / EN stéréo copiés sans réencodage, paquets/init
identiques. Audio PTS/DTS +7 ms, consigné. Originaux intacts.

| Windows headless | FR/EN sans recharger vidéo | Signal correct en lecture/après seek | WebVTT FR/forced/EN |
|---|---|---|---|
| Chrome 155.0.8059.40 | 6/6 changements | 431,7–450,1 ms | Trois cues et captures visibles |
| Edge 155.0.4283.45 | 6/6 changements | 424–447,8 ms | Trois cues et captures visibles |
| Firefox 155.0.1 | 6/6 changements | 377–1205 ms | Trois cues et captures visibles |

Pause conservée et signal confirmé après reprise ; seek arrière en EN vérifié.
Vidéo même URL/blob et même SourceBuffer. FFT ~171 ms, RMS ~21 ms, polling
max56 ms ; silences et gaps détaillés dans le protocole. Pas de promesse sans coupure.
Les latences du remux URL8bits précédent ne sont pas un benchmark comparable.
Video.js10 et Shaka : **NT**. CPU/RSS, mono/7.1, sortie physique5.1,
10bits physique, endurance, EOS et dérive A/V : NT. Le nombre de canaux reste
propre à chaque piste, sans imposer du5.1. Choix définitif ouvert.

[Protocole, échecs, comparaison et recommandation](P0C_MSE_EXPERIMENT.md).
[Préparation](evidence/2026-10-15/mse/preparation.json),
[Chrome](evidence/2026-10-15/mse/chrome.json),
[Edge](evidence/2026-10-15/mse/msedge.json),
[Firefox](evidence/2026-10-15/mse/firefox.json).

## P0C — Shaka DASH comparé au MSE, 2026-10-15

Shaka5.2.13 fixé et vérifié ; mêmes fMP4 AV1 Main10/Opus FR5.1 et ENstéréo,
MPD SegmentList sur vrais byte ranges, aucun réencodage ni modification des
originaux. Six parcours Windows headless, 108 changements, 36 marqueurs A/V,
FR/forced par défaut puis choix manuel conservé, WebVTT FR/forced/EN et off.

| Navigateur | MSE : médiane / P95=max | Shaka : médiane / P95=max |
|---|---:|---:|
| Chrome | 370,4 / 383,2 ms | 160,7 / 181,2 ms |
| Edge | 365,9 / 386,7 ms | 152 / 211 ms |
| Firefox | 1120 / 1151 ms | 167 / 207 ms |

Shaka plus rapide à sélectionner le signal, mais36/36 changements en lecture
avec waiting/seeking ; silences échantillonnés jusqu’à150ms et gaps vidéo
jusqu’à182,6ms. URL vidéo conservée ne signifie pas lecture ininterrompue.
Les36 marqueurs respectent±200ms, indicateur média non physique. CPU/RAM Rust
et arbre navigateur mesurés séparément, instrumentation coûteuse explicitée.
Choix provisoire : poursuivre Shaka et comparer safeMargin ; garder MSE comme
témoin. Video.js NT, aucun lecteur définitif ni ajoutP1.
[Protocole, chiffres, ressources et limites](P0C_SHAKA_EXPERIMENT.md).

## P0C — safeMargin terminé (2026-10-15)

Shaka5.2.13 conservé, aucun média/packaging réencodé ou modifié. MSE conservé.
424 essais :420 changements mesurés,4 échecs de préparation Firefox conservés,
complétés par4 essais indépendants. Marges0/.5/1/2/4, trois navigateurs Windows.
20 mesures de lecture et4 pause/4 après-seek par marge et navigateur.

| Navigateur | Marge0 latence P50/P95 | Marge0,5 latence P50/P95 |
|---|---:|---:|
| Chrome |163/186ms|409/437ms|
| Edge |166/197ms|402/422ms|
| Firefox |185/214ms|1123/1134ms|

Marge0,5 : zéro silence détecté et zéro frame perdue signalée en lecture et
après-seek. Mais les336 probes arrière de TOUTES les marges positives rejouent
l'ancienne langue ; les84 probes marge0 sont corrects. Aucun réglage ne résout
tout. Candidat fluidité :0,5s, NON promu ; défaut0 conservé. Marge4 retarde le
signal d'environ2,3s sans bénéfice démontré.420 marqueurs A/V dans±200ms,
indicateur média non physique ; polling maximal168ms limite les conclusions.

9 tests Rust, fmt/clippy, syntaxes et statistiques passent. Anciennes suites
navigateur :4/6 réussies ; ShakaChrome échoue sur progression vidéo aprèsseek,
MSEFirefox timeout PCM FR. Ces échecs sont conservés, causes non résolues.
Pas de lecteur définitif ni ajout Video.js/SvelteKit.
[Protocole, chiffres, échecs et prochaines vérifications](P0C_SAFE_MARGIN_EXPERIMENT.md).
Preuves : docs/evidence/2026-10-15/safe-margin/.

## Vérification courte de la resélection audio (2026-10-15)

Six essais ciblés FR→EN, safeMargin0,5, Chrome155.0.8059.40,
Edge155.0.4283.45 et Firefox155.0.1. Script scripts/verify-audio-seek.cjs.
Sans attente500ms après seek1,25s, le contrôle initial trouve déjà le signalEN
après retour0,5s sur les trois navigateurs : cela ne prouve aucune correction.
Avec la même attente500ms que la matrice précédente, le défaut est reproduit
sur les trois : pisteEN sélectionnée mais signalFR après seekarrière.
Resélectionner la pisteEN déjà active avec selectAudioTrack(track,0), puis
observer700ms, laisse le mauvais signalFR dans les trois cas. Cette méthode
simple ne valide donc pas une invalidation du buffer. Le résultat dépend du
point de changement dans les segments ; aucune correction générale livrée.
Source vidéo conservée et37 empreintes originales identiques avant/après
chaque essai. Syntaxe du nouveau script et deux suites statistiques passées.
Pas de modification Rust : suite Rust non relancée, pas de répétition de la
matrice420points pour cette vérification ciblée (token-saver appliqué).
Preuves docs/evidence/2026-10-15/audio-seek-quick/, contrôles et reproductions
séparés. Prochain essai : mécanisme supporté d'invalidation audio, puis preuve
FFT ; aucune méthode privée ou double commutation non validée ajoutée.

## Nettoyage forcé de la variante active — preuve ciblée (2026-10-15)

API publique Shaka5.2.13 testée : selectVariantTrack(activeVariant,true,0).
La source versionnée player.js/switchVariant_ confirme force=true pour la
variante déjà active si clearBuffer=true, contrairement à selectAudioTrack.
Le nettoyage concerne aussi la vidéo : ce n'est PAS une invalidation audio seule.

Six essais, FR→EN et EN→FR sur Chrome155.0.8059.40, Edge155.0.4283.45,
Firefox155.0.1 : défaut préalable reproduit6/6 après changement marge0,5 et
seekarrière ; nettoyage forcé puis bonne langue réellement décodée6/6.
FFT trois observations consécutives, six/deux voies, confirmation après700ms.
URL vidéo conservée, frames vidéo à nouveau présentées,37 originaux intacts
par essai. Aucun load(), réencodage, API privée ou double changement de piste.

| Navigateur | Retour au signal correct FR→EN | EN→FR |
|---|---:|---:|
| Chrome |354,9ms|778,2ms|
| Edge |395,7ms|368,3ms|
| Firefox |639ms|161ms|

Des événements waiting/seeking/buffering sont observés. Une frame perdue
signalée dans le cas FR→EN Edge et dans le cas FR→EN Firefox. Ne pas annoncer
une récupération sans interruption. Un essai par sens/navigateur : ni P95
ni stabilité à long terme, ni synchronisation physique validés ici.

Script scripts/verify-audio-seek.cjs : MEDIA_SEEK_RECOVERY=variant-clear,
MEDIA_SEEK_FROM=fr ou en, MEDIA_BROWSER_CHANNEL et MEDIA_REPORT_DIR neuf.
Sans MEDIA_SEEK_RECOVERY, l'essai de simple resélection reste disponible.
Preuves docs/evidence/2026-10-15/variant-clear-quick/.

Conclusion : mécanisme public de récupération démontré sur ces6cas, pas encore
intégré automatiquement au seek. Défaut0 du banc inchangé. Prochain petit jalon :
politique de récupération sur seek avec pause conservée et prévention des boucles,
puis répétitions/seek avant-arrière et mesure A/V. Ne pas choisir le lecteur final.
Seul le script expérimental a changé ; syntaxeJS vérifiée, suite Rust non
relancée car aucun code Rust/serveur modifié. Tests précédents non effacés.


## 2026-10-15 — P0C, politique expérimentale après seek

Cause partielle établie : safeMargin positif conserve des paquets audio anciens ;
resélection de la piste active ne les purge pas. MPD/IDs distincts, aucune preuve
justifiant un changement de packaging. Cause des blocages vidéo après purge
non identifiée. Purge publique selectVariantTrack(active,true,0) = contournement,
pas correction fiable ; purge audio ET vidéo. Marge normale0 conservée.

Contrôleur isolé audio-seek-policy.js, opt-in dans /shaka : tentative unique,
verrou/récursion/concurrence, position/pause/vitesse/choix manuel conservés,
append frais attendu, timeout et erreur terminale, aucune FFT de production.
Watchdog5s après reprise : blocage>2s en lecture -> erreur, pas seconde purge.
Vrai changement audio marge0 nettoyant déjà l'audio seul : pas purge vidéo inutile.

36 essais préliminaires Chrome/Edge : seuls8 seeks protégés mesurés,7 avec
langue finale correcte,3 marqueurs A/V réussis ; autres échecs préparation ou
synchronisation.12/12 contrôles mesurés reproduisent l'ancienne langue.
Ces résultats incomplets ne sont PAS un taux de fiabilité produit.
Deux démarrages Firefox WebDriver expirés45s : aucune validation Firefox actuelle.
Contrôle final préparation1,25s FR->EN seek0,5s : Chrome et Edge1/1 protégé
correct chacun, marqueurs A/V réussis ; contrôle Chrome déjà correct sans purge,
contrôle Edge incorrect. Signal307/1755ms ; silence238/1677ms ; frames perdues6/46.
Position conservée à8,7ms près. Fluidité insuffisante, option désactivée par défaut.

Oracle représentatif : FR six fréquences440/550/660/80/770/880Hz, EN990/1100Hz,
pas le couple mono440/880 ancien. Aucun média/packaging réencodé ou modifié ;
hash37 originaux avant/après chaque série complète. Tests antérieurs conservés.
Rust fmt/clippy strict/9 tests intégration passent. Tests Node politique/statistiques
passent ; CI ajoutée mais pas exécutée à distance. Précisions et preuves :
docs/P0C_AUDIO_SEEK_RECOVERY.md et docs/evidence/2026-10-15/audio-seek-policy/.

Prochain jalon P0 seulement : isoler blocages vidéo, refaire Firefox fonctionnel,
puis répétitions séquentielles et seeks avant/arrière dans une même présentation.
Ni solution définitive, ni P1, ni Video.js validés.

Validation finale Node : 10 tests politique + 2 suites statistiques =12/12, exit0. Syntaxe des cinq scripts modifiés vérifiée. Le dernier ajustement distinguant audioId/marge0 est couvert unitairement ; pas de nouvelle matrice navigateur après cet ajustement.
# Pistes externes — observation du 18 octobre 2026

Chrome 155.0.8059.40, Windows 10.0.26300, headless : fixture synthétique AV1
10 bits + Opus de 12 secondes, MP4 puis MKV, sidecars `.opus`/`.ogg` extraits
sans réencodage, SRT français forcé et WebVTT anglais. Après scan puis scan
avec probes vidéo réutilisés, le lecteur
affiche les cues SRT converties (capture inspectée), décode un signal PCM audio
externe non nul (pic environ 0,048) et suit la vidéo après pause/seek à 4 s
(écart observé inférieur à 300 ms). Désactivation des sous-titres, sauvegarde,
réouverture avec reprise et marquage vu/non vu passent avant et après redémarrage.

Preuves : `evidence/2026-10-18/external-tracks/`. La vidéo et les audios originaux
restent inchangés. Aucun remux de lecture, aucune preuve de synchronisation à
l'échantillon, de film long, de réseau instable ou de restitution physique 5.1.
ASS/SSA, sélection arbitraire de pistes internes et raw ne sont pas déclarés
lisibles. Les anciens résultats MSE/Shaka ci-dessous restent des expériences
séparées, pas des capacités automatiquement intégrées à ce moteur natif.


## 18 octobre 2026 — pistes internes préparées (premier parcours local)

Chrome 155.0.8059.40, Windows 10.0.26300, headless : sources synthétiques AV1
10 bits + Opus FR 5.1 / EN 2.0, MP4 puis MKV après vrai redémarrage Core.
Quatre changements FR/EN par conteneur ; signal décodé distingué par WebAudio
(~662 Hz FR dans le mélange 5.1, ~990 Hz EN). Plein écran et URL vidéo conservés
entre pistes préparées. ASS externe rendu comme texte sur deux lignes, SRT/VTT,
pause/seek et progression vérifiés. Capture ASS inspectée visuellement.
Preuves : `evidence/2026-10-18/player-finalization/`.

FFmpeg local facultatif, vidéo copiée et Opus extrait en Ogg ; aucune preuve
nouvelle de packaging distant, libass, sortie physique 5.1 ou autre navigateur.
Contrat et limites : [préparation des pistes](PLAYER_TRACK_PREPARATION.md).
Les essais longs sont consignés séparément dans `player-long/` ; ne pas confondre
une minute observée sur un fichier de 90 minutes avec 90 minutes continues.

Validation longue finale : `MEDIA_TEST_LONG=1` avec le test `browser_scanner`,
1/1 réussi en 249,71 s. Fichiers synthétiques de 90 minutes, seeks 1/2700/5380 s,
60 secondes observées par conteneur, écart d'horloges 67,23..72,84 ms. Requête audio
interrompue et reprise explicite, offsets, sous-titres, progression et vu/non vu.
Le test court final (40,41 s) vérifie aussi le rendu du sous-titre intégré forcé,
l'offset négatif et l'accès clavier. Régression séries (38,66 s) réussie : suivant
manuel/automatique opt-in et inter-saisons, permissions/isolation des comptes.
Résumé machine : `evidence/2026-10-18/player-verification.json`.

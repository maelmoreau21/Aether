# P0C — première expérience MSE, 15 octobre 2026

Périmètre exécuté : **MSE minimal seulement**, sous Windows x64 10.0.26300,
headless, profils par défaut. Video.js 10 et Shaka **non testés**. Ce jalon
ne clôt pas leur comparaison et ne choisit aucun lecteur définitif.
Video.js 10 est disponible : [annonce officielle 10.0.0](https://videojs.org/blog/videojs-10),
[changelog 10.1.0](https://videojs.org/changelog). Disponibilité ≠ compatibilité.

## Préparation reproductible

Depuis les fixtures représentatives déjà générées :

```powershell
python scripts/prepare-mse.py
cargo run --locked -p media-poc
```

Ouvrir `http://127.0.0.1:3000/mse`, cliquer **Préparer MSE**, puis lecture.
La préparation refuse un dossier existant ; elle ne remplace aucun original.
Son dossier fixe est `tests/fixtures/generated/mse/verified/`. Le dossier parent
contient aussi le premier essai de préparation échoué, conservé séparément.

Source : `representative.mkv`, 12 s, AV1 Main 10 bits SDR BT.709 320×180/24,
FR Opus 5.1 et EN Opus stéréo. FFmpeg 9.0.2 sépare les pistes avec `-c copy`,
`-copyts`, `-avoid_negative_ts disabled`,
`+frag_keyframe+empty_moov+default_base_moof`, fragments de 2 s.
Chaque fichier contient un init puis plusieurs fragments moof/mdat ; copies
init/media séparées aussi produites et vérifiées. **Aucun manifeste DASH**.
Le POC append les fichiers fragmentés complets de 12 s, et ne constitue pas
un ordonnanceur de segments adapté aux films longs.

- 288 paquets AV1 et 601 paquets par piste Opus : chaque payload et chaque
  codec init comparés, identiques ; décodage FFmpeg réussi.
- Vidéo : timestamps inchangés, timebase changée par le muxeur. Audio :
  PTS/DTS **+7 ms** constants, durées quantifiées jusqu'à 1 ms ; Opus init
  inchangé. Ce décalage est mesuré, sans compensation dans le POC.
- MIME issu de av1C : `video/mp4; codecs="av01.0.00M.10"` ; audio
  `audio/mp4; codecs="opus"`. Initialisations et détails dans le rapport.
- Trois extractions/conversions SubRip → WebVTT, FR complet, FR forcés, EN.
  Douze cues : textes/instants identiques vérifiés. C'est une conversion de
  texte, pas une copie binaire ni une preuve de sélection de sous-titres intégrés.
- Préparation unique : 1,88 s sur cette machine, pas un benchmark NAS.
  Empreintes des originaux identiques avant/après préparation et navigateurs.

## Méthode et résultats

Une MediaSource, un SourceBuffer vidéo, un SourceBuffer audio. Une langue
change uniquement le buffer audio : remove de toute sa plage, append du
fragmenté choisi. Le buffer vidéo et l'URL blob restent identiques ; aucune
affectation `video.src`, aucun `load()`, aucun repositionnement forcé pendant
la commutation. Remplacer aussi le passé audio évite de retrouver l'ancienne
langue lors d'un seek arrière. Ce choix simple recopie 12 s à chaque changement.

Six changements par navigateur : EN/FR en lecture, EN/FR en pause, EN/FR
après seek à 7 s. La pause est conservée 100 ms puis le test reprend pour
mesurer le signal ; elle n'est pas déclarée audible pendant la pause. Seek
arrière à 3,25 s en EN puis avant à 7 s : signal EN conservé. Seek à 4,25 s
pour sélectionner successivement les trois sous-titres.

La FFT 8192 à 48 kHz (~171 ms) et le RMS 1024 (~21 ms), polling cible 10 ms,
observent les six fréquences FR ou les deux EN et quatre voies silencieuses.
Trois échantillons corrects consécutifs exigés après changement. Ces fenêtres
et les files du décodeur font partie du délai observé : ce n'est pas une
mesure du premier échantillon physique. Polling maximal final : 56 ms (arrondi supérieur).

| Navigateur | Changements | Demande → signal correct, lecture/après seek | Pause, puis reprise incluse | Silence RMS échantillonné en lecture | Gap callback vidéo en lecture |
|---|---:|---:|---:|---:|---:|
| Chrome 155.0.8059.40 | 6/6 | 431,7–450,1 ms | 544,8–549,5 ms | 0–30,6 ms | 50,3–66,8 ms |
| Edge 155.0.4283.45 | 6/6 | 424–447,8 ms | 545,4–563,6 ms | 0–20,8 ms | 50,3–66,7 ms |
| Firefox 155.0.1 | 6/6 | 377–1205 ms | 1305–1327 ms | 0–51 ms | 46,1–53,4 ms |

Gaps vidéo de référence avant changement : respectivement 75 / 66,6 /
45,3 ms, sur seulement 300 ms. Les callbacks incluent le scheduling headless ;
ils ne prouvent pas une absence de gel sur un écran. Les silences de pause
volontaire sont exclus de la colonne silence. Un silence plus court que le
polling peut être manqué ; **aucune promesse de commutation sans interruption**.
Firefox conserve notamment plus longtemps l'ancien signal. Un seul parcours
par navigateur, pas de percentile ni de comparaison statistique.

Dans les trois navigateurs : frames présentées, pixels non uniformes, six/deux
signaux PCM vérifiés, état pause conservé, seeks, URL vidéo et buffer conservés,
trois TextTracks externes avec cue attendu à 4,25 s. Neuf captures montrent
effectivement les textes FR COMPLET / FR FORCE / EN FULL ; rendu headless
observé, affichage physique et écoute humaine NT. Forced est ici un libellé et
une donnée de préparation, **pas une sélection automatique validée**.

Erreurs vérifiées : HTTP 404 sur fichier inconnu et rejet d'une langue invalide.
Huit tests Rust passent : inventaire fixe MSE, MIME, exactitude des octets Range,
416, refus des fichiers non autorisés et Origin hostile, plus tests P0 existants.
Les segments tronqués en MSE, pertes réseau, EOS/fin, dérive A/V après plusieurs
changements et l'endurance restent NT dans ce jalon.

## Échecs conservés

1. Préparation initiale : comparaison textuelle erronée entre heures SRT et
   WebVTT ; corrigée en secondes numériques, seconde génération séparée réussie.
2. Premier Chrome : duration=12 refusée car des paquets audio finissent après
   12 s ; corrigée en utilisant le maximum des fins réellement bufferisées.
3. Firefox sans clic utilisateur : activation AudioContext bloquée, timeout.
   Harness corrigé avec vrai clic WebDriver sur Préparer MSE, sans modifier les
   préférences autoplay ; attentes désormais bornées et progression conservée.

## Comparaison et recommandation provisoire

| Méthode | Preuve locale | Limite déterminante |
|---|---|---|
| HTML5 natif MKV/MP4 représentatifs | A/V dans les 3 navigateurs | AudioTracks absent, sous-titres intégrés non exposés |
| Remux par langue + remplacement URL | 48 changements 8 bits validés, 139–288 ms environ jusqu'au signal stable | Recharge vidéo, silences/frames précoces ; fixture et protocole différents de cet essai 10 bits |
| MSE minimal + pistes séparées | 18 changements 10 bits, URL/buffer vidéo conservés, WebVTT visible | Délai jusqu'à 1,205 s en lecture, buffer complet de 12 s, scheduler à construire |
| Video.js 10 | NT | Tech/adaptateur réel, versions et même packaging à tester séparément |
| Shaka Player | NT | Manifeste DASH valide, versions et sélection audio réelle à tester séparément |

**Direction recommandée pour la prochaine expérience : vidéo commune MSE,
audio séparé et WebVTT externe, puis essai Shaka avec un manifeste réel**.
Le POC prouve cette séparation et évite le rechargement vidéo, mais ne justifie
pas d'écrire un moteur complet maison ni d'écarter Video.js. Le choix définitif
attend les deux intégrations et les mêmes séquences/mesures. Garder la méthode
de remplacement d'URL comme témoin, sans comparer directement ses latences
8 bits aux nouvelles mesures 10 bits.

CPU/RSS navigateur : NT, arbre de processus non instrumenté. Aucune conclusion
sur le NAS, un film long, une sortie physique 5.1/7.1 ou un affichage 10 bits.
Le nombre de canaux appartient à chaque piste : mono, stéréo, 5.1, 7.1 sont
des cas distincts, sans conversion imposée. **Mono et 7.1 restent à tester** ;
ils ne sont pas déclarés validés par ce passage 5.1 ↔ stéréo.

## Rejouer les navigateurs

Avec Playwright disponible pour Node et geckodriver installé localement :

```powershell
$env:MEDIA_BROWSER_CHANNEL='chrome'
$env:MEDIA_REPORT_DIR='artifacts/mse-chrome'
node scripts/browser-mse.cjs
$env:MEDIA_BROWSER_CHANNEL='msedge'
$env:MEDIA_BROWSER_EXECUTABLE='C:\Program Files (x86)\Microsoft\EdgeCore\155.0.4283.45\msedge.exe'
$env:MEDIA_REPORT_DIR='artifacts/mse-msedge'
node scripts/browser-mse.cjs
$env:MEDIA_BROWSER_CHANNEL='firefox'
$env:MEDIA_BROWSER_EXECUTABLE='C:\Program Files\Mozilla Firefox\firefox.exe'
$env:MEDIA_REPORT_DIR='artifacts/mse-firefox'
node scripts/browser-mse.cjs
Remove-Item Env:MEDIA_BROWSER_EXECUTABLE
```

Adapter les chemins aux installations présentes ; aucune installation navigateur
automatique. Pour Chrome, supprimer MEDIA_BROWSER_EXECUTABLE hérité avant test.
Versions réellement utilisées : Playwright 1.62.1 et geckodriver 0.37.1.
Voir [rapports et captures](evidence/2026-10-15/mse/).
Pour l'écoute physique : suivre [la procédure matérielle](MANUAL_BROWSER_CHECKS.md),
puis répéter EN/FR en lecture/pause/après seek sur `/mse` ; résultats humains NT.

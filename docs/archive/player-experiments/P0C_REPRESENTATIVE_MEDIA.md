# P0C — AV1 10 bits, Opus 5.1 et sous-titres

Jalon exécuté le 15 octobre 2026. Quatre fixtures synthétiques de 12 secondes,
320×180/24 fps, SDR BT.709 limité : représentatives des formats testés, pas du
débit, de la résolution ou de la durée d'un film réel. Aucun média antérieur modifié.

## Préparation et preuves

`python scripts/generate-representative-fixtures.py` : Python standard,
FFmpeg/ffprobe 9.0.2. Refus d'un dossier de sortie existant. La mire est
convertie en pixels 10 bits et encodée une seule fois en AV1 Main ; aucune
vidéo existante n'est transcodée. Les variantes suivantes copient AV1/Opus.

- `representative.mkv` : AV1 `yuv420p10le`, FR Opus 5.1, EN Opus stéréo,
  SubRip FR complet, FR forced et EN complet.
- `representative.mp4` : paquets vidéo/audio copiés, textes convertis SRT →
  `mov_text`, faststart. La conversion de texte n'est pas une copie bit à bit.
- Deux dérivés `representative-en.{mkv,mp4}` : copie vidéo, EN et sous-titres.
  Leur chargement distinct ne prouve pas une sélection interne ou la restauration
  automatique de position sur ces nouvelles variantes.

288 paquets AV1 et 601 paquets par audio sont comparés individuellement par
SHA-256 ; init codec identique dans les sept comparaisons de copie. PTS et
timebases sont conservés dans le rapport sans prétendre leur représentation
identique entre conteneurs. Décodage intégral A/V ; **20 contrôles PCM de canaux
et 60 marqueurs audio réussis**, résolution temporelle 10 ms hors navigateur.
FR : FL 440, FR 550, FC 660, LFE 80, BL 770, BR 880 Hz ; EN : 990/1100 Hz.
Layout `5.1`/`stereo`, 48 kHz, init et padding Opus dans les probes.

Extraction/décodage texte hors navigateur également vérifié par
`python scripts/representative_subtitles.py` : 12 pistes, 48 cues avec texte
exact et horodatages à ±20 ms. Ce contrôle est intégré aux nouvelles
générations ; le jeu actuel a été vérifié après création, dans un rapport
distinct `subtitle-verification.json`. Cela ne prouve pas le rendu navigateur.

Flashs et hausses d'amplitude aux temps 2–2,5, 6–6,5 et 9–9,5 s ; cues
texte distincts à ces instants et à 4–5 s. Le MP4 expose `default` sur vidéo
et premier sous-titre malgré les dispositions `0` demandées, alors que MKV
les conserve. `forced` du deuxième sous-titre et defaults audio corrects.
`strict_disposition_preservation=false` maintient cet écart visible.

Préparation complète unique : 21,09 s, 2 266 372 octets de médias ajoutés.
Aucun benchmark répété ni résultat NAS ; FFmpeg uniquement hors lecture.

## Mesures navigateur et portée

Scénario commun `MEDIA_TEST_SCENARIO=representative`, Chrome/Edge Playwright
1.62.1, Firefox installé via geckodriver 0.37.1, Windows x64 10.0.26300,
headless par défaut, HTTP loopback, navigateurs séquentiels. Pixels décodés,
PCM séparé par `ChannelSplitterNode(6)`, fréquences avant/après seek, pause,
seek avant/arrière, reprise, fin et erreur HTTP 404/MediaError 4 observés.
Les quatre sorties excédentaires restent silencieuses en EN stéréo.

Quatre marqueurs par fichier : initial à 2 s, seek 5,7 → marqueur 6, retour
1,7 → marqueur 2, seek 8,7 → marqueur 9. Vidéo : pixels blancs et PTS via
`requestVideoFrameCallback`. Audio : RMS des 1024 derniers échantillons,
seuil trois fois le niveau initial, polling visé 10 ms. Mesure signée :
`currentTime au seuil audio − PTS de la frame blanche`. Temps mural entre
callbacks également consigné. Tolérance grossière préétablie ±200 ms.

**12 parcours A/V, 48 marqueurs et 48 seeks vérifiés**, canaux PCM conformes
avant/après seek. Polling maximum observé 27,5 ms, RMS ~21,3 ms, cadence vidéo
~41,7 ms. Les délais Web Audio annoncés sont consignés, sans corriger ces
mesures en une preuve physique. Chiffres par navigateur dans la matrice.

## Limites

- Six PCM FR distincts sont observés avant le downmix, mais la destination
  annonce seulement `channelCount=2`, `maxChannelCount=2`. **Restitution physique
  5.1 : NT**, sans écoute ni équipement identifié. Six canaux décodés ne prouvent
  pas que six haut-parleurs reçoivent les bonnes voies.
- La source 10 bits est décodée ; canvas/captures ordinaires ne prouvent pas
  un **affichage physique 10 bits**. HDR, 4K et décodeur matériel/logiciel : NT.
- `audioTracks` absent sur les trois clients ; EN vérifié par fichier dérivé,
  aucune commutation interne validée.
- Trois sous-titres dans chaque fichier mais **zéro `textTracks` native exposée**
  dans les douze cas. Sélecteur désactivé, sélection/cues/rendu non validés,
  y compris forced. Aucun VTT externe ajouté pour masquer cette limite ;
  menus internes et rendu physique encore à inspecter humainement.
- Mesure A/V grossière dans le navigateur, pas une certification lip-sync ni
  une mesure de dérive sur film long, de sortie physique ou de réseau distant.

Preuves : `docs/evidence/2026-10-15/representative/` ; commandes dans
`TEST_PLAN.md`. Comparaison distincte proposée sans moteur choisi :
`PLAYER_COMPARISON_EXPERIMENT.md`.

# P0C — expérience de remux par langue

Jalon limité : quatre fichiers dérivés, préparés hors lecture à partir des
fixtures MKV et MP4 existantes, avec `-map 0:v:0 -map 0:a:0` (FR) ou
`-map 0:v:0 -map 0:a:1` (EN), `-c copy` ; MP4 avec `+faststart`.
Pas d'encodage, pas de FFmpeg pendant la lecture, pas de format raw/MSE.
Les 192 paquets AV1 et les 401 paquets Opus de chaque sortie sont comparés
individuellement par SHA-256 aux flux sources correspondants (EN index 2
devient index 1). Toutes les sorties sont aussi décodées intégralement.

## Parcours et critères automatisés

Le menu **Langue par remux préparé** remplace l'URL par un asset connu, attend
ses métadonnées, restaure `currentTime`, attend le seek puis reprend si la
lecture était active. Une lecture en pause reste en pause. Le menu natif
`audioTracks` reste séparé : il s'agit bien de deux expériences différentes.
Les commandes sont désactivées pendant le changement ; un échec affiche un
motif explicite, sans promettre que l'ancienne source a été restaurée.

Le scénario `MEDIA_TEST_SCENARIO=remux` exerce le menu réel pour MKV et MP4 :

- six changements pendant la lecture, EN → FR répété trois fois ;
- mesure de la fréquence dominante attendue 880/440 Hz, PCM non nul et trois
  observations correctes consécutives, pas seulement un changement de libellé ;
- erreur de restauration de position ≤150 ms, vidéo reprenant après le seek ;
- deux changements supplémentaires à 4,25 s en pause, puis reprise FR et fin ;
- diversité des pixels RGB, erreurs source absente, SHA-256 des originaux
  avant/après. Une sélection ou une mesure requise manquante échoue le scénario.

## Ce que mesurent les chronomètres

- `ui.completedMs` : demande → métadonnées/seek/préservation de l'état/reprise
  de `play()`. Ce n'est pas la preuve que le son ou la première image est revenu.
- `requestToStableAudioMs` : demande → trois observations PCM avec la bonne
  fréquence. FFT 8192 échantillons à 48 kHz, soit une fenêtre glissante d'environ
  171 ms. Cette détection inclut le délai du détecteur ; elle n'est pas une
  mesure directe de l'interruption physique aux haut-parleurs.
- `sampledSilenceTotalMs` : somme des intervalles échantillonnés avec RMS <0,001,
  analyse PCM de 1024 échantillons (~21 ms), polling visé 10 ms. Le pas maximal
  réellement observé et la trace sont conservés. **Zéro silence observé ne
  prouve pas zéro coupure**, notamment sous la résolution temporelle.
- `requestToRestoredVideoFrameMs` : demande → première callback de frame après
  la fin du seek de la nouvelle source et à une position compatible avec celle
  demandée. Ce filtre évite d'attribuer une ancienne frame à la reprise.
- `crossSourceFrameGapMs` : écart entre la dernière callback avant demande et
  cette frame restaurée ; inclut le rythme normal des images. Le maximum des
  écarts de la fenêtre de référence précédente est enregistré séparément.
  Les callbacks indiquent une soumission au compositeur, pas un écran physique ;
  leur ordonnancement sur le thread principal ajoute de l'incertitude.

Les essais navigateur sont séquentiels pour éviter leur contention mutuelle.
Ils portent sur des fichiers de huit secondes, en HTTP loopback, headless,
sans limitation réseau. Ils ne mesurent ni NAS, ni film long, ni synchronisation
fine par marqueurs, ni charge. Statistiques descriptives d'un passage local,
pas un engagement de latence ni un percentile fiable en production.

## Comparaison à conserver ouverte

| Méthode | Ce qui change | Coûts et limites à vérifier |
| --- | --- | --- |
| Remux par langue puis changement d'URL | Fichier, démuxeur et source média rechargés | Préparation et stockage redondant de vidéo ; seek/rebuffer/interruptions ; position restaurée explicitement |
| Sélection interne dans un même fichier | Piste sélectionnée par API/moteur | Pas d'API `audioTracks` exposée par défaut dans les trois clients testés précédemment ; aucune mesure comparable de commutation interne disponible |
| Packaging et sélection MSE | Segments/SourceBuffers et sélection du moteur | Expérience distincte, append et sélection non testés ici ; pas déduits d'un `isTypeSupported=true` |

Le remux par langue est une **preuve de compromis**, pas le choix définitif du
produit. Il faudra comparer les mêmes fichiers, positions et mesures avec un
moteur capable de vraie sélection interne/segments. Aucune promesse de
commutation sans interruption n'est faite.

Les résultats numériques observés et les rapports sont référencés dans
`STREAMING_COMPATIBILITY.md`. L'écoute humaine reste à effectuer selon
`MANUAL_BROWSER_CHECKS.md`. Le jalon 10 bits/5.1/sous-titres a ensuite été
exécuté séparément : `P0C_REPRESENTATIVE_MEDIA.md`. Ne pas attribuer à ces
nouveaux dérivés les mesures de changement de source 8 bits de ce document.

Références consultées : [copie des flux FFmpeg](https://ffmpeg.org/ffmpeg.html#Streamcopy),
[chargement de source HTML](https://developer.mozilla.org/en-US/docs/Web/API/HTMLMediaElement/load),
[callbacks vidéo](https://developer.mozilla.org/en-US/docs/Web/API/HTMLVideoElement/requestVideoFrameCallback).

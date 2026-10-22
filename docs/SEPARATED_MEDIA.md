# Médias séparés : état vérifié au 22 octobre 2026

Ce lot étend les parcours existants, sans remplacement du moteur Rust/Svelte.
Le support complet des flux bruts **n'est pas livré**. L'incident Avatar/Synology
reste ouvert faute de fichier et d'accès au NAS.

## Parcours opérationnel

Le scanner découvre MP4, MKV et désormais WebM. Pour AV1 avec Opus internes,
le WebM conserve aussi sa vidéo originale et utilise les fenêtres audio MSE
existantes, au lieu de préparer une copie vidéo complète. Un MP4/WebM sans audio peut
utiliser plusieurs fichiers Ogg Opus indépendants, servis directement avec
HEAD/GET/Range via les autorisations existantes du Core et du Node. Ces fichiers
ne sont pas extraits ni réencodés pendant la lecture. Le navigateur garde le
même élément vidéo lorsque les pistes externes changent. Une vidéo détectée
sans audio n'apparaît plus comme une piste audio fictive.

L'association automatique existante utilise un même radical et des suffixes
explicites : `film.mp4`, `film.fr.opus`, `film.en.opus`, `film.fr.forced.srt`.
Les radicaux ambigus restent refusés. SRT, VTT et ASS/SSA texte suivent le
convertisseur existant ; le rendu complet des styles ASS et l'OCR ne sont pas
ajoutés. VF et sous-titres français forcés utilisent les préférences existantes.

## Association explicite facultative, version 1

Pour `film.webm`, créer `film.webm.media.json` dans le même répertoire :

```json
{
  "version": 1,
  "audio": [
    {"file": "voix-fr.opus", "language": "fra", "offset_seconds": 0},
    {"file": "voix-en.ogg", "language": "eng", "offset_seconds": 0.25}
  ],
  "subtitles": [
    {"file": "texte.srt", "language": "fr", "forced": true, "sdh": false}
  ]
}
```

Le manifeste est autoritaire pour cette vidéo : seules ses pistes sont
associées, sans repli silencieux sur les noms si le document est invalide.
Une liste omise équivaut à une liste vide. Les fichiers doivent être présents
dans le même répertoire, sans chemin, URL, lien ni traversée. Le manifeste ne
permet pas de contourner ffprobe, les limites de taille ou les autorisations.
64 Kio maximum ; 8 audios, 64 sous-titres ; champs inconnus, doublons et versions
inconnues refusés. `fra/fre` et `eng` sont normalisés en `fr` et `en`.
Décalage audio entre -30 et +30 secondes, positif lorsque l'audio commence plus
tard sur la timeline vidéo ; il ne remplace pas les timestamps du fichier.
Les modifications sont prises en compte au prochain scan, même si la vidéo
n'a pas changé. Aucun fichier original n'est modifié.

Ce contrat décrit des associations, **pas** un format de flux ou un index raw.
Aucune migration SQL ni nouvelle route de lecture n'est nécessaire : le Node
publie les mêmes pistes validées dans l'inventaire. Une piste absente/invalide
produit un avertissement de scan ; elle n'est pas annoncée comme lisible.

## Flux bruts : limites et prochain parcours à implémenter

Les `.obu` sont maintenant signalés par une erreur de scan explicite, au lieu
d'être ignorés. Ils ne sont pas exposés comme sources lisibles. Aucun 25 fps de
repli n'est accepté comme preuve de cadence. Les temps d'affichage AV1 ne se
déduisent pas systématiquement du nombre d'OBU : la spécification distingue les
informations de timing et les intervalles d'affichage constants ou non définis.
[Spécification AV1, timing](https://aomediacodec.github.io/av1-spec/).

Le prochain parcours raw doit valider un index indépendant lié à l'identité du
fichier : timebase rationnelle, PTS/DTS/durées par unité temporelle, plages
d'octets, points d'accès et initialisation codec. CFR peut utiliser une cadence
exacte attestée à la création de l'index ; VFR doit conserver les timestamps
réels. Un index ne peut pas réinventer les timestamps perdus. Un export depuis
un conteneur peut conserver ces informations avant extraction. Il faut ensuite
prouver un packaging par fenêtres à partir des points d'accès, sans rescanner
ou copier tout le film, et comparer paquets, timestamps et décodage après seek.
Ce packager et son lecteur MSE vidéo restent à développer et tester.

`.opus` ne signifie pas paquet Opus brut : le parcours actuel vérifie le
conteneur Ogg. Pour des paquets vraiment bruts, il faut des limites de paquets,
l'initialisation, la correspondance des canaux, le pré-skip, le gain, la fin
utile et le pré-roll de seek, avec timestamps à 48 kHz. Des paquets concaténés
sans ces données restent insuffisants. Aucun démultiplexeur raw Opus n'est
ajouté. [RFC 7845](https://www.rfc-editor.org/rfc/rfc7845.html),
[RFC 6716](https://www.rfc-editor.org/rfc/rfc6716.html).

## Reproduction des tests

```powershell
python scripts/generate-dual-opus-fixture.py
python scripts/generate-separated-fixtures.py
cargo test -p media-node --lib sidecar_manifest -j 1 -- --test-threads=1
$env:MEDIA_TEST_FIXTURE='tests/fixtures/generated/separated-media/dual-8.mp4'
$env:MEDIA_TEST_VIDEO_CONTAINER='webm'
$env:MEDIA_REPORT_DIR='target/separated-browser'
node scripts/browser-dual-opus.cjs
```

La génération encode uniquement des mires/tons de test légaux, hors lecture.
La vidéo existante et les pistes préparées sont ensuite copiées par paquets.
Le harnais vérifie les empreintes de tous les paquets vidéo/audio remuxés,
les images décodées et le signal PCM FR/EN distinct, FR/EN/FR répété, un seul
chargement vidéo avant réouverture, plein écran, pause/seeks, refus HTTP 404
avec ancien audio récupérable et réouverture. Les rapports se trouvent dans
`evidence/2026-10-22/separated-media/`. Le graphe WebAudio mesure les tests
uniquement ; la production ne fait aucun downmix volontaire.

Les tests locaux ne prouvent pas une sortie physique 5.1/7.1, un matériel NAS,
une longue séance, Firefox ou un Node réellement distant. Les anciennes preuves
Docker/Nginx/long MP4 et sécurité multi-Node restent distinctes ; elles ne sont
pas réexécutées implicitement par ce lot. H.264/HEVC restent soumis au décodage
réel du navigateur ; aucune nouvelle validation navigateur de ces codecs ici.
Le P0 Avatar et le P2 brut ne doivent pas être déclarés terminés.

Observations ponctuelles de ce lot (Windows, headless, cache de fichiers hôte non vidé) :

| Navigateur | Vidéo seule | Audio | Première image | Lecture avec audio prête |
|---|---|---|---:|---:|
| Chrome 155 | MP4 | 2 canaux | 46 ms | 183 ms |
| Chrome 155 | MP4 | 6 canaux | 47 ms | 201 ms |
| Edge 155 | WebM | 6 canaux | 125 ms | 222 ms |
| Edge 155 | WebM | 8 canaux | 48 ms | 122 ms |

Ces durées ne sont pas des garanties NAS. Le signal audio est mesuré ensuite par analyse PCM ; `playReadyMs` mesure la fin de chargement/démarrage des éléments, pas une latence de sortie matérielle.

Le test Core `webm_uses_original_video_and_authorizes_explicit_external_audio`
vérifie ce descripteur WebM, les langues/décalages du manifeste, l’absence
de chemins privés exposés et les statuts 401/200/206/416 sur audio externe et
fenêtre interne. Le rapport `chrome-webm-segmented/report.json` prouve aussi
images/PCM, changements, frontière de fenêtres et annulation sur WebM interne.
Le scénario Séries réel est conservé dans `series/report.json` : bouton manuel
supprimé, enchaînement automatique volontaire conservé, transitions et
permissions testées.

# Modèle média — P0 et suite

## Extraction MP4/Opus en Rust et repli vidéo progressif — 24 octobre 2026

`crates/node/src/mp4_audio.rs` extrait les paquets Opus d'un sous-ensemble
strict de MP4 non fragmentés, sans FFmpeg ni fichier temporaire. Le scan valide
les tables avant d'annoncer `native_opus_tracks_v1`. L'URL
`prepared/opus-{trackId}-chunk-{n}` utilise le vrai `track_ID` du conteneur,
distinct de l'index ordinal ffprobe des routes historiques `audio-{index}`.
Ce support ne constitue pas un décodeur universel MP4/MKV ou une preuve de
lecture pour tous les navigateurs.

Le chemin accepte une description `Opus`/`dOps` à 48 kHz, avec configuration
mono/stéréo ou famille de mapping 1 jusqu'à huit canaux, une référence locale
`dref` et une seule édition `elst` de vitesse 1, sans décalage de composition
non nul. Les tables `stts`, `stsc`, `stsz` et `stco`/`co64` déterminent les
durées, tailles et positions réelles. Les références externes, éditions
complexes, sources fragmentées et autres variantes non prises en charge
échouent explicitement ; elles ne sont pas interprétées approximativement.

Chaque fenêtre couvre dix secondes nominales, avec jusqu'à une seconde avant
et une seconde après. Les paquets et canaux restent inchangés. Le `tfdt`
conserve le temps de décodage absolu issu de `stts` ; l'édition `elst` et
`dOps` sont conservés, sans soustraire deux fois le priming. Les groupes
`roll` décrivent le pré-roll de 80 ms en tenant compte des durées variables
des paquets, selon [l'encapsulation Opus en ISO BMFF](https://opus-codec.org/docs/opus_in_isobmff.html).
L'initialisation contient des tables d'échantillons vides et `mvex` ; les
fragments contiennent `moof`/`traf`/`tfdt`/`trun` et `mdat`, suivant le
[format ISO BMFF pour MSE](https://www.w3.org/TR/mse-byte-stream-format-isobmff/).
Les références `sbgp` locales au fragment commencent à `0x10001`, distinctes
des descriptions globales de l'initialisation ; ce choix est aussi explicite
dans [Shaka Packager](https://github.com/shaka-project/shaka-packager/blob/main/packager/media/formats/mp4/fragmenter.cc).
L'initialisation conserve une description `roll -4` mais ne mappe aucun
échantillon ; les fragments portent les associations réellement calculées.

Bornes du parseur : `moov` de 16 Mio au maximum, cache d'index de 32 Mio et
huit entrées, quatre millions d'échantillons et 65 536 runs `stts`/`stsc`.
Les tables restent compactes, avec préfixes clairsemés ; seul le plan de la
fenêtre est matérialisé, limité à 8 192 échantillons et 8 Mio de paquets.
La représentation HTTP est virtuelle : en-têtes fMP4 puis lectures du fichier
original par blocs de 64 Kio, avec regroupement des lectures contiguës ou
proches. Aucun tampon de huit Mio n'est réservé pour recopier la fenêtre.
`HEAD` et les Range simples, y compris `206` et `416`, portent sur cette
représentation. Ces bornes ne sont pas des mesures de RSS du processus.

Le repli vidéo MSE utilise un flux fMP4 continu unique copié par FFmpeg depuis
un descripteur confiné, avec backpressure et quatre processus simultanés au
maximum. Un seek remplace et annule ce flux ; fermeture et annulation arrêtent
le processus. Cette route n'accepte pas Range ; `HEAD` ne lance pas FFmpeg.
Les fenêtres vidéo indépendantes chevauchant des GOP ont été rejetées après
un blocage réel de décodage MSE. Les autres préparations historiques restent
distinctes ; aucun de ces chemins n'ajoute de réencodage vidéo ou audio.
Méthode et limites des mesures : [banc système](STREAMING_SYSTEM_BENCHMARK.md).

## Extension P3 du 18 octobre 2026

Les sources vidéo conservent leur identité existante. Un scan peut leur associer
des instantanés de sous-titres SRT/WebVTT et des audios Ogg/Opus indépendants,
avec détection stricte des ambiguïtés. Les sidecars sont séparés des Tracks
internes du probe et restent privés côté inventaire. Le descripteur public
ne transforme pas tous les flux détectés en choix de lecture natifs.
Les migrations additives 0014 existent dans les deux moteurs SQL ; la 0015
ajoute la progression par utilisateur et Work. Pas de transfert de base ni de
réidentification des œuvres existantes. Contrat, limites, raw et validations :
[P3/P4](P3_P4_FEATURES.md).

NFO Films : métadonnées éditoriales séparées du probe, candidates par asset et
sélection/provenance au niveau Work, sans réidentification ni fusion par ID NFO.
Migration0005 additive ; [LOCAL_FILM_NFO.md](LOCAL_FILM_NFO.md).

## Inventaire Films persistant P1

Migration0004 implémente Work/Edition/MediaVersion/MediaSource/Asset/Track, sans
fusion au titre. Une version/source par fichier, édition locale neutre ; piste
identifiée par UUID et index réel de flux, métadonnées ffprobe conservées en JSON.
Le descripteur normalise fra/fre→fr et eng→en ; les tags techniques restent bruts.
L'index complet paquets/cues et la normalisation générale BCP47 restent différés.
Voir [LOCAL_FILM_SCANNER.md](LOCAL_FILM_SCANNER.md). Les paragraphes « modèle futur »
ci-dessous décrivent l'état antérieur et les éléments encore hors de ce jalon.

## Association bibliothèque et Node — 16 octobre 2026

Contrat cible : Library possède un Node unique et une racine explicite ; chaque
MediaSource appartient à cette bibliothèque et au même Node. Le Node analyse
les conteneurs et pistes, puis fournit l'inventaire au Core. Aucun modèle
persistant ajouté ici : administration et permissions précèdent le scanner.
Voir [LIBRARIES_AND_SCANNER.md](LIBRARIES_AND_SCANNER.md).


Jalon représentatif ajouté : AV1 Main `yuv420p10le` SDR BT.709, FR Opus 5.1
et EN stéréo, trois sous-titres intégrés. Les probes conservent layout,
initial_padding et init codec ; le PCM canal par canal est vérifié séparément.
MKV SubRip puis conversion texte explicite en MP4 mov_text, AV1/Opus copiés.
Un flux décodé six canaux ne prouve pas sa restitution physique. Zéro piste
texte native exposée sur les clients testés : ne pas confondre piste de fichier,
TextTrack/cue et rendu. Voir `P0C_REPRESENTATIVE_MEDIA.md`.

Le banc conserve un tableau de pistes, jamais un champ vidéo/audio unique.
Chaque piste garde son `index` de flux, type, codec, timebase, tags et tous les
autres champs disponibles de ffprobe : profil, résolution, canaux, dispositions,
durée, start PTS, initial padding et données d'initialisation codec notamment.
Les champs absents ne sont pas inventés. Le nom de conteneur provient du probe,
pas d'une déduction des codecs à partir de l'extension.

Les identifiants `fixture-*` sont des identifiants stables du banc uniquement.
Une piste est repérée par `(asset de fixture, stream index)` ; ce ne sont pas les
identifiants persistants définitifs du produit. Le prototype ne construit pas
d'index paquet/cues complet. ffprobe expose les timebases et paramètres disponibles,
sans prouver que tous les points de seek sont indexés.

Les tags source `fra`/`eng` sont préservés tels quels ; leur normalisation BCP 47
en `fr`/`en` appartient au prochain modèle métier. Aucun choix automatique de
langue n'est effectué. Les deux audios sont analysés et décodés hors navigateur ;
leur commutation interne reste non validée : `audioTracks` absent
sur Chrome/Edge/Firefox testés avec réglages par défaut. Le sélecteur du banc
ne fabrique pas des pistes sélectionnables à partir du probe. P0C valide
séparément des remux mono-audio FR/EN, changement d'URL et position restaurée
(voir `P0C_AUDIO_SWITCHING.md`). Plusieurs vidéos et sous-titres
peuvent être représentés dans le tableau, mais ne sont pas encore testés.

Le modèle futur conserve les distinctions de l'architecture : Work → Edition →
MediaVersion → MediaSource → Asset(s) et Track(s), reliées par SourceTrack.
Ne pas fusionner œuvre, encodage et copie disque. Prévoir IDs persistants,
langue BCP 47, rôle forced/SDH/commentaire, offsets, durées, timestamps,
initialisation et index pour chaque association ; pas de schéma DB anticipé ici.

P3 distinguera fichiers externes indépendants, y compris `.opus` généralement Ogg,
et véritables paquets Opus/OBU AV1 sans conteneur. Ces derniers nécessitent
limites de paquets, timebases, horodatages, index et initialisation explicites.
Aucun format raw ni manifeste propriétaire n'est créé en P0.

## Résultat P0C Shaka

Les mêmes fMP4 mono-piste et WebVTT que MSE sont maintenant référencés dans
un vrai MPD SegmentList, sans modifier les paquets ni les timestamps. FR5.1/EN2
et FRforced sont détectés/sélectionnés par Shaka5.2.13 sur les trois navigateurs,
choix manuels conservés après seek. Cela ne rend pas accessibles les pistes
intégrées par l'API HTML5 native. Nombre de canaux conservé par piste ; mono/7.1
et restitution physique multicanal restent NT. Voir P0C_SHAKA_EXPERIMENT.md.


État des médias indépendants, manifeste d’association versionné et limites raw :
[SEPARATED_MEDIA.md](SEPARATED_MEDIA.md).

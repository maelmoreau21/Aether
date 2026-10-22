# Modèle média — P0 et suite

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

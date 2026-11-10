# HLS par copie pour les lecteurs natifs — 10 novembre 2026

AVPlayer (iOS, tvOS), ExoPlayer (Android), AirPlay et Chromecast lisent les
fichiers de la bibliothèque en HLS, **sans réencodage** : les paquets vidéo et
audio d'origine sont copiés dans des segments fMP4. Le navigateur garde son
lecteur existant.

## Parcours

1. L'application, connectée par cookie ou jeton d'appareil
   ([LOCAL_SESSIONS.md](LOCAL_SESSIONS.md)), demande une URL :
   `POST /api/v1/assets/{id}/hls` → `{url, expiresAt}`.
2. `url` vaut `{origine}/api/v1/assets/{id}/hls/{autorisation}/master.m3u8`.
   Elle se suffit à elle-même : on la donne telle quelle à AVPlayer, à un
   Chromecast ou à AirPlay, qui n'envoient ni cookie ni en-tête.
3. Toutes les URI des playlists sont relatives : playlists média, segment
   d'initialisation et segments restent sous le même chemin autorisé.

| Ressource | Contenu |
|---|---|
| `master.m3u8` | Construite par le Core à partir du probe : un variant vidéo, une rendition `TYPE=AUDIO` par piste lisible (nom, langue, canaux, piste par défaut), `BANDWIDTH`, `RESOLUTION`, `CODECS` exacts ou omis plutôt que devinés |
| `hls-v{v}.m3u8`, `-init.mp4`, `-{n}.m4s` | Rendition vidéo, servie par le Node |
| `hls-v{v}-a{a}.m3u8`, `-init.mp4`, `-{n}.m4s` | Rendition audio de la piste `a`, découpée sur les images clés de `v` |

Codecs proposés : vidéo H.264, HEVC (étiqueté `hvc1`, exigé par Apple), AV1 ;
audio AAC, AC-3, E-AC-3, FLAC, ALAC, MP3, Opus. Une vidéo d'un autre codec
renvoie `422 hls_video_unsupported` ; une piste DTS ou TrueHD n'est pas listée.

## Découpage (Node, `crates/node/src/hls/`)

- Les instants des images clés sont lus dans l'index du conteneur (Cues
  Matroska, tables `stss`/`stts`/`ctts`/`elst` MP4), sans lire le contenu :
  quelques lectures, même pour un fichier de 50 Go sur NAS. Index et sections
  d'initialisation sont gardés en mémoire (64 entrées par Node).
- Playlist VOD complète d'avance, segments d'environ 6 s commençant chacun sur
  une image clé. Chaque segment est produit seul par `ffmpeg -c copy`, dans
  n'importe quel ordre : ni session, ni fichier temporaire, saut immédiat.
- Vidéo coupée **au fragment** : FFmpeg ouvre un fragment à chaque image clé ;
  seuls les fragments dont le premier échantillon s'affiche dans la fenêtre du
  segment sont gardés. Les B-frames (ordre de décodage ≠ ordre d'affichage)
  rendent impossible une coupe par instant juste pour la vidéo et l'audio à la
  fois : c'est la raison des renditions séparées.
- Audio coupé par instant (`-copypriorss 0`, `-to`), exact puisque décodage et
  affichage coïncident.
- Chronologie décalée de +10 s pour toutes les renditions : les premiers
  paquets ont des instants de décodage négatifs qu'un `tfdt` non signé ne peut
  porter.
- Les segments sont transmis au fil de l'eau ; FFmpeg est arrêté dès que le
  segment est complet. Six copies simultanées au plus par Node.

## Autorisations de lecture

Aléatoires, stockées en SHA-256 (`playback_grants`), liées à **un** asset et à
la session émettrice : 12 h au plus, 32 par session. Chaque requête revalide la
session, le compte, les droits de bibliothèque et le contrôle parental. Une
déconnexion ou une révocation les supprime ; un renouvellement de jeton les
transfère à la nouvelle session, pour ne pas couper une diffusion en cours.
Ces URL n'acceptent jamais de cookie. Elles sont lisibles par toute origine,
sans identifiants (`Access-Control-Allow-Origin: *`), pour la page du récepteur
Chromecast.

## Vérification

- `cargo test -p media-node --lib hls:: -- --include-ignored`, avec le FFmpeg 9
  de l'image Node : recolle tous les segments servis dans le désordre, sur un
  AV1 et sur un H.264 à GOP de 2 s et 3 B-frames, en MKV et en MP4. La vidéo
  contient chaque paquet d'origine exactement une fois ; l'audio ne perd ni ne
  répète aucun paquet aux jonctions.
- `cargo test -p media-poc --test postgres -- --include-ignored` : émission,
  playlist maîtresse avec CORS pour une origine Chromecast, playlist média,
  refus sur un autre asset, survie au renouvellement et fin à la déconnexion,
  sur SQLite et PostgreSQL 18.

Non vérifié : lecture réelle sur iPhone, Apple TV, Android, Chromecast et
AirPlay ; HEVC à GOP ouvert ; Dolby Vision. Une piste audio DTS/TrueHD seule
demandera une conversion AAC, non faite.

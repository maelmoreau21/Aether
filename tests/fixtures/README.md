# Fixtures synthétiques P0

Générées exclusivement avec `testsrc2` et `sine` de FFmpeg : mire animée et
sinusoïdes mathématiques, aucun média tiers téléchargé. Aucune nouvelle licence
du dépôt n'est décidée par ce jalon. FFmpeg reste un outil externe, non redistribué.

`scripts/generate-fixtures.ps1` crée huit secondes de vidéo AV1 Main 8 bits
YUV420p, 320×180, 24 fps, et deux Opus mono 48 kHz (FR 440 Hz, EN 880 Hz).
Le MKV est encodé une fois ; MP4 faststart, WebM et MP4 fragmenté sont des
copies de paquets (`-map 0 -c copy`). Tous les flux sont décodés avec
`ffmpeg -xerror`; les SHA-256 de chaque paquet encodé sont comparés par piste.

Les médias générés sont ignorés par Git. `generation.json` conserve versions,
arguments, métadonnées, tailles et SHA-256. Une copie de référence est dans
`docs/evidence/2026-10-15/`. Les octets du conteneur peuvent varier lors d'une
nouvelle génération (identifiants du muxeur) ; les sommes de référence sont
celles des fichiers effectivement testés, pas une promesse de build bit à bit.

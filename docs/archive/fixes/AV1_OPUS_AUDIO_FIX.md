# Sélection audio AV1/Opus — 22 octobre 2026

## Cause reproduite sur develop

Le Core expose correctement les indices absolus des pistes Opus : `internal-1`
et `internal-2`, associés à `prepared/audio-1` et `prepared/audio-2`. Le Node
utilise `-map 0:{index} -c:a copy`, sans filtre ni `-ac`. La vidéo utilise
`-c:v copy`. Aucun changement Tdarr ou du transport sécurisé n'est nécessaire.

Le défaut restant était dans la présélection Web : `playbackMime()` fabriquait
`video/mp4; codecs="av01"` (ou `av01,opus`). Ce paramètre AV1 est incomplet :
profil, niveau/tier et profondeur sont obligatoires selon la
[spécification AV1 ISOBMFF](https://aomediacodec.github.io/av1-isobmff/#codecsparam).
Chrome et Edge testés renvoient une chaîne vide pour ce MIME, mais décodent
réellement le fichier AV1 10 bits. `chooseSource()` excluait alors les deux
pistes préparées et retombait sur l'original multiaudio, de langue `und`.

Le message « Votre langue précédente est indisponible » provenait uniquement
de la comparaison de cette langue `und` avec la préférence FR/EN, après le
chargement. Il ne signalait ni une piste absente de ffprobe, ni une erreur de
décodage. Un ID sauvegardé `internal-default` pouvait prolonger cette sélection
ambiguë lors des réouvertures. Ce défaut est reproduit sur les fixtures ; le
fichier personnel Avatar n'a pas été fourni ni testé.

## Correction

- Utiliser le MIME du conteneur tant que la configuration codec complète n'est
  pas disponible. La sonde reste indicative ; ses réponses classent les
  empaquetages et ne suppriment plus une langue. Un échec de chargement/décodage
  reste explicite, sans autre langue substituée automatiquement.
- Préférer les pistes indexées, normaliser fra/fre/fr et eng/en, restaurer les
  choix manuels explicites, migrer l'ancien ID original ambigu vers la langue
  préférée, puis FR. Ne pas enregistrer `und` comme préférence linguistique.
- Retirer du menu audio l'original ambigu lorsque toutes ses pistes disposent
  d'un choix explicite ; conserver l'original dans les sources et si nécessaire.
- Distinguer une langue absente d'une langue inconnue ou d'un choix manuel ;
  effacer un ancien avis après une sélection réussie.

La commutation transactionnelle et la coordination des deux éléments HTML5
déjà présentes sont conservées. La préparation Ogg/Opus reste une copie sans
réencodage ; une vidéo fMP4 commune est réutilisée entre langues. La première
préparation peut nécessiter la lecture complète du composant, selon le cache
et les limites du Node. Ce correctif ne rend pas cette préparation instantanée.

## Vérifications reproductibles

Depuis la racine, avec FFmpeg/ffprobe et les dépendances Web installés :

```powershell
python scripts/generate-dual-opus-fixture.py
node scripts/browser-dual-opus.cjs
cargo test --locked -p media-node --lib packaging::tests::copies_dual_51_opus_mp4_without_changing_packets_or_channels -j 1 -- --ignored --test-threads=1
npm test --prefix apps/web -- --reporter=dot
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo test --locked -p media-poc --test browser_scanner -j 1 -- --ignored --nocapture --test-threads=1
```

La nouvelle fixture synthétique dure 16 secondes : AV1 Main 1080p 12 fps,
`yuv420p10le`, FR par défaut et EN, chacun Opus 48 kHz 5.1. Les signaux FR
440 Hz et EN 990 Hz permettent de vérifier la langue réellement décodée.
Le générateur refuse l'écrasement ; les médias personnels ne sont pas utilisés.
Le test Rust supplémentaire est explicitement ignoré par défaut car il exige
cette fixture optionnelle ; la commande ci-dessus l'exécute réellement.

Le script navigateur utilise le code TypeScript de production et un serveur
HTTP isolé avec HEAD/Range. Il vérifie images décodées, PCM, FR/EN/FR répété,
un seul `loadstart` vidéo, plein écran, chronologie conservée, seek avant/arrière
en pause, reprise, échec HTTP 404 d'un candidat audio et réouverture FR/EN.
Le graphe WebAudio est seulement un instrument de test, absent de la production.

Pour Edge hors de l'emplacement Playwright standard, dans un terminal distinct :

```powershell
$env:MEDIA_BROWSER_CHANNEL='msedge'
$env:MEDIA_BROWSER_EXECUTABLE='C:/Program Files (x86)/Microsoft/EdgeCore/155.0.4283.62/msedge.exe'
$env:MEDIA_REPORT_DIR='target/dual-opus-edge'
node scripts/browser-dual-opus.cjs
```

Adapter le chemin à la version installée. Les rapports sont écrits dans
`target/dual-opus-browser`, `target/dual-opus-edge` et `target/browser-scanner`
par défaut ; `MEDIA_REPORT_DIR` permet de choisir un autre dossier.

## Résultats exécutés

- Windows 10.0.26300, Chrome 155.0.8059.85 et Edge 155.0.4283.62, headless :
  scénario dual-5.1 réussi. Rapports [Chrome](evidence/2026-10-22/dual-opus/chrome.json)
  et [Edge](evidence/2026-10-22/dual-opus/edge.json). Cinq choix FR/EN/FR/EN/FR,
  un chargement vidéo par séance, PCM distinct, seek/pause/reprise et échec réel
  du chargement Ogg HTTP 404 avec conservation de l'ancien audio. Les écarts
  d'horloges mesurés ponctuellement figurent dans les rapports, sans garantie
  physique ou sur une durée longue.
- Parcours Core/SQLite/Chrome complet : réussi en 39,96 s, VF par défaut,
  scan incrémental, quatre sélections par conteneur, préférences serveur EN et
  FR restaurées à la réouverture sans faux avis, sous-titres/progression et
  redémarrage réel du Core. Fixtures historiques FR 5.1/EN stéréo dans ce
  parcours, distinctes du nouveau MP4 à deux 5.1 :
  [MP4](evidence/2026-10-22/dual-opus/core-mp4.json),
  [MKV après redémarrage](evidence/2026-10-22/dual-opus/core-mkv.json).
- Rust packaging : nouveau test dual-5.1 exécuté avec `--ignored`, réussi ;
  deux tests existants réussis, dont copie 7.1 MKV/MP4. Paquets SHA-256 identiques
  par piste, vidéo toujours AV1 10 bits 1080p et six canaux par audio.
- Web : 80 tests réussis ; cinq régressions supplémentaires de l'audit existant
  réussies via `apps/web/node_modules/.bin/vitest run --config docs/evidence/2026-10-21/player-audit/vitest.config.mts`.
  Check Svelte 0 erreur/avertissement, lint et build réussis.
- `cargo test --locked -p media-poc --test scanner_flow --test distributed_multi_node -j 1 -- --test-threads=1` :
  deux tests réussis. Contrôles d'accès vivants, HEAD/GET/Range et deux Nodes
  distants sur la machine locale ; aucune preuve réseau multi-machine déduite.
- `cargo fmt --check`, `cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings`
  et `git diff --check` : réussis. Pas de suite Rust workspace générale relancée.

## Limites de ces résultats

Les mesures sur deux horloges HTML5 sont ponctuelles et ne garantissent pas
une synchronisation physique universelle ni l'absence de dérive sur un épisode
entier. La conservation des six canaux et des paquets ne prouve pas leur sortie
physique 5.1 : navigateur, OS, pilote et équipement restent déterminants.
Pas de preuve sur le fichier Avatar, le NAS, un réseau multi-machine, les
appareils physiques, Firefox/Safari, HDR/4K ou une lecture longue dans ce lot.

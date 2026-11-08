# Lecture : diagnostics typés, première image / premier son, matrice Direct Play — 7 novembre 2026

Cette passe ne change aucun codec, aucun chemin de préparation et n'impose aucun
transcodage. Elle rend les échecs distinguables, mesure séparément l'image et le
son au démarrage, et vérifie le Direct Play sur une matrice de médias synthétiques.
Tout ce qui suit a été mesuré sur **médias synthétiques**, en **boucle locale**,
dans **Chromium 156 Linux headless (Docker)**. Rien ici ne vaut pour un NAS, un
film réel, Edge, Chrome de marque, Firefox ou Safari (voir « Non exécuté »).

## Diagnostics distincts

Avant : une `MediaError` (code 2, 3 ou 4) ou un délai produisait un texte unique
« réseau, conteneur ou codec », et un échec de codec déclenchait quand même le
basculement automatique vers une copie équivalente (même fichier ou même runtime :
elle échoue à l'identique).

Après, [`player/diagnostics.ts`](../apps/web/src/lib/player/diagnostics.ts) porte un
type `PlaybackFailure` dont `kind` vaut :

| Genre | Établi à partir de | Basculement auto |
| --- | --- | --- |
| `codec` | `MediaError` 3, ou 4 alors que le conteneur seul est accepté par `canPlayType` | non |
| `container` | `MediaError` 4 avec conteneur refusé ; `unsupported_preparation` du Node | non |
| `network` | `MediaError` 2 ; `fetch` sans réponse ; 408/429/502/503/504 sans code Node | oui |
| `permission` | HTTP 401/403 de Core | non |
| `preparation` | `preparation_busy`, cache plein, délai, outil/source absent, source modifiée, capacité | oui |
| `storage` | `source_permission_denied`, `source_io_error` (Node) | oui |
| `node` | `node_unavailable`, `node_revoked`, `stream_timeout` | oui |
| `unknown` | rien de ce qui précède n'est établi | oui (comportement antérieur) |

Une panne **en cours de lecture** (Node coupé) n'a que `MediaError` : le lecteur
interroge alors l'URL fautive par une requête `Range: bytes=0-0` (3 s maximum, après
la mise en pause, donc sans retard de lecture) et ne retient le verdict codec que si
les octets arrivent. Une réponse HTTP précise l'emporte sur l'hypothèse de décodage ;
jamais l'inverse. Le diagnostic reste une hypothèse : statut HTTP, code Node et code
`MediaError` sont conservés dans le message pour le support. Le panneau d'erreur
expose `data-failure-kind`.

Côté Node ([`packaging.rs`](../crates/node/src/packaging.rs)), FFmpeg quittant avec
un code non nul était toujours `unsupported_preparation` (donc « conteneur »), même
quand le fichier était illisible. Le tampon stderr déjà conservé sert désormais à
distinguer `source_permission_denied` (permissions du partage) et `source_io_error`
(erreur d'E/S du stockage) ; un vrai rejet de conteneur reste
`unsupported_preparation`. « source modifiée, rescanner » sortait en
`preparation_failed` alors que le client attendait `source_changed_rescan_required` :
corrigé. Les autres genres (délai, cache plein, occupé) ne sont jamais réécrits
depuis du texte.

## Première image et premier son, séparément

[`player/startup-timeline.ts`](../apps/web/src/lib/player/startup-timeline.ts) est
branché dans `NativePlayer` (`startupSummary()` et une ligne `[media playback]
stage:startup` en console). Étapes : `prepared` (HEAD de préparation), `metadata`
(réseau + index du conteneur), `video-ready`, `audio-ready` (init MSE + première
fenêtre), `buffered`, `play-resolved`, `first-frame`, `first-audio`.

- `first-frame` : premier `requestVideoFrameCallback` (frame remise au compositeur) ;
  repli sur `playing`.
- `first-audio` : `audio-element` quand un élément audio séparé (externe ou MSE) a
  démarré ; `media-clock` quand la piste est dans l'élément vidéo, qui n'émet aucun
  événement audio : l'horloge média qui avance est un **indicateur indirect**, pas
  une mesure de son. Le champ `audioSignal` le dit.

Coûts : la résolution du descripteur se passe avant `load()` (non incluse) ; la sonde
de codecs et le remux sont côté Node (`prepared` les englobe, plus les journaux
`node_prepared_response`) ; réseau = `metadata` ; MSE = `audio-ready` ; décodage =
`first-frame` − `buffered`.

## Matrice Direct Play (Chromium 156 Linux headless, boucle locale)

[`scripts/generate-playback-matrix.sh`](../scripts/generate-playback-matrix.sh)
produit 12 fichiers de 12 s, 320×180, 24 i/s : AV1 8 et 10 bits × MP4/MKV/WebM ×
Opus stéréo et 5.1, avec une piste FR (440 Hz) et une piste EN (1050 Hz).
[`scripts/browser-playback-matrix.cjs`](../scripts/browser-playback-matrix.cjs)
les sert par Range (blocs de 64 Kio, rien n'est préparé ni réencodé) et mesure,
trois ouvertures par fichier (la première en contexte neuf) :

| Indicateur (médiane sur 12 fichiers) | 1re ouverture | ouvertures 2 et 3 |
| --- | ---: | ---: |
| Métadonnées | 17 ms | 17 ms |
| Première frame | 42 ms | 39 / 36 ms |
| Premier son (pic > −70 dBFS) | 16 ms | 17 / 17 ms |
| Reprise après pause → son | 17 ms | 9 / 9 ms |

Les 12 combinaisons se lisent en Direct Play, `canPlayType` répond `probably` pour
toutes, la pause tient, les deux seeks (8 s puis 2 s) reprennent image et son en
17–26 ms. La piste audible par défaut est FR (431 Hz mesuré, bin de 23 Hz). Le
son précède l'image de quelques dizaines de ms sur ces fichiers : afficher la
première frame n'implique pas que le son soit déjà là, d'où deux mesures.
Limites : fichiers de 0,5 Mo servis d'un seul `Range` ; la boucle locale masque
tout coût réseau ; le disque de l'hôte n'est pas « froid » ; le décodage AV1 est
probablement logiciel, sur le CPU d’un PC. **Le 5.1 est lu mais non vérifié comme 5.1** : un
`MediaElementSource` Web Audio rapportait 2 canaux, y compris pour le 5.1, donc aucune preuve de
sortie multicanal. Chromium n'expose pas `audioTracks` : un élément natif ne peut pas
choisir FR/EN, ce qui justifie le chemin par fenêtres Opus (inchangé).

## Non-régression du lecteur de production

Le banc existant [`browser-streaming-performance.cjs`](../scripts/browser-streaming-performance.cjs)
(injecte les vrais modules du lecteur) a été remis à niveau : il ignorait les
nouveaux modules et, depuis le commit i18n, `../i18n` — il ne pouvait plus s'exécuter
sur `HEAD` sans cela. Sur `av1-10bit-opus-stereo.mp4`, 5 reprises : original et
segmenté **réussissent sans erreur**. Segmenté : première frame 28,5 ms, premier PCM
32,4 ms, seek 28,7 / 26,6 ms. Les 10 changements FR↔EN gardent la même source vidéo
(`videoSourceUnchanged` vrai), nouveau PCM médian 32,1 ms, plus grand trou vidéo
50 ms ; le premier switch (fenêtre froide) a pris 105 ms, les suivants 22–24 ms.
**Aucun changement de piste inférieur à la seconde n'est promis** : ces chiffres sont
une boucle locale où le Node est un serveur JavaScript, pas FFmpeg sur un NAS ; les
anciennes mesures avec 300 ms d'attente injectée (STREAMING_PERFORMANCE.md) montrent
que la nouvelle langue n'arrive pas plus vite que le réseau.

## Remux progressif, cache, verrous, allocations

- **Remux progressif** : non étudié plus loin. Rien dans ces mesures n'en démontre le
  besoin : le Direct Play et l'original + audio segmenté démarrent en ≈30–50 ms ici ;
  le repli par remux complet reste le coût ouvert des gros fichiers à conteneur
  refusé (documenté dans STREAMING_PERFORMANCE.md), à mesurer sur NAS d'abord.
- **Cache de préparation, verrous, invalidations, concurrence** : relus dans les
  documents existants (PLAYER_TRACK_PREPARATION.md, NAS_PLAYBACK_DIAGNOSTICS.md) ;
  **pas ré-audités dans le code** lors de cette passe. Le seul changement Rust est la
  classification des erreurs ; aucune structure de verrou ou d'invalidation n'a bougé.
- **Allocations** : le banc sert par blocs de 64 Kio et aucun nouveau code n'alloue
  proportionnellement à la taille d'un média (le tampon stderr FFmpeg reste borné à 16 Kio).

## Commandes

```powershell
docker run --rm -v "${PWD}:/work" -w /work aether-rust-dev sh scripts/generate-playback-matrix.sh
$env:MEDIA_BROWSER_CHANNEL='chrome'   # ou 'msedge' ; sinon MEDIA_BROWSER_EXECUTABLE
node scripts/browser-playback-matrix.cjs
$env:MEDIA_TEST_FIXTURE='tests/fixtures/generated/matrix/av1-10bit-opus-stereo.mp4'
node scripts/browser-streaming-performance.cjs
```

Preuves : [matrice](evidence/2026-11-07/playback/direct-play-matrix-chromium-156-linux.json),
[lecteur de production](evidence/2026-11-07/playback/native-player-av1-10bit-mp4-chromium-156-linux.json).
Vérifié : 403 tests Web, ESLint, i18n (24), `cargo fmt --check`, Clippy `-D warnings`
sur `media-node`, 2 nouveaux tests Rust. `svelte-check` garde une erreur préexistante
(`process` dans `format.test.ts`). Six tests `packaging::tests` échouent dans cet
environnement faute des médias `representative/*` et d'encodeurs externes ; ils
dépendent de fichiers absents, pas des erreurs touchées ici (non rejoués sur la base
sans modification).

## Non exécuté

- **Edge, Chrome de marque, Firefox, Safari** : seul Chromium 156 (Linux, Docker) a
  tourné ; les scripts acceptent `MEDIA_BROWSER_CHANNEL=chrome|msedge`. À exécuter sur Windows.
- **NAS, fichiers réels, réseau distant, Node Rust réel sous charge.**
- Plein écran, sous-titres, changement de version et annulation **en navigateur réel
  pour ce commit** : couverts par les tests unitaires existants (403) et les parcours
  `browser_scanner`/`browser-immersive`, non rejoués ici.
- **Déconnexion d'un Node en cours de lecture** : testée en unité (la `MediaError` suivie
  d'une réponse 503 `node_unavailable` donne `node`, et le basculement reste actif) et
  par le test Core de failover du commit précédent ; pas rejouée avec un vrai Node coupé.
- Plusieurs sessions concurrentes, cache chaud/froid côté Node Rust, longue durée.
- AV1 8/10 bits × Opus 7.1 et pistes multiples > 2 (la matrice en a 2).

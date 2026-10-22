# Lecteur : audit et fiabilité locale — 22 octobre 2026

Branche `develop`. Ce jalon traite P0 de la demande d'optimisation du lecteur.
Il ne clôture pas le programme P1–P4, ni l'incident du fichier personnel sur NAS.
Les mesures sont des observations locales, pas des garanties ni un gain statistique.

## Diagnostic DUT / DTS-HD MA

Le fichier décrit par l'utilisateur est un MKV H.264 1080p avec DTS-HD MA 5.1
néerlandais. Il n'est pas accessible dans cet environnement. Aucun média personnel
n'a été téléchargé, modifié ou utilisé comme fixture.

Chrome 155.0.8059.85 et Edge 155.0.4283.62 installés sous Windows déclarent
`video/x-matroska; codecs="avc1.640028"` lisible (`probably`), mais rejettent
les déclarations `dtsc`, `dtsh`, `dtsl`, tant dans MKV que dans MP4.
Ce résultat distingue le H.264 du problème audio. Ce n'est pas un décodage du
fichier personnel, ni une preuve que son transport est sain.

Le code Chromium traite le DTS comme une capacité de compilation/plateforme,
pas comme une conséquence de la reconnaissance du codec par FFprobe :
[options média Chromium](https://chromium.googlesource.com/chromium/src/media/+/3667d78ad23505f65394c287ca213f1f0f2eb614/media_options.gni),
[discussion Chromium sur DTS](https://groups.google.com/a/chromium.org/g/chromium-dev/c/ELYS1TJwWq8).
Les capacités du navigateur exécuté restent vérifiées à chaque tentative concernée.

Le lecteur refuse désormais avant préparation/chargement une piste DTS dont la
combinaison codec/conteneur est rejetée par `canPlayType`. Pour le profil MA :

> Audio DTS-HD MA non pris en charge selon les capacités déclarées par ce navigateur
> dans ce conteneur MKV. Aucun transcodage serveur : choisissez une autre version
> ou une piste compatible.

Le message conserve la langue et l'identité de piste. Un original multi-audio
avec des codecs différents n'est pas rejeté arbitrairement comme s'il sélectionnait
forcément le DTS. Une réponse positive de capacité laisse toujours le vrai
chargement/décodage décider ; aucun codec n'est annoncé fonctionnel sur ce seul indice.
Cette vérification négative est volontairement limitée au DTS connu. Les codecs
inconnus ne sont pas inventés, et aucune configuration AV1 incomplète n'est produite.
Le test DTS utilise un descripteur synthétique et le vrai navigateur, **pas une
fixture DTS-HD MA décodée**. Le refus immédiat ne prouve donc pas le fichier personnel.

## Corrections vérifiées

- Échec d'une nouvelle langue : reprise de l'ancienne présentation selon son état
  précédent, conservation de la piste et des préférences. L'UI affiche un avis
  récupérable et ne remet plus cette présentation en pause par son gestionnaire
  d'erreur terminale. Si la reprise échoue aussi, l'erreur reste terminale.
- Seek pendant un fetch ou une mise à jour SourceBuffer : vérification de génération
  avant append, abandon des données obsolètes, relance immédiate à la nouvelle
  position après libération de l'opération précédente.
- Timeout : distinction métadonnées/seek, états HTMLMediaElement et code MediaError.
  La cause réseau/codec n'est pas déduite arbitrairement de l'expiration.
- Labels de sous-titres externes : caractères UTF-8 `·` et `forcés` réparés.
  Le parcours UI complet avait révélé ces chaînes déjà corrompues ; le premier
  essai échouait sur le libellé ASS malgré la réussite de la restauration audio.

## Audit du parcours existant et suite

| Chemin | Constat dans le code et les tests | Conséquence pour la suite |
| --- | --- | --- |
| MP4/WebM AV1 + Opus | Vidéo originale avec Range, fenêtres audio MSE copiées | Pas de copie vidéo complète nécessaire dans ce chemin |
| MKV AV1 + Opus | Préparation historique vidéo/audio complète possible | Goulet structurel du démarrage à froid à mesurer séparément |
| H.264 + DTS | Source originale sans préparation Opus appropriée | Refus immédiat du DTS non déclaré, aucun réencodage de secours |
| Packaging | Une fenêtre de 10 s, preroll 1 s, sortie bornée à 12 s/16 Mio, un worker | Jusqu'à environ 360 lancements FFmpeg/heure pour une piste sans réutilisation ; coût NAS non mesuré |
| Cache Node | 64 entrées, LRU des fenêtres inactives, leases HTTP, deux attentes bornées | Réutilisation existante ; ne pas augmenter les limites sans profilage |
| Index | Probe inventorié/réutilisé, seek FFmpeg via index du conteneur | Pas de nouveau probe par seek ; index natif Rust non justifié par ces mesures |
| Streaming | Blocs de 64 Kio, GET/HEAD/206/416, relais borné et signatures | Préserver les contrôles et la mémoire bornée |
| Audio Web | Deux horloges HTML5, recalage explicite, candidat préparé pendant la pause | L'arrêt visible au changement demeure ; pas de promesse de commutation transparente |
| UI | État piloté par événements média et actions sérialisées | Mesurer commandes/progression avant toute refonte réactive |

La prochaine étape P1 est de mesurer réellement le silence et les images pendant
la préparation d'un candidat, puis de tester sa préparation pendant que l'ancien
audio continue, avec réalignement final et restauration transactionnelle. Une
simple durée de promesse ne suffit pas à valider cette optimisation.
P2 doit profiler disque/CPU/processus/cache sur NAS avant de choisir durées de
fenêtres, sessions FFmpeg persistantes ou démux natif. P3 comparera ensuite le
pipeline actuel à un pipeline MSE commun sur les mêmes médias. P4/raw `.obu`
reste distinct ; aucune nouvelle compatibilité brute n'est annoncée.

## Reproduction et lecture des mesures

```powershell
npm test --prefix apps/web
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
$env:MEDIA_TEST_SEGMENTED='1'
$env:MEDIA_REPORT_DIR='target/player-performance-chrome'
node scripts/browser-dual-opus.cjs
# Adapter le chemin à la version Edge effectivement installée.
$env:MEDIA_BROWSER_EXECUTABLE='C:/Program Files (x86)/Microsoft/EdgeCore/155.0.4283.62/msedge.exe'
$env:MEDIA_TEST_FIXTURE=(Resolve-Path tests/fixtures/generated/dual-opus/dual-opus-long.mp4).Path
$env:MEDIA_REPORT_DIR='target/player-performance-edge-long'
node scripts/browser-dual-opus.cjs
```

Fixtures légales déjà présentes : 1080p AV1 Main 10 bits, 12 fps, deux Opus 5.1,
16 s et 3500 s/336 Mio. Les seeks éloignés ne constituent pas une séance complète
de 58 minutes, encore moins plusieurs heures de synchronisation.

La référence avant correction (`before-chrome.json`) donne première image 185,5 ms
et promesse de lecture prête 188,3 ms. Les rapports après sont séparés. Ces essais
ponctuels n'établissent **aucune amélioration statistique de latence**. Le serveur
HTTP du banc est isolé et son cache de fenêtres n'est pas celui du Node Rust ;
les tests Core/Node réels complètent ce banc sans en transformer les chiffres
en mesures du packaging Rust ou du NAS.

Derniers essais après correction (processus locaux concurrents possibles, un essai
par ligne ; les nombres ne sont pas comparables à des quantiles de charge) :

| Navigateur / fixture | Première image | Lecture prête | Changements effectifs, fin de promesse | Dérive max échantillonnée pendant les changements | Refus DTS |
| --- | --- | --- | --- | --- | --- |
| Chrome / 16 s | 291 ms | 613 ms | 16–117 ms | 47 ms | 2,4 ms |
| Edge / 3500 s | 264 ms | 355 ms | 17–183 ms | 50 ms | 2 ms |

Le démarrage Chrome de ce dernier essai est **plus lent que la référence** ; aucun
gain de démarrage n'est revendiqué. La correction vise la fiabilité et ne dispense
pas de mesures répétées et isolées pour P1. Rapports complets :
[preuves P0](evidence/2026-10-22/player-performance-p0/).

`switchReadyMs` mesure seulement la fin de `selectAudio`, pas le premier son ni le
silence maximal. La première entrée resélectionne la piste courante : son quasi-zéro
ne doit pas être compté comme changement de langue. Le signal PCM et les images
sont contrôlés ensuite, séparément. La dérive est un échantillon des horloges,
pas une preuve de synchronisation perceptive sur plusieurs heures. Le graphe
WebAudio est exclusivement une sonde de test ; aucun downmix serveur n'est ajouté.
`firstImageMs` démarre juste avant l'appel `openPlayer`, après l'action de plein
écran du banc ; ce n'est pas une mesure du clic DOM jusqu'à la réaction visuelle
des commandes Svelte.

CPU/RSS Core/Node/navigateur, première onde sonore, silence maximal, images répétées,
écritures disque, bande passante totale, 4K, réseau lent et quantiles p50/p95/p99
ne sont pas mesurés dans ce jalon. Ne pas déduire ces métriques des délais ci-dessus.

## Vérifications exécutées

- Web : 91 tests, check sans erreur/avertissement, lint, build statique réussis.
- Chrome et Edge réels headless : images et PCM distincts FR/EN, changements
  répétés sans rechargement vidéo, pause/seeks, rejet HTTP 404 conservant la
  lecture, fermeture pendant fetch, refus DTS avant chargement. Edge : seeks
  8/900/1800/3480/2 s sur la grande fixture.
- Core/SQLite/Chrome/Svelte : `browser_scanner` réussi en MP4 puis MKV après
  redémarrage, dont la nouvelle panne réseau simulée au changement de langue,
  préférence précédente inchangée et aucun clic Lecture nécessaire pour continuer.
- `cargo test --locked -p media-node --lib stream:: -j 1 -- --test-threads=1` :
  3 réussis (HEAD/GET/Range/416, confinement et lease).
- `cargo test --locked -p media-poc --test scanner_flow --test distributed_multi_node -j 1 -- --test-threads=1` :
  3 réussis (inventaire/probes réutilisés, droits vivants, WebM, deux Nodes locaux).
- Test de label UTF-8 ciblé réussi ; tests packaging : copie/cache et Opus 7.1
  réussis, test dual-5.1 ignoré par défaut exécuté explicitement et réussi.
- Fmt, Clippy workspace/all-targets strict et `git diff --check` vérifiés.
  Pas de suite Rust générale ni nouvelle CI Linux distante exécutée.

Le premier essai UI complet a échoué sur le libellé ASS corrompu ; son rapport est
conservé séparément. Après réparation UTF-8, les deux conteneurs passent. Le premier
lancement Edge par `channel=msedge` ne trouvait pas le chemin attendu par Playwright ;
les essais réussis utilisent l'exécutable EdgeCore installé, sans installation nouvelle.

## Diagnostics en cas de prochaine panne

Console navigateur : événements `[media playback]` avec phase de refus ou
`metadata-failed`/`seek-failed`, `networkState`, `readyState`, `mediaError` numérique.
Ni URL, ni message brut du décodeur, ni jeton ne sont ajoutés à ces événements.
`[media audio]` décrit séparément les fenêtres reçues. Conserver l'onglet Réseau
pour connaître le statut HTTP et le contenu exact de Range/Content-Range.

Core : événements JSON `media_http_start` et `media_http_response` pour l'original,
local ou relayé. Node direct : `node_media_start` et `node_media_response`.
Champs : identifiant d'asset, méthode/présence Range, HTTP, taille annoncée,
présence Content-Range et temps de disponibilité des en-têtes. Aucun chemin disque,
cookie, URL signée ou query string n'est écrit. Ce temps n'est pas le temps total
d'envoi du corps. Les événements de préparation existants restent inchangés.

```sh
docker logs --since 10m aether-core 2>&1
# Même commande sur le Media Node distant, avec son vrai nom de conteneur.
```

Un refus DTS avant toute requête média ne produit normalement aucun événement
`media_http_start`. En cas de vraie tentative : 401/403 orientent vers les droits,
404 vers l'asset/source, 416 vers la plage, 5xx vers Core/Node ; 206 et la présence
de Content-Range ne prouvent pas à eux seuls que le corps est complet ni décodable.
La vérification d'un proxy/NAS particulier exige encore ses traces réelles.

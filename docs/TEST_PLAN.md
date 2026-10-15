# Tests P0 — exécutés et restants

15 octobre 2026, Windows x64 10.0.26300, Rust/Cargo 1.96.1, FFmpeg/ffprobe
9.0.2, Chrome 155.0.8059.40, Playwright 1.62.1.

Commandes réellement lancées à la racine :

```powershell
./scripts/generate-fixtures.ps1
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run -p media-poc
```

Le script de génération conserve les arguments détaillés réellement utilisés
dans `generation.json` : encodage synthétique unique, trois remux `-c copy`,
probe, hachage des paquets et décodage intégral de **toutes** les pistes.
Résultat : quatre fichiers générés et décodés, paquets identiques après remux.
Les médias personnels ne sont ni lus ni modifiés.

Test navigateur effectué dans un second processus, avec le Playwright fourni
par l'environnement local (pas d'installation npm exécutée pendant ce jalon) :

```powershell
$env:NODE_PATH = 'C:\Users\Mael\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\node_modules'
node scripts/browser-smoke.cjs
```

Résultat : quatre lectures réussies, erreur de source manquante explicite.
Le script enregistre le rapport avant de retourner un code non nul si une
lecture échoue ; un échec est une observation à conserver, pas à masquer.
Reproduction sur une autre machine : installation dev Playwright indiquée
dans le README, Chrome installé et serveur déjà démarré.

Tests Rust : **5 réussis sur Windows**, couvrant :

- Probe de quatre vrais conteneurs et trois pistes chacun ; langues et timebases,
  retrait du chemin disque ; rejet de bytes invalides et d'un MP4 tronqué.
- GET complet et HEAD, exactitude des bytes et Content-Length ; Range fermé,
  suffixe, ouvert, fin au-delà de la taille ; 206/416 et Content-Range.
- Identifiant inconnu, tentatives de chemins Windows/traversée, POST refusé,
  Host/Origin et Sec-Fetch-Site hostiles refusés.
- Premier chunk borné à 64 KiB, abandon du corps, requête suivante fonctionnelle.
- Copie temporaire de fixture portée à 1 GiB : longueur annoncée correcte,
  premier chunk ≤64 KiB sans collecte intégrale. **Pas une mesure RSS.**

Un test supplémentaire de lien sortant est compilé uniquement sous Unix.
Il n'a **pas** été exécuté sur Windows. Le workflow CI Linux a été ajouté,
mais aucun résultat GitHub Actions n'est revendiqué.

Restent NT : sandbox hostile du parseur, TOCTOU concurrent, junctions Windows,
RSS/CPU/débit/charge, clients lents, délais réseau, toutes les positions de seek,
troncature en cours de lecture, timeouts/sortie excessive du probe, plusieurs
vidéos et sous-titres, commutation audio, autres navigateurs, choix Video.js/Shaka.
Ne pas assimiler annulation locale du corps à une campagne réseau complète.

## Second jalon : Edge/Firefox et limite audio

Même date, OS et fixtures ; Chrome 155.0.8059.40, Edge 155.0.4283.45 et
Firefox 155.0.1 headless. Résultats détaillés dans la matrice et les rapports
`evidence/2026-10-15/native-audio/` : **12 lectures natives réussies**.
Pixels RGB sans compter l'alpha, PCM avant/après seek et fréquence dominante
439,453125 Hz, pause/seek/reprise/fin et erreur 404/MediaError 4 vérifiés.
**Sélection EN et retour FR non validés : audioTracks absent sur tous les cas.**
`audioSelection=unavailable` n'est pas compté comme un succès. Le champ `passed`
principal et le code de sortie 0 concernent la lecture native et ses erreurs.
Une sélection réellement tentée mais échouée rendrait le code de sortie non nul.
La branche FR/EN/FR du scénario reste NT. MSE : annonce seulement, pas d'append.

Serveur relancé : `cargo run --locked -p media-poc`. Commandes effectivement
exécutées dans trois processus PowerShell séparés (ne pas conserver le chemin
Edge pour le lancement de Firefox/Chrome) :

```powershell
$env:NODE_PATH = 'C:\Users\Mael\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\node_modules'
$env:MEDIA_BROWSER_CHANNEL = 'msedge'
$env:MEDIA_BROWSER_EXECUTABLE = 'C:\Program Files (x86)\Microsoft\EdgeCore\155.0.4283.45\msedge.exe'
node scripts/browser-matrix.cjs
```

```powershell
$env:MEDIA_BROWSER_CHANNEL = 'firefox'
node scripts/browser-matrix.cjs
```

```powershell
$env:NODE_PATH = 'C:\Users\Mael\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\node_modules'
$env:MEDIA_BROWSER_CHANNEL = 'chrome'
node scripts/browser-matrix.cjs
```

Firefox installé a été utilisé via WebDriver, pas un Firefox modifié Playwright.
Outil dev préparé depuis Mozilla, sans installation système (dossier
`artifacts/tools` créé auparavant, ignoré par Git) :

```powershell
Invoke-WebRequest 'https://github.com/mozilla/geckodriver/releases/download/v0.37.1/geckodriver-v0.37.1-win64.zip' -OutFile artifacts/tools/geckodriver.zip
Expand-Archive -LiteralPath artifacts/tools/geckodriver.zip -DestinationPath artifacts/tools/geckodriver
```

SHA-256 de l'archive :
`DFED9315ABE8D2FBC1B6161A2EE8002452E79CF05EE92FDC653A4E26BC35EDD8`.
Edge existant trouvé dans EdgeCore : aucune installation nécessaire.

Contrôles relancés : `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`
(5 réussis), `node --check` sur les trois scripts de test et `bench.js`.
Les SHA-256 sont identiques avant/après chaque scénario et correspondent aussi
à `generation.json` du premier jalon. Aucune fixture régénérée ni modifiée.
Restent humains/NT : [procédure précise](MANUAL_BROWSER_CHECKS.md), menus audio
intégrés et écoute physique. Aucun résultat NAS/CI/MSE supplémentaire revendiqué.

## P0C — remux et changement de source

Préparation réellement lancée : `./scripts/prepare-language-remux.ps1`.
Le script refuse un dossier existant (cas vérifié), n'écrit pas dans les
originaux et archive versions, arguments, SHA-256, comparaison des paquets,
probes et décodage. Rapport complet local dans le dossier généré ; copie
compacte des comparaisons dans `evidence/2026-10-15/remux/preparation.json`.
Les grandes listes de hashes de paquets restent dans le rapport local complet.

Serveur redémarré avec `cargo run --locked -p media-poc`. Série finale réellement
exécutée dans un même PowerShell, séquentiellement pour éviter la contention :

```powershell
$env:NODE_PATH = 'C:\Users\Mael\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\node_modules'
$env:MEDIA_TEST_SCENARIO = 'remux'
foreach ($testChannel in @('chrome','msedge','firefox')) {
    $env:MEDIA_BROWSER_CHANNEL = $testChannel
    $env:MEDIA_REPORT_DIR = "artifacts/remux-$testChannel"
    if ($testChannel -eq 'msedge') {
        $env:MEDIA_BROWSER_EXECUTABLE = 'C:\Program Files (x86)\Microsoft\EdgeCore\155.0.4283.45\msedge.exe'
    } elseif ($testChannel -eq 'firefox') {
        $env:MEDIA_BROWSER_EXECUTABLE = 'C:\Program Files\Mozilla Firefox\firefox.exe'
    } else {
        Remove-Item Env:MEDIA_BROWSER_EXECUTABLE -ErrorAction SilentlyContinue
    }
    node scripts/browser-matrix.cjs
    if ($LASTEXITCODE -ne 0) { throw "Échec : $testChannel" }
}
```

Résultats : **36 changements pendant lecture et 12 en pause réussis**, position
et signaux réels vérifiés ; reprise/fin et 404/code 4 réussis. [Matrice chiffrée](STREAMING_COMPATIBILITY.md)
et [méthode/incertitudes](P0C_AUDIO_SWITCHING.md). Les mesures sont locales,
headless, sur huit secondes, pas des preuves haut-parleurs/écran physique/NAS.
Une tentative Chrome intermédiaire échouée est conservée avec son motif ;
la correction attend le contrôle activé en plus de `readyState`.

Tests Rust : **6 réussis**. Le test Range/HEAD/GET couvre désormais les huit
assets. Nouveau contrôle des associations source/langue/index, des deux pistes
des remux et du refus d'une préparation partielle. Les tests invalides, accès
refusés, annulation et fichier temporaire 1 GiB restent présents.
Commandes : `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked`, contrôles `node --check` sur le script de page et les
scénarios/runner. La CI prépare maintenant les remux avant Cargo, mais n'a pas
été exécutée à distance. Suite 10 bits/5.1/sous-titres : **plan uniquement**
dans `NEXT_MEDIA_TESTS.md`.

Régression native Chrome rejouée après P0C : `MEDIA_TEST_SCENARIO=native`,
`MEDIA_BROWSER_CHANNEL=chrome`, `MEDIA_REPORT_DIR=artifacts/native-after-remux`,
puis `node scripts/browser-matrix.cjs` : quatre lectures initiales et erreur 404
réussies. [Rapport](evidence/2026-10-15/remux/native-regression-chrome.json).
Les quatre SHA-256 des dérivés ont aussi été recomparés à leur génération
après les essais : identiques.

## P0C — profil AV1 10 bits, Opus 5.1 et sous-titres

Préparation réellement exécutée, Python 3 standard + FFmpeg/ffprobe 9.0.2 :

```powershell
python scripts/generate-representative-fixtures.py
```

Dossier cible inexistant requis ; refus d'écrasement vérifié. Pas de réécriture
des huit anciens médias. Encodage initial de mire uniquement ; remux suivants
copient AV1/Opus et vérifient chaque paquet/init. Texte SRT→mov_text explicitement
converti. Rapport complet local `representative/generation.json` ; copie compacte
sans les listes volumineuses de paquets dans les preuves du dépôt.

Serveur redémarré : `cargo run --locked -p media-poc`. Chrome lancé seul, puis
Edge/Firefox séquentiellement. Équivalent reproductible des commandes exécutées :

```powershell
$env:NODE_PATH = 'C:\Users\Mael\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\node_modules'
$env:MEDIA_TEST_SCENARIO = 'representative'
foreach ($testChannel in @('chrome','msedge','firefox')) {
    $env:MEDIA_BROWSER_CHANNEL = $testChannel
    $env:MEDIA_REPORT_DIR = "artifacts/representative-$testChannel"
    if ($testChannel -eq 'msedge') {
        $env:MEDIA_BROWSER_EXECUTABLE = 'C:\Program Files (x86)\Microsoft\EdgeCore\155.0.4283.45\msedge.exe'
    } elseif ($testChannel -eq 'firefox') {
        $env:MEDIA_BROWSER_EXECUTABLE = 'C:\Program Files\Mozilla Firefox\firefox.exe'
    } else {
        Remove-Item Env:MEDIA_BROWSER_EXECUTABLE -ErrorAction SilentlyContinue
    }
    node scripts/browser-matrix.cjs
    if ($LASTEXITCODE -ne 0) { throw "Échec : $testChannel" }
}
```

**12 parcours A/V positifs**, 48 marqueurs A/V, 48 seeks, graphes PCM FR6/EN2
conformes avant/après seek ; les vérifications dérivées après seek figurent
aussi dans `summary.json`. **Sous-titres non validés** : trois pistes dans les
fichiers, zéro piste texte native exposée partout. Sélection audio interne
indisponible ; EN lu par fichier dérivé. Destination audio deux canaux maximum,
pas de preuve physique 5.1/10 bits. Méthode et tolérances dans
`P0C_REPRESENTATIVE_MEDIA.md`, valeurs observées dans la matrice.

`cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked` : **7 tests réussis**. Nouveau test profil/pistes/langues/
forced, MP4 représentatif tronqué et dossier partiel. Range/HEAD/GET couvrent
les 12 assets. Sous Windows, un premier test a rencontré le binaire en cours
(accès refusé) ; arrêt du seul prototype puis relance complète réussie.
`node --check` sur page/runner/scénario et parsing AST du générateur Python réussis.
Régressions Chrome des scénarios `native` et `remux` rejouées, rapports archivés.
CI mise à jour mais non exécutée à distance. Aucun test MSE/Video.js/Shaka ajouté.

Contrôle texte supplémentaire exécuté : `python scripts/representative_subtitles.py`.
12 pistes intégrées extraites via FFmpeg sans mapper de vidéo/audio, 48 cues
comparés (texte exact et temps à ±20 ms), tous corrects. Aucun média modifié.
Rapport distinct archivé ; cette vérification fait aussi partie des futures
générations fraîches. Ce n'est pas un succès de rendu de sous-titres navigateur.

## MSE P0C limité

Préparation : python scripts/prepare-mse.py (dossier neuf uniquement), puis
cargo run --locked -p media-poc. Test : node scripts/browser-mse.cjs, avec
MEDIA_BROWSER_CHANNEL et MEDIA_BROWSER_EXECUTABLE/REPORT_DIR appropriés.
[Commandes complètes et méthode](P0C_MSE_EXPERIMENT.md).
8 tests Rust, dont whitelist MSE, MIME, octets Range et416/Origin ; navigateurs
18 commutations, FFT six/deux voies, pause, seek EN arrière/avant, source/blob
et SourceBuffer vidéo invariants, pixels, cues externes/captures. Géométrie des
canaux mono/7.1, CPU/RSS et physique restent NT. Video.js/Shaka séparés NT.
La CI génère le packaging, sans prétendre exécuter ces tests Windows.

## Comparaison Shaka/MSE P0C

Préparer python scripts/prepare-dash.py et python scripts/prepare-shaka.py,
puis cargo run --locked -p media-poc. Scénario commun
node scripts/browser-player-comparison.cjs, MEDIA_PLAYER_ENGINE=mse ou shaka,
MEDIA_BROWSER_CHANNEL=chrome/msedge/firefox. Dossier neuf via MEDIA_REPORT_DIR.
[Commandes, tolérances et résultats](P0C_SHAKA_EXPERIMENT.md).
108 changements, 36 marqueurs±200ms, choix manuels conservés et WebVTT/off.
9tests Rust et tests JS des statistiques réussis ; CI inclut packaging DASH et
statistiques sans revendiquer une exécution distante ou des navigateurs Windows.
Sampler Windows optionnel : MEDIA_SAMPLE_RESOURCES=1 et MEDIA_SERVER_PID,
ressources Rust/navigateur séparées ; overhead et limites conservés dans JSONL.
P95 par rang supérieur, n12 en lecture donc P95=max, pas de promesse statistique.
RMS nouveau sur les derniers1024 du tableau8192 ; chiffres historiques non
comparables directement. Mono/7.1, physique, réseau lent et films longs NT.

## Matrice safeMargin Shaka5.2.13

Protocole fixé dans P0C_SAFE_MARGIN_EXPERIMENT.md : 0/.5/1/2/4,
20essais en lecture et4par catégorie pause/après-seek, par marge et navigateur.
Un changement mesuré par présentation12s indépendante, aucun reload pendant
la mesure ; réinitialisation entre essais explicitement distinguée.
500ms d'observation après signal, FFT/RMS commun, deltas droppedVideoFrames
et totalVideoFrames, marqueur A/V±200ms, seekarrière avec signal réel.
node scripts/browser-safe-margin.cjs ; node scripts/summarize-safe-margin.cjs.
Le nouveau test node scripts/safe-margin-stats.test.cjs vérifie comptes d'échecs,
groupement par sens et mode, frames et régressions seek ; CI adaptée.
Les anciennes suites restent présentes et doivent être rejouées séparément.


### Non-régression seek Shaka

node --test scripts/audio-seek-policy.test.cjs : mocks état/pause/vitesse/position, purge unique, concurrence/récursion, timeout terminal, watchdog blocage/pause, distinction vraie nouvelle piste marge0 et resélection active. Ces mocks ne prouvent pas la lecture.
node scripts/browser-seek-policy.cjs : contrôles appariés FR/EN, positions0,5/1,99/2,01, PCM représentatif, requêtes HTTP/IDs, pause/vitesse, marqueur A/V ; aucun seuil des anciennes suites modifié. Séries complètes actuellement en échec, Firefox bloqué au démarrage automatisé. Protocole et résultats : P0C_AUDIO_SEEK_RECOVERY.md.

## Audio AV1/Opus — 22 octobre 2026

Audit et corrections de fiabilité P0 du lecteur :
[PLAYER_PERFORMANCE_P0.md](PLAYER_PERFORMANCE_P0.md). Comprend refus DTS selon
capacités réelles Chrome/Edge, restauration audio sans pause terminale de l'UI,
races de seek MSE, diagnostics HTTP et rapports avant/après avec limites explicites.

Le parcours MP4 original + audio MSE segmenté est décrit dans
[NAS_PLAYBACK_DIAGNOSTICS.md](NAS_PLAYBACK_DIAGNOSTICS.md) : petite fixture et gros
MP4, décodage FR/EN, limites Docker, vrai Nginx, pause/seek/annulation, première
image et audio prêt. Le job navigateur CI exécute aussi ce scénario sur la
fixture dual-5.1, avec rapport obligatoire et artefact archivé.

Voir [AV1_OPUS_AUDIO_FIX.md](AV1_OPUS_AUDIO_FIX.md) pour génération légale du vrai
MP4 1080p AV1 10 bits avec deux Opus 5.1, tests de copie sans perte, scripts
Chrome/Edge et parcours Core avec préférences restaurées. Les tests unitaires
de sélection sont inclus dans `npm test --prefix apps/web`.

## Administration complète — 19 octobre 2026

Vérifications ciblées : `cargo test --locked -p media-poc --test admin_users --test admin_access --test admin_backups --test backup_bundle --test scheduler --test preferences`. Comptes, rôle/session/CSRF, protection du dernier administrateur, groupes/droits vivants, clés expirables et révocables, scans planifiés, archivage chiffré et restauration réelle SQLite. Les tests du paquet couvrent altération, troncature, permutation, clé incorrecte, base vide, destinations existantes et certificats.

PostgreSQL réel : `pwsh -NoProfile -File scripts/test-admin-postgres.ps1` — base18 jetable, vrais pg_dump/pg_restore, paquet chiffré, extraction, restauration dans une base vide, comparaison de toutes les tables et des paramètres. Les clients du conteneur sont invoqués via un pont de test, sans identifiants dans les arguments.

Navigateur : build Web puis `cargo test --locked -p media-poc --test browser_admin -- --ignored --nocapture`, avec MEDIA_BACKUP_KEY de test unique dans l'environnement du processus. Vrai Core/SQLite jetable, création compte/groupe, planification/scan, création/révocation clé, sauvegarde/téléchargement et planification, largeur mobile et erreurs JS. Ce scénario ne cible aucun média personnel. `test-admin-ui.mjs` reste un contrôle séparé avec API simulée.

Les images applicatives complètes, le NAS et la charge ne sont pas validés par ces essais. La cible Docker `backup-runtime` seule a été construite ; pg_dump/pg_restore18.6 exécutés avec succès.

> Jalon OIDC (18 octobre 2026) : Code + PKCE S256, découverte/JWKS, identités
> issuer/subject et sessions Core communes aux deux modes Docker.
> Contrat, configuration et limites : [OIDC](OIDC.md). Les exclusions OIDC
> dans les jalons historiques ci-dessous ne décrivent plus ce parcours.

# Vérification affiches Films — 16 octobre 2026

## Extension P3/P4 — 18 octobre 2026

Depuis la racine :

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm test --prefix apps/web
npm run build --prefix apps/web
cargo test --locked -p media-poc --test browser_scanner --test browser_series -- --ignored --nocapture
./scripts/test-postgres.ps1
./scripts/raw-poc.ps1
```

Le scénario `browser_scanner` prépare lui-même ses sidecars synthétiques,
exerce MP4 + `.opus` avant redémarrage puis MKV + `.ogg` après redémarrage,
avec SRT forcé et WebVTT, signal PCM externe, seek/pause, reprise et vu/non vu.
`browser_series` vérifie l'épisode suivant, l'opt-in automatique inter-saisons,
le placement mobile des commandes et les refus d'accès/changements de compte.
Les tests HTTP communs SQLite/PostgreSQL couvrent CSRF, conflit de révision,
isolation utilisateur, seconde session, révocation et Range des sous-titres.
Le test distribué vérifie aussi les vrais octets Ogg servis par un Node distant.
Les limites navigateur et raw restent celles de [P3/P4](P3_P4_FEATURES.md).

## Vérifications historiques

- Validation finale : cargo fmt --check ; cargo clippy --locked --all-features
  --all-targets -- -D warnings ; cargo test --locked --all-features réussis :
  50 tests réussis,5 ignorés (4 navigateurs explicites et helper subprocess).
  Inclut migrations0004→0005→0006 avec IDs/metadata conservés, priorités/formats,
  absences/corruptions/extensions trompeuses/oversize/jonction Windows, cache
  images, modifications/suppressions sans probe, droits GET/HEAD de2 bibliothèques,
  désactivation/grants/révocation de session, graphes/NFO/originaux inchangés.
- Web : npm run check (0 erreur/avertissement), npm run lint, npm test (13/13),
  npm run build réussis. Chrome explicite : cargo test --locked --test
  browser_posters -- --ignored --nocapture,1/1 réussi en39,51s ; Chrome155.0.8059.40,
  Windows10.0.26300 headless,4 phases initial/restart/update/delete, vrais arrêts
  et relances Core avec même SQLite. Nouveaux pixels/dimensions après rescan,
  Accueil/Films/recherche/fiche, lazy/repli après erreur, MIME/nosniff/no-store,
  refus anonyme GET/HEAD, NFO échappé, lectureMP4/pause/seek4s et aucune erreur
  JS/CSP. Preuves : docs/evidence/2026-10-16/posters/ ; captures initial/delete
  inspectées visuellement. Originaux vidéo/NFO/affiche comparés, seules mutations
  explicites des copies jetables par tests. Aucun média personnel connecté.
- Essais initiaux corrigés : verrou Windows sur exécutable lors de compilations
  concurrentes ; attente de scan navigateur remplacée par polling API ; Clippy
  if imbriqué simplifié. Validation centrale finale sérialisée et réussie.
- Pas de preuve NAS/LAN/charge/CI distante ni nouveau navigateur. Prochain petit
  jalon recommandé : durcir les ouvertures de fichiers contre TOCTOU avant toute
  exposition à un stockage modifiable par un acteur hostile. Séries/backdrops/
  pistes externes/OIDC/multi-Node/PostgreSQL/Docker restent hors périmètre.

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

## P1 — intégration SvelteKit

Voir P1_WEB_INTEGRATION.md pour préparation, commandes et résultats observés.
10 tests Rust (9 P0 conservés + contrat démo) ; npm check/lint/test/build/audit.
Runner scripts/browser-p1.cjs : vrais contrôles UI + pixels/PCM représentatif,
FR/EN pause/lecture, position/volume/vitesse, seeks, VTT, fin, erreurs.
Runner scripts/browser-p1-layout.cjs : viewports390/1280, clavier/CSP, fautes
Core/audio/VTT et tentative audio unique. Pas de comparaison de lecteurs.

### Première refonte visuelle P1

Même runner P1, Chrome/Edge à 1,5× et Firefox via MEDIA_TEST_SPEED=1.
Layout enrichi :360/390/768/1280/1440/1920,  images/CSP/clavier, carte MP4.
Résultats et limites : UI_DIRECTION.md ; preuves evidence/2026-10-15/p1-ui-v1/.

### Lecteur immersif P1

Scénarios browser-immersive.cjs et browser-immersive-firefox.cjs : plein écran réel,
repli refusé, menus, clavier, tactile simulé, masquage et nettoyage. Les runners P1
existants pilotent désormais les contrôles superposés ; leurs assertions média
sont conservées. Commandes et limites : IMMERSIVE_PLAYER.md.

## Premier catalogue P1 — validation locale du 15 octobre 2026

`cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` et
`cargo test --locked` réussissent : 15 tests Rust, nouveaux contrats et anciens
tests conservés. Svelte/TypeScript check 0 erreur/0 avertissement, lint, 8 tests
Vitest et build passent après les ajustements UI. `browser-p1.cjs` adapté à la
fiche du film réussit les deux parcours MKV/MP4 sous Chrome 155.0.8059.40,
avec pixels/PCM et hashes vérifiés ; assertions média conservées.

Les scripts P1/immersive utilisent maintenant
`/app/films/?work=fixture-film`, attendent les choix de version et conservent
pixels/PCM, FR/EN, pause/seek, WebVTT, erreurs et hashes. Le contrôle de l'ancien
décor de présentation est remplacé par celui des versions disponibles.
`browser-catalog.cjs` réussit sous Windows Chrome headless aux viewports
390/768/1440 px : navigation et accès directs, fiches, série/saison/épisode puis
lecture MP4, recherche vide, catalogue vide injecté, erreur 503 puis réessai,
fiche 404, pagination simulée de 72 œuvres avec 24 cartes maximum.
État de chargement et refus d'afficher un épisode comme film via `work`
détourné vérifiés dans la dernière relance. Captures
Accueil/Films/Séries desktop/mobile inspectées. `browser-p1-layout.cjs` passe
360/390/768/1280/1440/1920 px sans débordement/CSP ; audio404 (une tentative),
VTT404 et Core503 explicites. `browser-immersive.cjs` passe plein écran réel et
refus simulé, menus/clavier et tactile simulé390×844/844×390.
Preuves `evidence/2026-10-15/catalogue/` (report.json, regression, layout,
immersive). Détails dans CATALOGUE_P1.md ; aucun nouveau résultat Edge/Firefox.

Pas de relance des expériences codecs P0. Authentification/SQLite/scanner et
bibliothèques personnelles restent absents ; aucun test de fixture ne prouve
leur sécurité, ni la fluidité à grande échelle ou sur mobile physique.


## Navigation et menu profil — vérification du 15 octobre 2026

Commandes exécutées depuis la racine : npm run check --prefix apps/web,
npm run lint --prefix apps/web, npm test --prefix apps/web,
npm run build --prefix apps/web. Résultats : check0/0, lint/build réussis,
8 tests Vitest passent. Backend inchangé : pas de relance Rust pour cet ajustement.

Avec NODE_PATH pointant sur apps/web/node_modules et serveur local actif :
node scripts/browser-profile.cjs ; puis browser-catalog.cjs et
browser-immersive.cjs avec MEDIA_REPORT_DIR distincts. Les trois passent.
Chrome155.0.8059.40 / Windows10.0.26300 headless : profil en360/390/768/1440px,
4liens principaux sans Paramètres, loupe SVG, Profil désactivé, Paramètres
fonctionnel, Entrée/Espace/Tab/Échap, focus restauré, second clic sur avatar,
clic extérieur et tactile simulé. Menu contenu dans la fenêtre, aucune erreur
JS/CSP ni débordement ; captures après fin d'animation inspectées.

Non-régression catalogue (390/768/1440px) et lecteur immersif (fullscreen,
menus, clavier, audio/texte/source, retour/nettoyage, tactile simulé) réussie.
Preuves : evidence/2026-10-15/profile-navigation/ et sous-dossiers catalogue/
et immersive/. Aucun nouveau résultat Edge/Firefox/Safari/mobile physique.

## Identité SQLite et bootstrap local — 16 octobre 2026

`cargo test --locked` : 22 tests réussis, un helper de processus ignoré par
le harness mais lancé par competing_processes. Les six tests identité vérifient
concurrence de migrations et bootstrap entre pools/processus, PHC Argon2id,
contraintes SQL, réouverture, fermeture durable, rollback, erreurs et CLI sans TTY.
Formatage/Clippy strict réussis. Détails/limites : [LOCAL_ADMIN.md](LOCAL_ADMIN.md).
Pas de nouveau test navigateur ni de saisie interactive manuelle revendiquée.

## Connexion, sessions et permissions — 16 octobre 2026

Commandes et périmètre : [LOCAL_SESSIONS.md](LOCAL_SESSIONS.md). Les tests auth
vérifient connexion valide/invalide, réponse identique compte absent/mauvais
secret, hash de session seul en DB, sessions différentes, réouverture, expirations
idle/absolue, révocation, désactivation, CSRF et limite durable de tentatives.
Le budget Argon2 concurrent et les cookies HTTP/HTTPS ont des assertions dédiées.

`tests/permissions.rs` couvre 12 chemins protégés × GET/HEAD × Range absent,
valide ou invalide : 401 anonyme/cookie invalide, 403 rôle user, 401 désactivation.
L'administrateur obtient 200/206/416 selon la requête. Un test HTTPS configuré
vérifie Host/Origin et Secure, sans prétendre déployer un serveur TLS réel.
`tests/core_restart.rs` arrête/redémarre réellement le binaire avec une base
jetable, puis confirme la persistance de la session et de sa révocation.

`cargo test --locked --all-features` inclut le banc historique P0 via la feature
`fixture-bench`, désormais explicite et jamais sélectionnée par le binaire.
Le test navigateur ignoré par défaut est lancé séparément :
`cargo test --locked --test browser_auth -- --ignored --nocapture`.
Chrome155.0.8059.40, Windows10.0.26300 headless, 390/1440px : connexion invalide
puis valide, cookie HttpOnly/Strict, absence de jeton localStorage, identité,
Administration, catalogue, lecture MP4/seek, rechargement, déconnexion, 401
catalogue/Range, aucune erreur JS/CSP. Captures de connexion inspectées.
Preuve : [rapport](evidence/2026-10-16/auth/report.json).
Pas de nouvelle validation audio PCM, Firefox/Edge/Safari, HTTPS navigateur,
NAS, charge ou bibliothèque réelle dans ce jalon.

## Bibliothèques et droits P1

`cargo test --locked --all-features` : migrations/CRUD, persistance et grants,
HTTP session/rôle/CSRF, rollback, racines canoniques et junction extérieure,
filtrage recherche, fiches Films/Séries, médias et sous-titres GET/HEAD/Range,
désactivation/révocation. core_restart arrête et relance de vrais processus :
configuration désactivée conservée et contenu refusé après redémarrage.
`cargo test --locked --test browser_auth -- --ignored --nocapture` utilise
SQLite temporaire et Chrome : login, création/modification/désactivation Web,
responsive 390/1440, catalogue, lecture MP4/seek, logout, CSP.
Aucun NAS ni ingestion/scanner ; pas de nouvelle preuve audio/codec.

## Scanner Films local P1 — 16 octobre 2026

- `cargo fmt --check` : réussi.
- `cargo clippy --locked --all-features --all-targets -- -D warnings` : réussi.
- `cargo test --locked --all-features` :38 réussis,3 ignorés par harness.
  Les2 tests navigateurs sont exécutés séparément ; le helper bootstrap ignoré
  est exercé par competing_processes. Aucun échec dans la passe finale.
- `npm run check` :0 erreur/avertissement ; `npm run lint`, `npm test` :13/13,
  `npm run build` : réussis depuis apps/web.
- `cargo test --locked --test browser_scanner -- --ignored --nocapture` :
  parcours réel décrit dans [LOCAL_FILM_SCANNER.md](LOCAL_FILM_SCANNER.md),
  scan initial puis0 probe/2 réutilisés, une carte/deux versions, lecture MP4,
  pause/seek4s et vrai redémarrage du binaire même base. Chrome155 Windows headless.
  Preuves [rapport initial](evidence/2026-10-16/scanner/report.json) et
  [après redémarrage](evidence/2026-10-16/scanner/report-restart.json).
- `cargo test --locked --test browser_auth -- --ignored --nocapture` :1/1 réussi,
  non-régression session/administration/lecteur aux largeurs390/1440, aucun JS/CSP.
- scanner_flow : MKV AV1 10bits/Opus FR5.1+EN2/sous-titres internes, MP4 AV1/Opus,
  corruption/temporaire/sans NFO, versions, IDs/cache/reconstruction Core,
  deux bibliothèques/droits et révocation/désactivation GET/HEAD/Range, hashes.
  Tests Node : refus de jonction Windows réelle et de liens, parcours annulé,
  entrée relative invalide, conflits de noms/ID. Tests inventory : réouverture,
  disparition, parcours incomplet, racine modifiée/réassociation et cache invalidé.
  Tests jobs : doublon409 déterministe, shutdown, job running→error au restart.
- Originaux conservés ; aucune preuve de charge, PCM/audio physique ou nouveau
  résultat MKV/Edge/Firefox/Safari/NAS. FFprobe ne garantit pas le Direct Play.

## NFO Films locaux — 16 octobre 2026

- `cargo fmt --check`, `cargo clippy --locked --all-features --all-targets -- -D warnings` : réussis.
- `cargo test --locked --all-features` :45 réussis,4 ignorés par harness
  (3 navigateurs explicites et helper bootstrap exercé par competing_processes).
  Régressions scanner/lecteur/permissions conservées et exécutées.
- Parser/discovery : métadonnées réelles, NFO exact/partagé/absent, UTF8/BOM,
  XML malformé, encodage invalide, XXE/DTD/expansion/reférences interdites,
  taille/profondeur/champs/genres bornés, HTML hostile comme texte, jonction Windows.
- Persistance et parcours HTTP : upgrade0004 sans perte/IDs, candidat éditorial
  déterministe, update/delete/invalid snapshot/racine modifiée, deux versions,
  aucun probe vidéo inutile, conflits TMDb/IMDb sans fusion, droits et hashes.
- Web `npm run check` :0 erreur/avertissement, `npm run lint`, `npm test` :13/13,
  `npm run build` : réussis.
- `cargo test --locked --test browser_nfo -- --ignored --nocapture` :1/1 réussi,
  quatre phases initial/restart/update/delete avec vrais processus Core même DB.
  Chrome155.0.8059.40 Windows10.0.26300 headless1440px : Accueil/Films/Recherche/
  fiche, titre/année/genres/synopsis, HTML échappé, MP4/pause/seek4s après restart,
  aucun nouveau probe lors des rescans éditoriaux, aucune erreur JS/CSP.
  [Preuve après restart](evidence/2026-10-16/nfo/report-restart.json).
- Migration0004 et13 anciens fichiers de preuve inchangés (SHA256). Mutations NFO
  du test sur copies jetables explicites ; comparaisons sources avant/après.
  Pas de nouvelle preuve mobile/autres navigateurs/PCM/NAS/charge.
  Contrat réalisé et limites : [LOCAL_FILM_NFO.md](LOCAL_FILM_NFO.md).

## Fiches, demi-étoiles et réactivité — 20 octobre 2026

- `series_flow` : lecture/écriture de notes 4,5, refus des quarts d'étoile et valeurs hors limites, isolation des comptes, réouverture SQLite ; affiches de saison et vignette exacte d'épisode ; image conditionnelle 304 vide pour un compte autorisé, 403 pour un compte sans droit.
- `media-node --lib` : image conditionnelle sans corps, validation des changements du fichier conservée ; Range/HEAD et confinement existants.
- `native.test.ts` : langue préférée chargée directement, une seule ouverture vidéo, sans chargement FR intermédiaire.
- `browser_series` explicite : cœur et note 4,5 persistés après rechargement, absence du bandeau de retour, images saison/épisode décodées, thèmes clair/sombre aux largeurs 390/1440, lecture/pause/seek et transitions existantes. `episodeLaunchMs` mesure le délai clic → progression vidéo sur fixture locale, sans comparaison NAS ni mesure audio physique.
- Migration 0030 SQLite : vérification en mémoire de conservation note/favori, demi-étoile, refus d'un quart et cascade de suppression. La migration PostgreSQL est fournie ; son exécution réelle exige un serveur disponible.

## Extensions locales — parcours synthétique vérifié

Après build Web et compilation de la fixture SDK pour wasm32-unknown-unknown :
`cargo test --locked -p media-poc --test extensions -j 1 -- --include-ignored --test-threads=1 --nocapture`.
Inclut Chrome réel, SDK compilé, signatures, limites runtime, consentements,
révocation pendant exécution et cycle d'installation/désinstallation.
Résultats et frontières : [evidence/2026-10-20/extensions/README.md](evidence/2026-10-20/extensions/README.md).
# Lot produit du 20 octobre 2026

Contrats et limites : [PRODUCT_FEATURES.md](PRODUCT_FEATURES.md).

- `cargo test --locked -p media-poc --lib product_tests -j 1 -- --test-threads=1` :
  migration de la version 31 avec conservation bibliothèques/droits/progression,
  scan musical réel et UUID stables, Range, restrictions/HEAD/ressources dérivées,
  filtres avant pagination, affiches, listes privées, groupes, partage de progression
  et de favoris, préférences sur deux sessions, isolation des comptes et clés API.
- `cargo test --locked -p media-poc --test account -j 1 -- --test-threads=1` :
  secret actuel, CSRF, changement de mot de passe et révocation des cookies existants.
- `cargo test --locked -p media-node --lib -j 1 -- --test-threads=1` : arrêt brutal
  d’un sous-processus pendant une écriture cache, récupération et préservation d’une
  entrée active, en plus des tests de confinement, probes et copie de paquets.
- `distributed_multi_node` et `node_enrollment_and_relay` : deux Nodes locaux,
  scan/stream musical distant, préparation distante et HEAD/Range/416 en mode
  entrant ou sortant, révocation et perte/reprise d’un Node.
- `browser_series` ignoré par défaut : Chrome réel headless, chapitres muxés depuis
  une fixture légale, décodage vidéo, seek/pause, navigation série et comptes,
  changement personnel de mot de passe et révocation d’un autre contexte navigateur.
- Web : `npm run check`, `npm run lint`, `npm test`, `npm run build`,
  `npx tsc -p src/service-worker/tsconfig.json --noEmit`, `node scripts/test-pwa.mjs`.
  Le dernier utilise un serveur de fixture HTTP. Depuis le 21 octobre, il vérifie
  le téléchargement natif exact (nom/octet, sans copie IndexedDB), puis les copies
  hors ligne antérieures, la clé d'appareil, la récupération des anciennes copies
  protégées par phrase, Opus PCM/440 Hz, seek/Range, suppression et rendu mobile.

## Miniatures, simplification média et dépôts — 21 octobre 2026

- `cargo test --release --locked -p media-node --lib image_preview -j 1 -- --test-threads=1 --nocapture` :
  poids/dimensions, cache invalidé après changement, transparence, conditions et Range.
- `posters_flow` : largeurs 320/640/1280 sur GET/HEAD autorisés, mauvaise largeur,
  Range/416, fichier modifié et permissions/session révoquées. `distributed_multi_node`
  compare les mêmes miniatures en accès direct signé et via relais Core.
- `browser_posters -- --ignored --test-threads=1` : cartes réellement décodées avec
  `width=320`, absence de téléchargement du fond décoratif de 2 Mo, rayons indépendants,
  cache et modifications/retrait de l'affiche. Fixture minuscule, sans preuve de débit NAS.
- `browser_series -- --ignored --test-threads=1` : clics de notes rapprochés pendant
  une écriture retenue, dernier choix persistant, listes via +, version/audio uniques
  masqués, multi-version conservée, vidéo décodée, pause/seek et transitions.
- `cargo test --locked -p media-poc --lib extensions::repositories -j 1 -- --test-threads=1` :
  formats, adresses privées, bornes, droits/CSRF, signatures et snapshots des dépôts.
- `cargo test --locked -p media-poc --lib browser_repository_catalogue_version_trust_and_install -j 1 -- --ignored --nocapture --test-threads=1` :
  vrai Core/SQLite et navigateur, choix de deux versions, approbation explicite,
  installation désactivée, actualisation, persistance et retrait. Les documents
  HTTPS sont substitués uniquement sous `cfg(test)` ; ce n'est pas un essai d'hébergement public.



État des médias indépendants, manifeste d’association versionné et limites raw :
[SEPARATED_MEDIA.md](SEPARATED_MEDIA.md).

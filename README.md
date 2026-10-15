# Aether — banc P0 AV1 / Opus

Premier jalon de développement, 15 octobre 2026. Serveur Rust/Axum local et
fixtures synthétiques uniquement. P0A/P0B restent partiels ; aucun produit
standalone, compte, catalogue, déploiement NAS ou moteur de lecture définitif.

Prérequis testés : Windows x64, Rust/Cargo 1.96.1, FFmpeg/ffprobe 9.0.2
avec libaom-av1/libopus, PowerShell 7, Chrome 155.0.8059.40.
Les versions Rust sont verrouillées dans `Cargo.lock`. FFmpeg et ffprobe sont
des outils temporaires de banc P0, pas une décision de dépendance du produit.
Le serveur appelle ffprobe au démarrage seulement, jamais pendant la lecture.

Depuis la racine du dépôt, PowerShell :

```powershell
./scripts/generate-fixtures.ps1
./scripts/prepare-language-remux.ps1
python scripts/generate-representative-fixtures.py
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --locked -p media-poc
```

Ouvrir [le banc local](http://127.0.0.1:3000), choisir une source, charger,
puis lancer la lecture. Arrêt : Ctrl+C. Le script de génération refuse un dossier
existant ; ne pas le relancer sur des médias personnels. Pour une nouvelle
génération, choisir un nouveau dossier avec `-OutputDirectory`, puis définir
`MEDIA_FIXTURE_DIR` avant le lancement du serveur. Les tests Rust utilisent
le dossier standard `tests/fixtures/generated`.

La troisième préparation utilise Python 3 standard, refuse le dossier existant
et ajoute AV1 10 bits, FR Opus 5.1/EN stéréo et trois sous-titres dans MKV/MP4,
plus deux dérivés EN. Aucun média précédent n'est réécrit. Ces quatre sources
sont proposées dans le banc ; le test `MEDIA_TEST_SCENARIO=representative`
vérifie pixels, PCM/canaux, marqueurs A/V et seek. Trois sous-titres sont détectés
mais non exposés par les API natives testées. Physique 5.1/affichage 10 bits NT.
Voir [méthode et résultats](docs/P0C_REPRESENTATIVE_MEDIA.md) et
[comparaison des lecteurs proposée](docs/PLAYER_COMPARISON_EXPERIMENT.md).

Test navigateur automatisé, dans un second terminal PowerShell :

```powershell
npm install --no-save --package-lock=false playwright@1.62.1
node scripts/browser-smoke.cjs
```

Chrome doit être installé. `browser-smoke.cjs` appelle désormais
`browser-matrix.cjs`, avec un scénario commun Chrome/Edge/Firefox.
Aucun service Node.js permanent.
`MEDIA_PORT` configure le port du banc (3000 par défaut) ;
`MEDIA_TEST_URL` configure l'URL du test. `MEDIA_REPORT_DIR` change la destination
(par défaut `artifacts/browser-{channel}`). `MEDIA_BROWSER_CHANNEL=msedge` ou
`firefox` sélectionne le client ; `MEDIA_BROWSER_EXECUTABLE` désigne un binaire
précis. Firefox installé nécessite geckodriver : [commandes testées](docs/TEST_PLAN.md).
`MEDIA_GECKODRIVER` change son chemin et `MEDIA_WEBDRIVER_PORT` son port local
(4444 par défaut). Aucun profil personnel réutilisé.

Chrome, Edge et Firefox ont lu les quatre sources headless. La commutation audio
native reste non validée : `audioTracks` absent avec les réglages par défaut.
Voir la [procédure humaine](docs/MANUAL_BROWSER_CHECKS.md).

P0C : le second script prépare les remux FR/EN sans réencodage, dans un nouveau
dossier, sans toucher aux originaux et en refusant tout écrasement. Redémarrer
le serveur si celui-ci était déjà lancé, puis utiliser **Langue par remux préparé**.
Le changement recharge un fichier et restaure position et état lecture/pause ;
ce n'est pas la sélection interne native. Pour le scénario mesuré :

```powershell
$env:MEDIA_TEST_SCENARIO = 'remux'
$env:MEDIA_REPORT_DIR = 'artifacts/remux-chrome'
node scripts/browser-matrix.cjs
```

Reprendre les variables navigateur décrites dans le plan de tests pour Edge et
Firefox, avec un dossier de rapport différent. Pour le scénario natif initial,
utiliser `MEDIA_TEST_SCENARIO=native`. Le prototype reste utilisable sans les
remux ; les tests Rust complets nécessitent les deux étapes de préparation.
Méthode, incertitudes et comparaison ouverte : [expérience P0C](docs/P0C_AUDIO_SWITCHING.md).

Résultats, limites et preuves : [compatibilité](docs/STREAMING_COMPATIBILITY.md),
[tests](docs/TEST_PLAN.md), [sécurité](docs/SECURITY.md), [API](docs/API.md).
Le dossier `docs/` était absent au démarrage : les documents ajoutés ici
décrivent ce jalon et les règles existantes, sans prétendre reconstituer une
spécification historique manquante. Les modifications initiales des documents
racine ont été conservées.

### Première expérience MSE P0C

Après les fixtures représentatives : python scripts/prepare-mse.py, puis
cargo run --locked -p media-poc et ouvrir http://127.0.0.1:3000/mse.
Cliquer Préparer MSE puis lecture ; FR/EN utilisent une vidéo commune.
[Résultats et commandes navigateur](docs/P0C_MSE_EXPERIMENT.md).
Video.js10 et Shaka non encore testés ; aucun choix définitif de lecteur.

### Comparaison Shaka/MSE P0C

Avec les fixtures MSE existantes : python scripts/prepare-dash.py puis
python scripts/prepare-shaka.py (dossiers neufs uniquement). Le second script
prépare Shaka5.2.13 depuis npm avec SHA512 fixé ; --archive accepte l'archive
locale correspondante. Lancer le serveur Rust et ouvrir /shaka ; /mse conservé.
[Mesures, limites, protocole navigateur et CPU/RAM](docs/P0C_SHAKA_EXPERIMENT.md).
Shaka change la langue plus vite sur ce banc mais présente des microcoupures ;
aucun lecteur définitif choisi. Video.js n'est pas inclus dans ce jalon.

## Première application Web P1 (localhost)

Frontend SvelteKit servi par Rust : [instructions et résultats](docs/P1_WEB_INTEGRATION.md).
Après préparation des fixtures, `npm ci --prefix apps/web`,
`npm run build --prefix apps/web`, `cargo run --locked -p media-poc`,
puis http://127.0.0.1:3000/app/. Fixtures uniquement, pas encore d'authentification.

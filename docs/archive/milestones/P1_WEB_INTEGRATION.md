# P1 — Première intégration Web locale (15 octobre 2026)

La décision utilisateur autorise P1 avec HTML5 natif et remux par langue.
Les expériences approfondies Shaka/MSE/Video.js sont suspendues ; les anciens
bancs restent disponibles, leurs résultats historiques et limites restent valables.
P0 n'est pas déclaré exhaustivement terminé et P1 n'est pas le standalone sécurisé.

## Parcours et limites

Svelte 5.57.2, SvelteKit 3.1.0, TypeScript strict, Vite 8.3.4 et adapter-static
4.0.0 : versions vérifiées dans npm, lockfile conservé. Le frontend est compilé
puis servi par Axum sous `/app/`. Node.js sert au build et aux tests seulement.
Configuration SvelteKit 3 dans vite.config.ts, conformément à la documentation
[adapter-static](https://svelte.dev/docs/kit/adapter-static) et au code installé.

Deux médias synthétiques (MKV et MP4) AV1 Main 10 bits SDR, 320×180, 12 secondes.
Chaque langue a sa source mono-audio préparée : FR Opus 5.1, EN Opus stéréo.
Aucun changement implicite de langue, aucune piste AAC, aucun transcodage A/V.
Les pistes et sources viennent du Core ; le composant n'appelle que l'interface
Player (load/play/pause/seek/audio/subtitles/volume/speed/state/destroy).
Le moteur concret NativePlayer est branché au montage ; Shaka/MSE ne sont pas chargés.

Changement audio : pause, URL exacte de la langue, métadonnées, restauration de
position/volume/vitesse, texte manuel conservé, reprise si la lecture était active.
Une pause reste une pause. Opérations concurrentes refusées. Timeout 10 secondes
pour chargement/seek/démarrage, attente bloquée en lecture arrêtée après 10 secondes.
Erreurs terminales visibles ; aucune relance, retour de langue ou boucle automatique.
Le bouton Charger permet une nouvelle tentative explicite (FR, texte désactivé).
Sous-titres FR complet/forced et EN : WebVTT préparés en P0, servis via une route
publique de fixture sans dépendance au moteur MSE. Forced reste manuel.

Le serveur écoute uniquement 127.0.0.1 et conserve Host/Origin/Fetch-Site/CSP,
inventaire fixe et streaming 64 KiB/Range. Aucun compte, bibliothèque personnelle,
SQLite, OIDC, multi-Node ou plugin ajouté. Pas d'exposition via proxy/tunnel/NAS.
Confinement canonique non atomique face à un écrivain hostile : limite P0 conservée.

## Démarrer depuis la racine du dépôt

Les fixtures représentatives P0 et leurs exports WebVTT doivent exister. Pour
un checkout neuf, exécuter une fois les générateurs P0 documentés dans TEST_PLAN.md
(génération initiale, remux linguistique, representative, prepare-mse et prepare-dash).
Ils refusent d'écraser leurs dossiers existants.

```powershell
python scripts/prepare-p1.py
npm ci --prefix apps/web
npm run build --prefix apps/web
cargo run --locked -p media-poc
```

Ouvrir http://127.0.0.1:3000/app/ ; le banc historique reste à `/`, les expériences
à `/mse` et `/shaka`. Relancer uniquement build après changement frontend.
Le serveur doit être lancé depuis la racine. Pas de serveur Node permanent.
Le mode vite dev est un outil frontend ; le parcours validé utilise le build Axum
sur la même origine, sans proxy de développement ni CORS ajouté.

prepare-p1.py accepte uniquement les deux petites fixtures représentatives
(entrée ≤2 MiB chacune), produit quatre copies dans generated/p1, sortie limitée
par FFmpeg à 4 MiB par fichier et vérification agrégée ≤16 MiB. Pas de cache
à croissance automatique. Refus du dossier existant, pas de nettoyage destructif.
Chaque paquet AV1 et Opus est comparé par SHA-256 ; originaux vérifiés avant/après.
Mesure locale : 1 812 240 octets de médias ajoutés, rapport JSON séparé.
Un dossier de préparation incomplet fait échouer le chargement ; s'il est absent,
le catalogue démo vide affiche l'instruction de préparation.

## Tests réellement exécutés

- `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo test --locked` : 10 tests intégration réussis, dont 9 P0 conservés.
  Le nouveau contrat vérifie sources/pistes/langues/canaux, 206/416, VTT,
  méthode interdite, nom inconnu et refus d'origine tierce.
- `npm run check --prefix apps/web` : zéro erreur/avertissement Svelte/TypeScript.
  `npm run lint --prefix apps/web`, `npm test --prefix apps/web` : 5 tests passent.
  Pause/position/volume/vitesse, concurrence, timeout sans retry, blocage terminal
  et annulation au démontage. Ces tests simulés ne prouvent pas le décodage.
- `npm run build --prefix apps/web`, `npm audit --prefix apps/web --audit-level=high` :
  succès, aucune vulnérabilité signalée au moment du test.
- Windows x64 10.0.26300, headless, Chrome 155.0.8059.40 et Edge 155.0.4283.45 :
  MKV/MP4, pixels, six/deux fréquences PCM réelles, FR→EN en pause puis EN→FR
  en lecture, état/position conservés, texte manuel préservé au changement audio,
  trois cues WebVTT, désactivation, seeks arrière/avant EN, fin et erreur404 visibles.
- Layout Chrome à 390/1280 px : absence de débordement, focus clavier, pas d'erreur
  CSP. Pannes injectées : Core503, VTT404, audio404 (une seule tentative, pause).

Commandes navigateur reproductibles (Chrome installé ; Edge/Firefox utilisent
les adaptateurs P0 et leurs chemins d'exécutables documentés dans TEST_PLAN.md) :

```powershell
$env:NODE_PATH = (Resolve-Path apps/web/node_modules).Path
$env:MEDIA_BROWSER_CHANNEL = 'chrome'
$env:MEDIA_REPORT_DIR = 'artifacts/p1-chrome'
node scripts/browser-p1.cjs
node scripts/browser-p1-layout.cjs
```

Le runner mesure le PCM du graphe audio et les pixels, sans écoute humaine,
preuve de sortie physique 5.1/10 bits, film long, NAS, charge ou Safari/mobile réel.
Les captures à 390 px sont une simulation de viewport desktop.
CI adaptée (préparation P1, checks/tests/build Web), aucune exécution GitHub revendiquée.

## Détails CSP vérifiés

Le bootstrap inline de SvelteKit est externalisé au build dans un script local
classique : script-src self, aucun unsafe-inline. Le style inline fixe du div
svelte-announcer est autorisé uniquement par son SHA256 via style-src-attr
unsafe-hashes ; les styles arbitraires restent interdits. Les tests navigateur
contrôlent les violations CSP. Les assets Web générés sont ignorés par Git.

## Prochaine petite étape P1

Créer le premier administrateur local sécurisé avec SQLx/SQLite, bootstrap unique,
hachage robuste, sessions révocables et limites de débit testées. Valider ce jalon
avant d'ouvrir la première bibliothèque en lecture seule avec autorisations Core.
Le prototype actuel ne doit pas être exposé à des médias personnels.

## Première direction visuelle

L'écran initial a été remplacé à la demande utilisateur par la direction
cinématographique décrite dans UI_DIRECTION.md. Logique média conservée,
CSP img-src self pour les images locales. Six parcours MKV/MP4 sur trois
navigateurs après refonte ; Firefox validé à 1× seulement, vitesse FR 5.1à 1,5×
reste ouverte. Six viewports 360–1920 px, cartes et pannes testés.

## Lecteur immersif

Le parcours courant ouvre un dialog plein écran avec contrôles Svelte superposés,
plus de lecteur intégré à la page. Moteur/API/fixtures conservés. Voir IMMERSIVE_PLAYER.md.

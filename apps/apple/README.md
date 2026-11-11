# Aether pour iPhone, iPad et Apple TV

Application SwiftUI native pour iOS/iPadOS 17+ et tvOS 17+ (une cible chacun,
mêmes sources), au-dessus du cœur Rust partagé `crates/client`, exposé à Swift
par UniFFI (`crates/client-ffi`), comme l'app Android.

- Connexion par mot de passe ou par code QR / code approuvé depuis un appareil
  déjà connecté (la voie conseillée sur Apple TV). Sur iPhone et iPad, Réglages
  → « Connecter une TV » approuve le code affiché par une TV.
- Plusieurs comptes sur un même appareil : Réglages → Comptes liste les comptes
  connectés, en ajoute un (le compte actif est mis de côté, pas déconnecté) et
  passe de l'un à l'autre sans se reconnecter ; la page de connexion propose
  les comptes mis de côté.
- Jeton d'appareil dans le Trousseau (lisible après le premier déverrouillage,
  jamais synchronisé), renouvelé au retour dans l'app et chaque jour en tâche de
  fond (`BGAppRefreshTask`).
- Accueil : « Continuer » (positions commencées, `GET /api/v1/progress`) puis une
  rangée par bibliothèque ; onglets Bibliothèques (grille paginée), Rechercher,
  Téléchargements (iOS), Appareils et Réglages.
- Fiche : affiche, synopsis, Lire ou « Reprendre à … », saisons et épisodes.
- Lecture du HLS du Core ([docs/HLS.md](../../docs/HLS.md)) par
  `AVPlayerViewController` : AirPlay, Picture in Picture (y compris au passage en
  arrière-plan), pistes audio et sous-titres WebVTT de la playlist maître dans le
  menu du lecteur, titre dans le centre « À l'écoute ». Reprise à la position
  enregistrée, sauvegarde toutes les 15 s, à la pause et à la sortie ; vu à 95 %.
- Téléchargements hors ligne (iOS et iPadOS) par `AVAssetDownloadURLSession` en
  arrière-plan : qualité d'origine, toutes les pistes audio et tous les
  sous-titres. Une copie terminée se lit sans réseau. Un téléchargement qui échoue
  (souvent l'URL HLS expirée après 12 h) repart une fois avec une nouvelle URL.
- Télécommande et reprise entre appareils (`/api/v1/remote/ws`, `kind` `tv` sur
  Apple TV, `phone` sur iPhone et iPad) : l'onglet Appareils pilote les autres
  appareils ou « Reprend ici » ; les commandes reçues (lecture, pause, avance,
  arrêt, ouverture d'un titre) agissent sur le lecteur. Reçues app ouverte.
- Apple TV : navigation au focus de la Siri Remote (cartes `.card` qui se
  soulèvent, rangées horizontales), bouton Menu pour quitter le lecteur.

## Construire (macOS, Xcode 16+)

Depuis la racine du dépôt :

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-tvos aarch64-apple-tvos-sim
sh apps/apple/scripts/build-xcframework.sh   # Generated/AetherFFI.xcframework + aether_ffi.swift
brew install xcodegen
(cd apps/apple && xcodegen generate)         # Aether.xcodeproj (non versionné)
open apps/apple/Aether.xcodeproj             # schémas Aether (iOS) et AetherTV (tvOS)
```

Les simulateurs sont ceux des Mac Apple silicon (tranches arm64 seulement).
Pour installer sur un appareil, choisir son équipe dans Signing & Capabilities.

Le workflow [`.github/workflows/apple.yml`](../../.github/workflows/apple.yml)
fait la même chose sur un runner macOS et compile les deux apps sans signature
(`CODE_SIGNING_ALLOWED=NO`, appareils et simulateurs) ; les `.app` non signées
sont publiées comme artefact.

Sans Mac, la partie Rust se vérifie dans Docker (bibliothèques statiques des
quatre cibles Apple, sans édition de liens) :

```sh
docker build -t aether-apple-check apps/apple/docker
docker run --rm -v "$PWD:/work" -w /work \
  -v aether-cargo:/usr/local/cargo/registry -v aether-target-apple:/work/target \
  aether-apple-check sh apps/apple/scripts/check-rust-linux.sh
```

## Limites actuelles

Pas d'icône ni d'image Top Shelf (obligatoires pour l'App Store). Pas de
téléchargements sur Apple TV (tvOS n'a pas `AVAssetDownloadURLSession`). La
position d'une lecture hors ligne n'est envoyée au Core que si le réseau est là.
Un Core en certificat auto-signé n'est pas accepté ; utiliser un vrai certificat
ou HTTP sur le réseau local.

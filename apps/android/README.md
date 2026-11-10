# Aether pour Android

Application native (Kotlin, Compose, Media3) au-dessus du cœur Rust partagé
`crates/client`, exposé à Kotlin par UniFFI (`crates/client-ffi`).

- Connexion par mot de passe ou par code QR approuvé depuis un autre appareil.
- Jeton d'appareil chiffré par une clé du Keystore Android, renouvelé au démarrage.
- Catalogue films et séries avec recherche.
- Lecture HLS par copie, décodée par le matériel ([docs/HLS.md](../../docs/HLS.md)),
  reprise à la position enregistrée et sauvegarde toutes les 15 s.

## Construire

Depuis la racine du dépôt, avec Docker (la construction de l'image accepte la
licence du SDK Android) :

```sh
docker build -t aether-android apps/android/docker
docker run --rm -v "$PWD:/work" -w /work \
  -v aether-cargo:/usr/local/cargo/registry -v aether-target-android:/work/target \
  -v aether-gradle:/gradle-cache aether-android sh apps/android/scripts/build.sh
```

L'APK de debug est produit dans `app/build/outputs/apk/debug/app-debug.apk`
(arm64-v8a et x86_64). Installation : `adb install -r app-debug.apk`.

## Limites actuelles

Pas encore de téléchargements hors ligne, d'affiches, de Chromecast, de choix
d'épisode (une série lance son premier épisode) ni de renouvellement du jeton
en tâche de fond. Un Core en certificat auto-signé n'est pas accepté ; utiliser
un vrai certificat ou HTTP sur le réseau local.

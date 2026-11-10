# Aether pour Android

Application native (Kotlin, Compose, Media3) pour téléphones, tablettes, Android
TV, Google TV et Fire TV (même APK), au-dessus du cœur Rust partagé
`crates/client`, exposé à Kotlin par UniFFI (`crates/client-ffi`).

- Connexion par mot de passe ou par code QR approuvé depuis un autre appareil.
- Jeton d'appareil chiffré par une clé du Keystore Android, renouvelé au démarrage
  et chaque jour en tâche de fond (WorkManager), même si l'app n'est pas ouverte.
- Sur TV : lanceur et bannière de l'écran d'accueil, navigation à la télécommande
  avec un contour sur l'élément sélectionné ; connexion conseillée par code QR.
- Catalogue films et séries avec affiches et recherche ; écran série avec saisons
  et épisodes.
- Lecture HLS par copie, décodée par le matériel ([docs/HLS.md](../../docs/HLS.md)),
  reprise à la position enregistrée et sauvegarde toutes les 15 s.
- Téléchargements hors ligne en arrière-plan (service Media3 avec notification,
  relancé au retour du réseau ou après redémarrage) : qualité d'origine, toutes
  les pistes audio. Le cache ignore l'autorisation contenue dans l'URL HLS : un
  téléchargement repris avec une nouvelle URL (validité 12 h) garde ses segments,
  et une copie terminée se lit sans réseau. La lecture en ligne n'écrit pas dans
  ce cache.

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

Pas encore de Chromecast ni de téléphone télécommande ; la
position d'une lecture hors ligne n'est envoyée au Core que si le réseau est là. Un Core en certificat auto-signé n'est pas accepté ; utiliser
un vrai certificat ou HTTP sur le réseau local.

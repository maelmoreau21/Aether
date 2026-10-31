# Trickplay Aether — génération et import Jellyfin (prototype)

## Fonctionnement

Une prévisualisation se dessine au survol de la barre de progression (sans seek permanent).
Le Core autorise d'abord l'asset. S'il existe un cache Aether valide, les planches natives sont proposées.
Sinon, sur le **Node local uniquement**, une planche Jellyfin correspondant exactement au nom du fichier vidéo est acceptée en lecture seule.
Si rien n'existe, une génération FFmpeg est **demandée en tâche asynchrone** et le lecteur interroge périodiquement le manifeste.

Le serveur n'écrit jamais dans les dossiers médias ; les générations vont dans `data/trickplay`, ou `MEDIA_TRICKPLAY_CACHE_DIR` si défini.
La génération ne convertit pas le film original : elle décode ponctuellement ses images pour fabriquer des JPEG.
Les utilisateurs voient une barre de progression ordinaire si la préparation n'est pas disponible.

## Routes

- `GET /api/v1/assets/{id}/trickplay` : `200` + manifeste lorsque prêt, `202` + `status=generating`, `503` si occupé/échec, `404` si indisponible/désactivé.
- `GET/HEAD /api/v1/assets/{id}/trickplay/{index}` : planche JPEG.
- Autorisation de bibliothèque et contrôle parental identiques aux autres lectures d'assets (voir `permissions.rs`).
- Une route HTTP ne reçoit jamais de chemin disque, seulement un asset déjà inventorié et un index numérique.

## Réglages

- `MEDIA_TRICKPLAY_GENERATE=false` désactive la génération native (import Jellyfin toujours utilisable).
- `MEDIA_TRICKPLAY_CACHE_DIR=/data/trickplay` recommande un stockage **persistant** séparé des médias.
- `MEDIA_TRICKPLAY_FFMPEG=/usr/bin/ffmpeg` binaire utilisé par le Core pour ce premier prototype (à inclure dans l'image Core).
- `MEDIA_TRICKPLAY_INTERVAL_MS=10000` intervalle en millisecondes, plage 1000–60000.

La génération choisit 240 px par vignette et des grilles 10×10 (JPEG).
Un seul worker effectue la génération, limité à 30 minutes par média, 128 planches et 256 Mio par lot. Le cache est publié après la génération dans un dossier temporaire renommé.
Les métadonnées sont invalidées par empreinte associant la version, la taille, la date de modification et les paramètres.
La priorité à l'interactivité est partielle : une lecture active déclenche une attente de cinq secondes avant FFmpeg ; **aucune préemption stricte d'une génération déjà démarrée n'est mise en place**.

## Compatibilité Jellyfin

Convention actuellement reconnue :

```
Film.mp4
Film.trickplay/
  320 - 10x10/
    0.jpg
    1.jpg
```

Le nom et le nombre de vignettes sont supposés standard et `MEDIA_TRICKPLAY_INTERVAL_MS` doit correspondre à l'intervalle réel Jellyfin. La priorité est accordée aux planches natives Aether lorsqu'elles sont disponibles.
Les dossiers `Film (2025).trickplay` non associés à la version exacte restent ignorés : ils peuvent représenter une autre version éditoriale du film.

## Limites et problèmes connus à corriger avant fusion

1. **Aucun test Rust/Web ni Docker exécuté depuis cette session** : travail via GitHub connecté, sans clone/rustc. Ne pas considérer le code comme prêt pour la production.
2. Le format Jellyfin peut employer d'autres grilles, résolutions, intervalles et métadonnées. Import limité à `320 - 10x10` ; `0.jpg` initial requis.
3. Le générateur tourne pour le moment dans le **Core local**. Un Core distribué sans média monté ne peut pas générer pour un Media Node distant. Une vraie implémentation devra déplacer le travail et le cache côté Node, avec protocole signé et tâches administrables.
4. La gestion de quotas au niveau global, l'éviction LRU, la récupération des dossiers temporaires après crash et la persistance de la file de travaux restent à intégrer.
5. Le cache Aether est volontairement indépendant du catalogue (empreinte disque), mais une suppression/recréation de fichier entre deux opérations doit être couverte par un test de sécurité réel.
6. Le codec AV1 10 bits doit être testé avec le `ffmpeg` effectivement livré dans l'image Docker ; le Core seul n'a pas toujours ce binaire.
7. Contrôles à faire : `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` ; Web `npm run check`, `lint`, `test`, `build`.
8. Tests E2E : film MP4 long, génération à froid/chaud, interruption/redémarrage, mauvais fichier/symlink, non-admin 403, restrictions parentales, Range/HEAD, mobile/tactile, source modifiée, absence de FFmpeg, saturation disque, plusieurs générations concurrentes.

Ce lot est volontairement maintenu dans une **PR en brouillon**. Il ne remplace pas un service Trickplay distribué et durable.

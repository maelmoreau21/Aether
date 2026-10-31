# Trickplay — aperçus lors du déplacement dans la vidéo

## Périmètre du premier lot (31 octobre 2026)

- Compatibilité **en lecture seule** avec les sprites Jellyfin locaux déjà existants.
- Association **exacte au fichier vidéo** : `Film.mp4` → `Film.trickplay/320 - 10x10/0.jpg`, `1.jpg`, etc.
- L'alternative `Film (2025).trickplay` (au niveau de l'œuvre) est volontairement **ignorée** pour éviter d'associer les aperçus d'une édition différente.
- Les autres tailles et grilles ne sont pas encore détectées automatiquement.
- Les Media Nodes distants retournent « non disponible » : aucune prise en charge distribuée annoncée.
- Aucune génération, aucun transcodage, aucune écriture dans la bibliothèque.
- Barre de progression : une miniature se dévoile au survol et la position n'est modifiée qu'à la validation du curseur.

## Configuration

Variable facultative `MEDIA_TRICKPLAY_INTERVAL_MS` : intervalle entre deux miniatures, **10 000 ms par défaut**, de 1 000 à 60 000 ms. La présence de planches Jellyfin ne prouve pas leur intervalle réel : renseigner la valeur correspondant à la génération Jellyfin si elle diffère, sinon les aperçus peuvent être décalés.

## Contrat HTTP

Après session utilisateur et contrôles Core habituels de bibliothèque et de restrictions :

- `GET /api/v1/assets/{assetId}/trickplay` : métadonnées `intervalMs, width, height, columns, rows, sheetCount, thumbnailCount, urlPrefix`.
- `GET/HEAD /api/v1/assets/{assetId}/trickplay/{index}` : planche JPEG par identifiant de média et index numérique ; aucun chemin fourni depuis le navigateur.
- Aucun contenu disponible → 404 ; utilisateur non autorisé → politique existante (403/401).

Le fichier est ouvert par le dispositif de confinement du Media Node (interdiction des liens symboliques/junctions et sorties de racine) et envoyé en fragments. Limites : 512 planches, 32 Mio par planche, dimensions JPEG maximales 8 192 × 8 192. Le manifeste ne liste que les planches numérotées contiguës depuis zéro.

## Suite à développer

1. **Inspection des métadonnées Jellyfin** pour retrouver l'intervalle réel sans hypothèse : `.trickplay`, formats réels, grilles et bases de temps variables.
2. **Media Nodes distants** : synchroniser les métadonnées dans l'inventaire et servir les planches via une cible de lecture explicitement autorisée dans le protocole signé/relayé.
3. **Génération native Aether** : tâche basse priorité, génération dans `/data` hors bibliothèque, contrôle CPU/RAM, annulation, dédoublonnage et nettoyage du cache.
4. **Tests réels** : JPEG Jellyfin et film long sur NAS, droits d'accès utilisateur, 401/403/404, symlinks, HEAD/Range/416, survol desktop/tactile/clavier, changement de version, absence de planches et régression du lecteur AV1/Opus.

## Vérification

Ce lot a été écrit depuis l'API GitHub connectée. Le dépôt ne peut pas être cloné depuis l'environnement d'exécution actuel et la toolchain Rust n'est pas disponible. **Aucune compilation Rust/Web ni test de navigateur n'a pu être exécuté ici.** Ne pas fusionner sans exécuter `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, puis dans `apps/web` les commandes `npm ci`, `npm run check`, `npm run lint`, `npm test` et `npm run build`.

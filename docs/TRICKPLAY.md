# Trickplay Aether — Génération distribuée, Cache et Compatibilité Jellyfin

## 1. Vue d'ensemble architecturale

Le système Trickplay d'Aether fournit des planches de miniatures (spritesheets) pour la prévisualisation instantanée lors du survol et du défilement tactile de la barre de progression temporelle.

L'architecture sépare rigoureusement les responsabilités entre le **Core** et les **Media Nodes** :

```
┌─────────────────────────────────────────────────────────────┐
│                           Client Web                        │
│   (SvelteKit : survol souris, scrubbing tactile, prefetch)  │
└──────────────────────────────┬──────────────────────────────┘
                               │ GET /api/v1/assets/{id}/trickplay
                               │ GET /api/v1/assets/{id}/trickplay/{sheet}
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                           Core                              │
│  - Authentification & RBAC (administrator / user)           │
│  - Contrôle d'accès bibliothèque & politique parentale      │
│  - Résolution de l'asset et options de bibliothèque         │
│  - Routage : Node local (in-process) ou Node distant (relay)│
└──────────────┬──────────────────────────────┬───────────────┘
               │ LocalNode.trickplay()        │ HTTP / mTLS interne
               ▼                              ▼
┌──────────────────────────────┐┌─────────────────────────────┐
│      Media Node (Local)      ││     Media Node (Distant)    │
│  - Détection Jellyfin        ││  - Détection Jellyfin       │
│  - Génération FFmpeg native  ││  - Génération FFmpeg native │
│  - Cache persistant LRU      ││  - Cache persistant LRU     │
│  - Priorité absolue streaming││  - Priorité absolue stream  │
└──────────────────────────────┘└─────────────────────────────┘
```

1. **Core** :
   - Autorise l'accès (RBAC strict : rôles `administrator` et `user` uniquement).
   - Vérifie la visibilité de l'œuvre et les restrictions parentales.
   - Ne reçoit et ne manipule aucun chemin disque arbitraire depuis le client Web (seuls l'identifiant opaque d'asset et l'index de planche sont acceptés).
   - Délègue directement à l'instance locale `LocalNode.trickplay()` en mode standalone, ou relaie de façon authentifiée (`relay_from_remote_node`) vers le Media Node détenteur de la bibliothèque en mode distribué.

2. **Media Nodes** :
   - Chaque nœud gère son propre stockage de cache persistant (`MEDIA_TRICKPLAY_CACHE_DIR`).
   - Analyse dynamique des dossiers Jellyfin existants (`.trickplay/`) montés en lecture seule (`:ro`) dans les répertoires médias.
   - Génération locale via FFmpeg avec détection et conversion adaptée pour les codecs modernes (notamment AV1 10 bits `yuv420p10le`).
   - Priorité absolue aux flux de streaming actifs (`active_streams > 0`), suspendant ou espaçant le travail en arrière-plan.

---

## 2. Compatibilité Jellyfin dynamique

Aether détecte automatiquement les planches Trickplay générées par Jellyfin sans présumer d'une grille 10×10 ou d'un intervalle fixe de 10 secondes :

- **Emplacement** : Dossier `{nom_du_fichier}.trickplay/` situé à côté du fichier vidéo dans la bibliothèque (en lecture seule).
- **Grille et résolution dynamiques** : Le nom du sous-dossier est analysé sous la forme `{largeur} - {colonnes}x{lignes}` (ex. `320 - 10x10`, `240 - 5x5`, `160 - 8x8`).
- **Dimensions réelles** : Les dimensions en pixels de la planche et de chaque vignette sont extraites directement des en-têtes JPEG de la première planche (`0.jpg`) via un décodeur d'en-tête léger, sans dépendre d'hypothèses arbitraires.
- **Continuité des planches** : Le Media Node compte le nombre de planches consécutives (`0.jpg`, `1.jpg`, etc.) présentes sur le disque.
- **Intervalle temporel** : Calculé automatiquement à partir de la durée totale du média et du nombre effectif de vignettes disponibles, avec repli sur le paramètre configuré.

Si une planche Jellyfin valide existe, elle est immédiatement servie sans nécessiter de calcul FFmpeg.

---

## 3. Génération FFmpeg native & Gestionnaire de Codecs

Lorsque aucune planche n'existe et que la génération est activée, le Media Node produit des planches de prévisualisation natives :

### Ligne de commande FFmpeg optimisée
```bash
ffmpeg -v error -threads 2 -ss {seek_start} -i {source_video} \
  -vf "fps=1000/{interval_ms},scale={width}:-2:flags=fast_bilinear,format=yuvj420p,tile={cols}x{rows}" \
  -start_number {start_sheet} -q:v {quality} -f image2 {output_pattern}
```

### Particularités techniques
1. **AV1 10-bit (`yuv420p10le`) et HDR** :
   L'insertion de `format=yuvj420p` dans le filtre vidéo convertit de façon robuste les flux 10 bits ou 12 bits vers un espace colorimétrique JPEG 8 bits compatible sans échec du muxer `image2`.
2. **Priorité absolue au streaming** :
   Le Media Node suit le compteur `active_streams`. Dès qu'une lecture vidéo démarre sur le nœud, la génération trickplay est différée et cède immédiatement la priorité CPU et E/S disque.
3. **Reprise rapide après interruption (Resumable)** :
   Si un nœud redémarre ou si une tâche est interrompue, les planches déjà écrites dans le dossier temporaire `.{fingerprint}.in_progress` sont conservées. FFmpeg reprend directement avec `-ss` (fast-seek) et `-start_number`, évitant de recalculer les premières planches.
4. **Non-modification des médias** :
   Les vidéos originales ne sont jamais modifiées. Les bibliothèques médias sont montées en lecture seule (`:ro`). Les miniatures sont stockées dans le dossier dédié de cache.

---

## 4. Gestionnaire de Cache persistant

Le cache Trickplay est administré par le `TrickplayManager` du Media Node :

- **Empreinte unique (Deduplication / Fingerprint)** :
  Chaque cache est identifié par un hash SHA-256 combinant :
  - La taille du fichier source en octets
  - L'horodatage nanoseconde de modification (`modified_ns`)
  - Le chemin relatif du média
  - La durée en secondes
  - La largeur demandée (`width`)
  - L'intervalle (`interval_ms`)
  - Le niveau de qualité JPEG (`quality`)
- **Manifeste persistant** :
  Chaque dossier de cache validé contient un fichier `manifest.json` précisant les paramètres (dimensions, nombre de planches, intervalle, nombre de colonnes et de lignes).
- **Quota global et éviction LRU** :
  Un quota configurable en mégaoctets (`MEDIA_TRICKPLAY_MAX_CACHE_MB` ou réglage Core) est surveillé. Lorsque la taille du cache dépasse le quota, l'éviction LRU supprime automatiquement les ensembles de planches les plus anciens jusqu'à atteindre un fil d'eau de sécurité (80 % du quota).
- **Nettoyage automatique des orphelins** :
  Au démarrage du nœud et périodiquement, les dossiers temporaires résiduels (`*.tmp.*`) ou les dossiers `.in_progress` abandonnés depuis plus de 24 heures sont purgés.

---

## 5. Configuration & Paramètres d'Administration

### Réglages globaux du serveur (Core `server_settings`)
Les administrateurs peuvent configurer le comportement global de Trickplay depuis le panneau d'administration Web (`/admin/server-settings`) ou via l'API REST `/api/v1/server/presentation` :

| Champ API / JSON | Type | Défaut | Description |
| :--- | :--- | :--- | :--- |
| `trickplayEnabled` | bool | `true` | Active ou désactive totalement le service de prévisualisation |
| `trickplayAutoGenerate` | bool | `false` | Lance automatiquement la génération en arrière-plan lors de l'inventaire |
| `trickplayQuality` | u8 (1–31) | `7` | Qualité JPEG FFmpeg (`-q:v`, 1 = quasi sans perte, 31 = très compressé) |
| `trickplayWidth` | u16 (160–1920) | `320` | Largeur en pixels de chaque vignette |
| `trickplayIntervalSeconds` | u16 (1–60) | `10` | Intervalle temporel en secondes entre deux vignettes |
| `trickplayMaxCacheMb` | u32 (128–1048576) | `5120` | Quota maximal du cache en Mo (ex. 5120 Mo = 5 Go) |

### Options par bibliothèque (`LibraryOptions`)
Chaque bibliothèque peut surcharger ces réglages pour adapter la génération à son type de contenu (films 4K vs épisodes courts) :
- `trickplayEnabled`: booléen d'activation au niveau de la bibliothèque.
- `trickplayIntervalSeconds`: intervalle dédié (ex. 5s pour les animations courtes, 10s pour les longs métrages).
- `trickplayWidth`: largeur des vignettes (ex. 240 ou 320 px).
- `trickplayQuality`: qualité de compression JPEG (1–31).

### Variables d'environnement Docker

| Variable | Composant | Défaut | Description |
| :--- | :--- | :--- | :--- |
| `MEDIA_TRICKPLAY_CACHE_DIR` | Media Node | `{data_dir}/trickplay` | Emplacement persistant du cache de miniatures |
| `MEDIA_TRICKPLAY_MAX_CACHE_MB` | Media Node | `5120` | Quota maximum du cache disque par nœud en Mo |
| `MEDIA_TRICKPLAY_GENERATE` | Media Node | `true` | Autorise ou interdit l'appel FFmpeg sur ce nœud |
| `MEDIA_TRICKPLAY_FFMPEG` | Media Node | `/usr/bin/ffmpeg` | Chemin d'accès au binaire FFmpeg |
| `MEDIA_TRICKPLAY_INTERVAL_MS`| Media Node | `10000` | Intervalle par défaut en millisecondes |

---

## 6. Expérience Frontend SvelteKit

L'interface de lecture vidéo (`PlayerControls.svelte`) intègre les fonctionnalités suivantes :

1. **Survol souris et Déplacement tactile** :
   - Écouteurs unifiés `pointerdown`, `pointermove`, `pointerup` et `pointercancel` combinés à l'événement `input` du slider.
   - Les coordonnées tactiles sont bornées avec précision pour refléter fidèlement la position temporelle.
2. **Préchargement intelligent (Prefetching)** :
   - Le composant calcule l'index de planche active (`currentSheet = Math.floor(frameIndex / framesPerSheet)`).
   - Les planches adjacentes immédiates (`currentSheet - 1` et `currentSheet + 1`) sont préchargées en arrière-plan via des objets `Image()` en mémoire (`preloadedSheets` réactif).
   - Transition instantanée sans scintillement ni temps de chargement perçu lors du glissement.
3. **Résilience et absence d'impact sur le lecteur** :
   - Si les miniatures ne sont pas encore prêtes (statut `202 Generating` ou indisponibilité `404`), l'interface affiche l'indicateur de temps ordinaire sans interrompre ni gêner la lecture en cours.
   - Requêtes `GET` avec cache HTTP strict (`Cache-Control: public, max-age=86400`) et en-tête de sécurité `X-Content-Type-Options: nosniff`.

---

## 7. Sécurité et Bonnes Pratiques

- **Aucun chemin arbitraire** : Le client demande uniquement `/api/v1/assets/{id}/trickplay` et `/api/v1/assets/{id}/trickplay/{index}`. Le Core vérifie l'existence de l'asset dans la base de données SQLite/PostgreSQL et valide son appartenance à une bibliothèque autorisée pour l'utilisateur.
- **Isolement des accès disques** : Les Media Nodes utilisent `OpenedFile::open_in_root` et `validate_relative_components` pour interdire toute traversée de dossier (`../`), symlink vers un fichier hors racine ou injection de chemin.
- **Rôles stricts** : Seuls les rôles `administrator` et `user` sont reconnus conformément à `AGENTS.md`. Les utilisateurs sans droits sur une bibliothèque reçoivent une réponse `404 Not Found` (ou `403 Forbidden` selon la politique d'exposition).

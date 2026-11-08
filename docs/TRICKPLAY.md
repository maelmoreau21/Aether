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

### Ligne de commande FFmpeg optimisée (WebP par défaut, repli JPEG)
```bash
ffmpeg -hide_banner -loglevel error -nostdin -threads 1 -filter_threads 1 -ss {seek_start} -n -i {source_video} \
  -map 0:v:0 -an -sn -dn -vf "fps=1000/{interval_ms},scale={width}:-2:flags=fast_bilinear,format=yuv420p,tile={cols}x{rows}" \
  -frames:v {remaining_sheets} -c:v libwebp -quality {webp_quality} -compression_level 3 -start_number {start_sheet} -f image2 {output_pattern}.webp
```

### Particularités techniques
1. **AV1 10-bit (`yuv420p10le`), HDR et encodage `.webp`** :
   L'insertion de `format=yuv420p` dans le filtre vidéo convertit de façon robuste les flux 10 bits ou 12 bits vers un espace colorimétrique 8 bits compatible avec `libwebp` et `mjpeg`. Le profil FFmpeg Docker (`scripts/build-ffmpeg.sh`, `Dockerfile`) active explicitement `libwebp` (`--enable-libwebp --enable-encoder=webvtt,mjpeg,libwebp,pcm_s16le`), produisant des planches `.webp` natives (version 2 du manifeste, qualité par défaut `7` soit `86 %` WebP). Si `libwebp` est absent d'un binaire FFmpeg hôte externe, le Media Node se replie sur `mjpeg` (`.jpg`).
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
- **Format WebP** : dès que le FFmpeg du nœud fournit `libwebp`, les planches sont en WebP ; un ancien cache JPEG est ignoré et régénéré. Après publication, les anciennes planches du même asset (média modifié, profil changé) sont supprimées.
- **Manifeste persistant** :
  Chaque dossier de cache validé contient un fichier `manifest.json` précisant les paramètres (dimensions, nombre de planches, intervalle, nombre de colonnes et de lignes).
- **Quota global et éviction LRU** :
  Un quota configurable en mégaoctets (`MEDIA_TRICKPLAY_MAX_CACHE_MB` ou réglage Core) est surveillé. Lorsque la taille du cache dépasse le quota, l'éviction LRU supprime automatiquement les ensembles de planches les plus anciens jusqu'à atteindre un fil d'eau de sécurité (80 % du quota).
- **Nettoyage automatique des orphelins** :
  Au démarrage du nœud et périodiquement, les dossiers temporaires résiduels (`*.tmp.*`) ou les dossiers `.in_progress` abandonnés depuis plus de 24 heures sont purgés.

---

## 5. Configuration & Paramètres d'Administration

### Configuration par bibliothèque et synchronisation depuis les paramètres serveur

L'activation, la préparation après scan, la qualité, la largeur, l'intervalle et la taille des planches (`trickplayColumns` × `trickplayRows`) se règlent par bibliothèque dans Administration → Bibliothèques → modifier → Lecture, ainsi que globalement dans Administration → Paramètres du serveur → Général → Aperçus dans la barre de lecture (qui synchronise les bibliothèques lors de l'enregistrement, sans qu'une ancienne ligne `server_settings` ne puisse bloquer une bibliothèque explicitement activée).

| Option de bibliothèque / serveur | Défaut | Comportement |
| :--- | :--- | :--- |
| `trickplayEnabled` | `true` | Cache natif `.webp` puis repli Jellyfin en lecture seule |
| `trickplayDuringScan` / `trickplayAutoGenerate` | `true` | Préparation automatique après publication d'un scan |
| `trickplayWidth` | `320` | Largeur 160–1920 pixels |
| `trickplayIntervalSeconds` | `10` | Intervalle 1–60 secondes |
| `trickplayQuality` | `7` | Compression 1–31 (`7` ≈ `86 %` en WebP, `1` étant la meilleure qualité) |
| `trickplayColumns` | `10` | Colonnes par planche (1–20) |
| `trickplayRows` | `10` | Lignes par planche (1–20) |
| `trickplaySaveToMedia` | `false` | Aucune écriture à côté des médias (`:ro`) |
| `chapterImagesEnabled` | `true` | Chapitres réels, images à la demande |
| `chapterImagesDuringScan` | `false` | Pas de préparation systématique des chapitres |

Ces défauts s'appliquent aux nouvelles bibliothèques et aux champs absents des
anciens JSON. Les valeurs explicites, notamment `false`, restent conservées ;
aucune migration ne réactive une bibliothèque déjà configurée.

Le scan enregistre atomiquement des tâches durables de priorité 1 dans la file
existante. L'empreinte inclut l'asset, sa taille/date et le profil. Les probes
réutilisés sont inclus : activer ou modifier un profil prépare donc aussi les
fichiers existants au prochain scan. La comparaison par lots de 128 clés indexées
évite tâches dupliquées et réécritures lors des scans inchangés. La limite globale
de 100 000 tâches en attente reste appliquée ; le reste peut être préparé lors d'un
prochain scan ou à la demande. La désactivation est revérifiée avant dispatch ;
une réactivation reprend le même travail annulé pour cette raison. Les annulations
administratives et les échecs gardent leur contrat de reprise explicite.

Local et distant transmettent le même profil. Les manifests et planches distantes
conservent durée/profil sur HTTP signé et canal sortant signé. Le Node partage son
unique worker entre préparation durable et demandes du lecteur, et respecte
`MEDIA_TRICKPLAY_GENERATE`. Le moteur de tâches cède à une lecture active. FFmpeg
crée des images dans le cache séparé ; aucun encodage vidéo n'est imposé à la lecture.

### Variables d'environnement Docker

| Variable | Composant | Défaut | Description |
| :--- | :--- | :--- | :--- |
| `MEDIA_TRICKPLAY_CACHE_DIR` | Media Node | `{data_dir}/trickplay` | Emplacement persistant du cache de miniatures |
| `MEDIA_TRICKPLAY_MAX_CACHE_MB` | Media Node | `5120` | Quota maximum du cache disque par nœud en Mo |
| `MEDIA_TRICKPLAY_GENERATE` | Media Node | `true` | Autorise ou interdit l'appel FFmpeg sur ce nœud |
| `MEDIA_TRICKPLAY_FFMPEG` | Media Node | `/usr/bin/ffmpeg` | Chemin d'accès au binaire FFmpeg |

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

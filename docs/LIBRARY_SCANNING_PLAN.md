# Plan de spécification — Scanner de bibliothèques P1

> **Statut : Spécification d'architecture et plan de test pour le jalon P1 autonome.**
> **Règle absolue : Aucun code de scanner, aucune migration SQLite et aucune modification des médias réels ne sont réalisés dans cette étape.**
> Ce document définit le contrat technique pour l'ingestion des dossiers organisés selon les conventions Radarr et Sonarr, en lecture seule (`:ro`), en coexistence pacifique avec Jellyfin.

---

## 1. Contexte et objectifs P1

Le scanner de bibliothèques est le composant du **Media Node** chargé de découvrir, vérifier et indexer les fichiers présents sur le stockage local pour les présenter au **Core** sous forme d'inventaire typé.

### 1.1 Principes non négociables (issus d'`AGENTS.md`)
1. **Lecture seule stricte (`:ro`)** : Le Media Node n'écrit **jamais**, ne renomme **jamais** et ne supprime **aucun** fichier dans les répertoires multimédias. Aucune modification des fichiers ou caches Jellyfin.
2. **Volumes distincts pour les états** : La base de données SQLite, les index, les caches et les métadonnées générées résident exclusivement dans le répertoire de travail d'Aether (`MEDIA_DATA_DIR`, ex. `/var/lib/aether` ou `data/`).
3. **Priorité multimédia P0 préservée** : Détection prioritaire du couple **AV1 vidéo + Opus audio** dans les conteneurs `.mkv` et `.mp4`, sans transcodage vidéo obligatoire (notamment pour les cibles matérielles sans iGPU tel le Synology DS1522+).
4. **Contrôle d'accès et sécurité** : Seul le rôle `administrator` peut déclarer un dossier de bibliothèque ou déclencher une opération de scan. Le rôle `user` consomme uniquement le catalogue via les permissions accordées par le Core.
5. **Découplage conceptuel** : Même en mode autonome monopoint (`standalone`), la logique de découverte physique (Node) reste rigoureusement distincte de la logique d'unification éditoriale (Core).

---

## 2. Conventions d'arborescence supportées

Le scanner cible en priorité les conventions standard générées par les gestionnaires de médias Radarr et Sonarr.

### 2.1 Films (convention Radarr)

Structure type :
```text
/Films/
  └── Titre du Film (2024) [tmdb-12345]/
      ├── Titre du Film (2024) [Remux-1080p].mkv
      ├── movie.nfo
      ├── folder.jpg
      ├── backdrop.jpg
      ├── logo.png
      ├── Titre du Film (2024).fr.forced.srt
      ├── Titre du Film (2024).fr.srt
      └── Titre du Film (2024).en.srt
```

- **Nom du dossier** : `Titre (Année) [tmdb-ID]` ou `Titre (Année) [imdb-ttID]` ou `Titre (Année)`.
- **Fichier de métadonnées** : `movie.nfo` (ou `<nom_du_film>.nfo`).
- **Médias vidéo** : Un ou plusieurs fichiers `.mkv` ou `.mp4`.
- **Sous-titres externes** : Fichiers `.srt` nommés d'après le nom de base du média ou du dossier, avec code de langue et marqueur optionnel.
- **Illustrations** :
  - Affiche (`poster`) : `poster.jpg`, `folder.jpg`, `<nom_du_film>-poster.jpg`.
  - Fond (`backdrop` / `fanart`) : `backdrop.jpg`, `fanart.jpg`, `<nom_du_film>-fanart.jpg`.
  - Logo (`logo`) : `logo.png`, `clearlogo.png`.
- **Artefacts tiers à ignorer impérativement** : Dossiers `.trickplay/`, fichiers `.bif` (index de vignettes), dossiers `.actors/`, `extrafanart/`, fichiers `.ignore`.

### 2.2 Séries (convention Sonarr)

Structure type :
```text
/Séries/
  └── Titre de la Série (2023) [tvdb-67890]/
      ├── tvshow.nfo
      ├── poster.jpg
      ├── backdrop.jpg
      ├── logo.png
      ├── Season 01/
      │   ├── season.nfo
      │   ├── season01-poster.jpg
      │   ├── Titre - S01E01 - Pilote.mkv
      │   ├── Titre - S01E01 - Pilote.nfo
      │   ├── Titre - S01E01 - Pilote.fr.forced.srt
      │   └── Titre - S01E01 - Pilote.fr.srt
      └── Season 02/
          ├── Titre - S01E02 - Épisode 2.mp4
          └── ...
```

- **Nom du dossier série** : `Titre (Année) [tvdb-ID]` ou `Titre (Année) [tmdb-ID]` ou `Titre (Année)`.
- **Métadonnées globales** : `tvshow.nfo`.
- **Dossiers de saisons** : `Season 01`, `Season 02`, `Season 00` ou `Specials`.
- **Métadonnées de saison** : `season.nfo` et illustrations de saison (`season01-poster.jpg` ou `poster.jpg` dans le dossier de saison).
- **Fichiers épisodes** : Format standard `Titre - SxxExx - Nom.ext` ou `SxxExx.ext`.
- **Métadonnées d'épisode** : Fichier `.nfo` associé à l'épisode portant le même nom de base.
- **Cas particuliers** : Épisodes multi-parties (ex. `S01E01-E02.mkv`).

---

## 3. Modélisation et regroupement éditorial

Conformément à `ARCHITECTURE.md` et `docs/MEDIA_FORMATS.md`, le scanner ne doit **jamais assimiler un fichier physique à une œuvre éditoriale**.

```text
Work (Film ou Épisode)
  └── Edition (Montage cinéma, Director's Cut, Édition spéciale)
       └── MediaVersion (Profil d'encodage : 1080p SDR, 4K HDR, AV1 Main 10)
            └── MediaSource (Copie physique sur le Media Node)
                 ├── Asset (Fichier conteneur MKV/MP4 ou sous-titre externe SRT)
                 └── Track (Flux vidéo, audio multicanal, sous-titres)
```

### 3.1 Règles d'unification et de regroupement
1. **Identité canonique par ID vérifié** :
   - Pour les films : TMDb ID prioritaire (`tmdb-12345`), complété par IMDb ID (`tt...`).
   - Pour les séries : TVDb ID prioritaire (`tvdb-67890`), complété par TMDb ID (`tmdb-...`) et IMDb ID.
   - Les identifiants sont extraits en priorité du fichier `.nfo` (balises XML dédiées), et à défaut de la balise entre crochets du dossier racine.
2. **Gestion des éditions et des doublons** :
   - Si un même dossier de film contient plusieurs fichiers vidéo (ex. `Film (2024) [Director Cut].mkv` et `Film (2024) [Theatrical].mkv`), le scanner crée **un seul `Work`** contenant **deux `Edition`** (ou deux `MediaVersion`), et **non deux films distincts** dans le catalogue.
   - Si deux fichiers identiques ou de résolutions différentes sont présents, ils sont rattachés sous la même œuvre comme sources alternatives.
3. **Hiérarchie Série / Saison / Épisode** :
   - La série est un `Work` de type `series`.
   - Chaque saison regroupe ses épisodes.
   - Chaque épisode est un `Work` éditorial de type `episode` portant son propre numéro de saison et d'épisode, rattaché à la série parente, et n'est jamais présenté comme un film isolé dans les listes principales.

---

## 4. Lecture des NFO et extraction des métadonnées

Le scanner analyse les fichiers `.nfo` au format XML standard (schémas Kodi/Jellyfin/Emby).

### 4.1 Balises extraites pour les Films (`movie.nfo`)
- `<title>` : Titre usuel de l'œuvre.
- `<originaltitle>` : Titre original.
- `<year>` / `<premiered>` : Année de sortie et date exacte.
- `<plot>` / `<outline>` : Synopsis court et détaillé.
- `<uniqueid type="tmdb" default="true">` ou `<tmdbid>` : Identifiant numérique TMDb.
- `<uniqueid type="imdb">` ou `<imdbid>` : Identifiant alphanumérique IMDb (`tt...`).
- `<mpaa>` : Classification d'âge (ex. `FR:Tous publics`, `PG-13`).
- `<rating>` : Note globale.
- `<genre>` : Liste des genres.

### 4.2 Balises extraites pour les Séries (`tvshow.nfo`) et Épisodes
- Séries (`tvshow.nfo`) :
  - `<title>`, `<year>`, `<plot>`.
  - `<uniqueid type="tvdb">` ou `<tvdbid>`, `<uniqueid type="tmdb">`.
- Saisons (`season.nfo`) :
  - `<seasonnumber>`, `<plot>`.
- Épisodes (`<episode>.nfo`) :
  - `<title>`, `<season>`, `<episode>`, `<aired>`, `<plot>`.
  - `<uniqueid type="tvdb">` / `<uniqueid type="tmdb">`.

### 4.3 Résilience et gestion des manques
- **Absence de NFO** : Si aucun fichier `.nfo` n'est présent, le scanner bascule sur l'analyse heuristique du nom de dossier : expression régulière extrayant le titre, l'année entre parenthèses et l'identifiant entre crochets `[tmdb-xxxx]`.
- **Fichiers mal formés** : Un fichier XML syntaxiquement corrompu ne bloque pas l'indexation du média ; le scanner émet un avertissement dans le journal de scan et utilise les informations dérivées du nom de fichier.

---

## 5. Détection des affiches et illustrations locales

Les visuels locaux présents dans les bibliothèques doivent être détectés et associés sans duplication inutile.

### 5.1 Priorités de détection

| Rôle | Cible Film | Cible Série | Cible Saison | Cible Épisode |
|---|---|---|---|---|
| **Poster** (affiche) | `poster.jpg` > `folder.jpg` > `<film>-poster.jpg` > `<film>.jpg` | `poster.jpg` > `folder.jpg` | `seasonXX-poster.jpg` > `folder.jpg` dans le dossier de saison | `<ep>-thumb.jpg` > `<ep>.jpg` |
| **Backdrop** (fanart) | `backdrop.jpg` > `fanart.jpg` > `<film>-fanart.jpg` | `backdrop.jpg` > `fanart.jpg` | Repli sur le backdrop de la série | N/A (repli série) |
| **Logo** (titre transparent) | `logo.png` > `clearlogo.png` | `logo.png` > `clearlogo.png` | N/A | N/A |

Formats d'image acceptés : `.jpg`, `.jpeg`, `.png`, `.webp`.

### 5.2 Contrat de service des images
- **Aucun chemin physique direct** : Le chemin sur disque (`/mnt/films/.../folder.jpg`) n'est jamais divulgué au navigateur.
- **Identifiant d'Asset image** : Le Media Node déclare les images comme des `Asset` de type image (ex. `asset-img-<hash>`).
- **Diffusion sécurisée** : Une route dédiée `/api/v1/assets/{id}/image` sert le fichier en vérifiant les droits et le type MIME exact (`image/jpeg`, `image/png`, `image/webp`).
- **Conformité CSP** : La directive existante `img-src 'self'` dans les en-têtes CSP est respectée sans ajouter d'origine distante.

---

## 6. Détection réelle des codecs et association des sous-titres

### 6.1 Inspection technique des conteneurs (Probe)
- Le scanner ne déduit **jamais** les codecs à partir des extensions de fichiers `.mkv` ou `.mp4`.
- L'adaptateur d'inspection technique (`media::probe`) est exécuté sur chaque conteneur vidéo pour extraire la structure exacte des flux :
  - **Vidéo** : Codec (`av1`, `hevc`, `h264`), profil (ex. `Main`, `High 10`), dimensions, taux de rafraîchissement, timebase, dispositions (`default`).
  - **Audio** : Codec (`opus`, `aac`, `ac3`, `eac3`, `flac`), nombre de canaux (1, 2, 6 pour 5.1, 8 pour 7.1), disposition des voies, fréquence d'échantillonnage, décalage initial (`initial_padding`), tags de langue.
  - **Sous-titres intégrés** : Codec (`subrip`, `mov_text`, `ass`), tags de langue, dispositions (`forced`, `default`, `hearing_impaired`).

### 6.2 Normalisation linguistique
- Les codes de langue présents dans les conteneurs (souvent en ISO 639-2/B ou 639-2/T : `fre`, `fra`, `eng`, `und`) sont rigoureusement normalisés vers le standard **BCP 47** (`fr`, `en`, `und`).

### 6.3 Association des sous-titres externes
- Les sous-titres externes `.srt` sont analysés selon la convention :
  `<base_name>.<lang>[.<flags>].srt`
  - Exemples : `Film (2024).fr.srt`, `Film (2024).fra.forced.srt`, `Film (2024).en.sdh.srt`.
- Détection des rôles :
  - `forced` : Détecté via `.forced.` ou balise de disposition, destiné aux passages en langue étrangère.
  - `sdh` : Détecté via `.sdh.` ou balise malentendants.
- **Règle linguistique produit** : La détection identifie et indexe explicitement le français (`fr`) et le français forcé (`fr.forced`), préparant la sélection automatique selon les préférences de l'utilisateur, tout en conservant le choix manuel prioritaire.

---

## 7. Gestion des anomalies, doublons et fichiers tiers

### 7.1 Fichiers absents ou médias déconnectés
- Si un fichier indexé disparaît du disque (déconnexion de volume ou suppression) :
  - Le `MediaSource` associé est marqué comme `unavailable` / `offline`.
  - **Pas de suppression destructrice immédiate** des métadonnées éditoriales (`Work`, progression de lecture, favoris). L'historique utilisateur est ainsi préservé en cas de déconnexion temporaire d'un partage réseau.

### 7.2 Doublons et collisions
- **Même film présent dans deux dossiers différents** : Rapprochement via le TMDb ID. Les deux dossiers constituent des `MediaSource` concurrentes rattachées au même `Work`.
- **Fichiers vidéo multiples dans le même dossier** : Analyse des noms pour identifier les versions (ex. `4K`, `1080p`, `Director's Cut`). Si aucune distinction n'est trouvable, ils sont exposés comme des sources alternatives sans écrasement mutuel.

### 7.3 Filtrage strict des fichiers temporaires et non pertinents
Sont strictement ignorés lors du parcours récursif :
- Fichiers d'échantillons courts : `*sample*` d'une durée inférieure à 2 minutes ou d'une taille négligeable.
- Fichiers temporaires ou en cours de téléchargement : `.part`, `.crdownload`, `.tmp`, `.filepart`.
- Fichiers système et cachés : `.DS_Store`, `Thumbs.db`, répertoires débutant par `.`.

### 7.4 Neutralité absolue vis-à-vis des caches Jellyfin / Plex / Emby
Les dossiers de bibliothèques partagés contiennent fréquemment des artefacts générés par d'autres serveurs multimédias :
- Dossiers `.trickplay/` et fichiers `.bif`.
- Dossiers `extrafanart/`, `extrathumbs/`, `.actors/`.
- Fichiers `.ignore`.

**Consigne stricte** : Ces répertoires et fichiers sont reconnus comme artefacts tiers, **totalement ignorés** lors de l'indexation, et **jamais modifiés, renommés ou purgés**.

---

## 8. Robustesse et sécurité du scanner

Le scanner manipule des arborescences de fichiers externes et des contenus XML potentiellement hostiles. Des barrières défensives strictes sont requises.

### 8.1 Confinement de chemin et traversée de répertoires (Path Traversal)
- Chaque bibliothèque possède une racine canonique vérifiée (`library_root`).
- **Canonicalisation** : Avant toute ouverture de fichier, le chemin absolu résolu est canonicalisé (gestion des spécificités Windows UNC via `dunce::canonicalize` pour éviter les préfixes `\\?\` problématiques).
- **Contrôle de confinement** : Vérification stricte que `target_path.starts_with(&library_root)`. Tout fichier résolu en dehors de la racine est immédiatement rejeté avec un événement de sécurité dans les logs.

### 8.2 Liens symboliques et jonctions (Symlinks & Junctions)
- **Liens pointant vers l'extérieur** : Si un symlink ou une jonction NTFS pointe en dehors du périmètre de la bibliothèque, il est ignoré et non suivi.
- **Détection des cycles récursifs** : Mémorisation des identifiants de fichiers uniques (`(dev, ino)` sous Linux/macOS, `file_index` / volume serial sous Windows) pour détecter immédiatement les boucles de récursion et prévenir les débordements de pile.

### 8.3 Sécurisation du parseur XML (NFO)
- **Protection contre les attaques XXE (XML External Entity)** : Utilisation d'un parseur Rust configuré pour désactiver totalement la résolution des entités externes et les DTD.
- **Protection contre les XML Bombs (Billion Laughs)** :
  - Taille maximale autorisée pour un fichier NFO : **1 Mo** (au-delà, rejet immédiat).
  - Limite stricte sur la profondeur d'imbrication des balises XML (ex. maximum 16 niveaux).
- **Encodage** : Prise en charge transparente de l'UTF-8 (avec ou sans BOM) et repli propre en cas de caractères non conformes.

### 8.4 Isolation et limitation des ressources du Probe
- Exécution de l'outil technique de probe (`ffprobe` temporaire en P0/P1) avec les garde-fous déjà éprouvés dans `media::probe` :
  - Protocoles limités strictement à `file`.
  - Démuxeurs restreints aux conteneurs autorisés (`matroska`, `webm`, `mov,mp4`).
  - Allocation mémoire maximale bornée (`-max_alloc 67108864`, soit 64 Mo).
  - Timeout strict d'exécution : **10 secondes**, avec terminaison forcée du processus (`kill_on_drop(true)`).
  - Sortie JSON plafonnée à **1 Mo**.

---

## 9. Scans incrémentaux et performance

Un scan de bibliothèque ne doit jamais dégrader la réactivité de la machine hôte ni monopoliser l'accès disque.

### 9.1 Exécution asynchrone hors des requêtes HTTP
- Le déclenchement d'un scan via l'API d'administration (`POST /api/v1/libraries/{id}/scan`) démarre une **tâche d'arrière-plan Tokio**.
- La requête HTTP répond immédiatement un code `202 Accepted` avec un identifiant de tâche (`job_id`).
- L'état d'avancement (nombre d'éléments analysés, pourcentage, erreurs non bloquantes) est consultable via une route de statut (`GET /api/v1/jobs/{job_id}`).

### 9.2 Stratégie incrémentale par empreinte temporelle
Pour éviter d'exécuter des milliers de probes coûteux à chaque scan :
- Le scanner enregistre dans l'index local le triplet `(chemin_relatif, mtime, taille)`.
- Lors d'un scan ultérieur :
  1. Si `mtime` et `taille` sont identiques à l'index : le fichier est considéré comme intact, **aucun appel à ffprobe n'est lancé**, les métadonnées existantes sont conservées.
  2. Si `mtime` ou `taille` a changé : ré-analyse complète du fichier.
  3. Si un fichier a disparu de l'arborescence : passage à l'état `unavailable`.

### 9.3 Bornage de la mémoire et contrôle du parallélisme
- **Parcours par flux (Streaming Directory Traversal)** : L'arborescence est parcourue via des itérateurs asynchrones sans charger l'intégralité des 50 000 entrées d'un disque en mémoire vive (`Vec<PathBuf>`).
- **Sémaphore de concurrence** : Le nombre d'analyses techniques simultanées (probes) est borné par un `tokio::sync::Semaphore` (ex. **2 probes simultanés par défaut**, configurable jusqu'à 4) afin de garantir une consommation CPU minimale sur les NAS basse consommation (DS1522+ à 2 cœurs/4 threads).

---

## 10. Matrice des cas de test

Avant toute implémentation définitive, le scanner devra être validé sur une suite complète de fixtures synthétiques reproduisant fidèlement les cas nominaux et les situations dégradées.

### 10.1 Cas nominaux

| Identifiant | Description du cas | Structure de test | Résultat attendu |
|---|---|---|---|
| `TEST-SCAN-01` | Film Radarr nominal complet | Dossier `Titre (2024) [tmdb-123]`, `movie.nfo`, `film.mkv` (AV1/Opus), `folder.jpg`, `backdrop.jpg`, `logo.png`, `film.fr.forced.srt`, `film.en.srt` | 1 Work `film`, métadonnées extraites du NFO, 1 MediaSource, 1 flux vidéo, 1 audio, 2 sous-titres externes (FR forced et EN identifiés), 3 assets d'illustrations associés. |
| `TEST-SCAN-02` | Série Sonarr nominale multi-saisons | `Série (2023) [tvdb-456]`, `tvshow.nfo`, `poster.jpg`, `Season 01/` (`season.nfo`, `S01E01.mkv`, `S01E01.nfo`), `Season 02/` (`S02E01.mp4`) | 1 Work `series` avec 2 saisons et 2 épisodes. Les épisodes sont rattachés à leur saison respective avec leurs métadonnées propres. |
| `TEST-SCAN-03` | Film sans NFO mais dossier tagué | `Autre Film (2022) [tmdb-789]/video.mp4` sans aucun `.nfo` | Titre extrait : "Autre Film", Année : 2022, TMDb ID : 789. MediaSource indexée sans erreur. |

### 10.2 Cas de regroupement et doublons

| Identifiant | Description du cas | Structure de test | Résultat attendu |
|---|---|---|---|
| `TEST-SCAN-04` | Film multi-versions dans un seul dossier | `Film (2024) [tmdb-123]/` contenant `film-1080p.mkv` et `film-4k.mp4` | 1 seul Work créé, avec 2 `Edition` ou 2 `MediaVersion` distinctes. Aucun doublon dans le catalogue global. |
| `TEST-SCAN-05` | Collision de TMDb ID dans 2 dossiers distincts | Dossier A `Film (2024) [tmdb-100]` et Dossier B `Film Edition Longue (2024) [tmdb-100]` | Unification sous le même Work éditorial avec 2 MediaSources provenant de dossiers distincts. |
| `TEST-SCAN-06` | Épisode multi-parties | `S01E01-E02.mkv` avec son NFO | Détection d'un fichier unique couvrant les épisodes 1 et 2 de la saison 1. |

### 10.3 Robustesse, tolérance aux pannes et sécurité

| Identifiant | Description du cas | Structure de test | Résultat attendu |
|---|---|---|---|
| `TEST-SEC-01` | Symlink pointant hors bibliothèque | Symlink `evil_link -> /etc/passwd` ou `C:\Windows` dans le dossier de film | Détection du franchissement de racine, rejet immédiat du symlink, journalisation de sécurité, aucun crash. |
| `TEST-SEC-02` | Boucle récursive de symlinks | `dir_a/link_to_b -> dir_b` et `dir_b/link_to_a -> dir_a` | Détection de cycle, arrêt de la descente dans le sous-dossier récursif sans dépassement de pile. |
| `TEST-SEC-03` | Attaque XML XXE dans un NFO | `movie.nfo` contenant `<!DOCTYPE foo [ <!ENTITY xxe SYSTEM "file:///etc/hosts"> ]>` | Rejet du chargement d'entité externe par le parseur, parsing sécurisé du reste du document ou rejet sans fuite de données. |
| `TEST-SEC-04` | Fichier NFO géant (XML Bomb) | Fichier `movie.nfo` gonflé à 15 Mo de texte ou entités imbriquées | Rejet immédiat dès la vérification de la taille (> 1 Mo) avant parsing XML. |
| `TEST-RES-01` | Coexistence Jellyfin / Trickplay | Présence d'un dossier `.trickplay/` et de fichiers `movie.bif` | Dossier et fichiers strictement ignorés, aucun fichier modifié ni altéré. |
| `TEST-RES-02` | Fichier incomplet de téléchargement | Fichier `film.mkv.part` ou `.crdownload` présent | Fichier filtré par l'extension temporaire, non transmis au probe. |
| `TEST-RES-03` | NFO XML corrompu / mal formé | Balise XML non fermée dans `movie.nfo` | Avertissement émis, repli gracieux sur les informations déduites du nom de dossier. |

### 10.4 Performance et incrémentalité

| Identifiant | Description du cas | Structure de test | Résultat attendu |
|---|---|---|---|
| `TEST-PERF-01` | Scan incrémental sans modification | Scan initial complet sur 10 médias, suivi d'un second scan sans aucune modification disque | Le second scan s'exécute quasi instantanément avec **0 appel probe ffprobe**, toutes les métadonnées sont réutilisées depuis le cache. |
| `TEST-PERF-02` | Détection de modification ciblée | 1 seul fichier vidéo sur 10 voit son `mtime` mis à jour | 1 seul probe ré-exécuté, les 9 autres médias sont validés par leur cache. |
| `TEST-PERF-03` | Disparition temporaire de média | 1 fichier vidéo déplacé ou supprimé entre deux scans | Le média passe au statut `unavailable`, les données de lecture associées ne sont pas purgées. |

---

## 11. Choix restant à valider avant implémentation

Ces points d'arbitrage technique devront être formellement tranchés avant d'écrire le code du scanner :

1. **Choix de la bibliothèque de parsing XML en Rust** :
   - *Option A* : `quick-xml` (très rapide, streaming, sans allocation superflue, naturellement insensible aux attaques DTD/XXE quand non configuré pour les entités externes).
   - *Option B* : `roxmltree` (arbre immuable en mémoire, syntaxe concise, robuste contre les entités externes).
   - *Recommandation* : `quick-xml` pour sa performance et sa faible empreinte mémoire sur CPU NAS.

2. **Stratégie de service des illustrations locales** :
   - *Option A* : Service direct du fichier image source depuis le Media Node via un flux sécurisé (`/api/v1/assets/{id}/image`).
   - *Option B* : Génération d'une copie redimensionnée/convertie en WebP dans le cache applicatif (`data/cache/posters/`).
   - *Recommandation P1* : Option A pour le MVP P1 afin de ne pas consommer de CPU en conversion d'image ; évaluation de l'Option B ultérieurement si la taille des posters d'origine ralentit l'affichage.

3. **Traitement des divergences entre le NFO et l'analyse technique** :
   - *Exemple* : Le fichier NFO indique un flux audio `DTS-HD 5.1`, mais le probe réel détecte `Opus stéréo`.
   - *Arbitrage proposé* : La réalité technique du conteneur (probe) fait foi absolue pour le plan de lecture et les capacités du lecteur ; les métadonnées éditoriales du NFO (titre, synopsis, acteurs) font foi pour l'affichage de la fiche.

4. **Persistance SQLite de l'inventaire** :
   - Structure relationnelle des tables d'indexation (`scanned_libraries`, `scanned_files`, `works`, `editions`, `media_versions`, `media_sources`, `assets`, `tracks`) à modéliser dans le prochain jalon dédié à SQLite et au bootstrap administrateur.

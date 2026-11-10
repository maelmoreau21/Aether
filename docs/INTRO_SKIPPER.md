# IntroSkipper — Architecture et limites de validation

IntroSkipper est le composant de détection automatique des introductions et génériques de fin pour la plateforme Aether. Il combine l'analyse des chapitres de conteneurs (`.mkv`, `.mp4`), l'extraction acoustique Chromaprint sur les Media Nodes et un algorithme de clustering / consensus multi-épisodes exécuté dans une extension WebAssembly isolée.

---

## 0. Refonte du 8 novembre 2026 (plugin 0.2.0)

Ce qui empêchait la détection d'aboutir, et ce qui a changé :

| Problème | Correction |
|---|---|
| Les films étaient exclus partout (file d'attente, persistance, corrections manuelles refusées en 422) | Un film a ses chapitres lus (`intro.parse_chapters`, `mediaType: "movie"`) ; ses corrections manuelles sont acceptées ; un générique de film peut durer 20 min (plafond 1200 s pour `credits`, 600 s pour le reste) |
| `intro.detect` ne recevait pas les chapitres : aucune fusion chapitre + audio | Le Core envoie les chapitres de l'épisode cible ; le plugin garde un chapitre nommé tel quel (0,99 si l'audio confirme) et cale une limite audio sur un chapitre à moins de 3 s (`chapter_snap_window`) |
| Le Node donnait le fichier à ffmpeg par un tube : MP4 sans *faststart* illisibles, queue atteinte en décodant tout le fichier (délai de 90 s) | Entrée `-fd 0 -i fd:` (relisible, vrai seek) ; repli sur le tube pour un ffmpeg antérieur à 6.1 |
| Noms `OP1`, `ED2`, `01 - Opening`, `♪ Ending`, `オープニング`, `Abspann`… non reconnus | Nouveau classement : nom découpé à ses séparateurs, numérotation ignorée, listes FR/EN/DE/ES/PT/IT/JA ; « Opening Night », « Post-credits scene » ne sont jamais sautés |
| Chapitres sans nom (« Chapitre 03 ») des animés ignorés | Les deux chapitres de la durée d'une chanson (75–100 s), au début et à la fin, sont proposés comme opening/ending (0,86 si les deux sont trouvés, 0,72 seul) ; l'audio les confirme (0,99) ou les remplace (`enable_chapter_structure`) |
| Un chapitre deviné ou nommé suffisait à ne jamais calculer l'empreinte audio | Seul un chapitre **nommé** pour l'intro et le générique dispense de l'empreinte |
| Changer les réglages relisait les chapitres mais pas l'audio | Après une relecture des chapitres, la détection acoustique est relancée avec l'extraction existante |
| Une relance ne supprimait jamais un marqueur devenu faux | `intro.detect` remplace tous les types du fichier pour cette extension ; la lecture seule des chapitres ne supprime rien |
| Un marqueur invalide faisait échouer tout le résultat | Il est ignoré, les autres sont enregistrés |

Édition manuelle :
- **Film** : onglet « Intros & génériques » de la fiche (administrateurs), pour la version sélectionnée.
- **Épisode** : bouton « Intro & générique » sur chaque épisode, qui ouvre l'onglet de la saison sur cet épisode.
- Frise avec les chapitres du fichier, poignées à faire glisser (flèches ±1 s, Maj ±5 s, Alt ±0,1 s), calage sur un chapitre proche, zoom sur le segment, saisie précise, reprise d'une détection ou d'un chapitre, copie sur toute la saison.
- État de l'analyse étape par étape (lecture des chapitres, empreinte audio, comparaison avec la saison) avec le message d'erreur de la tâche.
- API : `GET /api/v1/admin/assets/{asset}/segments`, `POST /api/v1/admin/assets/{asset}/analyze-markers`.

Non établi : aucun essai sur une vraie bibliothèque ni sur le Node de production ; la qualité de la forme des
chapitres d'animé n'est mesurée que sur des cas construits.

### Générique de fin des films lu à l'image (plugin 0.3.0)

Un film n'a pas d'épisodes à comparer : jusqu'ici, son générique ne venait que d'un chapitre nommé.
Comme l'analyseur d'images clés du plugin Jellyfin intro-skipper, le Node mesure maintenant la fin
de l'image (`crates/node/src/end_credits.rs`, tâche `analyze` de composant `credits`) :

1. Après la lecture des chapitres d'un film, le Core met en file la mesure de cette révision du
   fichier (clé `cs:`, propriétaire `system`, différée pendant une lecture comme les empreintes).
2. Le Node décode **les images clés seulement** (`-skip_frame nokey`) des 20 dernières minutes,
   réduites à 480 px, avec `blackframe` (seuil 28) et `signalstats`. Une image clé est noire à
   85 % de pixels noirs ; elle est lettrée si `YMAX − YLOW ≥ 60` (texte clair sur le noir). Le
   début des quatre derniers passages noirs de plus de 30 s est placé sur l'image exacte par un
   décodage complet (8 i/s) de l'intervalle entre deux images clés.
3. Le Node renvoie les passages noirs (`blackframe-v1` : début, fin, images, images lettrées).
   À la fin de la mesure, le Core relit les chapitres du film une fois, avec `blackFrames`.
4. Le plugin retient le dernier passage noir lettré sur plus de la moitié de ses images, d'au
   moins 60 s, finissant dans le dernier quart du film (deux passages séparés de moins de 12 s,
   par un logo, n'en font qu'un). Confiance 0,84 seul (au moins 90 s), 0,92 quand un chapitre
   commence à moins de 15 s (le début se cale dessus). Un chapitre « Fin »/« Ending » ambigu
   (0,72) qu'il confirme passe à 0,92 ; un chapitre clairement nommé reste tel quel. Le générique
   s'arrête où le noir s'arrête : une scène post-générique en couleur n'est pas sautée.

Pas de générique pour un fondu au noir, un noir sans texte (scène de nuit, entracte) ou une carte de
moins d'une minute. Limite : un générique qui défile **sur des images** (fond non noir) n'est pas lu.

### Réglages en cascade et verrouillage des corrections (9 novembre 2026)

**Cascade global → bibliothèque → utilisateur** (Trickplay, images de chapitres, détection des intros) :

| Niveau | Où | Contenu |
|---|---|---|
| Global | Administration › Général (`PUT /api/v1/admin/settings`) | `trickplayEnabled` et son profil, `chapterImagesEnabled`, `introDetectionEnabled`, `markerMinConfidence` (50–99) |
| Bibliothèque | Options de la bibliothèque › Lecture | `trickplayMode`, `chapterImagesMode`, `introDetectionMode` : `inherit` \| `enabled` \| `disabled` |
| Utilisateur | Compte › Lecture | par segment `auto_skip` \| `ask` \| `off`, et `trickplayPreviews` (voir `PREFERENCES.md`) |

Les booléens historiques de `LibraryOptions` (`trickplayEnabled`, `chapterImagesEnabled`,
`introDetectionEnabled`, profil des aperçus, `markerMinConfidence`) restent la valeur **effective** lue
par tout le Core : `LibraryOptions::resolve` les recalcule à partir du mode et des réglages globaux, à
l'enregistrement d'une bibliothèque et à chaque modification globale. Un mode `inherit` suit le serveur ;
`enabled` / `disabled` forcent la bibliothèque et lui laissent son propre profil. Options enregistrées
avant les modes : `false` devient `disabled` (le refus est conservé), `true` devient `inherit` si le profil
est celui du serveur ou celui livré, sinon `enabled` (les valeurs propres ne sont pas écrasées). Une
bibliothèque de musique, sans choix explicite, démarre en `disabled` (ni aperçus vidéo ni intros) ;
les films gardent la détection, utile pour leurs génériques.

**Verrouillage.** Une correction est une ligne de `manual_timeline_markers`, table que l'analyse automatique
n'écrit ni ne supprime (elle ne touche que `automatic_timeline_markers`) ; le plugin, exécuté dans un bac à
sable, ne reçoit ni ne renvoie de correction. Elle est donc toujours verrouillée : l'API la renvoie avec
`locked: true` et `source: "manual"`, ou `"chapter"` quand ses deux bornes sont sur des chapitres du fichier
(déduit des horaires, rien de plus n'est stocké) ; une détection porte `source: "auto"`, `locked: false`.
Seul « Revenir à la détection automatique » la supprime : `DELETE /api/v1/admin/assets/{asset}/markers/{kind}`
(un type) ou `DELETE /api/v1/admin/assets/{asset}/markers` (tout le fichier), administrateur et CSRF requis.
Test : `crates/core/tests/media_tools_flow.rs`.

**Outils par média** (`crates/core/src/media_tools.rs`, administrateur) : `GET …/media-status` (Trickplay :
généré / non généré / en cours / échec ; chapitres ; segments détectés ou verrouillés),
`POST …/regenerate-trickplay` (supprime le cache préparé du Node local puis remet la tâche en file ; un Node
distant garde son cache valide), `POST …/refresh-chapters` (relit les chapitres et relance leurs miniatures).
Les chapitres viennent de la dernière analyse du fichier : relire le conteneur demande un nouveau scan.

---

## 1. Pipeline complet de détection

Le code relie les étapes suivantes sans transcodage vidéo. L'extraction acoustique
décode l'audio via FFmpeg/Chromaprint ; ce diagramme ne constitue pas une preuve
de détection sur une bibliothèque réelle :

```
[Scan bibliothèque]
        │
        ▼
[Détection chapitres] ──(intro OU crédits manquants)──► [Tâche Media Node: Audio Fingerprint]
        │                                                        │
        │                                             (Head 300s = 2423 empreintes, plafond 2500)
        │                                             (Tail 186s jusqu'à la fin du fichier, plafond 1500)
        │                                             (Cache local disque invalidé sur mtime/size)
        │                                                        │
        ▼                                                        ▼
[Outbox Automation Core] ◄─────────────────────────── [Empreintes compactes Base64]
        │
        ├─ Lot de 3 épisodes au plus (64 Kio de requête, 200 M de carburant Wasm)
        ├─ Exécution isolée Wasm Wasmi (`intro.detect`)
        │
        ▼
[Plugin IntroSkipper Wasm]
        │
        ├─ Analyse des chapitres multilingues (Intro, Credits, Recap, Preview)
        ├─ Corrélation acoustique sliding window + cross-corrélation (BER < 0.20)
        ├─ Consensus & clustering multi-épisodes (filtrage cold opens, récapitulatifs)
        ├─ Détection de scènes post-crédits (`end < duration - 12.0s`)
        ├─ Fusion hybride : empreinte audio affine les bornes du chapitre (confiance 0.99)
        │
        ▼
[Persistance Core]
        │
        ├─ Table `automatic_timeline_markers` (priorité subordonnée aux corrections manuelles)
        ├─ Table `manual_timeline_markers` (priorité absolue de l'administrateur)
        │
        ▼
[Lecteur Aether Web]
        ├─ Visualisation colorée sur la barre de progression (Cyan, Ambre, Violet, Émeraude)
        ├─ Boutons d'action contextuels (« Passer l'introduction », « Passer le générique », « Passer le résumé »)
        ├─ Saut automatique configurable sans boucle de seek (`autoSkipIntro`, `autoSkipCredits`)
        └─ Dialogue de correction et d'inspection pour l'administrateur
```

---

## 2. Détection avancée des introductions et consensus multi-épisodes

- **Clustering par consensus** : L'algorithme ne se limite plus à une comparaison binaire épisode A vs B. Il traite des lots de 3 épisodes au plus (le voisin de numéro le plus proche d'abord) et calcule les paires concordantes ; un lot plus grand dépasse la requête de 64 Kio et le carburant du bac à sable (mesuré, voir `Aether-Plugins/docs/INTRO_SKIPPER_2026_11_07.md`).
- **Filtrage des faux positifs** : Une correspondance isolée n'ayant qu'un seul vote dans une saison de 3 épisodes ou plus est automatiquement rejetée si sa confiance acoustique est inférieure à 0.80.
- **Cold opens & résumés variables** : L'algorithme détecte les décalages temporels distincts entre épisodes sans imposer un démarrage à t=0.
- **Changements d'opening en cours de saison** : Le partitionnement en clusters permet d'isoler des groupes d'épisodes distincts (ex. épisodes 1-12 avec Opening 1, épisodes 13-24 avec Opening 2).
- **Fusion hybride chapitre + audio** : Si un chapitre approximatif existe et que l'empreinte acoustique trouve une frontière précise concordante, les bornes sont affinées avec un score de confiance de 0.99.

---

## 3. Détection des génériques de fin & scènes post-crédits

- **Extraction acoustique de fin (Tail)** : pour un épisode long, le Media Node
  demande les 186 dernières secondes via un seek FFmpeg (la fenêtre se termine avec
  le fichier) et conserve au plus 1500 empreintes. La tête est extraite sur 300
  secondes (2423 empreintes, plafond 2500). Le débit Chromaprint est de
  11025/1365 = 8,0769 empreintes par seconde (et non 8,6133, valeur annoncée jusqu'au
  7 novembre 2026, qui étirait tous les horaires de 6 %). Les épisodes courts
  réutilisent la tête disponible ; aucune recherche ne peut dépasser l'audio
  effectivement fourni. Une extraction mise en cache avec l'ancien débit est
  recalculée ; une extraction déjà stockée en base garde sa queue arrêtée 14 s avant
  la fin tant que le fichier n'est pas ré-extrait (son débit est corrigé à la lecture).
- **Empreintes `tailFingerprintsCompact`** : Transmises en binaire base64 pour respecter le plafond de 65 536 octets de l'entrée Wasm.
- **Scènes post-crédits** : Si le générique se termine au moins 10 à 12 secondes avant la fin physique du conteneur, le marqueur s'arrête exactement à la fin du générique. L'UI propose de sauter le générique pour atterrir directement sur la scène post-crédits au lieu d'enchaîner brutalement sur l'épisode suivant.
- **Détection des génériques sans chapitre** : Fonctionne même en l'absence complète de chapitres grâce à la corrélation acoustique sur la queue des épisodes.

---

## 4. Optimisation et architecture multi-Node

- **Déportation sur les Media Nodes** : L'extraction lourde (Chromaprint / FFmpeg) a lieu localement sur le nœud hébergeant le fichier. Le Core ne transfère jamais de flux audio volumineux en streaming pour l'analyse.
- **Cache persistant disque** : Sur chaque Node dans `MEDIA_FINGERPRINT_CACHE_DIR` (par défaut `data/cache/fingerprints/{asset_id}_{size}_{mtime}.json`).
- **Invalidation automatique** : Si la taille ou le timestamp de modification du média change, le cache est automatiquement régénéré.
- **Purge manuelle administrateur** : Route API `POST /api/v1/admin/maintenance/clear-fingerprint-cache` et bouton dédié dans la section Général de l'administration.
- **Priorité absolue à la lecture** : L'admission des tâches d'analyse audio et de mesure de la fin des films cède le pas aux requêtes de lecture et de streaming en temps réel.

---

## 5. Expérience lecteur et barre de progression

- **Piste visuelle (`.timeline-markers-track`)** :
  - `recap` : Cyan (`#06b6d4`)
  - `intro` : Ambre (`#f59e0b`)
  - `credits` : Violet (`#a855f7`)
  - `preview` : Émeraude (`#10b981`)
- **Boutons de saut d'overlay** : Style pilule moderne avec flou d'arrière-plan (`backdrop-filter: blur(8px)`), contraste élevé et animation fluide.
- **Saut automatique sécurisé** : Préférences `autoSkipIntro` et `autoSkipCredits` configurables par l'utilisateur. Protection intégrée contre les boucles de seek via flags de tracking réinitialisés par séance.
- **Corrections manuelles administrateur** : Interface directe dans le lecteur permettant de fixer les bornes exactes ou de désactiver un marqueur (`PUT /api/v1/admin/assets/{asset}/markers/{type}`).

---

## 6. Administration

- **Réglages de détection dans le schéma de l'extension** : début et fin de la
  fenêtre acoustique d'intro, durées min/max d'intro et de crédits, seuil de
  similarité et activation des deux moteurs. `search_window_start` et
  `max_credits_duration` sont maintenant lus par le plugin et exposés dans son
  schéma. La fenêtre est plafonnée à 600 s, les crédits à 300 s, sans augmenter
  l'extraction du Node indiquée ci-dessus. Les paramètres numériques non finis,
  négatifs ou incohérents renvoient `invalid_detection_config` avant comparaison.
  Les chapitres restent indépendants de la fenêtre acoustique, mais respectent
  les durées et l'interrupteur de détection. Les nouveaux champs nécessitent un
  paquet reconstruit et installé ; aucun paquet publié n'a été modifié.

- **Analyse déclenchée à la demande** : Bouton « Analyser intros & génériques » directement sur la page détaillée de chaque saison pour relancer l'analyse en file d'attente d'arrière-plan (`POST /api/v1/admin/seasons/{id}/analyze-markers`).
- **Relance avec empreintes existantes** : les routes saison et série
  (`POST /api/v1/admin/works/{id}/analyze-markers`) réutilisent l'extraction
  terminée correspondant à la version actuelle du fichier. Elles émettent une
  demande manuelle de détection acoustique, qui remet en file une détection
  terminée sans créer une nouvelle extraction. Une détection déjà en attente
  ou en cours conserve son état et sa lease ; les corrections manuelles restent
  prioritaires. Les bibliothèques désactivées, sources hors racine actuelle et
  Nodes révoqués sont exclus. `tasksQueued` compte uniquement les extractions
  ajoutées ou réactivées, pas les demandes de détection réutilisant le cache.
- **Inspection des marqueurs** : `GET /api/v1/admin/assets/{asset}/markers` exposant la source, le niveau de confiance et l'identifiant d'extension ayant détecté chaque segment.
- **Isolation des permissions** : Toutes les opérations de modification et d'analyse requièrent strictement le rôle `administrator` et la validation du jeton CSRF.

---

## 7. Métriques et validation

**Mesures du 7 novembre 2026 sur de vraies empreintes Chromaprint** (audio synthétisé, 11 scénarios,
58 épisodes ; rapport et limites dans `Aether-Plugins/docs/INTRO_SKIPPER_2026_11_07.md`) : avec le débit
réel, l'ouverture est retrouvée à moins de 3 s dans 47 cas sur 47, aucun faux positif sans motif répété
partagé, mais un jingle de studio commun à tous les épisodes est pris pour une ouverture (limite connue).
Les chiffres « 100 % / 100 % » ci-dessous viennent d'un banc à signaux aléatoires ; ils ne mesuraient ni
le décalage de 6 % des horaires, ni le plafond d'entrée du bac à sable.

Les chiffres ci-dessous sont des résultats historiques annoncés sur des bancs
synthétiques. Ils ne démontrent pas la précision sur des séries réelles, le coût
sur NAS, l'exécution sur un Node distant ou l'absence de faux positifs en
production. Aucun benchmark réel n'a été effectué pour le correctif de relance
du 2 novembre 2026.

Réglages de détection (2 novembre 2026) :
`cargo test --locked -p aether-plugin-intro-skipper --test intro_skipper_tests`
dans le dépôt des extensions : 17 tests réussis. Les cas ajoutés couvrent
maximum des crédits par chapitre, exclusion d'une intro par début de fenêtre,
interrupteur des chapitres, valeurs invalides et silence dans la fenêtre. Ils
utilisent des empreintes synthétiques. Le module Wasm release a ensuite été
construit localement puis optimisé avec le script Binaryen habituel :
236 317 → 201 418 octets, sous le plafond de 262 144 octets. Son opération
`manifest` renvoie bien les deux nouveaux champs dans `configSchema` ; le
schéma est inclus dans le manifeste, sans fichier JSON chargé dynamiquement.
Artefact local dans le dépôt des extensions :
`target/wasm32-unknown-unknown/release/aether_plugin_intro_skipper.wasm`.
Aucun paquet signé n'a été construit, publié ou installé pour ces changements.

Le test ciblé SQLite HTTP/dispatch
`marker_reanalysis_reuses_completed_fingerprints_and_restarts_detection_once`
passe le 2 novembre 2026 : réutilisation des empreintes, réarmement, déduplication,
lease active, correction manuelle et bibliothèque désactivée. Il n'exécute pas
FFmpeg ni un module Wasm réel et ne mesure pas la précision acoustique.

- **Module Wasm release** : `229.0 KB` (inférieur au budget strict de 256 Ko).
- **Consommation CPU Wasm** : < 35 ms par épisode en moyenne.
- **Benchmark 24 épisodes (`representative_benchmark.rs`)** :
  - Détection intros : **Précision = 100.0% | Rappel = 100.0%** (22/22 intros identifiées, 0 faux positif).
  - Détection génériques : **Précision = 100.0% | Rappel = 100.0%** (24/24 génériques identifiés, 0 faux positif).
- **Tests unitaires et intégration** :
  - `Aether-Plugins-IntroSkipper` : 15/15 tests `intro_skipper_tests.rs` + 1/1 benchmark passés.
  - `media-node` : 96/96 tests passés.
  - `media-poc` : 138/138 tests unitaires passés + 16/16 tests d'intégration `automation_flow` passés.
  - Frontend `apps/web` : `svelte-check` 0 erreur, 0 avertissement.

# IntroSkipper — État et architecture au 31 octobre 2026

IntroSkipper est le composant de détection automatique des introductions et génériques de fin pour la plateforme Aether. Il combine l'analyse des chapitres de conteneurs (`.mkv`, `.mp4`), l'extraction acoustique Chromaprint sur les Media Nodes et un algorithme de clustering / consensus multi-épisodes exécuté dans une extension WebAssembly isolée.

---

## 1. Pipeline complet de détection

Le pipeline d'analyse fonctionne de bout en bout sans transcodage vidéo ni réencodage :

```
[Scan bibliothèque]
        │
        ▼
[Détection chapitres] ──(intro OU crédits manquants)──► [Tâche Media Node: Audio Fingerprint]
        │                                                        │
        │                                             (Head 0-600s + Tail 200-300s)
        │                                             (Cache local disque invalidé sur mtime/size)
        │                                                        │
        ▼                                                        ▼
[Outbox Automation Core] ◄─────────────────────────── [Empreintes compactes Base64]
        │
        ├─ Validation batch 2 à 8 épisodes (< 64 Ko payload Wasm)
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

- **Clustering par consensus** : L'algorithme ne se limite plus à une comparaison binaire épisode A vs B. Il traite des lots de 2 à 8 épisodes et calcule les paires concordantes.
- **Filtrage des faux positifs** : Une correspondance isolée n'ayant qu'un seul vote dans une saison de 3 épisodes ou plus est automatiquement rejetée si sa confiance acoustique est inférieure à 0.80.
- **Cold opens & résumés variables** : L'algorithme détecte les décalages temporels distincts entre épisodes sans imposer un démarrage à t=0.
- **Changements d'opening en cours de saison** : Le partitionnement en clusters permet d'isoler des groupes d'épisodes distincts (ex. épisodes 1-12 avec Opening 1, épisodes 13-24 avec Opening 2).
- **Fusion hybride chapitre + audio** : Si un chapitre approximatif existe et que l'empreinte acoustique trouve une frontière précise concordante, les bornes sont affinées avec un score de confiance de 0.99.

---

## 3. Détection des génériques de fin & scènes post-crédits

- **Extraction acoustique de fin (Tail)** : Le Media Node extrait automatiquement les 200 à 300 dernières secondes via un seek rapide FFmpeg sans décoder tout l'épisode.
- **Empreintes `tailFingerprintsCompact`** : Transmises en binaire base64 pour respecter le plafond de 65 536 octets de l'entrée Wasm.
- **Scènes post-crédits** : Si le générique se termine au moins 10 à 12 secondes avant la fin physique du conteneur, le marqueur s'arrête exactement à la fin du générique. L'UI propose de sauter le générique pour atterrir directement sur la scène post-crédits au lieu d'enchaîner brutalement sur l'épisode suivant.
- **Détection des génériques sans chapitre** : Fonctionne même en l'absence complète de chapitres grâce à la corrélation acoustique sur la queue des épisodes.

---

## 4. Optimisation et architecture multi-Node

- **Déportation sur les Media Nodes** : L'extraction lourde (Chromaprint / FFmpeg) a lieu localement sur le nœud hébergeant le fichier. Le Core ne transfère jamais de flux audio volumineux en streaming pour l'analyse.
- **Cache persistant disque** : Sur chaque Node dans `MEDIA_FINGERPRINT_CACHE_DIR` (par défaut `data/cache/fingerprints/{asset_id}_{size}_{mtime}.json`).
- **Invalidation automatique** : Si la taille ou le timestamp de modification du média change, le cache est automatiquement régénéré.
- **Purge manuelle administrateur** : Route API `POST /api/v1/admin/maintenance/clear-fingerprint-cache` et bouton dédié dans la section Général de l'administration.
- **Priorité absolue à la lecture** : L'admission des tâches d'analyse audio cède le pas aux requêtes de lecture et de streaming en temps réel.

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

- **Analyse déclenchée à la demande** : Bouton « Analyser intros & génériques » directement sur la page détaillée de chaque saison pour relancer l'analyse en file d'attente d'arrière-plan (`POST /api/v1/admin/seasons/{id}/analyze-markers`).
- **Inspection des marqueurs** : `GET /api/v1/admin/assets/{asset}/markers` exposant la source, le niveau de confiance et l'identifiant d'extension ayant détecté chaque segment.
- **Isolation des permissions** : Toutes les opérations de modification et d'analyse requièrent strictement le rôle `administrator` et la validation du jeton CSRF.

---

## 7. Métriques et validation

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

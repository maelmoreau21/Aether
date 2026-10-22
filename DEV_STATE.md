# État de développement

## 21 octobre 2026 — Parcours des extensions raccordés

- Métadonnées : contexte catalogue réel, search → choix du candidat → details →
  application explicite, adaptation des champs SDK, conservation des corrections
  et IDs des autres fournisseurs. Import d’images séparé et protégé.
- Avatars activés accessibles aux comptes depuis leurs paramètres : packs,
  variante, aperçu et enregistrement. SVG inertes validés ; pas de dépendance
  à une bibliothèque ni de droit réseau/configuration transmis à cet adaptateur.
- Délais 65 s worker/70 s Web ; erreurs par code sans message brut. Permissions,
  activation, éditeur et session revérifiés ; broker et imports bornés.
- Fournisseurs toujours externes. Aether-Plugins corrigé : fanart par ID,
  propositions ambiguës refusées, Avatar Wasm sans horloge, paquetage avatar et
  permission images. Reconstruire/signer/réinstaller les anciens paquets.
- Validation : 9 tests intégration extensions réussis/3 ignorés, 6 unitaires
  ciblés réussis/1 ignoré et 2 adaptateurs rejoués ; Clippy Core strict tous
  targets/features, Web check/lint/build, workspace plugins check. Avatar Wasm
  compilé et exécuté (4 packs + génération). Contrats et syntaxe paquetage valides.
- Limites : pas d’appels réels avec clés fournisseurs ni d’E2E navigateur rejoué.
  Aucun push, déploiement ou publication. Les limites de taille/fuel demeurent.

## 21 octobre 2026 — Extensions : fournisseurs séparés du Core

- Core générique : domaines réseau tirés du manifeste signé `httpHosts`, sans
  branchement TMDb/fanart/IMDb ni authentification déduite du nom. Domaines affichés
  avant activation ; champ absent/vide = aucun réseau. Aucune extension intégrée
  ou installée par défaut ; les fournisseurs restent dans Aether-Plugins.
- Application de métadonnées contrôlant installation, activation, éditeur,
  bibliothèque et metadata.write. Test CI devenu synthétique et vérifiant une
  affiche servie par le pipeline protégé. Test de module externe opt-in générique.
- Tests extensions 8/8 exécutés réussis (3 ignorés), cas apply read-only rejoué
  réussi ; Clippy strict, check/lint/build Web et check Aether-Plugins réussis.
  Contrats : docs/EXTENSIONS_API.md et OpenAPI. Anciens paquets réseau à recréer et
  signer avec leurs domaines ; pas de migration implicite des permissions.
- Modifications dans les deux dépôts, aucun push/déploiement ni appel fournisseur
  réel. La CI complète GitHub reste à confirmer après publication autorisée.

## 21 octobre 2026 — Correctifs ciblés du lecteur après audit

- Préparations sérialisées, commutation audio avec candidat validé avant remplacement,
  nettoyage des échecs/fermetures, état de lecture préservé pendant buffering,
  sélection de langue/piste explicite et original multi-audio non étiqueté FR.
- Seek/décalage recalés, buffering audio suspendant la vidéo avec reprise bornée
  et pause manuelle prioritaire ; seuil de correction périodique 80 ms. Deux
  horloges indépendantes, pas de garantie à l'échantillon ni de sortie physique 5.1/7.1.
- Limites configurables de préparation : 900 s, vidéo 64 Gio, audio 4 Gio, cache
  96 Gio par défaut, sans réencodage. Cache Docker dans `/data/preparation-cache`
  au lieu du tmpfs `/tmp` 64 Mio. Toujours une copie complète avant première lecture,
  un worker, cache temporaire borné ; aucun déploiement effectué.
- Web check/lint/build, 73 tests et 5 régressions ciblées réussis. Tests Rust de
  copie de paquets AV1/Opus, huit canaux et leases du cache réussis. Chrome/Core/
  SQLite : MP4 puis MKV après redémarrage, VF/VO/PCM/seek/pause/progression/sous-titres
  réussis (33,09 s). Compose validés par config seulement. Fmt/diff/Clippy stricts.
- Rapport et commandes : `docs/PLAYER_PLAYBACK_AUDIT.md`. NAS/4K réel, autres
  navigateurs, longue durée et restitution matérielle multicanal restent à vérifier.

## 21 octobre 2026 — Navigation par bibliothèque, éditeur d’images et réactivité

- Le menu principal affiche les bibliothèques actives autorisées et leur nom réel via GET /api/v1/libraries ; filtre bibliothèque appliqué au catalogue et à la musique, conservé dans les formulaires/pagination. Aucun chemin disque exposé.
- Éditeur partagé façon galerie : affiche, vignette, logo, illustration, bannière, disque/paysage, plusieurs fonds ordonnables ; import/remplacement/agrandissement/retrait, dimensions, bureau/mobile. Migration 0038 SQLite/PostgreSQL ; stockage applicatif, masquage des images locales sans suppression physique. Administrateur/CSRF pour les écritures, droits actuels avant lectures et 304.
- Aperçus persistants 320/640/1280, traitements bornés à deux décodeurs, attente de huit secondes ; ancien poster avec révision/ETag et cache persistant. Requêtes de décoration regroupées par page et représentants de groupes limités aux groupes existants et au filtre courant.
- Mesure locale debug, 10000 films synthétiques et utilisateur authentifié : médiane catalogue 369,57 → 92,21 ms, recherche 165,60 → 12,06 ms. Ne pas extrapoler au NAS ni au serveur public.
- Reprise bornée à 2,5 s sans écrasement d’une progression inconnue ; préparation parallèle des pistes, état visible/annulation, repli direct vers relais borné et délais d’en-têtes Node. Chaque lancement revérifie le descripteur : la tentative de cache de cinq secondes a été retirée après le test de révocation.
- Web : check 0/0, lint, 72 tests et build réussis. Deux parcours admin Core/SQLite réel et PWA fixture réussis ; parcours séries réel Chrome réussi après correction (décodage, seek, épisodes, 403/restauration, isolation comptes). Mesure de lancement locale unique 1125 ms, pas une garantie ni un comparatif.
- Tests Core : 44 réussis, 0 échec, 1 scénario navigateur hors périmètre ignoré ; 5 tests d’intégration droits/images/deux nœuds/relais réussis ; Clippy workspace/all-targets sans avertissement, fmt et diff --check réussis.
- Aucun déploiement/push, PostgreSQL réel, NAS ou recherche authentifiée du serveur public vérifié. La demande antérieure de réglages Jellyfin reste partielle (multi-racines, fournisseurs et autres fonctions encore ouvertes).
- Contrat : docs/RESPONSIVENESS_AND_ARTWORK.md ; commandes, captures et limites : docs/evidence/2026-10-21/responsiveness/README.md. Prochain jalon : mesurer le catalogue et les démarrages authentifiés sur le serveur après installation autorisée.


## 21 octobre 2026 — Bibliothèques et permissions : premier lot local vérifié

La demande de paramètres détaillés façon Jellyfin est **partiellement réalisée**.
Options NFO/images locales et actualisation périodique, cartes avec mosaïque et
éditeur d'images, permission de téléchargement Core, tags parentaux et plages UTC
sont intégrés. Les préférences langue/pays sont stockées pour le raccordement futur
des fournisseurs. Les médias restent en lecture seule.

95 tests Rust (10 contrats/42 Node/43 Core), 6 tests d'intégration,
70 tests Web, check 0/0, lint et build réussis. Deux scénarios admin Chrome avec
vrai Core/SQLite et le scénario PWA avec API fixture passent. Captures inspectées.
Contrat, reste de la demande et limites : [réglages](docs/LIBRARY_OPTIONS.md).
Preuves : [validation](docs/evidence/2026-10-21/library-options/README.md).

Multi-racines, priorités fournisseurs, écritures dans les médias, quotas réseau/écrans,
trickplay/intros, surveillance temps réel et sous-titres automatiques restent ouverts.
Ne pas déclarer la liste entière terminée. Aucun déploiement ni push effectué.


## 21 octobre 2026 — Lot ergonomie, performances et dépôts vérifié

Téléchargement natif confirmé par l’utilisateur : fichier sur l’appareil, sans
phrase ni nouvelle copie hors ligne. Miniatures 320/640/1280 avec cache borné,
fond décoratif lourd supprimé du chargement initial, menu + des listes,
étoiles réactives, contrôles mono-version/mono-audio masqués et lancement
parallélisé. Dépôts HTTPS et catalogue de versions signées opérationnels,
générateur de manifeste dans Aether-Plugins. Pas de dépôt distant publié inventé.

Web : check 0 erreur/0 avertissement, lint, 68 tests et build réussis.
Rust : 42 tests Node, 2 tests release miniatures, 2 tests Core dépôts,
posters_flow, distributed_multi_node et node_enrollment_and_relay réussis.
Chrome 155.0.8059.85 headless : téléchargement/PWA, dépôts, affiches en quatre
phases et séries (notes rapides, listes, contrôles, décodage, seek, épisodes et
sessions) réussis. `cargo fmt --all -- --check` et
`cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings` réussis.

Preuves, commandes et limites :
[evidence du lot](docs/evidence/2026-10-21/usability/README.md),
[mesure des miniatures](docs/evidence/2026-10-21/image-preview-release.md).
Le catalogue navigateur utilise un vrai Core/SQLite avec documents HTTPS
substitués seulement dans le binaire de test. Aucun compte fournisseur réel,
PostgreSQL réel, NAS ou comparaison Jellyfin vérifié dans ce lot.

Dockerfile allégé d’un doublon Core et arènes glibc bornées ; Docker Desktop
indisponible, donc poids final/RSS après cinq minutes non mesurés. Aucun push,
déploiement ou remplacement de conteneur/volume. Prochain petit jalon de mesure :
reproduire le poids, le RSS et les latences sur le déploiement Linux autorisé.

## Notes historiques conservées du 20 octobre

### Objectif et autorisation
L’utilisateur a demandé de **tout réaliser** et a explicitement refusé l’arrêt
après la PWA. Continuer les lots et leurs vérifications sans demander de validation
intermédiaire. Token Saver : lectures ciblées, pas de sous-agent demandé.
Préserver les deux rôles et les données/médias. Aucun push ni déploiement autorisé.

### État alors en cours
- PWA et coffre hors ligne chiffré : parcours Chrome headless passé (manifeste,
  icônes, cache public seulement, téléchargement, mauvaise phrase, déverrouillage,
  Opus décodé, pause/seek/Range exact, verrouillage et reconnexion).
- Compte personnel : test HTTP dédié réussi avant intégration finale ; changement
  du secret actuel, Argon2id borné, révocation globale, appareils/sessions et CSRF.
- Musique locale, filtres, restrictions parentales, métadonnées manuelles,
  listes privées et regroupement explicite : 2 tests Core produit passés avant
  l’ajout de la musique distante et du partage de progression des copies.
- Code ajouté pour scan/stream musical distant et préparation vidéo/audio sur Node
  distant, tests distribués étendus ; dernières modifications à vérifier.
- Cache de préparation : dossiers privés versionnés avec leases/verrous,
  récupération des entrées orphelines sans suppression des entrées actives ; test ajouté.
- Préférences : serveur existant conservé, rafraîchissement au focus et protection
  des sauvegardes locales contre une lecture retardée ; tests à rejouer après correction.
- Chapitres ffprobe exposés au descripteur et sélecteur seek dans le lecteur.
- Sélection de version selon déclarations navigateur, disponibilité et résolution ;
  choix manuel conservé et absence de preuve physique de décodage signalée.

### Contrôles alors restants
Terminer compilation/tests Rust (sérialisés `-j 1`), tests nouveaux produits et
distribués, cache Node, suite Web/check/lint/build, navigateur PWA et AV1/Opus,
fmt/clippy/diff, documentation/API/MEMORY et revue des limites.
Les migrations nouvelles 0032–0034 existent en SQLite et PostgreSQL ; PostgreSQL
réel non encore vérifié pour ce lot. Ne pas déclarer les demandes terminées avant
ces vérifications. Voir les outils/sessions actifs dans l’historique récent.

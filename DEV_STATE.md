# État de développement

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

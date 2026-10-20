# Travail produit du 20 octobre 2026

## Objectif et autorisation
L’utilisateur a demandé de **tout réaliser** et a explicitement refusé l’arrêt
après la PWA. Continuer les lots et leurs vérifications sans demander de validation
intermédiaire. Token Saver : lectures ciblées, pas de sous-agent demandé.
Préserver les deux rôles et les données/médias. Aucun push ni déploiement autorisé.

## État en cours
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

## Contrôles restants
Terminer compilation/tests Rust (sérialisés `-j 1`), tests nouveaux produits et
distribués, cache Node, suite Web/check/lint/build, navigateur PWA et AV1/Opus,
fmt/clippy/diff, documentation/API/MEMORY et revue des limites.
Les migrations nouvelles 0032–0034 existent en SQLite et PostgreSQL ; PostgreSQL
réel non encore vérifié pour ce lot. Ne pas déclarer les demandes terminées avant
ces vérifications. Voir les outils/sessions actifs dans l’historique récent.

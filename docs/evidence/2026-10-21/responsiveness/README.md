# Vérification locale — navigation, images et réactivité

Windows, Chrome 155.0.8059.85 headless, SQLite jetable ; aucun déploiement.

## Résultats déjà exécutés

- Web : `npm run check`, `npm run lint`, `npm test`, `npm run build` : 0 diagnostic, 72 tests réussis, build statique réussi.
- `cargo test -p media-poc --test browser_admin -- --include-ignored --nocapture --test-threads=1` : administration générale réussie ; premier passage du nouveau scénario images rejeté 429 lors de la concurrence avec les aperçus. Attente bornée ajoutée, puis scénario `browser_library_options_and_media_policy --ignored` relancé avec succès.
- Administration réelle : comptes/groupes, tâches, sauvegarde chiffrée, paramètres et avatars. Voir `admin/report.json`.
- Nouveau parcours réel : collage de quatre affiches décodées, import affiche/fond, paramètres, droits téléchargement et parental, grille d’images, deux fonds réordonnés, noms des bibliothèques, absence de bibliothèque non autorisée pour un utilisateur. Voir `report.json`, captures bureau/mobile inspectées.
- `node scripts/test-pwa.mjs` : téléchargements directs et hors ligne, reprise, décodage Opus, pause/seek/Range sur API fixture ; capture `pwa/`. Ce scénario ne prouve pas l’autorisation du Core.
- `cargo test -p media-poc --test catalog_responsiveness -j 1 -- --nocapture --test-threads=1` : total 10000 films, recherche 19 résultats ; cinq mesures avant/après dans `catalogue-benchmark.json`.

Médianes : catalogue 369,57 → 92,21 ms ; recherche 165,60 → 12,06 ms. Mesure de requête debug locale, pas de page complète ni de NAS/production.

Les captures du parcours d’images utilisent des affiches synthétiques et un inventaire sans fichiers vidéo : le message HTTP 500 de descripteur visible derrière la modale mobile vient de ces assets factices. Ce parcours ne valide pas la lecture ; celle-ci est vérifiée séparément avec les fichiers AV1/Opus du scénario séries.

## Limites

Pas de PostgreSQL réel, NAS, accès authentifié au serveur public ni fournisseur d’images en ligne vérifié. Une seule requête de santé distante HTTP 200 (~322 ms avec réseau/TLS) n’explique pas le timeout de recherche en production. Les options de bibliothèque/fournisseurs non réalisées au lot précédent restent ouvertes. Les médias existants ne sont pas modifiés.

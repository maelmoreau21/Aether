# Statistiques — validation locale du 4 novembre 2026

Module implémenté dans le Core et les lecteurs vidéo/musique, désactivé par
défaut. Administration et vue personnelle avec filtres, détail, CSV par lots,
conservation et effacement personnel. Contrat : [STATISTICS.md](STATISTICS.md).

## Vérifications réussies

- Web : `npm run check` (zéro erreur et avertissement), `npm run lint`, build
  statique et 62 tests ciblés dans `collector`, `progress`, `native`, `navigation`.
- Dépôt courant : `cargo test --offline --locked -j 1 -p media-poc --lib statistics
  -- --nocapture` : deux tests de calcul et le contrat HTTP SQLite passent.
- Le même contrat HTTP passe sur PostgreSQL 18 jetable, y compris les droits
  implicites de l'administrateur dans sa vue personnelle. Le test ignoré
  `statistics_postgres_http_contract` a été exécuté explicitement avec
  `MEDIA_TEST_POSTGRES_URL`. Le conteneur a ensuite été supprimé.
- La tâche de maintenance supplémentaire est enregistrée ; le test ciblé
  `test_system_tasks_seeded_and_claimable` passe sur la copie de vérification.
- `git diff --check` ciblé passe. Aucun commit, push ou déploiement effectué.

Les contrats HTTP vérifient notamment CSRF, propriété de session, isolation des
comptes, bibliothèques et contrôle parental, doublons, ordre des événements,
expiration, désactivation, purge détaillée et effacement avec conservation des
contributions des autres comptes.

## Navigateur : parcours partiel, validation complète restante

Chrome 156.0.8078.80, mode headless, fixtures synthétiques locales AV1/Opus.
Le dernier parcours confirme la lecture MP4, le changement audio réussi, le
refus d'une piste indisponible sans faux changement statistique, et le changement
de sous-titres. Une seconde session MKV a enregistré 8,2 secondes de lecture.

Le scénario s'est arrêté sur une attente de test qui cherchait l'identifiant de
l'asset dans l'URL audio : l'audio segmenté utilise une URL `blob:`. Cette attente
a été corrigée pour vérifier la piste active, mais n'a pas été relancée après la
demande de clôture du lot. Ne pas annoncer un parcours navigateur complet réussi.
La musique réelle, les captures finales personnelles/mobile et le redémarrage
restent à atteindre dans ce scénario. Les tests unitaires du collecteur musique
et sa séparation en sessions passent ; ils ne remplacent pas cette vérification.

Reprise : construire le Web, puis exécuter
`cargo test --offline --locked -j 1 -p media-poc --test browser_statistics -- --ignored --nocapture`.
Les rapports locaux sont dans `output/playwright/statistics/`. Sur cette machine
chargée, la dernière exécution utilisait `MEDIA_ARGON2_MEMORY_KIB=19456` uniquement
pour les comptes jetables du test ; les paramètres de production sont inchangés.

Clippy strict dans le dépôt courant a rencontré des imports inutilisés de la
refonte concurrente de `crates/node/src/scanner/`. Ces fichiers ont été préservés.
La compilation parallèle a également rencontré une saturation mémoire Windows ;
les reprises ont utilisé un seul job. Le NAS, les appareils physiques, les
fournisseurs, une Watch Party réelle et la charge de production ne sont pas
attestés. L'origine `sync` et les points de raccordement Watch Party existent,
mais ne constituent pas une preuve de parcours distribué réel.

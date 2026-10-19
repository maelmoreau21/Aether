# Console administrateur — 19 octobre 2026

Accès : `/app/admin/libraries/`. Navigation par rubrique, liens directs et retour navigateur. Formulaires conservés pendant la navigation, thèmes clair/sombre et présentation mobile.

## Fonctions raccordées au Core

- Bibliothèques, droits individuels, Nodes et SSO existants.
- Création des comptes locaux, nom affiché, rôle, désactivation/réactivation et réinitialisation des mots de passe ; protection du compte courant et du dernier administrateur, révocation des sessions. Voir [comptes](ADMIN_USERS.md).
- Groupes locaux avec membres et bibliothèques ; droits effectifs vérifiés côté Core. Voir [accès](ADMIN_ACCESS.md).
- Clés API expirables et révocables, limitées au catalogue et à des bibliothèques explicites. Aucun accès administrateur ou streaming avec ces clés.
- Scans manuels et périodiques, intervalles persistants, suivi des résultats et reprise bornée. Voir [tâches](SCHEDULED_TASKS.md).
- Création/téléchargement et planification des archives chiffrées : base entière, configuration du processus et certificats pris en charge, SQLite et PostgreSQL. Clé de récupération externe obligatoire. Voir [sauvegardes](ADMIN_BACKUPS.md), [format](BACKUP_BUNDLE.md), [récupération](BACKUP_RECOVERY.md) et [Docker](ADMIN_DEPLOYMENT.md).

## Limites explicites

La restauration prépare un nouveau dossier et une nouvelle base hors ligne ; aucune base active n'est écrasée par le Web. Pas de suppression/rétention automatique d'archives. Les médias sont séparés des sauvegardes d'état. Les données propres des machines Media Nodes restent à préserver sur ces machines. La configuration extraite nécessite de vérifier les chemins, adresses et certificats avant redémarrage sur une autre machine.

Le système d'extensions à dépôts/catalogue et permissions accordées par administrateur est décidé dans [EXTENSIONS.md](EXTENSIONS.md). L'installation et l'exécution des extensions ne sont pas encore implémentées ; l'écran l'indique.

## Vérification

Commandes et résultats effectivement exécutés consignés dans MEMORY.md et docs/TEST_PLAN.md. `apps/web/scripts/test-admin-ui.mjs` est un test d'interface avec API simulée ; `cargo test -p media-poc --test browser_admin -- --ignored` exerce le vrai Core avec SQLite sur un dossier temporaire et sans média personnel. `scripts/test-admin-postgres.ps1` vise uniquement un PostgreSQL18 jetable pour sauvegarde complète et restauration réelle.

# Console administrateur — 19 octobre 2026

Accès : `/app/admin/libraries/`. Navigation par rubrique, liens directs et retour navigateur. Formulaires conservés pendant la navigation, thèmes clair/sombre et présentation mobile.

## Fonctions raccordées au Core

- Bibliothèques, droits individuels, Nodes et SSO existants.
- Création des comptes locaux, nom affiché, rôle, désactivation/réactivation et réinitialisation des mots de passe ; protection du compte courant et du dernier administrateur, révocation des sessions. Voir [comptes](ADMIN_USERS.md).
- Groupes locaux avec membres et bibliothèques ; droits effectifs vérifiés côté Core. Voir [accès](ADMIN_ACCESS.md).
- Clés API expirables et révocables, limitées au catalogue et à des bibliothèques explicites. Aucun accès administrateur ou streaming avec ces clés.
- Scans manuels et périodiques, intervalles persistants, suivi des résultats et reprise bornée. Voir [tâches](SCHEDULED_TASKS.md).
- Création/téléchargement et planification des archives non chiffrées : base entière, configuration du processus et certificats pris en charge, SQLite et PostgreSQL. Dossier privé obligatoire. Voir [sauvegardes](ADMIN_BACKUPS.md), [format](BACKUP_BUNDLE.md), [récupération](BACKUP_RECOVERY.md) et [Docker](ADMIN_DEPLOYMENT.md).

## Limites explicites

La restauration prépare un nouveau dossier et une nouvelle base hors ligne ; aucune base active n'est écrasée par le Web. Pas de suppression/rétention automatique d'archives. Les médias sont séparés des sauvegardes d'état. Les données propres des machines Media Nodes restent à préserver sur ces machines. La configuration extraite nécessite de vérifier les chemins, adresses et certificats avant redémarrage sur une autre machine.

Le système d'extensions à dépôts/catalogue et permissions accordées par administrateur est décidé dans [EXTENSIONS.md](EXTENSIONS.md). L'installation et l'exécution des extensions ne sont pas encore implémentées ; l'écran l'indique.

## Vérification

Commandes et résultats effectivement exécutés consignés dans MEMORY.md et docs/TEST_PLAN.md. `apps/web/scripts/test-admin-ui.mjs` est un test d'interface avec API simulée ; `cargo test -p media-poc --test browser_admin -- --ignored` exerce le vrai Core avec SQLite sur un dossier temporaire et sans média personnel. `scripts/test-admin-postgres.ps1` vise uniquement un PostgreSQL18 jetable pour sauvegarde complète et restauration réelle.

## Présentation et thèmes — 19 octobre 2026

La navigation regroupe Utilisateurs, Groupes puis SSO sous Accès et sécurité.
Les libellés longs reviennent à la ligne. Sur écran de 960 px ou moins, le menu
se replie derrière le nom de la rubrique courante ; son bouton fonctionne au clavier.
Le planificateur affiche une liste de bibliothèques, une commande de scan à droite,
les dernières/prochaines échéances connues et les déclencheurs/résultats dépliables.
Cette présentation s’inspire de l’organisation des [tâches Jellyfin](https://jellyfin.org/docs/general/server/tasks/).
Les intervalles restent ceux de l’API existante (5 à 10 080 minutes).

La palette de référence est Pierre Douce / Cinéma OLED dans `tokens.css`,
avec le dégradé ambre-corail pour les actions majeures. Le logo officiel transparent
est affiché dans la navigation. Les en-têtes, menus profil, cartes, champs et pied
de page suivent le thème actif ; l’écran SSO personnel évite les panneaux imbriqués.

Le script `node scripts/test-admin-ui.mjs` depuis `apps/web` utilise Chrome headless
et une API simulée : couleurs des deux thèmes, navigation sur neuf rubriques à
360/390/768/1024/1440 px, menu mobile, ouverture clavier des détails, requêtes CSRF
de scan et d’enregistrement, paramètres personnels et noms longs de bibliothèques.
Captures dans `docs/evidence/2026-10-19/admin-theme/`. Ces vérifications d’interface
ne prouvent pas la persistance Core, le RBAC backend ou un déploiement NAS.

## Parcours corrigé d’après les captures Jellyfin — 19 octobre 2026

La présentation dépliable décrite au jalon précédent est remplacée par une liste
rectangulaire compacte groupée par Bibliothèques et Maintenance. Cliquer le nom
ouvre une page dédiée avec retour à la liste, description, déclencheur et dernier
résultat connu. Le bouton de lancement reste indépendant à droite.

Les scans et la sauvegarde utilisent leurs endpoints existants.
Le déclencheur est réglable en minutes/heures/jours dans une boîte de dialogue ;
le serveur conserve un intervalle unique, de 5 minutes à 7 jours pour les scans et
de 1 heure à 7 jours pour les sauvegardes. Supprimer un déclencheur de scan utilise
DELETE ; pour une sauvegarde, cela désactive son plan via PUT. Les dates de scans
manuels ne sont pas assimilées aux dernières exécutions planifiées.

Lien direct d’exemple : `/app/admin/libraries/#tasks-section?task=scan-ID`.
La touche Retour du navigateur retrouve le détail ou la liste. Un identifiant
inconnu affiche un écran explicite. Une sauvegarde non configurée ne peut pas
être lancée et renvoie à la configuration des sauvegardes.

Paramètres personnels : Compte, Apparence, Lecture et Connexion SSO, avec pages
directes par fragment. Lecture expose les décalages audio/sous-titres existants.
Voir [revue des paramètres](SETTINGS_REVIEW.md) pour les fonctions raccordées et
les chantiers nécessitant un développement serveur distinct.
Captures actuelles : `docs/evidence/2026-10-19/settings-jellyfin/`.

Vérification de ce parcours : check/lint, 54 tests unitaires et build Web réussis.
Le test navigateur simulé couvre les deux thèmes et cinq largeurs. Le test
`cargo test -p media-poc --test browser_admin -- --ignored --nocapture` passe aussi
sur vrai Core/SQLite temporaire : scans et suppression relue, archivage de
l'ancien format chiffré,
planification/lancement/désactivation des backups depuis Tâches, préférences
`offsets` relues par API puis après rechargement. Ce résultat historique ne
valide pas le nouveau format sans clé ; ne pas fournir un dossier
`MEDIA_BACKUP_DIR` personnel à ce test. Il ne prouve pas un déploiement NAS.

## Extension Général / Réseau / profil / actualisation (19 octobre 2026)

Voir [contrats et limites](SETTINGS_AND_REFRESH.md) pour les paramètres persistants du serveur,
la configuration réseau effective, les avatars de compte et les quatre modes de scan.
Les fournisseurs de métadonnées en ligne restent à développer.

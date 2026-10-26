# Sauvegardes administrateur et planification

La console crée, liste et télécharge des archives chiffrées `.media-backup` contenant la base complète et un instantané des variables `MEDIA_*` / `OIDC_*` du processus (secrets inclus uniquement dans la partie chiffrée). Les paramètres enregistrés en base sont inclus : comptes, groupes, accès, catalogue, progression, tâches, clés API hachées et état des extensions. Les chemins/URL du moteur et URL publique reflètent la connexion réelle. La clé de sauvegarde et son éventuel chemin source restent exclus.

Cle de sauvegarde : aucune cle par defaut. Voir [BACKUP_KEY.md](BACKUP_KEY.md) (`MEDIA_BACKUP_KEY`, `MEDIA_BACKUP_KEY_FILE` ou cle d'installation generee par le CSPRNG, jamais dans l'archive). Une cle explicite invalide empeche le demarrage.

SQLite utilise le snapshot WAL cohérent existant, avec contrôle intégrité/tables. PostgreSQL utilise le `pg_dump` 18 existant, avec l'URL de la connexion SQLx réelle, y compris base distante. `pg_dump` 18 doit être installé dans l'environnement du Core ; une absence/erreur échoue explicitement. Ce support de code ne prouve pas une restauration distante réelle.

L'archive protège configuration et base avec le format versionné de `backup_bundle`. La création passe par un fichier de base temporaire dans le dossier privé, effacé à la fin/annulation. Aucun fichier temporaire n'est listé. Un arrêt brutal peut laisser un temporaire à nettoyer par l'opérateur. Les médias, certificats/fichiers référencés extérieurement, montages Docker, configuration du proxy et binaires externes doivent être conservés séparément. L'archive contient leurs variables de chemin lorsque configurées, pas le contenu de ces fichiers. Ne pas annoncer une copie intégrale de l'hôte.

## Contrat HTTP

- `GET /api/v1/admin/backups` : `available`, `reason`, `running`, `archives` (`id`, `size`, `createdAt` en secondes Unix), `schedule`.
- `POST /api/v1/admin/backups` : 201 après vérification et chiffrement ; une création à la fois, 409 si occupé/dossier refusé ; 501 si configuration incomplète, 500 si échec (sans chemins/secrets).
- `GET`/`HEAD /api/v1/admin/backups/{id}/download` : fichier au nom UUID v4, descripteur confiné, Range et mémoire bornée, téléchargement `no-store`.
- `PUT /api/v1/admin/backups/schedule` : `{ "enabled": true, "intervalMinutes": 1440 }`. Intervalle entier 60..10080 minutes, champs inconnus refusés. 422 hors bornes. Retourne la planification persistée.

Toutes les routes authentifient le rôle `administrator` côté Core ; toutes les mutations vérifient CSRF. Aucun chemin de fichier accepté depuis le navigateur.

## Stockage et planification

`MEDIA_BACKUP_DIR` définit un dossier privé ; repli à côté de SQLite, ou `backups` dans le dossier de travail pour PostgreSQL. Son parent doit exister. Traversées, liens/junctions des ancêtres et destinations dans les bibliothèques locales connues sont refusés. Sous Unix : création en 0700, dossier existant accessible au groupe/autres refusé. Aucune modification de permissions d'un dossier existant. Le dossier doit rester sous le contrôle de l'opérateur ; une substitution locale concurrente de répertoires reste hors de la frontière HTTP de création. Les téléchargements sont confinés par descripteur.

Migration 0024 dans les deux moteurs : planification désactivée par défaut, intervalle 24 h. Worker avec contrôle toutes les 15 secondes, uniquement quand le Core tourne. Les créations manuelles et planifiées partagent le même sémaphore. L'échéance est revendiquée atomiquement en base puis avancée à maintenant + intervalle : un redémarrage ne provoque pas une rafale de rattrapage. Résultats `never`, `running`, `succeeded`, `failed`, `interrupted`. Un échec attend la prochaine échéance. Au redémarrage, un résultat encore `running` devient `interrupted`. Un Core unique est la cible ; aucun cluster ni coordination multi-Core promis. Le worker ne conserve qu'une référence faible au service au repos et est annulé à la libération du service.

Aucune suppression ni rétention automatique. Maximum 10 000 entrées inspectées et 1 000 archives affichées.

## Récupération et vérifications

Arrêter le Core ; extraire/déchiffrer vers un nouvel emplacement privé avec la clé séparée ; restaurer la base via `media-restore` (SQLite destination nouvelle, PostgreSQL base vide), puis rétablir la configuration extraite et vérifier le redémarrage. La restauration de la base active depuis HTTP reste exclue.

Tests ciblés : `cargo test -p media-poc --test admin_backups` et `cargo test -p media-poc --lib admin_backups`. Couverture prévue : vrai snapshot SQLite, bundle chiffré téléchargé/déchiffré, comptes présents, absence de secret en clair, rôles/CSRF, identifiants, paramètres planification, sémaphore commun, échéance persistée, échec terminal et absence de rafale. Validation PostgreSQL distant et récupération complète à documenter selon les commandes réellement exécutées par le coordinateur.

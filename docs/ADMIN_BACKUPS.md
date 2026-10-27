# Sauvegardes administrateur et planification

La console crée, liste et télécharge des archives `.media-backup` **non chiffrées** contenant la base complète et un instantané des variables `MEDIA_*` / `OIDC_*` du processus. Les secrets sont donc lisibles par quiconque peut lire l'archive. Les paramètres enregistrés en base sont inclus : comptes, groupes, accès, catalogue, progression, tâches, clés API hachées et état des extensions. Les chemins/URL du moteur et URL publique reflètent la connexion réelle.

Le format et ses limites sont décrits dans [BACKUP_BUNDLE.md](BACKUP_BUNDLE.md). Réserver strictement le volume d'archives à l'opérateur ; le digest SHA-256 détecte les corruptions accidentelles sans authentifier une modification hostile.

SQLite utilise le snapshot WAL cohérent existant, avec contrôle intégrité/tables. PostgreSQL utilise le `pg_dump` 18 existant, avec l'URL de la connexion SQLx réelle, y compris base distante. `pg_dump` 18 doit être installé dans l'environnement du Core ; une absence/erreur échoue explicitement. Ce support de code ne prouve pas une restauration distante réelle.

L'archive regroupe configuration et base avec le format versionné de `backup_bundle`. La création passe par un fichier de base temporaire dans le dossier privé, effacé à la fin/annulation. Aucun fichier temporaire n'est listé. Un arrêt brutal peut laisser un temporaire à nettoyer par l'opérateur. Les médias, montages Docker, configuration du proxy et binaires externes doivent être conservés séparément. Les certificats TLS explicitement pris en charge sont inclus ; les autres fichiers externes ne le sont pas. L'archive conserve les variables de chemin configurées. Ne pas annoncer une copie intégrale de l'hôte.

## Contrat HTTP

- `GET /api/v1/admin/backups` : `available`, `reason`, `running`, `archives` (`id`, `size`, `createdAt` en secondes Unix), `schedule`.
- `POST /api/v1/admin/backups` : 201 après vérification et publication ; une création à la fois, 409 si occupé/dossier refusé ; 501 si configuration incomplète, 500 si échec (sans chemins/secrets).
- `GET`/`HEAD /api/v1/admin/backups/{id}/download` : fichier au nom UUID v4, descripteur confiné, Range et mémoire bornée, téléchargement `no-store`.
- `PUT /api/v1/admin/backups/schedule` : `{ "enabled": true, "intervalMinutes": 1440 }`. Intervalle entier 60..10080 minutes, champs inconnus refusés. 422 hors bornes. Retourne la planification persistée.

Toutes les routes authentifient le rôle `administrator` côté Core ; toutes les mutations vérifient CSRF. Aucun chemin de fichier accepté depuis le navigateur.

## Stockage et planification

`MEDIA_BACKUP_DIR` définit un dossier privé ; repli à côté de SQLite, ou `backups` dans le dossier de travail pour PostgreSQL. Son parent doit exister. Traversées, liens/junctions des ancêtres et destinations dans les bibliothèques locales connues sont refusés. Sous Unix : création en 0700, dossier existant accessible au groupe/autres refusé. Aucune modification de permissions d'un dossier existant. Le dossier doit rester sous le contrôle de l'opérateur ; une substitution locale concurrente de répertoires reste hors de la frontière HTTP de création. Les téléchargements sont confinés par descripteur.

Migration 0024 dans les deux moteurs : planification désactivée par défaut, intervalle 24 h. Worker avec contrôle toutes les 15 secondes, uniquement quand le Core tourne. Les créations manuelles et planifiées partagent le même sémaphore. L'échéance est revendiquée atomiquement en base puis avancée à maintenant + intervalle : un redémarrage ne provoque pas une rafale de rattrapage. Résultats `never`, `running`, `succeeded`, `failed`, `interrupted`. Un échec attend la prochaine échéance. Au redémarrage, un résultat encore `running` devient `interrupted`. Un Core unique est la cible ; aucun cluster ni coordination multi-Core promis. Le worker ne conserve qu'une référence faible au service au repos et est annulé à la libération du service.

Aucune suppression ni rétention automatique. Maximum 10 000 entrées inspectées et 1 000 archives affichées.

## Récupération et vérifications

Arrêter le Core ; extraire vers un nouvel emplacement privé ; restaurer la base via `media-restore` (SQLite destination nouvelle, PostgreSQL base vide), puis rétablir la configuration extraite et vérifier le redémarrage. La restauration de la base active depuis HTTP reste exclue.

Tests ciblés : `cargo test -p media-poc --test admin_backups` et `cargo test -p media-poc --lib admin_backups`. Couverture prévue : vrai snapshot SQLite, bundle non chiffré téléchargé/extrait, comptes présents, protection du dossier et format sans clé, rôles/CSRF, identifiants, paramètres planification, sémaphore commun, échéance persistée, échec terminal et absence de rafale. Validation PostgreSQL distant et récupération complète à documenter selon les commandes réellement exécutées par le coordinateur.

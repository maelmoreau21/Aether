# Sauvegardes administrateur et planification

La console crée, liste et télécharge des archives `.media-backup` **chiffrées** (`MDBK0003`) contenant la base complète et un instantané des variables `MEDIA_*` / `OIDC_*` du processus (sans la clé de sauvegarde). La clé externe est lue au démarrage de Core dans `MEDIA_BACKUP_KEY_FILE` (ou `MEDIA_BACKUP_KEY`) : **sans clé valide, la création manuelle ou planifiée échoue sans écrire de snapshot** (`backup_key_required`, 409) et `GET` renvoie `available:false`. Un redémarrage de Core est nécessaire après avoir défini la clé. Pour accepter explicitement des archives en clair : `MEDIA_BACKUP_ALLOW_UNENCRYPTED=true`. Les anciennes archives non chiffrées déjà présentes restent listées et téléchargeables ; elles ne sont ni converties ni supprimées. Les paramètres enregistrés en base sont inclus : comptes, groupes, accès, catalogue, progression, tâches, clés API hachées et état des extensions. Les chemins/URL du moteur et URL publique reflètent la connexion réelle.

Le format et ses limites sont décrits dans [BACKUP_BUNDLE.md](BACKUP_BUNDLE.md). Réserver le volume d'archives à l'opérateur et conserver la clé séparément des archives ; l'authentification AEAD détecte toute modification hostile d'une archive `MDBK0003`.

SQLite utilise le snapshot WAL cohérent existant, avec contrôle intégrité/tables. PostgreSQL utilise le `pg_dump` 18 existant, avec l'URL de la connexion SQLx réelle, y compris base distante. `pg_dump` 18 doit être installé dans l'environnement du Core ; une absence/erreur échoue explicitement. Ce support de code ne prouve pas une restauration distante réelle.

L'archive regroupe configuration et base avec le format versionné de `backup_bundle`. La création passe par un fichier de base temporaire dans le dossier privé, effacé à la fin/annulation. Aucun fichier temporaire n'est listé. Un arrêt brutal peut laisser un temporaire à nettoyer par l'opérateur. Les médias, montages Docker, configuration du proxy et binaires externes doivent être conservés séparément. Les certificats TLS explicitement pris en charge sont inclus ; les autres fichiers externes ne le sont pas. L'archive conserve les variables de chemin configurées. Ne pas annoncer une copie intégrale de l'hôte.

## Contrat HTTP

- `GET /api/v1/admin/backups` : `available`, `reason`, `running`, `archives` (`id`, `size`, `createdAt` en secondes Unix), `schedule`.
- `POST /api/v1/admin/backups` : 201 après vérification et publication ; une création à la fois, 409 si occupé/dossier refusé ; 501 si configuration incomplète. Un échec renvoie `{code,message}` sans chemins/secrets : `backup_key_required` (409, aucune clé valide), `backup_directory_unwritable` (403 si permission refusée, sinon 500), `backup_sqlite_snapshot_failed`, `backup_postgres_snapshot_failed`, `backup_assets_unavailable`, `backup_asset_integrity_failed` ou `backup_archive_failed` (500). Le serveur vérifie l'écriture réelle du dossier avant de commencer le snapshot.
- `GET`/`HEAD /api/v1/admin/backups/{id}/download` : fichier au nom UUID v4, descripteur confiné, Range et mémoire bornée, téléchargement `no-store`.
- `PUT /api/v1/admin/backups/schedule` : `{ "enabled": true, "intervalMinutes": 1440 }`. Intervalle entier 60..10080 minutes, champs inconnus refusés. 422 hors bornes. Retourne la planification persistée.

Toutes les routes authentifient le rôle `administrator` côté Core ; toutes les mutations vérifient CSRF. Aucun chemin de fichier accepté depuis le navigateur.

## Stockage et planification

`MEDIA_BACKUP_DIR` définit un dossier privé ; repli à côté de SQLite, ou, pour PostgreSQL, `/backups` si présent, puis `/data/backups` si `/data` existe, sinon `backups` dans le dossier de travail. Son parent doit exister. Traversées, liens/junctions des ancêtres et destinations dans les bibliothèques locales connues sont refusés. Sous Unix : création en 0700 recommandée ; les montages permissifs existants (ex. 0755 ou 0777 sur NAS) sont tolérés par défaut avec avertissement dans les journaux dès lors que le processus dispose des droits effectifs de lecture, écriture et suppression, sans altération forcée des permissions de l'hôte. Pour exiger un refus strict des répertoires non-0700, définir `MEDIA_BACKUP_STRICT_PERMISSIONS=true`. Le dossier doit rester sous le contrôle de l'opérateur ; une substitution locale concurrente de répertoires reste hors de la frontière HTTP de création. Les téléchargements sont confinés par descripteur.

Migration 0024 dans les deux moteurs : planification désactivée par défaut, intervalle 24 h. Worker avec contrôle toutes les 15 secondes, uniquement quand le Core tourne. Les créations manuelles et planifiées partagent le même sémaphore. L'échéance est revendiquée atomiquement en base puis avancée à maintenant + intervalle : un redémarrage ne provoque pas une rafale de rattrapage. Résultats `never`, `running`, `succeeded`, `failed`, `interrupted`. Un échec attend la prochaine échéance. Au redémarrage, un résultat encore `running` devient `interrupted`. Un Core unique est la cible ; aucun cluster ni coordination multi-Core promis. Le worker ne conserve qu'une référence faible au service au repos et est annulé à la libération du service.

Aucune suppression ni rétention automatique. Maximum 10 000 entrées inspectées et 1 000 archives affichées.

Une erreur de lecture du dossier rend maintenant `available:false` ; elle ne
simule plus une liste vide disponible. Un volume créé par une ancienne image
peut appartenir à l'UID 10001 alors que le Compose impose un autre utilisateur.
Le propriétaire du volume de sauvegardes doit correspondre à l'UID/GID effectif
du Core, avec permissions 0700. Le Core ne change pas automatiquement le
propriétaire du volume. Diagnostic réel et correction ciblée du 2 novembre :
[sauvegarde NAS](archive/fixes/BACKUP_NAS_FIX_2026_11_02.md).

Si Compose impose `user: UID:GID`, un nouveau volume nommé peut être initialisé
avec le propriétaire du dossier fourni par l'image (`10001:10001`), même si le
processus utilise un autre UID. À la création ou recréation du volume, vérifier
le montage exact avec `docker inspect`, puis son propriétaire avec `stat` sous
l'utilisateur du service. Faire correspondre le propriétaire du **seul dossier
du volume de sauvegardes** à l'UID/GID configuré, en conservant 0700 ; ne pas
appliquer de changement récursif ni de permissions ouvertes aux archives.
Conserver ce volume lors des mises à jour (ne pas le recréer avec `down -v`).
Un bind mount préparé avec ce propriétaire avant démarrage est une autre option.
L'API distingue désormais un refus de lecture lié aux permissions d'un chemin
configuré qui est un fichier ; les réponses ne révèlent aucun chemin du serveur.

Le 5 novembre 2026, le NAS présentait de nouveau `/backups` en `10001:10001`,
0700, avec un Core `1039:65539` et des erreurs `read_dir: PermissionDenied`.
Le montage a été vérifié : volume `aether_backups`, dossier hôte
`/volume2/@docker/volumes/aether_backups/_data`, monté en lecture/écriture.
Le propriétaire de ce seul dossier a été corrigé vers `1039:65539`, sans changer
le mode 0700. Lecture de la liste, création d'une sonde privée 0600, relecture et
suppression de cette seule sonde ont réussi sous l'utilisateur du service.
Ni redémarrage, ni modification de médias/archives, ni déploiement de code.
La création/restauration d'une archive complète et le parcours HTTP authentifié
de production n'ont pas été exercés lors de cette intervention.
Validation locale Windows : `cargo test -p aether-core --test admin_backups`
réussit (2 tests, dont création/téléchargement/extraction/restauration SQLite
et refus de chemin), ainsi que `cargo fmt --check` et `git diff --check`.
Le scénario ajouté de permissions Unix puis rétablissement attend une exécution
sous Unix ; il est exclu de la compilation Windows.

Un nouveau contrôle SSH le 5 novembre a retrouvé le même refus après recréation
du conteneur : `/backups` en `10001:10001`, mode 0700, pour un Core `1039:65539`.
Le volume porte toujours sa date de création du 19 octobre ; sa recréation
n'est donc pas établie. Le dossier était vide. La propagation Docker vers un
volume vide préexistant peut aussi réappliquer les attributs du dossier fourni
par l'image lors de la création du conteneur. Ce mécanisme est compatible avec
la récidive observée ; l'opération exacte ayant changé le propriétaire n'a pas
été retracée. Voir les [règles des volumes Docker](https://docs.docker.com/engine/storage/volumes/).
La correction non récursive du seul dossier vers `1039:65539` a rétabli la
lecture de la liste et l'écriture/relecture d'une sonde privée 0600 sous le Core ;
la sonde a été supprimée, sans redémarrage ni modification d'archive ou de média.

Pour un déploiement utilisant un UID/GID personnalisé, préparer d'abord le volume
privé pour cet utilisateur puis désactiver la propagation sur ce seul montage :

```yaml
services:
  aether:
    volumes:
      - type: volume
        source: aether_backups
        target: /backups
        volume:
          nocopy: true
```

Ce bloc remplace le montage court de `/backups` dans le Compose de déploiement.
Il suppose que le volume existe déjà et que son propriétaire a été préparé ;
un volume neuf avec `nocopy` peut appartenir à root et doit être initialisé avant
de lancer le Core. Le Compose générique et le Compose NAS n'ont pas été modifiés
lors de ce contrôle. Un bind mount privé préparé pour l'UID/GID effectif évite
également la propagation. Revérifier le propriétaire et la lecture sous
l'utilisateur du service après chaque recréation de conteneur.

## Récupération et vérifications

Depuis le 5 novembre 2026, le panneau mémoire conserve ses mesures et l’action
« Libérer la mémoire (Trim) », mais n’affiche plus le message de diagnostic
ni le conseil sur le cache de pages Linux. Le contrat de diagnostic API reste
inchangé.

Arrêter le Core ; extraire vers un nouvel emplacement privé ; restaurer la base via `media-restore` (SQLite destination nouvelle, PostgreSQL base vide), puis rétablir la configuration extraite et vérifier le redémarrage. La restauration de la base active depuis HTTP reste exclue.

Tests ciblés : `cargo test -p aether-core --test admin_backups` et `cargo test -p aether-core --lib admin_backups`. Couverture prévue : vrai snapshot SQLite, bundle chiffré téléchargé/extrait avec clé, absence des secrets dans les octets, refus sans clé ou avec clé faible, comptes présents, protection du dossier, rôles/CSRF, identifiants, paramètres planification, sémaphore commun, échéance persistée, échec terminal et absence de rafale. Validation PostgreSQL distant et récupération complète à documenter selon les commandes réellement exécutées par le coordinateur.

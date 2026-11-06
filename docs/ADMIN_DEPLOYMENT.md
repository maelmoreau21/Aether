# Déploiement de l'administration et des sauvegardes

Les images Core et autonome incluent `media-backup`, `media-restore` et
`media-recover`. Le Core conserve les sessions, utilisateurs, groupes, droits,
clés API et plans de scans dans sa base applicative. Les sauvegardes complètes
ajoutent la configuration serveur capturée à l'archive chiffrée (`MDBK0003`) ; elles ne
contiennent pas les médias ni l'infrastructure externe.

## Deux modes conservés

- `docker-compose.yml` : serveur autonome avec médias `:ro`, données dans
  `aether_data` et archives dans le volume séparé `aether_backups`.
- `compose.distributed.yaml` : Core avec `aether_core_data` et
  `aether_core_backups`, Nodes avec leurs propres données et médias `:ro`.

Les identités UID/GID `10001:10001`, les ports locaux par défaut,
`no-new-privileges` et la suppression des capacités Linux sont conservés.
Les clients PostgreSQL ne sont pas ajoutés à l'image Node.

## Répertoire privé de sauvegarde

`MEDIA_BACKUP_DIR=/backups` désigne le volume d'archives. L'image crée ce dossier
avec le propriétaire `10001:10001` et le mode `0700`. Un montage existant masque
les permissions de l'image : vérifier celles du volume ou du répertoire hôte.
Pour un volume déjà possédé par le compte du service, corriger son mode via :

```sh
docker compose exec aether chmod 0700 /backups
```

Pour un bind mount NAS, préparer un répertoire privé appartenant à UID/GID
10001 avant le démarrage. Adapter également les ACL du NAS ; la seule présence
d'un mode Unix ne prouve pas qu'aucune ACL supplémentaire ne donne accès.
Ne pas changer les droits des dossiers médias pour résoudre un problème de
sauvegarde.

Les archives `MDBK0003` sont chiffrées, mais le Core refuse d'en créer **sans clé** :
après mise à jour, les sauvegardes manuelles et planifiées échouent avec
`backup_key_required` tant que `MEDIA_BACKUP_KEY_FILE` n'est pas défini. Générer
une clé hors du dépôt et hors du volume `/backups`, par exemple :

```sh
umask 077 && openssl rand -hex 32 > ./backup.key
```

puis l'exposer au conteneur en lecture seule (fichier ou secret Docker) et définir
`MEDIA_BACKUP_KEY_FILE=/run/secrets/aether_backup_key`. Le fichier doit être lisible
par l'UID/GID du service (`user:`), pas par les autres comptes du NAS. Redémarrer
le Core. Copier la clé dans un gestionnaire de secrets : sans elle, les archives
sont irrécupérables. Ne pas y réutiliser la clé publiée dans l'historique Git
(voir [BACKUP_BUNDLE.md](BACKUP_BUNDLE.md)). Les archives antérieures `MDBK0002`
restent en place, lisibles et en clair : réserver `/backups` à l'opérateur, protéger
leurs copies et ne pas servir ce dossier par le proxy Web.

Pour les installations autonomes antérieures utilisant un volume anonyme créé
par le `VOLUME` de l'image, inventorier et conserver ce volume avant de passer au
nouveau volume nommé ; aucune migration automatique des anciennes archives
n'est réalisée par Compose.

## Compose minimal et UID personnalisé

Les images Core et autonome définissent déjà `MEDIA_BACKUP_DIR=/backups` et
`MEDIA_ASSETS_PATH=/data/assets`. Ces deux lignes sont inutiles dans Compose ;
conserver les montages `/data` et `/backups`. Une image existante doit être
recréée après sa mise à jour pour utiliser ses nouvelles valeurs par défaut.

Avec `user: 1039:65539` et `./backups:/backups`, préparer depuis le dossier
Compose sur le NAS :

```sh
sudo chown 1039:65539 ./backups
sudo chmod 0700 ./backups
```

Ces commandes portent uniquement sur le dossier de sauvegardes, sans récursion.
Adapter les ACL DSM si elles empêchent cet utilisateur d’accéder au dossier.
Le volume `/data` doit également être accessible en écriture à cet UID/GID.
Le serveur conserve les contrôles de confidentialité et ne change pas les
permissions des dossiers montés. Un mode trop ouvert est désormais indiqué
explicitement dans la console et dans les journaux.

## Clients PostgreSQL 18

Les images Core et autonome installent uniquement `postgresql-client-18`, pas
de serveur PostgreSQL. L'installation utilise le dépôt PGDG pour Debian Bookworm
et une clé limitée à ce dépôt avec `signed-by`. La procédure de référence est
la [documentation officielle PostgreSQL pour Debian](https://www.postgresql.org/download/linux/debian/).

`pg_dump` et `pg_restore` sont disponibles pour le moteur PostgreSQL. SQLite
reste le moteur par défaut et ne nécessite pas de service PostgreSQL. L'ajout
des outils clients ne transfère aucune donnée entre moteurs.

Les outils accèdent à la base avec la configuration du Core ; son réseau doit
donc joindre le serveur PostgreSQL. Les exigences de transport, authentification
et cible vide de la restauration existante s'appliquent toujours.

## Récupération et preuves

Suivre [BACKUP_RECOVERY.md](BACKUP_RECOVERY.md) pour extraire l'archive dans un
nouveau dossier privé, examiner `config.json`, puis restaurer hors ligne avec
`media-restore`. L'outil `media-recover` est fourni dans les deux images serveur ;
il ne modifie pas automatiquement les variables de service ou la base active.
Si l'archive contient `internal-ca.pem`, remonter ce certificat au chemin attendu
ou configurer explicitement `MEDIA_INTERNAL_CA_FILE` vers sa copie restaurée
avant redémarrage. Si des certificats PostgreSQL ont été capturés dans
`postgres-ca.pem`, `postgres-cert.pem` et `postgres-key.pem`, restaurer leurs
chemins originaux ou adapter les paramètres de chemins TLS dans `MEDIA_DB_URL`.
Les autres secrets sur fichiers restent à rétablir séparément.

Validation locale du 19 octobre 2026 : `docker compose config --quiet` réussi
pour les fichiers autonome et distribué, puis build réel de la cible
`backup-runtime` réussi avec l'image locale
`media-backup-clients:local-validation`. Deux conteneurs temporaires exécutés avec
`docker run --rm` ont confirmé `pg_dump` et `pg_restore` en version
`18.6 (Debian 18.6-1.pgdg12+2)`. L'image n'a pas été publiée.

Cette validation prouve l'installation et l'exécution des outils dans la base
Linux commune. Elle ne remplace pas le build des images Core/autonome complètes
ni un exercice de sauvegarde/restauration de l'application, de PostgreSQL distant
ou de Synology. Consigner séparément ces résultats dans le jalon principal.

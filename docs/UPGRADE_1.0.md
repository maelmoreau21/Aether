# Aether 1.0 — notes de version et mise à niveau depuis la 0.2

## Changement cassant : clé de sauvegarde obligatoire

Les sauvegardes utilisent le format chiffré et authentifié **`MDBK0003`**
(ChaCha20-Poly1305, clé externe de 32 octets). Sans clé, le Core **refuse** de créer
une archive : sauvegardes manuelles et planifiées échouent avec `backup_key_required`
(HTTP 409), et `backup --full` s'arrête.

### Étapes

1. Avant la mise à jour, faire une dernière sauvegarde avec la 0.2 et la copier hors
   du serveur. Les archives `MDBK0002` restent lisibles (sans clé) ; `MDBK0001`
   n'est plus lisible (clé publiée dans l'historique Git) : les recréer.
2. Générer une clé neuve, hors dépôt et hors du volume `/backups` :
   ```sh
   umask 077 && openssl rand -hex 32 > ./backup.key
   ```
   Ne jamais réutiliser la clé publiée dans l'historique.
3. La monter en lecture seule (fichier ou secret Docker), lisible par l'UID/GID du
   service, et définir `MEDIA_BACKUP_KEY_FILE=/run/secrets/aether_backup_key`
   (ou `MEDIA_BACKUP_KEY`, 64 hexadécimaux).
4. Sauvegarder cette clé dans un gestionnaire de secrets : **sans elle les archives
   `MDBK0003` sont irrécupérables**.
5. Mettre à jour les images, redémarrer le Core, lancer une sauvegarde de test et
   vérifier qu'elle s'ouvre avec `media-recover` (qui lit la clé dans l'environnement).

Dérogation temporaire (déconseillée) : `MEDIA_BACKUP_ALLOW_UNENCRYPTED=true` produit
encore du `MDBK0002` en clair, avec avertissement.

Détails : [BACKUP_BUNDLE.md](BACKUP_BUNDLE.md), [BACKUP_RESTORE.md](BACKUP_RESTORE.md),
[ADMIN_BACKUPS.md](ADMIN_BACKUPS.md), [ADMIN_DEPLOYMENT.md](ADMIN_DEPLOYMENT.md).

## Autres changements

- **Double authentification** TOTP et passkeys. Le web gère les deux étapes ; le bureau
  et Android gèrent le code TOTP et le code de secours (un compte qui n'a que des
  passkeys se connecte par Quick Connect ou par le web). Voir [LOCAL_SESSIONS.md](LOCAL_SESSIONS.md).
- **Téléchargement des originaux réservé aux applications natives**
  (voir [LIBRARY_OPTIONS.md](LIBRARY_OPTIONS.md#téléchargements)) : le web reçoit
  `403 media_download_native_only`.
- **Mise à jour des applications** : le bureau et Android signalent d'eux-mêmes quand
  leur version est plus ancienne que celle du serveur (bandeau « Télécharger » vers la
  dernière release). Le contrôle compare `MAJOR.MINOR.PATCH` et ne s'active que dans
  les builds de release (variable `AETHER_CLIENT_VERSION`).
- HLS par copie pour les lecteurs natifs ([HLS.md](HLS.md)).
- Images Docker multi-architecture `amd64` et `arm64`.
- La base est mise à niveau automatiquement au démarrage ([MIGRATIONS.md](MIGRATIONS.md)) :
  sauvegarder avant, une rétrogradation n'est pas prise en charge.

## Garantie d'API

`/api/v1` est stable à partir de la 1.0 ; ce qui ne l'est pas est listé en tête de
[API.md](API.md).

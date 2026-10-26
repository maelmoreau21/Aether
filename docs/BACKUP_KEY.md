# Clé de chiffrement des sauvegardes

Depuis le 25 octobre 2026, **aucune clé fixe ou publique ne protège plus une sauvegarde**. L'ancienne valeur de repli (64 zéros) a été supprimée du code ; elle est refusée à la création comme à l'extraction normale.

## Ordre de résolution (Core, `media-backup`)

1. `MEDIA_BACKUP_KEY` : 64 chiffres hexadécimaux. Une valeur définie mais invalide **empêche le démarrage** du Core (plus de repli silencieux). La clé tout à zéro est refusée.
2. `MEDIA_BACKUP_KEY_FILE` : fichier ordinaire (≤ 256 octets) contenant la clé ; lien symbolique, fichier trop gros ou contenu invalide refusés.
3. Clé d'installation : `<dossier de la base>/secrets/backup.key` (pour PostgreSQL : parent de `MEDIA_BACKUP_DIR`). Elle est générée au premier démarrage du Core par le CSPRNG du système (32 octets), créée en `create_new` (jamais écrasée) avec le mode `0600` (dossier `0700`) sous Unix, et réutilisée ensuite.

La clé n'est **jamais** copiée dans l'archive ; `MEDIA_BACKUP_KEY` et `MEDIA_BACKUP_KEY_FILE` sont retirées de la configuration sauvegardée, et un manifeste qui les contient est rejeté.

Les outils hors ligne `media-backup` et `media-recover` appliquent la même règle mais **ne génèrent jamais** de clé : sans clé explicite ni clé d'installation existante, ils échouent avec `no backup key configured`.

## Procédure de récupération

- Conserver la clé **séparément** des archives (gestionnaire de secrets ou copie hors ligne). Une sauvegarde du seul volume `/data` contient les archives *et* `secrets/backup.key` : elle ne protège donc pas la confidentialité si ce volume est copié sur un support non fiable. Pour une vraie séparation, fournir `MEDIA_BACKUP_KEY` ou `MEDIA_BACKUP_KEY_FILE` depuis un emplacement distinct.
- Perdre la clé rend les archives chiffrées irrécupérables ; il n'existe aucune porte dérobée.
- Sur un nouveau serveur : définir `MEDIA_BACKUP_KEY` (ou `MEDIA_BACKUP_KEY_FILE`) avec la clé d'origine, puis `media-recover ARCHIVE DOSSIER_NEUF` et `media-restore` comme décrit dans `docs/BACKUP_RECOVERY.md`.
- Rotation : changer la clé n'affecte que les nouvelles archives. Les anciennes restent lisibles uniquement avec l'ancienne clé.

## Anciennes sauvegardes (clé publique)

Les archives créées par les bêtas antérieures sans clé configurée ont été chiffrées avec la clé publique à zéros : **elles n'ont jamais été confidentielles** (toute personne ayant le fichier peut le lire). Elles ne sont ouvrables que par un chemin explicite :

```
media-recover ARCHIVE DOSSIER_NEUF --legacy-public-key
```

Un avertissement est affiché. Après restauration, considérer comme compromis et renouveler tous les secrets contenus (client OIDC, mots de passe de base, certificats), puis créer de nouvelles archives avec une vraie clé. Aucune migration automatique n'est faite ; aucune archive existante n'est supprimée ou modifiée.

## Tests

`cargo test -p media-poc --lib backup_bundle` : clé nulle et valeurs mal formées refusées, absence de repli en cas de clé explicite invalide, génération unique et persistante de la clé d'installation (modes Unix), précédence du fichier de clé, archive « legacy » refusée par `extract` et ouverte seulement par `extract_legacy_public_key`.

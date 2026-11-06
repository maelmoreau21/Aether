# Sauvegarde complète : paquet versionné

`backup_bundle::create` produit par défaut le format **`MDBK0003`** : chiffré et authentifié avec une clé externe. Le paquet contient un instantané cohérent de la base (SQLite ou `pg_dump` PostgreSQL), la configuration capturée, les certificats référencés pris en charge et les assets durables. Il ne contient pas les médias originaux.

Le contenu reste très sensible (configuration OIDC, URL de base avec mot de passe, clé privée TLS éventuelle, comptes et sessions dans la base) : le chiffrement protège l'archive au repos et en transit, pas la clé ni les fichiers extraits par `media-recover`.

## Clé de sauvegarde

- 32 octets aléatoires, 64 caractères hexadécimaux : `openssl rand -hex 32`.
- Fournie hors de l'archive et hors du dépôt Git : `MEDIA_BACKUP_KEY_FILE=/chemin/secret` (recommandé ; un secret Docker/Kubernetes, y compris lien symbolique, convient) ou `MEDIA_BACKUP_KEY`. Les deux à la fois sont refusés.
- Les clés triviales (moins de 16 octets distincts : répétitions, séquences) sont refusées.
- La clé n'est jamais écrite dans l'archive, dans `config.json` ni dans les journaux (`Debug` est expurgé, les messages d'erreur ne contiennent aucune valeur).
- **Sans clé valide, aucune archive n'est produite** (échec avant tout snapshot : `backup_key_required`, 409 côté API ; `media-backup --full` s'arrête). Pour conserver temporairement l'ancien comportement non chiffré il faut un choix explicite : `MEDIA_BACKUP_ALLOW_UNENCRYPTED=true` (avertissement journalisé). Une clé configurée l'emporte toujours.
- **Perdre la clé = perdre les archives.** La conserver séparément des archives (gestionnaire de secrets, support hors ligne). La rotation consiste à générer une nouvelle clé pour les prochaines archives ; les anciennes restent lisibles avec leur clé d'origine, qu'il faut donc conserver.

## Format `MDBK0003`

`MDBK0003` (8 octets), sel aléatoire (32 octets), puis des records `kind:u8`, `len:u32` big endian (longueur du chiffré), chiffré. Chaque record contient au plus 64 Kio de clair + 16 octets de tag.

- Clé de record : HKDF-SHA256(sel = sel du fichier, secret = clé opérateur). Une archive a donc une clé unique même avec la même clé opérateur.
- Chiffrement : ChaCha20-Poly1305 (`ring`). Nonce = compteur de record (`0, 1, 2…`) ; données authentifiées : `MDBK0003 ‖ sel ‖ kind ‖ longueur du clair`. Réordonner, supprimer, dupliquer ou modifier un record, changer le sel ou l'en-tête fait échouer l'authentification.
- Types : 1 manifeste JSON (≤ 64 Kio), 2 base, 3 fin (obligatoire, vide), 4 à 7 pièces jointes fixes, 8 en-tête d'asset, 9 octets d'asset. Une archive tronquée (record final absent) ou suivie d'octets supplémentaires est rejetée. SHA-256 et taille des assets sont recalculés en plus.
- Mauvaise clé et altération donnent la même erreur (`backup authentication failed`) ; une archive `MDBK0003` sans clé donne `KeyRequired`. Rien n'est publié (dossier de préparation supprimé).

## Format `MDBK0002` (historique, lecture seulement)

Plaintext suivi d'un SHA-256 : détecte la corruption accidentelle, aucune confidentialité ni authenticité. **Toujours lisible** par `media-recover` (sans clé) pour que les installations existantes puissent récupérer leurs archives. Il n'est plus produit sans `MEDIA_BACKUP_ALLOW_UNENCRYPTED=true`. Les archives existantes ne sont jamais converties, réécrites ni supprimées automatiquement : le lecteur n'ouvre l'archive qu'en lecture seule.

## Format `MDBK0001` (historique chiffré, non lisible ici)

Le lecteur correspondant a été retiré du code ; `media-recover` répond `legacy MDBK0001 archive` sans rien modifier. Conserver ces fichiers intacts et utiliser la version d'Aether qui les a créés avec la clé d'origine.

### Ancienne clé publiée dans l'historique Git

Une clé de 64 caractères a été publiée le 26 octobre 2026 dans `crates/core/secrets/backup.key`, retirée du checkout mais présente dans l'historique Git public (non réécrit ici ; la valeur n'est pas reproduite dans ce dépôt). Conséquences :

1. Toute archive `MDBK0001` créée avec cette clé doit être considérée comme **lisible par un tiers**, ainsi que les secrets qu'elle contient. Les isoler, puis **renouveler** ces secrets (secret client OIDC, mots de passe de base, clés de Nodes, clés TLS, sessions) selon leur sensibilité. Ne pas supprimer les archives sans décision de conservation de l'opérateur.
2. **Ne jamais réutiliser cette clé ni une variante** pour `MDBK0003`. Générer une clé neuve. Le code ne peut pas détecter la réutilisation (la valeur n'est volontairement pas recopiée) ; contrôler que `MEDIA_BACKUP_KEY(_FILE)` ne vient pas de l'ancien fichier, par exemple en comparant localement une empreinte SHA-256 sans l'afficher.
3. L'usage réel de cette clé par une installation n'est pas établi (aucun accès aux serveurs ni aux archives). `.gitignore` et `.dockerignore` excluent déjà `**/secrets/backup.key`.

## Fichiers temporaires, permissions, chemins

- Création : fichier temporaire privé `.media-backup-<uuid>` (mode `0600` sous Unix) dans le dossier cible, supprimé en cas d'erreur, publié par lien dur sans remplacer la cible. Le snapshot de base intermédiaire (`.database-<uuid>.tmp` ou `database.snapshot`) est lui aussi `0600` mais **en clair** pendant la sauvegarde ; il est supprimé à la fin ou à l'erreur. Un arrêt brutal (kill -9, coupure) peut le laisser sur disque. Le nettoyage `BackupStaging` des tâches système ne reconnaît que les fichiers `.database-*.tmp`, `.backup-*.tmp`, `.restore-*.tmp` et `.media-backup-*.tmp` : les restes `.media-backup-<uuid>` (sans `.tmp`, chiffrés en `MDBK0003`) et `.media-full-<uuid>/` de `media-backup --full` sont à supprimer manuellement par l'opérateur après vérification. Aucun nettoyage n'a été ajouté ici.
- Extraction : dossier de préparation privé (`0700`), publié sans remplacement. Les fichiers extraits (`config.json`, base, certificats, assets) sont **en clair**. Aucun chemin n'est lu depuis l'archive : seuls des noms fixes sont écrits, les assets sont nommés par leur SHA-256 validé (64 hexadécimaux).
- Le dossier parent, chaque ancêtre et les certificats embarqués ne peuvent pas être des liens symboliques ni des points de reparse Windows.
- Sous Windows, les modes Unix ne s'appliquent pas : vérifier les ACL du dossier d'archives et du dossier de récupération.

## Publication et récupération

`media-recover ARCHIVE DOSSIER_NEUF` (avec `MEDIA_BACKUP_KEY_FILE` pour `MDBK0003`) vérifie le format, la structure et l'authentification dans un dossier privé temporaire, puis publie le dossier extrait sans remplacement. Le Core et la base active ne sont pas modifiés. Utiliser ensuite `media-restore` et `media-restore-assets` selon [BACKUP_RESTORE.md](BACKUP_RESTORE.md).

## Vérification

`cargo test --locked -p media-poc --lib backup_bundle` et `cargo test --locked -p media-poc --test backup_bundle --test admin_backups --test full_backup_restore_e2e` couvrent : aller-retour chiffré SQLite/PostgreSQL synthétiques, absence des secrets et de la clé dans les octets de l'archive, mauvaise clé, absence de clé, modification de chaque zone, troncature, octets supplémentaires, lecture inchangée de `MDBK0002`, rejet distinct de `MDBK0001`, refus des liens symboliques, cible existante, configuration excessive et fail-closed sans clé. Le test PostgreSQL réel (`admin_postgres`) exige un PostgreSQL 18 jetable. Aucune de ces vérifications ne prouve un déploiement NAS.

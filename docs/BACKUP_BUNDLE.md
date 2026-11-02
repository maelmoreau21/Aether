# Sauvegarde complète : paquet versionné

Depuis le 27 octobre 2026, `backup_bundle::create` produit le format `MDBK0002` sans clé et sans chiffrement. Le paquet contient un instantané cohérent de la base (SQLite ou `pg_dump` PostgreSQL), la configuration capturée, les certificats référencés pris en charge et les assets durables. Il ne contient pas les médias originaux.

**L'archive contient des secrets en clair** : configuration OIDC, URL de base, certificats et clé privée TLS éventuelle, comptes et sessions dans la base. Réserver le dossier et tout support de copie à l'opérateur ; ne jamais servir ces fichiers par HTTP public, les joindre à un ticket ou les pousser sur Git. Sous Unix, la création utilise le mode `0600` et l'extraction un dossier `0700` ; sous Windows, vérifier les ACL du parent. Le SHA-256 final détecte la corruption accidentelle, mais un tiers pouvant modifier l'archive peut recalculer ce digest : il n'offre aucune authenticité.

## Format et limites

En-tête `MDBK0002` (8 octets), puis une suite de records `kind:u8`, `len:u32` big endian et `len` octets. Les types sont : 1 manifeste JSON (au plus 64 Kio), 2 base, 3 fin, 4 à 7 pièces jointes fixes, 8 en-tête d'asset et 9 octets d'asset. Chaque record est limité à 64 Kio. Les 32 octets après le record final sont SHA-256 de l'en-tête et de tous les records. Toute donnée supplémentaire est refusée. La base vide, les tailles incohérentes et les assets dont SHA-256/taille ne correspondent pas sont refusés. L'extraction n'accepte aucun chemin provenant de l'archive.

## Publication et récupération

La création écrit dans un fichier privé temporaire puis publie sans remplacer une cible existante. `media-recover ARCHIVE DOSSIER_NEUF` vérifie le format, la structure et le digest dans un dossier privé temporaire, puis publie le dossier extrait sans remplacement. Le Core et la base active ne sont pas modifiés par cette commande. Utiliser ensuite `media-restore` et `media-restore-assets` selon [BACKUP_RESTORE.md](BACKUP_RESTORE.md).

Les archives `MDBK0001` étaient chiffrées et ne sont plus prises en charge par le binaire actuel. Les conserver sans les modifier ; une récupération exige une version antérieure de `media-recover` et la clé utilisée à l'époque. La suppression de la gestion des clés ne convertit pas les anciennes archives.

Une clé de 64 caractères a été publiée le 26 octobre 2026 dans `crates/core/secrets/backup.key`. Ce fichier a été retiré du checkout, mais reste dans l'historique Git public. Son usage par une installation réelle n'a pas pu être établi faute d'accès aux serveurs et archives. Si une archive `MDBK0001` a été créée avec cette valeur, sa confidentialité doit être considérée comme perdue ; isoler les archives concernées et examiner les secrets qu'elles contiennent. Ne pas supprimer les anciennes archives sans décision de conservation de l'opérateur.

## Vérification

`cargo test --locked -p media-poc --test backup_bundle` couvre SQLite/PostgreSQL synthétiques, configuration sensible visible dans le paquet, corruption, troncature, octets supplémentaires, ancien magic, cible existante, configuration excessive et protection du parent symlink sur Unix. Les tests de restauration complète et d'assets sont séparés ; aucune de ces vérifications ne prouve un déploiement NAS.

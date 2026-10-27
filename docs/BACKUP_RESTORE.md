# Sauvegarde complète et restauration

## Contenu d'une archive `.media-backup`

Format courant `MDBK0002` sans clé et sans chiffrement. L'archive contient des secrets en clair ; voir [BACKUP_BUNDLE.md](BACKUP_BUNDLE.md).

| Élément | Présent | Vérification |
|---|---|---|
| Base SQL cohérente (SQLite snapshot / `pg_dump`) | oui | checksum SHA-256 du paquet |
| Configuration `MEDIA_*` / `OIDC_*` | oui | checksum SHA-256 du paquet |
| Certificats TLS référencés | oui | taille annoncée au manifeste |
| **Assets durables** (`/data/assets`, avatars, affiches, variantes) | oui (format courant) | SHA-256 + taille recalculés à l'écriture **et** à l'extraction |
| Caches régénérables (miniatures temporaires, `tmp/`) | non | — |
| Données durables des extensions (KV `extension_data`, objets `extension_objects` et leurs blocs) | oui (elles sont dans la base SQL) | via la base |
| Paquets Wasm installés (`/data/plugins`) | **non** (voir limites) | — |
| Médias originaux des NAS | jamais | — |

Le manifeste porte `assets` (nombre) et `assetBytes` (octets) ; une archive dont le décompte ne correspond pas est rejetée.

## Garanties

- Création : un asset absent, tronqué ou altéré fait échouer la sauvegarde (`AssetIntegrity`) ; aucun fichier d'archive n'est publié.
- Extraction : écriture dans un dossier privé de préparation, publication atomique seulement si la structure, le checksum et les assets sont vérifiés. Le checksum ne protège pas contre une modification hostile.
- Les lignes SQL `assets` viennent de la base restaurée ; les octets sont remis par `AssetStore::restore_object` (re-hachage, publication atomique, idempotent).
- **Une restauration n'est réussie que si `AssetStore::verify_all()` ne signale ni objet manquant ni corrompu.** Importer le SQL seul ne suffit pas.

## Procédure

1. Arrêter Core. `media-recover ARCHIVE NOUVEAU_DOSSIER` extrait `database.*`, `config.json`, `assets/<sha256>`, `assets.json`.
2. `media-restore` restaure la base vers une cible vierge.
3. `media-restore-assets NOUVEAU_DOSSIER` (même environnement que Core : `MEDIA_DB_*`, `MEDIA_ASSETS_PATH`) importe les assets puis audite **toute** la table `assets` ; code de sortie non nul s'il manque ou corrompt un objet.
4. Démarrer Core et contrôler l'accès.

## Anciennes archives

Les archives `MDBK0001` chiffrées ne sont plus lisibles avec le binaire courant. Les conserver et utiliser une ancienne version de `media-recover` avec leur clé d'origine si une restauration est nécessaire. Les archives `MDBK0002` sans assets restent lisibles ; `media-recover` signale qu'elles n'en contiennent aucun.

## Limites connues

- Les étapes 2 et 3 restent deux commandes. L'import et l'audit sont testés sur SQLite (`backup_carries_assets_and_restore_is_verified_on_a_fresh_store`, qui couvre aussi l'idempotence) ; le binaire `media-restore-assets` lui-même n'a pas été exécuté de bout en bout. Restauration PostgreSQL non testée (base externe requise).
- Les fichiers de `/data/plugins` (paquets Wasm) ne sont pas dans l'archive. Après restauration, les extensions concernées doivent être réinstallées depuis leur dépôt signé ; la conservation de leurs données lors de cette réinstallation n'a pas été testée sur une restauration.
- Les sauvegardes planifiées sont maintenant des tâches `durable_tasks` (cible `backup`) : bail, fencing, limite de 1 h, préemption par la lecture. Un crash entre la réservation de l'échéance et la mise en file saute une exécution jusqu'à l'intervalle suivant. Une sauvegarde manuelle (`POST`) et une planifiée peuvent se chevaucher (archives distinctes).
- La liste d'assets est prise après le snapshot SQL ; un asset ajouté entre les deux est embarqué sans ligne SQL (inoffensif, nettoyé par le collecteur).

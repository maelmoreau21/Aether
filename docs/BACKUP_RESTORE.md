# Sauvegarde complète et restauration

## Contenu d'une archive `.media-backup`

Format courant `MDBK0003` : chiffré et authentifié avec la clé externe `MEDIA_BACKUP_KEY_FILE` / `MEDIA_BACKUP_KEY` (obligatoire, voir [BACKUP_BUNDLE.md](BACKUP_BUNDLE.md)). Sans clé, aucune archive n'est créée. Les fichiers extraits par `media-recover` sont, eux, en clair.

| Élément | Présent | Vérification |
|---|---|---|
| Base SQL cohérente (SQLite snapshot / `pg_dump`) | oui | authentification AEAD de chaque record |
| Configuration `MEDIA_*` / `OIDC_*` (sans la clé de sauvegarde) | oui | authentification AEAD de chaque record |
| Certificats TLS référencés | oui | taille annoncée au manifeste |
| **Assets durables** (`/data/assets`, avatars, affiches, variantes) | oui (format courant) | SHA-256 + taille recalculés à l'écriture **et** à l'extraction |
| Caches régénérables (miniatures temporaires, `tmp/`) | non | — |
| Données durables des extensions (KV `extension_data`, objets `extension_objects` et leurs blocs) | oui (elles sont dans la base SQL) | via la base |
| Paquets Wasm installés (`/data/plugins`) | via la base (`extensions.package_json`) | reconstruits et vérifiés sur disque par `media-restore-assets` (`sync_installed` / `check_health`) |
| Médias originaux des NAS | jamais | — |

Le manifeste porte `assets` (nombre) et `assetBytes` (octets) ; une archive dont le décompte ne correspond pas est rejetée.

## Garanties

- Création : un asset absent, tronqué ou altéré fait échouer la sauvegarde (`AssetIntegrity`) ; aucun fichier d'archive n'est publié.
- Extraction : écriture dans un dossier privé de préparation, publication atomique seulement si l'authentification, la structure et les assets sont vérifiés. Une modification hostile d'une archive `MDBK0003` est détectée ; le SHA-256 des archives `MDBK0002` ne le détecte pas.
- Les lignes SQL `assets` viennent de la base restaurée ; les octets sont remis par `AssetStore::restore_object` (re-hachage, publication atomique, idempotent).
- **Une restauration n'est réussie que si `AssetStore::verify_all()` ne signale ni objet manquant ni corrompu.** Importer le SQL seul ne suffit pas.

## Procédure

1. Arrêter Core. Exporter la clé (`MEDIA_BACKUP_KEY_FILE=/chemin/secret`, sans l'afficher) pour une archive `MDBK0003`. `media-recover ARCHIVE NOUVEAU_DOSSIER` extrait `database.*`, `config.json`, `assets/<sha256>`, `assets.json`.
2. `media-restore` restaure la base vers une cible vierge.
3. `media-restore-assets NOUVEAU_DOSSIER` (même environnement que Core : `MEDIA_DB_*`, `MEDIA_ASSETS_PATH`, `MEDIA_PLUGINS_PATH`) importe les assets, audite **toute** la table `assets`, puis reconstruit et vérifie le stockage disque des extensions installées (`sync_installed` + `check_health`) ; code de sortie non nul s'il manque ou corrompt un objet ou une extension.
4. Démarrer Core et contrôler l'accès.

## Anciennes archives

Les archives `MDBK0002` (non chiffrées) restent lisibles sans clé, sans conversion ni suppression ; `media-recover` signale celles sans assets. Les archives `MDBK0001` ne sont pas lisibles avec le binaire courant : les conserver intactes et utiliser la version d'Aether qui les a créées avec leur clé d'origine. Si elles ont été chiffrées avec la clé publiée dans l'historique Git, les considérer comme divulguées (voir [BACKUP_BUNDLE.md](BACKUP_BUNDLE.md)).

## Limites connues

- Les étapes 2 et 3 restent deux commandes. L'import, l'audit et la reconstruction des extensions sont testés sur SQLite (`full_backup_restore_e2e.rs`, y compris l'exécution du binaire `media-restore-assets` dans `restore_assets_cli_rejects_partial_extension_sync`) ; l'enchaînement CLI complet `media-recover` → `media-restore` → `media-restore-assets` via les binaires seuls reste à couvrir en un test unique. La restauration de la base PostgreSQL et de la configuration a été testée sur PostgreSQL 18 jetable avec `scripts/test-admin-postgres.ps1` ; ce test ne prouve pas la restauration des assets sur PostgreSQL ni un déploiement réel.
- Les fichiers de `/data/plugins` ne sont pas stockés en tant que fichiers bruts dans l'archive, mais les paquets signés (`package_json`) sont inclus dans la table `extensions` de la base SQL et resynchronisés automatiquement sur disque par `media-restore-assets` (`sync_installed`). Une réinstallation manuelle depuis le dépôt n'est donc pas nécessaire pour les extensions déjà présentes dans la base restaurée.
- Les sauvegardes planifiées sont maintenant des tâches `durable_tasks` (cible `backup`) : bail, fencing, limite de 1 h, préemption par la lecture. Un crash entre la réservation de l'échéance et la mise en file saute une exécution jusqu'à l'intervalle suivant. Une sauvegarde manuelle (`POST`) et une planifiée peuvent se chevaucher (archives distinctes).
- La liste d'assets est prise après le snapshot SQL ; un asset ajouté entre les deux est embarqué sans ligne SQL (inoffensif, nettoyé par le collecteur).

- Les nouvelles initialisations ne créent plus de faux plugin `native` pour les avatars. Les lignes déjà présentes dans une base de travail ne sont ni supprimées ni modifiées automatiquement ; une ancienne entrée synthétique invalide peut encore empêcher la synchronisation et doit être examinée avant toute intervention sur cette base.

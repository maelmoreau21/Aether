# Sauvegardes chiffrées et authentifiées (MDBK0003)

Base : `develop`. À copier dans la description de la PR puis supprimer ce fichier.

## Problème

Le format `MDBK0002` n'était ni chiffré ni authentifié. Une archive (ou son téléchargement admin) exposait en clair : secrets OIDC, `MEDIA_DB_URL` avec mot de passe, certificats et clé privée TLS, hachages de mots de passe, jetons de sessions, secrets de Nodes. Le SHA-256 final peut être recalculé par quiconque modifie l'archive. Les archives `MDBK0001` avaient été chiffrées avec une clé publiée dans l'historique Git.

## Changements

- `backup_bundle` : nouveau format **`MDBK0003`**, ChaCha20-Poly1305 par record (`ring`, aucune nouvelle dépendance), clé par archive dérivée en HKDF-SHA256 d'une clé externe de 32 octets et d'un sel aléatoire, compteur comme nonce, en-tête et type de record authentifiés, record final obligatoire. Réordonnancement, troncature, ajout, mauvaise clé : rejetés sans rien publier.
- Clé externe : `MEDIA_BACKUP_KEY_FILE` (ou `MEDIA_BACKUP_KEY`), 64 hexadécimaux, clés triviales refusées, jamais archivée ni journalisée (`Debug` expurgé).
- **Fail closed** : sans clé valide, plus d'archive (admin : `backup_key_required` 409 avant tout snapshot, `available:false` ; CLI `--full` : arrêt). Le plaintext `MDBK0002` n'est produit que via `MEDIA_BACKUP_ALLOW_UNENCRYPTED=true` (avertissement).
- Anciens formats : `MDBK0002` reste lisible (sans clé), `MDBK0001` renvoie une erreur dédiée. Aucune conversion ni suppression automatique ; l'archive source n'est ouverte qu'en lecture.
- `media-recover` lit la clé dans l'environnement (jamais en argument).
- Tests et docs mis à jour (`BACKUP_BUNDLE`, `BACKUP_RESTORE`, `BACKUP_RECOVERY`, `ADMIN_BACKUPS`, `ADMIN_DEPLOYMENT`, `SECURITY`, README). Test `admin_postgres` : deux tables dérivées de la migration des déclencheurs sont exclues de la comparaison de comptes (échec antérieur à ce changement, sans lien avec les archives).

## Action requise au déploiement (changement cassant volontaire, bêta)

Créer une clé neuve hors dépôt et hors `/backups`, la monter en lecture pour l'UID du service, définir `MEDIA_BACKUP_KEY_FILE`, redémarrer Core. Tant que ce n'est pas fait, les sauvegardes manuelles et planifiées échouent (`backup_key_required`). Ne pas réutiliser la clé publiée dans l'historique. Conserver la clé séparément : sans elle les archives sont irrécupérables.

## Tests exécutés (Docker `rust:slim-bookworm`, Rust 1.99.1, PostgreSQL 18.6 réel)

| Commande | Résultat |
|---|---|
| `cargo fmt -p media-poc -- --check` | OK |
| `cargo clippy --locked -p media-poc --all-targets` | aucun avertissement |
| `cargo test --locked -p media-poc --lib -- backup_bundle admin_backups assets` | 24 passés |
| `--test backup_bundle` | 7 passés |
| `--test admin_backups` | 2 passés |
| `--test full_backup_restore_e2e` | 2 passés |
| `--test backup_cli` (binaires `media-backup --full` / `media-recover`) | 1 passé |
| `--test admin_postgres -- --ignored` (PostgreSQL 18.6, `pg_dump` 18.6) | 1 passé |

Non exécuté : la suite complète du workspace, `browser_admin` (Playwright, non lancé), Windows natif, un NAS réel.

## Risques résiduels

- La clé de `MDBK0001` publiée dans l'historique Git n'est pas détectable par le code (valeur non recopiée, historique non réécrit). Les archives `MDBK0001` créées avec elle sont à considérer comme divulguées ; les secrets associés sont à renouveler.
- Le snapshot SQL intermédiaire est en clair (`0600`, dossier privé) pendant la sauvegarde ; un arrêt brutal peut le laisser. Les restes `.media-backup-<uuid>` et `.media-full-*` ne sont pas couverts par le nettoyage `BackupStaging`.
- Les fichiers extraits par `media-recover` sont en clair. Sous Windows, les modes `0600`/`0700` ne s'appliquent pas : les ACL du dossier décident.
- `MEDIA_BACKUP_KEY` en variable d'environnement est visible de quiconque lit l'environnement du processus : préférer le fichier.
- Pas de rotation automatique ni de vérification d'empreinte de clé : une mauvaise clé se détecte à l'extraction.
- Aucune validation sur le NAS : tout ce qui précède est synthétique.

## Restauration

1. Arrêter Core. `MEDIA_BACKUP_KEY_FILE=/chemin/secret` (sans afficher la clé).
2. `media-recover ARCHIVE NOUVEAU_DOSSIER` (nouveau dossier, parent existant). Les archives `MDBK0002` n'ont pas besoin de clé.
3. `media-restore` (SQLite : cible neuve ; PostgreSQL : base vide), puis `media-restore-assets NOUVEAU_DOSSIER`.
4. Vérifier `AssetStore::verify_all`, démarrer Core, contrôler l'accès, puis supprimer de façon sûre le dossier extrait (en clair).

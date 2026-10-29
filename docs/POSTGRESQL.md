# PostgreSQL 18 facultatif

SQLite reste le moteur par défaut en standalone et en distribué. Un Core unique
peut utiliser un PostgreSQL 18 externe ; aucun conteneur de base n'est ajouté aux
Compose de déploiement. Les Media Nodes ne se connectent jamais à cette base.

Les champs de temps Unix du schéma initial PostgreSQL utilisent `bigint` pour représenter les dates au-delà de 2038. Ce changement concerne uniquement les bases vierges ; aucune migration des bases existantes n'est fournie. `scripts/test-fresh-schema.py` et `scripts/test-fresh-postgres-schema.ps1` ont été exécutés avec des dates supérieures à 2³¹. Le parcours `scripts/test-admin-postgres.ps1` a réussi sur un conteneur PostgreSQL 18 jetable : sauvegarde complète non chiffrée et restauration dans une autre base. Il ne prouve pas un déploiement NAS ou une restauration PostgreSQL des fichiers assets.

## Configuration

```dotenv
MEDIA_DB_ENGINE=postgres
MEDIA_DB_URL=postgresql://media:REMPLACER@database.example.test:5432/media?sslmode=verify-full
```

Créer une base dédiée avec un propriétaire autorisé à créer les tables, index et
fonctions de son schéma. Aucun superutilisateur ni extension n'est requis.
Utiliser un secret réel hors Git et une autorité TLS approuvée sur les connexions
entre machines. Les paramètres SQLx `sslmode`, `sslrootcert`, `sslcert`, `sslkey`
permettent de configurer la vérification TLS. Ne pas afficher l'URL dans les logs.

Pour SQLite, laisser `MEDIA_DB_ENGINE=sqlite`, `MEDIA_DB_URL` vide et conserver
`MEDIA_DB_PATH` (défaut `data/core.sqlite3`, `/data/core.sqlite3` dans Docker).
Un moteur inconnu ou une URL contradictoire fait échouer le démarrage.
Le bootstrap interactif `media-bootstrap` utilise exactement cette configuration.

Changer d'URL ou de moteur **ne transfère aucune donnée**. La nouvelle base est
indépendante ; ne pas abandonner un volume SQLite avant une procédure explicite
de transfert et de vérification. Aucun outil de transfert inter-moteurs n'est livré.

## Schémas et transactions

Le schéma neuf SQLite et PostgreSQL vient du seul fichier
`crates/core/schema.sql`. Le Core choisit la section correspondant au moteur
et l'applique dans une transaction uniquement si le schéma est vide. Il ne
convertit ni ne modifie automatiquement une ancienne base incompatible. La
table `schema_version` identifie la structure actuelle (version 1) ; si le
produit change sans procédure de mise à jour, une ancienne base est refusée
explicitement et ses données restent intactes.
SQLx utilise le même pool applicatif et les mêmes requêtes paramétrées pour les
comptes, sessions, permissions et catalogue. Les identifiants texte UUID existants
et leurs relations restent identiques ; aucune régénération d'identité.

Les séquences lecture/écriture précédemment protégées par `BEGIN IMMEDIATE`
utilisent un verrou transactionnel PostgreSQL avant la lecture d'état, notamment
pour fermer le bootstrap une seule fois. Les mises à jour d'activité des sessions
gardent une file bornée et un timeout de verrou court propre à la transaction.
SQLite garde WAL, ses clés étrangères et son timeout de verrouillage.

Quatre fonctions privées au schéma applicatif conservent les expressions
`unixepoch`, `instr`, `json_valid` et le sous-ensemble `json_extract('$.champ')`
utilisé par les scans. Ce dernier n'est pas une implémentation JSONPath générale.
Leur résolution interne est limitée à `pg_catalog`.

## Sauvegarde et restauration

SQLite conserve ses snapshots WAL cohérents `VACUUM INTO` et sa restauration
existante. PostgreSQL utilise les clients **PostgreSQL 18** `pg_dump` et
`pg_restore`, installés par l'opérateur sur la machine exécutant les commandes :

```powershell
$env:MEDIA_DB_ENGINE = 'postgres'
# Fournir MEDIA_DB_URL par votre mécanisme de secrets, sans l'afficher.
media-backup C:\Backups\media.dump
# Arrêter le Core ; sélectionner une base PostgreSQL vide dans MEDIA_DB_URL.
media-restore C:\Backups\media.dump
```

L'archive est au format custom ; les identifiants de connexion sont transmis par
environnement, jamais comme arguments de processus. Le fichier cible ne peut pas
être écrasé (`--replace` est refusé pour PostgreSQL), les répertoires médias sont
exclus et les fichiers Unix sont créés en mode `0600`. La restauration exige une
base vide, utilise une transaction unique et n'emploie ni `--clean` ni suppression
des données existantes. Le Core doit rester arrêté pendant cette opération.

Les images applicatives minimales n'embarquent pas les clients PostgreSQL :
exécuter ces commandes sur un hôte d'administration équipé, ou utiliser les outils
natifs PostgreSQL avec les mêmes précautions. Les volumes médias ne font jamais
partie de la sauvegarde de base.

## Tests reproductibles

Après génération des fixtures synthétiques AV1/Opus et installation de ffprobe :

```powershell
./scripts/test-postgres.ps1
```

Le script crée une instance PostgreSQL 18 temporaire en RAM, sur un port loopback
aléatoire, avec un mot de passe aléatoire. Il exécute les contrats HTTP/catalogue,
OIDC et multi-Node, puis supprime uniquement son propre conteneur.
`scripts/test-fresh-schema.py` vérifie la section SQLite et
`scripts/test-fresh-postgres-schema.ps1` valide séparément la création d'une
base neuve. `scripts/test-two-cores.ps1` démarre deux processus Core distincts
sur la même base temporaire, puis arrête brutalement le premier pour vérifier
que le second sert toujours l'API. Ce dernier essai ne couvre aucune tâche en
cours ni les effets durables.
Les tests créent des schémas isolés ; ne jamais fournir une base de production.

Pour une instance temporaire fournie séparément :

```powershell
cargo test --locked -p media-poc --test postgres -- --ignored --test-threads=1
cargo test --locked -p media-poc --lib oidc::tests -- --test-threads=1
cargo test --locked -p media-poc --test distributed_multi_node -- --test-threads=1
```

Ces commandes lisent `MEDIA_TEST_POSTGRES_URL`. Sans cette variable, le contrat
HTTP SQLite reste actif et le test PostgreSQL explicite reste ignoré.
Les tests Range vérifient les octets et les offsets de seek, pas un décodage
audio/vidéo dans un navigateur. Les preuves multi-machine physique, fournisseur
OIDC réel et navigateur restent distinctes des tests locaux et des conteneurs.

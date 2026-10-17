# Guide de déploiement Docker autonome & Synology DS1522+

Ce document décrit le déploiement opérationnel d'Aether en conteneur autonome « tout-en-un » via Docker et Docker Compose, avec les spécificités matérielles du NAS Synology DS1522+ et les règles de durcissement en environnement de production.

---

## 1. Vue d'ensemble & Architecture

La distribution Docker autonome d'Aether rassemble dans une image unique :
- L'application Web pré-compilée SvelteKit 5 (servie directement sous `/app/` par Axum).
- Le Core Aether (authentification locale, sessions persistantes, RBAC `administrator`/`user`, catalogue SQLite).
- Le Media Node local (scanner récursif de bibliothèques, extraction NFO, gestion des affiches locales, streaming HTTP Range 64 Kio).
- L'outil système `ffprobe` (analyse technique des conteneurs MKV/MP4, AV1 et Opus).
- Le gestionnaire de processus `tini` (gestion des signaux `SIGINT`, arrêt propre et moissonnage des processus zombies).

### Invariants architecturaux
1. **Zéro runtime Node.js permanent** : Le frontend SvelteKit est compilé statiquement lors de la première étape de build (`adapter-static`) et servi en fichiers statiques par Axum. Aucun interpréteur Node.js n'est présent dans l'image d'exécution.
2. **Processus unique** : Core et Media Node local s'exécutent au sein d'un seul binaire natif (`media-poc`), sans serveur HTTP secondaire ni dépendance externe obligatoire (pas de PostgreSQL ni Valkey pour le mode autonome).
3. **Moindre privilège** : L'image s'exécute sous un utilisateur non-root dédié `aether` (`UID:GID 10001:10001`). Les capacités Linux sont révoquées (`cap_drop: ALL`) et l'élévation de privilèges est interdite (`no-new-privileges: true`).
4. **Volumes étanches** : Séparation stricte entre les bibliothèques médias en lecture seule (`:ro`) et l'état persistant de l'application (`/data`).

---

## 2. Spécificités matérielles du Synology DS1522+

Le Synology DiskStation DS1522+ est équipé d'un processeur **AMD Ryzen Embedded R1600** (architecture Zen 1, 2 cœurs / 4 threads, 2.6 GHz base / 3.1 GHz boost).

### Contraintes matérielles critiques :
- **Absence totale d'iGPU** : Le Ryzen R1600 ne possède aucun processeur graphique intégré (pas d'AMD VCN/VCE, pas d'Intel QuickSync).
- **Interdiction de passthrough `/dev/dri`** : Ne JAMAIS déclarer de montage `/dev/dri:/dev/dri` dans `compose.yaml`. Sur DSM, ce nœud de périphérique n'existe pas et provoquerait le crash immédiat du conteneur au démarrage.
- **Lecture directe (Direct Play) prioritaire** : Aether privilégie la lecture directe AV1 vidéo + Opus audio dans les conteneurs `.mkv` et `.mp4`. Aucun transcodage vidéo en temps réel n'est imposé, garantissant une charge CPU minimale sur le DS1522+.

---

## 3. Topologie des volumes & Données

Le conteneur utilise deux montages distincts :

| Montage | Cible conteneur | Permissions | Usage |
|---|---|---|---|
| `aether_data` | `/data` | `rw` (`0750`, `aether:aether`) | Base SQLite (`core.sqlite3`), journaux WAL/SHM, sessions et index de scan |
| `${MEDIA_LIBRARY_DIR}` | `/media` | `ro` (`0755`) | Dossier racine des médias utilisateur (films, séries) |

### Intégrité des médias en lecture seule (`:ro`)
- Le montage `:ro` garantit qu'aucun fichier vidéo, sous-titre, fichier NFO ou affiche de votre bibliothèque ne peut être modifié, renommé ou altéré par Aether.
- Toute tentative d'écriture dans `/media` est rejetée au niveau du noyau Linux (`Read-only file system`).
- Les métadonnées découvertes et les affiches analysées sont indexées exclusivement dans la base de données `/data/core.sqlite3`.

---

## 4. Configuration réseau & Sécurité

### 4.1 Liaison d'écoute par défaut (Loopback sécurisé)
Dans `compose.yaml`, le port hôte est configuré par défaut sur :
```yaml
ports:
  - "127.0.0.1:3000:3000"
```
Cette liaison restreint l'accès à la boucle locale de l'hôte, empêchant toute exposition accidentelle non authentifiée sur le réseau local (LAN).

### 4.2 Accès direct sur le réseau local (LAN)
Pour autoriser les appareils de votre réseau local à accéder directement à Aether sans proxy inverse :
1. Dans `compose.yaml`, modifiez la redirection de port :
   ```yaml
   ports:
     - "0.0.0.0:3000:3000"
   ```
2. Mettez à jour la variable `MEDIA_PUBLIC_URL` avec l'adresse IP de votre NAS ou machine :
   ```yaml
   environment:
     - MEDIA_PUBLIC_URL=http://192.168.1.100:3000
   ```
3. Les en-têtes HTTP `Host` et `Origin` reçus doivent correspondre exactement à `MEDIA_PUBLIC_URL` pour franchir la protection CSRF et DNS-rebinding.

### 4.3 Exposition via Proxy Inverse HTTPS (Synology Reverse Proxy, Nginx, Caddy, Traefik)
Pour un accès sécurisé depuis Internet ou votre domaine local en HTTPS :
1. Conservez la liaison loopback locale `"127.0.0.1:3000:3000"` sur le conteneur.
2. Définissez `MEDIA_PUBLIC_URL` avec l'URL publique HTTPS canonique :
   ```yaml
   environment:
     - MEDIA_PUBLIC_URL=https://aether.mondomaine.fr
   ```
3. **Obligation de transfert d'en-tête `Host`** : Configurez votre proxy inverse pour relayer l'en-tête `Host` réel du client :
   - **Nginx** :
     ```nginx
     proxy_set_header Host $host;
     proxy_set_header X-Forwarded-Proto $scheme;
     proxy_set_header X-Forwarded-For $remote_addr;
     proxy_pass http://127.0.0.1:3000;
     ```
   - **Synology DSM Reverse Proxy** : Activé automatiquement via le portail de connexion / proxy inverse DSM.
   - **Caddy** :
     ```caddy
     aether.mondomaine.fr {
         reverse_proxy 127.0.0.1:3000
     }
     ```
4. **Sonde de santé découplée** : L'endpoint interne `/api/v1/health` est découplé de la validation stricte d'hôte externe, permettant à Docker d'interroger `http://127.0.0.1:3000/api/v1/health` avec succès même lorsque `MEDIA_PUBLIC_URL` est configuré sur un domaine HTTPS externe.

---

## 5. Variables d'environnement de référence

| Variable | Valeur par défaut conteneur | Description |
|---|---|---|
| `MEDIA_PORT` | `3000` | Port d'écoute interne du serveur HTTP |
| `MEDIA_BIND_ADDR` | `0.0.0.0` | Adresse IP d'écoute à l'intérieur du conteneur |
| `MEDIA_PUBLIC_URL` | `http://127.0.0.1:3000` | Origine publique canonique validée pour `Host`, `Origin` et cookies de session |
| `MEDIA_DB_PATH` | `/data/core.sqlite3` | Chemin du fichier SQLite dans le volume persistant |
| `MEDIA_SQLITE_BUSY_TIMEOUT_SECS` | `15` | Délai d'attente max sur verrou d'écriture SQLite avant erreur BUSY (45s+ recommandé sur 10k+ médias) |
| `MEDIA_LIBRARY_ROOT` | `/media` | Racine absolue du dossier des bibliothèques montées en lecture seule |
| `MEDIA_WEB_DIST` | `/app/apps/web/build` | Répertoire des assets Web statiques pré-compilés |

---

## 6. Procédure de déploiement pas à pas

### Étape 1 : Préparation de l'arborescence (ex. sur Synology DSM)
Connectez-vous en SSH à votre machine ou Synology, ou créez un projet dans **Synology Container Manager** :
```bash
mkdir -p /volume1/docker/aether
cd /volume1/docker/aether
```
Placez-y les fichiers `compose.yaml` (ou `docker-compose.yml`) et le fichier `.env` suivant :
```env
# Chemin vers votre dossier de vidéos existant
MEDIA_LIBRARY_DIR=/volume1/video
```

### Étape 2 : Construction et lancement du service
Si vous construisez l'image localement à partir des sources :
```bash
docker compose build
docker compose up -d
```
Vérifiez l'état de santé du conteneur :
```bash
docker compose ps
```
Le conteneur doit afficher l'état `Up ... (healthy)` après environ 6 à 10 secondes.

### Étape 3 : Amorçage sécurisé du premier administrateur (Bootstrap)
Aether n'inclut **aucun identifiant ni mot de passe par défaut**. Le premier compte administrateur local doit être créé de façon interactive via le terminal :

```bash
docker compose exec -it aether media-bootstrap
```
Le programme vous invite à saisir :
1. L'identifiant de connexion en minuscules (ex. `admin`).
2. Le mot de passe (saisie masquée sans écho).
3. La confirmation du mot de passe (saisie masquée).

```
Local administrator bootstrap. Protect the database directory with OS permissions.
Login (lowercase): admin
Password: [masqué]
Confirm password: [masqué]
Administrator created. Sign in through /app/login/.
```

> **Sécurité renforcée de l'amorçage :**
> - L'exécution non interactive sans terminal échoue immédiatement avec le code 1 (`bootstrap requires a local interactive terminal and accepts no arguments`).
> - Toute tentative ultérieure de réexécution de `media-bootstrap` est définitivement verrouillée par la base SQLite (`initial bootstrap is already closed`).

### Étape 4 : Connexion et création de bibliothèque
1. Ouvrez votre navigateur sur l'adresse configurée (ex. `http://127.0.0.1:3000/app/login/` ou votre URL publique).
2. Connectez-vous avec les identifiants créés.
3. Rendez-vous dans **Administration > Bibliothèques** (`/app/admin/libraries/`).
4. Créez une bibliothèque de type Films avec la racine `/media` (ou un sous-dossier de `/media`).
5. Déclenchez le scan. `ffprobe` analyse les fichiers sans modifier le dossier `:ro`, et peuple le catalogue ainsi que les affiches.

---

## 7. Exploitation, Maintenance & Sauvegarde

### Arrêt propre et reprise
Aether est configuré avec `stop_signal: SIGINT` et `stop_grace_period: 15s`. Lors d'un arrêt :
```bash
docker compose down
```
- Le processus `tini` transmet le signal `SIGINT` à `media-poc`.
- Les tâches de scan en cours sont annulées proprement.
- Les connexions SQLite sont vidées et le journal WAL est synchronisé.
- Au redémarrage (`docker compose up -d`), les scans interrompus sont automatiquement marqués en état `error` avec le libellé `"Scan interrompu par un redémarrage"` et l'inventaire existant est intégralement préservé.

### Sauvegarde et restauration fiables (SQLite WAL & Docker)
Toutes les données applicatives (comptes administrateur/utilisateurs, hachages Argon2id, sessions, graphe média, catalogue, bibliothèques et autorisations) résident dans `/data/core.sqlite3` avec le journal WAL SQLite.

> [!WARNING]
> **Ne jamais effectuer de copie naïve du seul fichier `core.sqlite3` à chaud** : en mode SQLite WAL, les transactions récentes résident dans `core.sqlite3-wal` et un instantané fichier direct produit une base corrompue ou incomplète.
> De plus, sur Synology DSM comme sur Linux standard, les volumes nommés Docker (ex. `aether_data`) sont gérés par le démon et ne se situent pas à un chemin arbitraire de l'hôte.

Aether intègre deux binaires natifs dédiés, légers et autonomes : `media-backup` et `media-restore` (aucun service tiers ou dépendance externe requis).

#### 1. Sauvegarde à chaud (cohérente WAL, sans coupure de service)
Exécutable en direct sans interrompre les utilisateurs connectés ni le streaming en cours :
```bash
docker compose exec aether media-backup
```
Par défaut, la sauvegarde est enregistrée dans `/data/backups/core_backup_<timestamp>.sqlite3`. Vous pouvez également spécifier un chemin cible précis :
```bash
docker compose exec aether media-backup /data/backups/core_backup.sqlite3
```
- **Mécanisme** : Utilise l'instruction SQLite `VACUUM INTO`, créant un instantané transactionnel point-in-time compact, défragmenté, qui consolide tous les journaux WAL dans un fichier SQLite unifié.
- **Sécurité et intégrité** :
  - Exécute automatiquement `PRAGMA integrity_check` avant confirmation.
  - Permissions strictes `0600` (`rw-------`) appliquées au fichier et `0750` au dossier.
  - Zéro fuite de mots de passe, hachages ou secrets dans les journaux ou sorties terminal (uniquement le résumé des compteurs).

#### 2. Sauvegarde à froid du volume Docker (Compose ou Synology Container Manager)
Si vous préférez sauvegarder le volume Docker à froid lors d'une fenêtre de maintenance :
- **Méthode 1 — Utilitaire `media-backup` hors ligne** :
  ```bash
  docker compose stop aether
  docker compose run --rm aether media-backup /data/backups/core_backup.sqlite3
  docker compose start aether
  ```
- **Méthode 2 — Instantané tar du volume Docker nommé** (universel sur Synology DSM et Linux) :
  ```bash
  docker compose stop aether
  docker run --rm -v aether_data:/data -v /volume1/backups:/backup alpine tar czf /backup/aether_data_$(date +%Y%m%d).tar.gz -C /data .
  docker compose start aether
  ```

#### 3. Restauration dans une installation neuve ou après incident
Pour restaurer la base sur une nouvelle machine, un nouveau disque ou un volume Docker vierge :

1. Créez le volume Docker vierge (si nécessaire) et lancez l'utilitaire de restauration :
   ```bash
   # Création du volume persistant
   docker volume create aether_data

   # Restauration atomique à partir d'un fichier de sauvegarde
   docker compose run --rm -v /volume1/backups:/backup aether media-restore /backup/core_backup.sqlite3
   ```
   *Ou si vous utilisez une archive tar de volume :*
   ```bash
   docker run --rm -v aether_data:/data -v /volume1/backups:/backup alpine tar xzf /backup/aether_data_20261017.tar.gz -C /data
   ```

2. **Garanties opérationnelles de `media-restore`** :
   - Validation stricte de l'intégrité SQLite (`PRAGMA integrity_check`) et contrôle de présence des tables requises avant toute altération du disque.
   - Nettoyage automatique des anciens journaux `-wal` et `-shm` résiduels pour interdire toute relecture transactionnelle asynchrone incompatible.
   - Remplacement atomique via fichier de staging (`core.sqlite3.restoring`) et rétablissement des permissions `0600`.
   - Exécution idempotente des migrations et rattrapage Unicode pour garantir la cohérence du schéma.
   - Préservation déterministe intégrale : identifiants UUID des œuvres, versions, pistes, comptes administrateur/utilisateurs et droits d'accès.
   - Reconnexion immédiate : l'administrateur et les utilisateurs se reconnectent immédiatement avec leurs mots de passe originaux sans réinitialisation requise.

3. Démarrez ensuite le service Aether :
   ```bash
   docker compose up -d
   ```

---

### Concurrence des écritures pendant la publication SQLite
Lors de l'ingestion initiale d'une volumineuse bibliothèque (ex. 10 000 nouveaux médias), la transaction d'écriture exclusive de publication (`publish_staged_scan` via `BEGIN IMMEDIATE`) peut être maintenue active pendant environ 30 secondes.

- **Lectures concurrentes non bloquées** : En mode SQLite WAL, les requêtes HTTP de lecture (`GET /api/v1/catalog`, streaming HTTP Range, affiches locales) sont servies en isolation de snapshot sans jamais attendre ni bloquer l'écrivain (latences mesurées : `p50 = 7,29 ms`).
- **Écritures concurrentes en attente** : Les opérations nécessitant une écriture (mise à jour de session `last_seen_at`, création ou édition de bibliothèques, déclenchement d'un nouveau scan) attendent la libération du verrou d'écriture via le gestionnaire SQLite Busy (`busy_timeout`).
- **Délai `busy_timeout` et configuration** :
  - Valeur par défaut : 15 secondes.
  - En cas de premier scan massif (> 10 000 nouveaux médias simultanés), configurez `MEDIA_SQLITE_BUSY_TIMEOUT_SECS=45` ou `60` dans votre `compose.yaml` pour éviter les erreurs `SQLITE_BUSY (code 5 : database is locked)` sur les requêtes d'écriture concurrentes.
- **Absence de blocage permanent** : Dès validation (`COMMIT`) de la transaction de publication, SQLite libère immédiatement le verrou d'écriture et toutes les opérations en attente aboutissent sans interblocage ni corruption.

---

### Diagnostics des scans et purge du staging
Aether optimise l'espace disque et la clarté des diagnostics :
- **Plafond mémoire et affichage borné** : Les 100 premières anomalies (erreurs d'analyse de conteneurs, XML NFO invalides, conflits de métadonnées) sont conservées et renvoyées dans le tableau `errors` de l'API `/api/v1/admin/libraries/{id}/scan` pour affichage dans l'interface web.
- **Compteurs totaux fiables** : Les champs `errorsCount` et `warningsCount` indiquent le nombre total exact d'anomalies détectées lors de la passe, même lorsque celles-ci dépassent le plafond de 100 éléments.
- **Purge systématique du staging** : Dès la fin de la publication atomique, les tables temporaires `scan_staging_files` et `scan_staging_issues` sont intégralement vidées (`DELETE FROM ...`), empêchant toute croissance non maîtrisée du fichier SQLite `/data/core.sqlite3`.
- **Protection contre les fuites de chemins** : Les messages d'anomalie n'exposent que les chemins relatifs à la racine de la bibliothèque. Tout chemin absolu d'hôte système en cas d'erreur fatale est automatiquement assaini (`/media`).

---

## 8. Métriques de performance mesurées en environnement réel

Mesures réelles enregistrées lors de la validation automatisée de bout en bout (AC-11) :

| Métrique | Valeur mesurée | Note |
|---|---|---|
| **Taille du contenu de l'image** | **213 Mo** | Image optimisée multi-stage Debian bookworm-slim avec `ffprobe` et `tini` |
| **Empreinte disque des couches** | 790 Mo | Couches d'image décompressées sur l'hôte Docker |
| **Délai de démarrage vers `healthy`** | **6,34 secondes** | Migrations SQLite automatiques, initialisation du pool et première sonde HTTP réussie |
| **Mémoire RSS du conteneur au repos (idle)** | **2,88 Mo** | Empreinte mémoire minimale après démarrage complet et service HTTP actif |
| **Mémoire RSS pic pendant le scan ffprobe** | **12,56 Mo** | Parcours récursif d'une bibliothèque et extraction de métadonnées |

Ces mesures confirment qu'Aether s'exécute avec une empreinte système particulièrement légère, parfaitement dimensionnée pour le Synology DS1522+ et les serveurs personnels à ressources modérées.
